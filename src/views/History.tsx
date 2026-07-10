import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { formatDate } from "../lib/format";
import { listTasks, searchHistory, type HistoryRow } from "../lib/ipc";
import { useApp, type ServiceId, type TaskSummary } from "../stores/app";

const services: ReadonlyArray<{ id: ServiceId; key: string }> = [
  { id: "vl16", key: "services.vl16" },
  { id: "pp_ocr_v6", key: "services.ppOcrV6" },
  { id: "structure_v3", key: "services.structureV3" },
];

const fileName = (path = "") => path.split(/[\\/]/).pop() || path;

export function History() {
  const { t, i18n } = useTranslation();
  const [tasks, setTasks] = useState<TaskSummary[]>([]);
  const [results, setResults] = useState<HistoryRow[] | null>(null);
  const [query, setQuery] = useState("");
  const [service, setService] = useState<ServiceId | "all">("all");
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const upsertTask = useApp((state) => state.upsertTask);
  const setSelectedTaskId = useApp((state) => state.setSelectedTaskId);
  const setView = useApp((state) => state.setView);

  useEffect(() => {
    let active = true;
    void listTasks("done").then(
      (rows) => {
        if (!active) return;
        setTasks(rows);
        setLoading(false);
        setFailed(false);
      },
      () => {
        if (!active) return;
        setLoading(false);
        setFailed(true);
      },
    );
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    const normalized = query.trim();
    if (!normalized) {
      setResults(null);
      return;
    }
    let active = true;
    const timer = window.setTimeout(() => {
      void searchHistory(normalized).then(
        (rows) => {
          if (active) {
            setResults(rows);
            setFailed(false);
          }
        },
        () => {
          if (active) setFailed(true);
        },
      );
    }, 300);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [query]);

  const taskById = useMemo(
    () => new Map(tasks.map((task) => [task.id, task])),
    [tasks],
  );
  const rows = useMemo(
    () =>
      (results ?? tasks.map((task) => ({
        task_id: task.id,
        file_name: fileName(task.input_path),
        snippet: "",
        created_at: task.created_at ?? 0,
      }))).filter((row) => {
        const rowService = taskById.get(row.task_id)?.service;
        return service === "all" || rowService === service;
      }),
    [results, service, taskById, tasks],
  );

  const open = (row: HistoryRow) => {
    upsertTask(
      taskById.get(row.task_id) ?? {
        id: row.task_id,
        status: "done",
        input_path: row.file_name,
        created_at: row.created_at,
      },
    );
    setSelectedTaskId(row.task_id);
    setView("viewer");
  };

  return (
    <div className="history-view">
      <div className="page-heading">
        <h1>{t("viewTitles.history")}</h1>
        <span>{t("history.resultCount", { count: rows.length })}</span>
      </div>
      <label className="search-field">
        <span>{t("history.searchLabel")}</span>
        <input
          type="search"
          value={query}
          placeholder={t("history.searchPlaceholder")}
          onChange={(event) => setQuery(event.currentTarget.value)}
        />
      </label>
      <div className="filter-chips" role="group" aria-label={t("history.serviceFilter")}>
        <button type="button" aria-pressed={service === "all"} onClick={() => setService("all")}>
          {t("history.allServices")}
        </button>
        {services.map((item) => (
          <button
            type="button"
            aria-pressed={service === item.id}
            onClick={() => setService(item.id)}
            key={item.id}
          >
            {t(item.key)}
          </button>
        ))}
      </div>
      {loading ? <p>{t("common.loading")}</p> : null}
      {failed ? <p role="alert">{t("history.loadFailed")}</p> : null}
      {!loading && !failed && rows.length === 0 ? (
        <p className="empty-state">{t("history.empty")}</p>
      ) : null}
      {rows.length > 0 ? (
        <ul className="history-list">
          {rows.map((row) => {
            const rowService = taskById.get(row.task_id)?.service;
            return (
              <li key={row.task_id}>
                <button type="button" onClick={() => open(row)}>
                  <span className="history-file">{row.file_name}</span>
                  {row.snippet ? <span className="history-snippet">{row.snippet}</span> : null}
                  <span className="history-meta">
                    {rowService ? t(services.find(({ id }) => id === rowService)?.key ?? "") : null}
                    {rowService ? " · " : null}
                    {formatDate(new Date(row.created_at * 1000), i18n.resolvedLanguage ?? i18n.language)}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      ) : null}
    </div>
  );
}
