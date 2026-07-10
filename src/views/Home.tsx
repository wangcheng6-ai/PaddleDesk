import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { DropZone } from "../components/DropZone";
import { ServiceCards } from "../components/ServiceCards";
import { TaskRowItem } from "../components/TaskRowItem";
import { createTasks, listTasks } from "../lib/ipc";
import { useApp } from "../stores/app";

export function Home() {
  const { t } = useTranslation();
  const service = useApp((state) => state.service);
  const tasks = useApp((state) => state.tasks);
  const mergeTasks = useApp((state) => state.mergeTasks);
  const setView = useApp((state) => state.setView);
  const setSelectedTaskId = useApp((state) => state.setSelectedTaskId);
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [postCreateRefreshFailed, setPostCreateRefreshFailed] = useState(false);

  const refresh = useCallback(async () => {
    const snapshot = useApp.getState().beginTaskSnapshot();
    const rows = await listTasks(null);
    mergeTasks(rows, snapshot);
  }, [mergeTasks]);

  useEffect(() => {
    let active = true;
    void refresh().then(
      () => {
        if (active) {
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
  }, [refresh]);

  const submit = useCallback(
    async (paths: string[]) => {
      setPostCreateRefreshFailed(false);
      await createTasks(paths, service);
      try {
        await refresh();
        setLoadFailed(false);
      } catch {
        setPostCreateRefreshFailed(true);
      }
    },
    [refresh, service],
  );
  const recent = useMemo(
    () =>
      [...tasks]
        .sort((left, right) => (right.created_at ?? 0) - (left.created_at ?? 0))
        .slice(0, 5),
    [tasks],
  );

  return (
    <div className="home-view">
      <h1>{t("viewTitles.home")}</h1>
      <DropZone onPaths={submit} />
      <div className="section-heading">
        <h2>{t("home.selectService")}</h2>
        <span>{t("home.serviceHelp")}</span>
      </div>
      <ServiceCards />
      <div className="section-heading">
        <h2>{t("home.recent")}</h2>
        <span>{t("home.recentHint")}</span>
      </div>
      {loading ? <p>{t("common.loading")}</p> : null}
      {loadFailed ? <p role="alert">{t("home.loadFailed")}</p> : null}
      {postCreateRefreshFailed ? (
        <p role="alert">{t("home.postCreateRefreshFailed")}</p>
      ) : null}
      {!loading && !loadFailed && recent.length === 0 ? (
        <p className="empty-state">{t("home.empty")}</p>
      ) : null}
      {recent.length > 0 ? (
        <ul className="task-list" aria-label={t("home.recent")}>
          {recent.map((task) => (
            <TaskRowItem
              task={task}
              key={task.id}
              onOpen={() => {
                setSelectedTaskId(task.id);
                setView("viewer");
              }}
            />
          ))}
        </ul>
      ) : null}
    </div>
  );
}
