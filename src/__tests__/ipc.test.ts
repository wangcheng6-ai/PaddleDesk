import { beforeEach, expect, test, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import {
  cancelTask,
  createTasks,
  getSettings,
  listTasks,
  onQueueEvent,
  onUsageUpdated,
  retryTask,
} from "../lib/ipc";
import { useApp } from "../stores/app";

beforeEach(() => {
  invokeMock.mockReset();
  listenMock.mockReset();
  useApp.setState({ view: "home", service: "vl16", tasks: [] });
});

test("getSettings preserves the Record returned by Tauri", async () => {
  const settings = { language: "zh-CN", theme: "system" };
  invokeMock.mockResolvedValue(settings);

  await expect(getSettings()).resolves.toBe(settings);
  expect(invokeMock).toHaveBeenCalledWith("get_settings");
});

test("task command wrappers use the current Rust argument names exactly", async () => {
  invokeMock
    .mockResolvedValueOnce({ batch_id: "batch-1", task_ids: ["task-1"] })
    .mockResolvedValueOnce([])
    .mockResolvedValueOnce(null)
    .mockResolvedValueOnce(null);

  await expect(createTasks(["C:/docs/a.png"], "pp_ocr_v6")).resolves.toEqual({
    batch_id: "batch-1",
    task_ids: ["task-1"],
  });
  await expect(listTasks(null)).resolves.toEqual([]);
  await cancelTask("task-1");
  await retryTask("task-1");

  expect(invokeMock.mock.calls).toEqual([
    ["create_tasks", {
      paths: ["C:/docs/a.png"],
      service: "pp_ocr_v6",
      options: { lang: null },
    }],
    ["list_tasks", { status: null }],
    ["cancel_task", { id: "task-1" }],
    ["retry_task", { id: "task-1" }],
  ]);
});

test("subscribes to submitted and four task lifecycle events", async () => {
  const updates: unknown[] = [];
  const unlisteners = Array.from({ length: 5 }, () => vi.fn());
  listenMock.mockImplementation(async () => {
    return unlisteners[listenMock.mock.calls.length - 1];
  });

  const cleanup = await onQueueEvent((update) => updates.push(update));
  const handlers = Object.fromEntries(
    listenMock.mock.calls.map(([name, handler]) => [name, handler]),
  );

  handlers["task:submitted"]({
    payload: { task: { id: "0", service: "vl16", status: "pending" } },
  });
  handlers["task:progress"]({
    payload: { id: "1", stage: "processing", page: 2, total: 4 },
  });
  handlers["task:done"]({ payload: { id: "1" } });
  handlers["task:failed"]({
    payload: { id: "2", kind: "Network", message: "timeout" },
  });
  handlers["task:canceled"]({ payload: { id: "3" } });

  expect(listenMock.mock.calls.map(([name]) => name)).toEqual([
    "task:submitted",
    "task:progress",
    "task:done",
    "task:failed",
    "task:canceled",
  ]);
  expect(updates).toEqual([
    { id: "0", service: "vl16", status: "pending" },
    {
      id: "1",
      status: "processing",
      progress_page: 2,
      total_pages: 4,
      error_kind: null,
      error_msg: null,
    },
    { id: "1", status: "done", error_kind: null, error_msg: null },
    {
      id: "2",
      status: "failed",
      error_kind: "Network",
      error_msg: "timeout",
    },
    { id: "3", status: "canceled", error_kind: null, error_msg: null },
  ]);

  cleanup();
  for (const unlisten of unlisteners) {
    expect(unlisten).toHaveBeenCalledOnce();
  }
});

test("real event sequences clear stale failure details through upsertTask", async () => {
  listenMock.mockImplementation(async () => vi.fn());
  const cleanup = await onQueueEvent(useApp.getState().upsertTask);
  const handlers = Object.fromEntries(
    listenMock.mock.calls.map(([name, handler]) => [name, handler]),
  );

  handlers["task:failed"]({
    payload: { id: "retry", kind: "Network", message: "timeout" },
  });
  handlers["task:progress"]({
    payload: { id: "retry", stage: "processing", page: 1, total: 2 },
  });
  handlers["task:done"]({ payload: { id: "retry" } });
  handlers["task:failed"]({
    payload: { id: "cancel", kind: "Server", message: "upstream" },
  });
  handlers["task:canceled"]({ payload: { id: "cancel" } });

  expect(useApp.getState().tasks).toEqual([
    expect.objectContaining({
      id: "retry",
      status: "done",
      error_kind: null,
      error_msg: null,
    }),
    {
      id: "cancel",
      status: "canceled",
      error_kind: null,
      error_msg: null,
    },
  ]);
  cleanup();
});

test("cleans completed subscriptions before rejecting a later listen failure", async () => {
  const firstUnlisten = vi.fn();
  const failure = new Error("listen failed");
  listenMock
    .mockResolvedValueOnce(firstUnlisten)
    .mockRejectedValueOnce(failure);

  await expect(onQueueEvent(vi.fn())).rejects.toBe(failure);
  expect(firstUnlisten).toHaveBeenCalledOnce();
});

test("forwards live usage events", async () => {
  const callback = vi.fn();
  const unlisten = vi.fn();
  listenMock.mockResolvedValue(unlisten);

  const cleanup = await onUsageUpdated(callback);
  const handler = listenMock.mock.calls[0][1];
  handler({ payload: { today_pages: 7 } });

  expect(listenMock).toHaveBeenCalledWith("usage:updated", expect.any(Function));
  expect(callback).toHaveBeenCalledWith(7);
  cleanup();
  expect(unlisten).toHaveBeenCalledOnce();
});
