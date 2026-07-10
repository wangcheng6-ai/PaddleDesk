import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const {
  getCurrentWebviewMock,
  invokeMock,
  onDragDropEventMock,
  openMock,
} = vi.hoisted(() => ({
  getCurrentWebviewMock: vi.fn(),
  invokeMock: vi.fn(),
  onDragDropEventMock: vi.fn(),
  openMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: getCurrentWebviewMock,
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openMock }));

import { initI18n } from "../i18n";
import { useApp, type TaskSummary } from "../stores/app";
import { Home } from "../views/Home";

const tasks: TaskSummary[] = [1, 6, 2, 5, 3, 4].map((created_at) => ({
  id: `task-${created_at}`,
  service: "vl16",
  status: "done",
  input_path: `C:/docs/task-${created_at}.png`,
  progress_page: 1,
  total_pages: 1,
  created_at,
}));

beforeEach(async () => {
  invokeMock.mockReset().mockImplementation(async (command) => {
    if (command === "get_settings") return { language: "zh-CN" };
    if (command === "list_tasks") return tasks;
    throw new Error(`unexpected command: ${command}`);
  });
  openMock.mockReset();
  onDragDropEventMock.mockReset().mockResolvedValue(vi.fn());
  getCurrentWebviewMock.mockReset().mockReturnValue({
    onDragDropEvent: onDragDropEventMock,
  });
  useApp.setState({
    view: "home",
    service: "vl16",
    tasks: [],
    taskRevision: 0,
    taskRevisions: {},
    taskFieldRevisions: {},
    taskSnapshotRequest: 0,
    taskSnapshotApplied: 0,
    selectedTaskId: null,
  });
  await initI18n();
});

afterEach(cleanup);

test("shows the newest five tasks and opens the double-clicked result", async () => {
  render(<Home />);

  const recent = await screen.findByRole("list", { name: "最近任务" });
  const rows = within(recent).getAllByRole("listitem");
  expect(rows).toHaveLength(5);
  expect(rows.map((row) => within(row).getByRole("button").textContent)).toEqual([
    expect.stringContaining("task-6.png"),
    expect.stringContaining("task-5.png"),
    expect.stringContaining("task-4.png"),
    expect.stringContaining("task-3.png"),
    expect.stringContaining("task-2.png"),
  ]);
  expect(within(recent).queryByText("task-1.png")).not.toBeInTheDocument();

  fireEvent.doubleClick(within(rows[0]).getByRole("button"));
  expect(useApp.getState().selectedTaskId).toBe("task-6");
  expect(useApp.getState().view).toBe("viewer");
  expect(invokeMock).toHaveBeenCalledWith("list_tasks", { status: null });
  expect(screen.getByText("文件将上传至百度云端识别。")).toBeInTheDocument();
});

test("opens a recent task from the keyboard", async () => {
  render(<Home />);
  const recent = await screen.findByRole("list", { name: "最近任务" });

  fireEvent.keyDown(within(recent).getAllByRole("button")[0], { key: "Enter" });

  expect(useApp.getState().selectedTaskId).toBe("task-6");
  expect(useApp.getState().view).toBe("viewer");
});

test("renders three service cards bound to the shared service selection", async () => {
  render(<Home />);
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("list_tasks", { status: null }));

  const services = screen.getByRole("group", { name: "选择服务" });
  expect(within(services).getAllByRole("button")).toHaveLength(3);
  fireEvent.click(within(services).getByRole("button", { name: /PP-OCRv6/ }));

  expect(useApp.getState().service).toBe("pp_ocr_v6");
});

test("refreshes list metadata after creating tasks", async () => {
  let listCalls = 0;
  invokeMock.mockImplementation(async (command, args) => {
    if (command === "list_tasks") {
      listCalls += 1;
      return listCalls === 1
        ? []
        : [
            {
              id: "created",
              service: "vl16",
              status: "pending",
              input_path: "C:/docs/created.png",
              created_at: 7,
            },
          ];
    }
    if (command === "create_tasks") return ["created"];
    throw new Error(`unexpected command: ${command} ${JSON.stringify(args)}`);
  });
  openMock.mockResolvedValue(["C:/docs/created.png"]);
  render(<Home />);
  await screen.findByText("还没有任务。选择或拖入文件即可开始。");

  fireEvent.click(screen.getByRole("button", { name: "选择文件" }));

  await screen.findByText("created.png");
  expect(listCalls).toBe(2);
  expect(useApp.getState().tasks[0]).toEqual(
    expect.objectContaining({
      id: "created",
      input_path: "C:/docs/created.png",
    }),
  );
});

test("does not report task creation as failed when only the follow-up refresh fails", async () => {
  let listCalls = 0;
  invokeMock.mockImplementation(async (command) => {
    if (command === "list_tasks") {
      listCalls += 1;
      if (listCalls === 1) return [];
      throw new Error("refresh unavailable");
    }
    if (command === "create_tasks") return ["created"];
    throw new Error(`unexpected command: ${command}`);
  });
  openMock.mockResolvedValue(["C:/docs/created.png"]);
  render(<Home />);
  await screen.findByText("还没有任务。选择或拖入文件即可开始。");

  fireEvent.click(screen.getByRole("button", { name: "选择文件" }));

  expect(
    await screen.findByText("任务已创建，但列表刷新失败。请前往任务队列查看。"),
  ).toBeInTheDocument();
  expect(screen.queryByText("无法添加文件。")).not.toBeInTheDocument();
  expect(
    invokeMock.mock.calls.filter(([command]) => command === "create_tasks"),
  ).toHaveLength(1);
});

test("clears an initial load error after a successful post-create refresh", async () => {
  let listCalls = 0;
  invokeMock.mockImplementation(async (command) => {
    if (command === "list_tasks") {
      listCalls += 1;
      if (listCalls === 1) throw new Error("initial load failed");
      return [
        {
          id: "created",
          service: "vl16",
          status: "pending",
          input_path: "C:/docs/created.png",
          created_at: 8,
        },
      ];
    }
    if (command === "create_tasks") return ["created"];
    throw new Error(`unexpected command: ${command}`);
  });
  openMock.mockResolvedValue(["C:/docs/created.png"]);
  render(<Home />);
  expect(await screen.findByText("无法加载最近任务。")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "选择文件" }));

  expect(await screen.findByText("created.png")).toBeInTheDocument();
  expect(screen.queryByText("无法加载最近任务。")).not.toBeInTheDocument();
});
