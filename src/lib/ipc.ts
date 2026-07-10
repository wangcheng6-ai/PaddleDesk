import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { TaskSummary } from "../stores/app";
import type { ServiceId } from "../stores/app";

export type BlockKind = "text" | "table" | "formula" | "seal" | "chart";
export type ExportFormat = "md" | "json" | "txt" | "csv";

export interface RecognitionBlock {
  id: string;
  kind: BlockKind;
  bbox: [number, number, number, number] | null;
  content: string;
}

export interface RecognitionPage {
  width: number;
  height: number;
  blocks: RecognitionBlock[];
}

export interface RecognitionResult {
  markdown: string;
  page_count: number;
  pages: RecognitionPage[];
}

export interface ResultSummary {
  task_id: string;
  service: ServiceId;
  file_name: string;
  snippet: string;
  created_at: number;
  temporary: boolean;
}

export type HistoryRow = ResultSummary;

export interface CreatedBatch {
  batch_id: string;
  task_ids: string[];
}

export interface CredentialStatus {
  configured: boolean;
  last_four: string | null;
}

export interface UsageRow {
  date: string;
  service: ServiceId;
  pages: number;
}

interface ProgressPayload {
  id: string;
  stage: "uploading" | "processing";
  page: number;
  total: number;
}

interface IdPayload {
  id: string;
}

interface SubmittedPayload {
  task: TaskSummary;
}

interface UsageUpdatedPayload {
  today_pages: number;
}

interface CaptureDonePayload {
  task_id: string;
}

interface FailedPayload extends IdPayload {
  kind:
    | "Auth"
    | "Quota"
    | "RateLimited"
    | "InvalidInput"
    | "Network"
    | "Server"
    | "Parse"
    | "Internal";
  message: string;
}

export const getSettings = () =>
  invoke<Record<string, string>>("get_settings");

export const setSettings = (map: Record<string, string>) =>
  invoke<void>("set_settings", { map });

export const validateToken = (token: string) =>
  invoke<boolean>("validate_token", { token });

export const getCredentialStatus = () =>
  invoke<CredentialStatus>("get_credential_status");

export const revealToken = () => invoke<string>("reveal_token");

export const deleteToken = () => invoke<void>("delete_token");

export const getScreenshotHotkey = () =>
  invoke<string>("get_screenshot_hotkey");

export const setScreenshotHotkey = (shortcut: string) =>
  invoke<string>("set_screenshot_hotkey", { shortcut });

export const createTasks = (paths: string[], service: ServiceId) =>
  invoke<CreatedBatch>("create_tasks", {
    paths,
    service,
    options: { lang: null },
  });

export const createTaskFromClipboard = (service: ServiceId) =>
  invoke<string>("create_task_from_clipboard", { service });

export const startCapture = () => invoke<string>("start_capture");

export const listTasks = (status: string | null) =>
  invoke<TaskSummary[]>("list_tasks", { status });

export const searchHistory = (query: string) =>
  invoke<HistoryRow[]>("search_history", { query });

export const listResults = (service: ServiceId, query = "") =>
  invoke<ResultSummary[]>("list_results", { service, query });

export const deleteResult = (taskId: string) =>
  invoke<void>("delete_result", { taskId });

export const clearResults = (service: ServiceId) =>
  invoke<void>("clear_results", { service });

export const dismissFailedTask = (taskId: string) =>
  invoke<void>("dismiss_failed_task", { taskId });

export const getUsage = (days: number) =>
  invoke<UsageRow[]>("get_usage", { days });

export const cancelTask = (id: string) =>
  invoke<void>("cancel_task", { id });

export const retryTask = (id: string) => invoke<void>("retry_task", { id });

export const getResult = (taskId: string) =>
  invoke<RecognitionResult | null>("get_result", { taskId });

export const getTaskSource = (taskId: string) =>
  invoke<ArrayBuffer>("get_task_source", { taskId });

export const exportResult = (
  taskId: string,
  format: ExportFormat,
  path: string,
  blockId?: string,
) =>
  invoke<string>("export_result", {
    taskId,
    format,
    path,
    blockId: blockId ?? null,
  });

export async function onQueueEvent(
  callback: (update: TaskSummary) => void,
): Promise<UnlistenFn> {
  const unlisteners: UnlistenFn[] = [];

  try {
    unlisteners.push(
      await listen<SubmittedPayload>("task:submitted", ({ payload }) =>
        callback(payload.task),
      ),
    );
    unlisteners.push(
      await listen<ProgressPayload>("task:progress", ({ payload }) =>
        callback({
          id: payload.id,
          status: payload.stage,
          progress_page: payload.page,
          total_pages: payload.total,
          error_kind: null,
          error_msg: null,
        }),
      ),
    );
    unlisteners.push(
      await listen<IdPayload>("task:done", ({ payload }) =>
        callback({
          id: payload.id,
          status: "done",
          error_kind: null,
          error_msg: null,
        }),
      ),
    );
    unlisteners.push(
      await listen<FailedPayload>("task:failed", ({ payload }) =>
        callback({
          id: payload.id,
          status: "failed",
          error_kind: payload.kind,
          error_msg: payload.message,
        }),
      ),
    );
    unlisteners.push(
      await listen<IdPayload>("task:canceled", ({ payload }) =>
        callback({
          id: payload.id,
          status: "canceled",
          error_kind: null,
          error_msg: null,
        }),
      ),
    );
  } catch (error) {
    unlisteners.forEach((unlisten) => unlisten());
    throw error;
  }

  let cleaned = false;
  return () => {
    if (cleaned) return;
    cleaned = true;
    unlisteners.forEach((unlisten) => unlisten());
  };
}

export async function onUsageUpdated(
  callback: (todayPages: number) => void,
): Promise<UnlistenFn> {
  return listen<UsageUpdatedPayload>("usage:updated", ({ payload }) =>
    callback(payload.today_pages),
  );
}

export async function onCaptureDone(
  callback: (taskId: string) => void,
): Promise<UnlistenFn> {
  return listen<CaptureDonePayload>("capture:done", ({ payload }) =>
    callback(payload.task_id),
  );
}
