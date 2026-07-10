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

export interface HistoryRow {
  task_id: string;
  file_name: string;
  snippet: string;
  created_at: number;
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

interface FailedPayload extends IdPayload {
  kind:
    | "Auth"
    | "Quota"
    | "RateLimited"
    | "InvalidInput"
    | "Network"
    | "Server"
    | "Parse";
  message: string;
}

export const getSettings = () =>
  invoke<Record<string, string>>("get_settings");

export const setSettings = (map: Record<string, string>) =>
  invoke<void>("set_settings", { map });

export const validateToken = (token: string) =>
  invoke<boolean>("validate_token", { token });

export const createTasks = (paths: string[], service: ServiceId) =>
  invoke<string[]>("create_tasks", {
    paths,
    service,
    options: { lang: null },
  });

export const listTasks = (status: string | null) =>
  invoke<TaskSummary[]>("list_tasks", { status });

export const searchHistory = (query: string) =>
  invoke<HistoryRow[]>("search_history", { query });

export const getUsage = (days: number) =>
  invoke<UsageRow[]>("get_usage", { days });

export const cancelTask = (id: string) =>
  invoke<void>("cancel_task", { id });

export const retryTask = (id: string) => invoke<void>("retry_task", { id });

export const getResult = (taskId: string) =>
  invoke<RecognitionResult | null>("get_result", { taskId });

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
