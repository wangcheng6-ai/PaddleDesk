import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { save } from "@tauri-apps/plugin-dialog";

import { OriginalPane } from "../components/OriginalPane";
import { ResultPane } from "../components/ResultPane";
import {
  exportResult,
  getResult,
  getTaskSource,
  type ExportFormat,
  type RecognitionResult,
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
  const taskId = useApp((state) => state.selectedTaskId);
  const task = useApp((state) =>
    state.tasks.find(({ id }) => id === state.selectedTaskId),
  );
  const [result, setResult] = useState<RecognitionResult | null>(null);
  const [sourceBytes, setSourceBytes] = useState<Uint8Array | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const [exportFailed, setExportFailed] = useState(false);

  useEffect(() => {
    if (!taskId) return;
    let active = true;
    setLoading(true);
    setLoadFailed(false);
    setSourceBytes(null);
    const isPdf = /\.pdf$/i.test(task?.input_path ?? "");
    void Promise.all([getResult(taskId), isPdf ? getTaskSource(taskId) : null]).then(
      ([next, bytes]) => {
        if (active) {
          setResult(next);
          setSourceBytes(bytes ? Uint8Array.from(bytes) : null);
          setLoading(false);
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
  }, [task?.input_path, taskId]);

  const runExport = async (format: ExportFormat, blockId?: string) => {
    if (!taskId) return;
    const source = task?.input_path?.split(/[\\/]/).pop() ?? "result";
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

  if (!taskId) return <p className="empty-state">{t("viewer.noSelection")}</p>;
  if (loading) return <p>{t("common.loading")}</p>;
  if (loadFailed) return <p role="alert">{t("viewer.loadFailed")}</p>;
  if (!result) return <p className="empty-state">{t("viewer.noResult")}</p>;

  return (
    <div className="viewer-view">
      <div className="viewer-title">
        <h1>{t("viewTitles.viewer")}</h1>
        <span>{task?.input_path?.split(/[\\/]/).pop()}</span>
      </div>
      {exportFailed ? <p role="alert">{t("viewer.exportFailed")}</p> : null}
      <div className="viewer-grid">
        <OriginalPane inputPath={task?.input_path ?? ""} result={result} sourceBytes={sourceBytes} />
        <ResultPane result={result} onExport={runExport} />
      </div>
    </div>
  );
}
