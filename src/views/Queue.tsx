import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { TaskRowItem } from "../components/TaskRowItem";
import {
  cancelTask,
  dismissFailedTask,
  listTasks,
  retryTask,
} from "../lib/ipc";
import { useApp } from "../stores/app";

export function Queue() {
  const { t } = useTranslation();
  const tasks = useApp((state) => state.tasks);
  const mergeTasks = useApp((state) => state.mergeTasks);
  const removeTask = useApp((state) => state.removeTask);
  const service = useApp((state) => state.service);
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [actionFailed, setActionFailed] = useState(false);

  useEffect(() => {
    let active = true;
    const snapshot = useApp.getState().beginTaskSnapshot();
    void listTasks(null).then(
      (rows) => {
        if (active) {
          mergeTasks(rows, snapshot);
          setLoading(false);
          setLoadFailed(false);
        }
      },
      () => {
        if (active) {
          setLoading(false);
          setLoadFailed(true);
        }
      },
    );
    return () => {
      active = false;
    };
  }, [mergeTasks]);

  const sorted = useMemo(
    () =>
      [...tasks].sort(
        (left, right) => (right.created_at ?? 0) - (left.created_at ?? 0),
      ).filter(
        (task) =>
          task.service === service &&
          ["pending", "uploading", "processing", "failed"].includes(
            task.status ?? "pending",
          ),
      ),
    [service, tasks],
  );
  const active = sorted.filter(({ status }) =>
    ["pending", "uploading", "processing"].includes(status ?? "pending"),
  ).length;
  const failed = sorted.filter(({ status }) => status === "failed").length;
  const batches = useMemo(() => {
    const groups = new Map<string, typeof tasks>();
    tasks
      .filter((task) => task.service === service && task.batch_id)
      .forEach((task) => {
        const group = groups.get(task.batch_id!) ?? [];
        group.push(task);
        groups.set(task.batch_id!, group);
      });
    return [...groups.entries()]
      .filter(([, group]) => {
        const hasActive = group.some(({ status }) =>
          ["pending", "uploading", "processing"].includes(status ?? "pending"),
        );
        return group.length > 1 && hasActive;
      })
      .map(([id, group]) => ({
        id,
        total: group.length,
        finished: group.filter(({ status }) =>
          ["done", "failed", "canceled"].includes(status ?? ""),
        ).length,
      }));
  }, [service, tasks]);
  const run = async (action: () => Promise<void>) => {
    setActionFailed(false);
    try {
      await action();
    } catch {
      setActionFailed(true);
    }
  };

  return (
    <div className="queue-view">
      <div className="queue-heading">
        <h1>{t("viewTitles.queue")}</h1>
        <span>{t("queue.summary", { active, failed })}</span>
      </div>
      {loading ? <p>{t("common.loading")}</p> : null}
      {loadFailed ? <p role="alert">{t("queue.loadFailed")}</p> : null}
      {actionFailed ? <p role="alert">{t("queue.actionFailed")}</p> : null}
      {batches.length > 0 ? (
        <div className="batch-progress-list">
          {batches.map((batch) => (
            <div className="batch-progress" key={batch.id}>
              <span>
                {t("queue.batchProgress", {
                  done: batch.finished,
                  total: batch.total,
                })}
              </span>
              <progress value={batch.finished} max={batch.total} />
            </div>
          ))}
        </div>
      ) : null}
      {!loading && !loadFailed && sorted.length === 0 ? (
        <p className="empty-state">{t("queue.empty")}</p>
      ) : null}
      {sorted.length > 0 ? (
        <ul className="task-list" aria-label={t("viewTitles.queue")}>
          {sorted.map((task) => (
            <TaskRowItem
              task={task}
              key={task.id}
              onRetry={() => void run(() => retryTask(task.id))}
              onCancel={() =>
                void run(async () => {
                  await cancelTask(task.id);
                  removeTask(task.id);
                })
              }
              onDismiss={() =>
                void run(async () => {
                  await dismissFailedTask(task.id);
                  removeTask(task.id);
                })
              }
            />
          ))}
        </ul>
      ) : null}
      <p className="queue-note">{t("queue.persistenceNote")}</p>
    </div>
  );
}
