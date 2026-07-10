import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { save } from "@tauri-apps/plugin-dialog";

import { useConfirm } from "../components/ConfirmDialog";
import { OriginalPane } from "../components/OriginalPane";
import { ResultPane } from "../components/ResultPane";
import {
  clearResults,
  deleteResult,
  exportResult,
  getResult,
  getTaskSource,
  listResults,
  type ExportFormat,
  type RecognitionResult,
  type ResultSummary,
} from "../lib/ipc";
import { useApp } from "../stores/app";

const extension: Record<ExportFormat, string> = {
  md: "md",
  json: "json",
  txt: "txt",
  csv: "csv",
};

export function Viewer() {
  const { t } = useTranslation();
  const { confirm, confirmDialog } = useConfirm();
  const service = useApp((state) => state.service);
  const taskId = useApp((state) => state.selectedTaskId);
  const setSelectedTaskId = useApp((state) => state.setSelectedTaskId);
  const removeTask = useApp((state) => state.removeTask);
  const [summaries, setSummaries] = useState<ResultSummary[]>([]);
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState(false);
  const [result, setResult] = useState<RecognitionResult | null>(null);
  const [sourceBytes, setSourceBytes] = useState<Uint8Array | null>(null);
  const [sourceUnavailable, setSourceUnavailable] = useState(false);
  const [loadingList, setLoadingList] = useState(true);
  const [loadingResult, setLoadingResult] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const [exportFailed, setExportFailed] = useState(false);
  const previousService = useRef(service);

  useEffect(() => {
    if (previousService.current === service) return;
    previousService.current = service;
    setQuery("");
    setSummaries([]);
    setResult(null);
    setSourceBytes(null);
    setSourceUnavailable(false);
    setSelectedTaskId(null);
    setLoadingList(true);
  }, [service, setSelectedTaskId]);

  useEffect(() => {
    let active = true;
    const timer = window.setTimeout(() => {
      void listResults(service, query.trim()).then(
        (rows) => {
          if (!active) return;
          setSummaries(rows);
          setLoadingList(false);
          setLoadFailed(false);
          const current = useApp.getState().selectedTaskId;
          if (!current || !rows.some((row) => row.task_id === current)) {
            setSelectedTaskId(rows[0]?.task_id ?? null);
          }
        },
        () => {
          if (active) {
            setLoadingList(false);
            setLoadFailed(true);
          }
        },
      );
    }, query.trim() ? 250 : 0);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [query, service, setSelectedTaskId]);

  useEffect(() => {
    if (!taskId) {
      setResult(null);
      setSourceBytes(null);
      return;
    }
    let active = true;
    setLoadingResult(true);
    setLoadFailed(false);
    setSourceBytes(null);
    setSourceUnavailable(false);
    void getResult(taskId).then(
      (next) => {
        if (active) {
          setResult(next);
          setLoadingResult(false);
        }
      },
      () => {
        if (active) {
          setLoadingResult(false);
          setLoadFailed(true);
        }
      },
    );
    void getTaskSource(taskId).then(
      (bytes) => {
        if (active) setSourceBytes(new Uint8Array(bytes));
      },
      () => {
        if (active) setSourceUnavailable(true);
      },
    );
    return () => {
      active = false;
    };
  }, [taskId]);

  const selected = useMemo(
    () => summaries.find((summary) => summary.task_id === taskId),
    [summaries, taskId],
  );

  const runExport = async (format: ExportFormat, blockId?: string) => {
    if (!taskId) return;
    const source = selected?.file_name ?? "result";
    const base = source.replace(/\.[^.]+$/, "");
    setExportFailed(false);
    try {
      const path = await save({
        defaultPath: `${base}.${extension[format]}`,
        filters: [
          {
            name: t(`viewer.formats.${format}`),
            extensions: [extension[format]],
          },
        ],
      });
      if (!path) return;
      await exportResult(taskId, format, path, blockId);
    } catch {
      setExportFailed(true);
    }
  };

  const removeSelected = async () => {
    if (!taskId || !(await confirm(t("viewer.confirmDelete")))) return;
    try {
      await deleteResult(taskId);
      removeTask(taskId);
      setSummaries((rows) => rows.filter((row) => row.task_id !== taskId));
      setSelectedTaskId(summaries.find((row) => row.task_id !== taskId)?.task_id ?? null);
    } catch {
      setLoadFailed(true);
    }
  };

  const clearCurrent = async () => {
    if (!(await confirm(t("viewer.confirmClear")))) return;
    try {
      await clearResults(service);
      summaries.forEach((summary) => removeTask(summary.task_id));
      setSummaries([]);
      setSelectedTaskId(null);
    } catch {
      setLoadFailed(true);
    }
  };

  return (
    <div className="results-workspace" data-collapsed={collapsed}>
      {confirmDialog}
      <aside className="results-list-panel" aria-label={t("viewer.resultList")}>
        <div className="results-list-heading">
          <h1>{t("viewTitles.viewer")}</h1>
          <button
            className="collapse-toggle"
            type="button"
            title={collapsed ? t("actions.expand") : t("actions.collapse")}
            aria-label={collapsed ? t("actions.expand") : t("actions.collapse")}
            aria-expanded={!collapsed}
            onClick={() => setCollapsed((value) => !value)}
          >
            {collapsed ? "»" : "«"}
          </button>
        </div>
        {!collapsed ? (
          <>
            <label className="search-field">
              <span>{t("history.searchLabel")}</span>
              <input
                type="search"
                value={query}
                placeholder={t("history.searchPlaceholder")}
                onChange={(event) => setQuery(event.currentTarget.value)}
              />
            </label>
            <button className="danger-button" type="button" onClick={() => void clearCurrent()}>
              {t("viewer.clearResults")}
            </button>
            {loadingList ? <p>{t("common.loading")}</p> : null}
            {!loadingList && summaries.length === 0 ? (
              <p className="empty-state">{t("history.empty")}</p>
            ) : null}
            <ul className="result-summary-list">
              {summaries.map((summary) => (
                <li key={summary.task_id}>
                  <button
                    type="button"
                    aria-current={taskId === summary.task_id ? "true" : undefined}
                    onClick={() => setSelectedTaskId(summary.task_id)}
                  >
                    <strong title={summary.file_name}>{summary.file_name}</strong>
                    {summary.temporary ? <small>{t("viewer.sessionOnly")}</small> : null}
                    <span>{summary.snippet}</span>
                  </button>
                </li>
              ))}
            </ul>
          </>
        ) : null}
      </aside>

      <section className="result-detail">
        {loadFailed ? <p role="alert">{t("viewer.loadFailed")}</p> : null}
        {exportFailed ? <p role="alert">{t("viewer.exportFailed")}</p> : null}
        {!taskId ? <p className="empty-state">{t("viewer.noSelection")}</p> : null}
        {loadingResult ? <p>{t("common.loading")}</p> : null}
        {taskId && !loadingResult && !result ? (
          <p className="empty-state">{t("viewer.noResult")}</p>
        ) : null}
        {taskId && result ? (
          <>
            <div className="viewer-title">
              <div>
                <h2 title={selected?.file_name}>{selected?.file_name}</h2>
                {selected?.temporary ? <span>{t("viewer.sessionOnly")}</span> : null}
              </div>
              <button className="danger-button" type="button" onClick={() => void removeSelected()}>
                {t("actions.delete")}
              </button>
            </div>
            <div className="viewer-grid">
              <OriginalPane
                key={taskId}
                inputPath={selected?.file_name ?? ""}
                result={result}
                sourceBytes={sourceBytes}
                sourceUnavailable={sourceUnavailable}
              />
              <ResultPane result={result} onExport={runExport} />
            </div>
          </>
        ) : null}
      </section>
    </div>
  );
}
