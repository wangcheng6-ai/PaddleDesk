import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { initI18n } from "../i18n";
import { useApp, type TaskSummary } from "../stores/app";
import { Queue } from "../views/Queue";

const failed: TaskSummary = {
  id: "failed",
  service: "vl16",
  status: "failed",
  input_path: "C:/docs/failed.pdf",
  error_kind: "nEtWoRk",
  error_msg: "upstream socket timeout",
  created_at: 2,
  batch_id: "batch-1",
};
const processing: TaskSummary = {
  id: "processing",
  service: "vl16",
  status: "processing",
  input_path: "C:/docs/processing.png",
  progress_page: 2,
  total_pages: 4,
  created_at: 1,
  batch_id: "batch-1",
};
const completedBatchTask: TaskSummary = {
  id: "batch-done",
  service: "vl16",
  status: "done",
  input_path: "C:/docs/done.png",
  created_at: 3,
  batch_id: "batch-1",
};
const zeroTotal: TaskSummary = {
  id: "zero-total",
  service: "vl16",
  status: "processing",
  input_path: "C:/docs/zero-total.pdf",
  progress_page: 0,
  total_pages: 0,
  created_at: 0,
};

beforeEach(async () => {
  invokeMock.mockReset().mockImplementation(async (command) => {
    if (command === "get_settings") return { language: "zh-CN" };
    if (command === "list_tasks") {
      return [failed, processing, zeroTotal, completedBatchTask];
    }
    if (
      command === "retry_task" ||
      command === "cancel_task" ||
      command === "dismiss_failed_task"
    ) return null;
    throw new Error(`unexpected command: ${command}`);
  });
  useApp.setState({
    view: "queue",
    service: "vl16",
    tasks: [],
    selectedTaskId: null,
  });
  await initI18n();
});

afterEach(cleanup);

test("localizes failed tasks, isolates raw detail, and wires retry and cancel", async () => {
  render(<Queue />);

  const list = await screen.findByRole("list", { name: "任务队列" });
  expect(screen.getByText("批量任务 2 / 3")).toBeInTheDocument();
  expect(screen.getByRole("progressbar", { name: "" })).toHaveAttribute(
    "max",
    "3",
  );
  const failedRow = within(list)
    .getByText("failed.pdf")
    .closest<HTMLElement>('[role="listitem"]');
  expect(failedRow).not.toBeNull();
  expect(within(failedRow!).getByText("网络连接失败。")).toBeInTheDocument();
  expect(
    within(failedRow!).getByText("检查网络或代理设置后重试。"),
  ).toBeInTheDocument();
  const details = within(failedRow!).getByText("技术详情").closest("details");
  expect(details).not.toBeNull();
  expect(within(details!).getByText("upstream socket timeout")).toBeInTheDocument();

  fireEvent.click(within(failedRow!).getByRole("button", { name: "重试" }));
  const processingRow = within(list)
    .getByText("processing.png")
    .closest<HTMLElement>('[role="listitem"]');
  fireEvent.click(within(processingRow!).getByRole("button", { name: "取消" }));

  await waitFor(() => {
    expect(invokeMock).toHaveBeenCalledWith("retry_task", { id: "failed" });
    expect(invokeMock).toHaveBeenCalledWith("cancel_task", { id: "processing" });
  });
  expect(within(processingRow!).getByRole("progressbar")).toHaveAttribute(
    "aria-valuenow",
    "2",
  );
  expect(useApp.getState().tasks.find(({ id }) => id === "failed")?.status).toBe(
    "failed",
  );
  expect(useApp.getState().tasks.find(({ id }) => id === "processing")).toBeUndefined();
  const zeroRow = within(list)
    .getByText("zero-total.pdf")
    .closest<HTMLElement>('[role="listitem"]');
  expect(within(zeroRow!).queryByRole("progressbar")).not.toBeInTheDocument();
  expect(zeroRow).not.toHaveTextContent("NaN");
});

test("maps all semantic error kinds case-insensitively", async () => {
  const errors: TaskSummary[] = [
    ["auth", "Auth", null],
    ["quota", "quota", null],
    ["rate", "rAtElImItEd", "retry after 5s"],
    ["input", "InvalidInput", "unsupported file"],
    ["network", "nEtWoRk", "socket timeout"],
    ["server", "Server", "gateway 503"],
    ["parse", "PARSE", "missing field"],
  ].map(([id, error_kind, error_msg], created_at) => ({
    id: String(id),
    service: "vl16",
    status: "failed",
    input_path: `C:/docs/${id}.pdf`,
    error_kind: String(error_kind),
    error_msg: error_msg ? String(error_msg) : null,
    created_at,
  }));
  invokeMock.mockImplementation(async (command) => {
    if (command === "list_tasks") return errors;
    throw new Error(`unexpected command: ${command}`);
  });

  render(<Queue />);
  const list = await screen.findByRole("list", { name: "任务队列" });

  for (const message of [
    "Token 无效或已过期。",
    "今日额度已用尽。",
    "服务当前请求较多。",
    "文件或识别参数不符合服务要求。",
    "网络连接失败。",
    "服务暂时不可用。",
    "无法解析识别结果。",
  ]) {
    expect(within(list).getByText(message)).toBeInTheDocument();
  }
  expect(within(list).getAllByText("技术详情")).toHaveLength(5);
});

test("uses the unknown copy instead of pretending a future error is Parse", async () => {
  invokeMock.mockImplementation(async (command) => {
    if (command === "list_tasks") {
      return [
        {
          ...failed,
          error_kind: "future_error",
          error_msg: "future raw detail",
        },
      ];
    }
    throw new Error(`unexpected command: ${command}`);
  });

  render(<Queue />);
  const list = await screen.findByRole("list", { name: "任务队列" });

  expect(within(list).getByText("识别任务失败。")).toBeInTheDocument();
  expect(
    within(list).getByText("发生了未知错误，请查看技术详情后重试。"),
  ).toBeInTheDocument();
  expect(within(list).queryByText("无法解析识别结果。")).not.toBeInTheDocument();
  expect(within(list).getByText("future raw detail")).toBeInTheDocument();
});
