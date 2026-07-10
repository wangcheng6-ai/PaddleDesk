import { useTranslation } from "react-i18next";

import type { TaskSummary } from "../stores/app";

const serviceKeys = {
  vl16: "services.vl16",
  pp_ocr_v6: "services.ppOcrV6",
  structure_v3: "services.structureV3",
} as const;

const errorKinds: Record<string, string> = {
  auth: "auth",
  quota: "quota",
  ratelimited: "rate_limited",
  invalidinput: "invalid_input",
  network: "network",
  server: "server",
  parse: "parse",
  internal: "internal",
};

interface TaskRowItemProps {
  task: TaskSummary;
  onOpen?: () => void;
  onRetry?: () => void;
  onCancel?: () => void;
  onDismiss?: () => void;
}

const fileName = (path = "") => path.split(/[\\/]/).pop() || path;

export function TaskRowItem({
  task,
  onOpen,
  onRetry,
  onCancel,
  onDismiss,
}: TaskRowItemProps) {
  const { t } = useTranslation();
  const status = task.status ?? "pending";
  const errorKind = (task.error_kind ?? "unknown")
    .toLowerCase()
    .replace(/[^a-z]/g, "");
  const errorKey = errorKinds[errorKind] ?? "unknown";
  const progress =
    status === "processing" && task.total_pages
      ? t("task.progress", {
          page: task.progress_page ?? 0,
          total: task.total_pages,
        })
      : t(`status.${status}`);
  const summary = (
    <>
      <span className="task-file">{fileName(task.input_path)}</span>
      <span className="task-meta">{progress}</span>
    </>
  );

  return (
    <li className="task-row" role="listitem">
      {onOpen ? (
        <button
          className="task-main task-open"
          type="button"
          onClick={onOpen}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              onOpen();
            }
          }}
        >
          {summary}
        </button>
      ) : (
        <div className="task-main">{summary}</div>
      )}
      {task.service && (
        <span className={`service-pill service-${task.service}`}>
          {t(serviceKeys[task.service])}
        </span>
      )}
      <span className={`status-pill status-${status}`}>{t(`status.${status}`)}</span>
      {status === "processing" && task.total_pages ? (
        <span
          className="task-progress"
          role="progressbar"
          aria-label={t("task.progressLabel", { name: fileName(task.input_path) })}
          aria-valuemin={0}
          aria-valuemax={task.total_pages}
          aria-valuenow={task.progress_page ?? 0}
        >
          <span
            style={{
              width: `${Math.min(
                100,
                ((task.progress_page ?? 0) / task.total_pages) * 100,
              )}%`,
            }}
          />
        </span>
      ) : null}
      {status === "failed" && (
        <div className="task-error">
          <strong>{t(`errors.${errorKey}.message`)}</strong>
          <span>{t(`errors.${errorKey}.suggestion`)}</span>
          {task.error_msg && (
            <details>
              <summary>{t("errors.technicalDetails")}</summary>
              <code>{task.error_msg}</code>
            </details>
          )}
        </div>
      )}
      <span className="task-actions">
        {onRetry && status === "failed" && (
          <button type="button" onClick={onRetry}>
            {t("actions.retry")}
          </button>
        )}
        {onCancel && !["done", "canceled", "failed"].includes(status) && (
          <button type="button" onClick={onCancel}>
            {t("actions.cancel")}
          </button>
        )}
        {onDismiss && status === "failed" && (
          <button type="button" onClick={onDismiss}>
            {t("actions.dismiss")}
          </button>
        )}
      </span>
    </li>
  );
}
