import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { TaskSummary } from "../stores/app";

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
  kind: "Auth" | "Quota" | "Network" | "Server" | "Parse";
  message: string;
}

export const getSettings = () =>
  invoke<Record<string, string>>("get_settings");

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
