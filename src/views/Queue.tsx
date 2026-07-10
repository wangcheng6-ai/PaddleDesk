import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { TaskRowItem } from "../components/TaskRowItem";
import { cancelTask, listTasks, retryTask } from "../lib/ipc";
import { useApp } from "../stores/app";

export function Queue() {
  const { t } = useTranslation();
  const tasks = useApp((state) => state.tasks);
  const mergeTasks = useApp((state) => state.mergeTasks);
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
      ),
    [tasks],
  );
  const active = tasks.filter(({ status }) =>
    ["pending", "uploading", "processing"].includes(status ?? "pending"),
  ).length;
  const failed = tasks.filter(({ status }) => status === "failed").length;
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
              onCancel={() => void run(() => cancelTask(task.id))}
            />
          ))}
        </ul>
      ) : null}
      <p className="queue-note">{t("queue.persistenceNote")}</p>
    </div>
  );
}
