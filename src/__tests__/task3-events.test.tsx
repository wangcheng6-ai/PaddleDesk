import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import App from "../App";
import { initI18n } from "../i18n";
import { useApp } from "../stores/app";

beforeEach(async () => {
  invokeMock.mockReset().mockImplementation(async (command) => {
    if (command === "get_settings") return { language: "zh-CN" };
    if (command === "get_usage") return [];
    if (command === "list_tasks") {
      return [
        {
          id: "live",
          service: "vl16",
          status: "processing",
          input_path: "C:/docs/live.pdf",
          created_at: 1,
        },
      ];
    }
    throw new Error(`unexpected command: ${command}`);
  });
  listenMock.mockReset().mockImplementation(async () => vi.fn());
  useApp.setState({
    view: "queue",
    service: "vl16",
    tasks: [],
    selectedTaskId: null,
    todayPages: { vl16: 0, pp_ocr_v6: 0, structure_v3: 0 },
  });
  await initI18n();
});

afterEach(cleanup);

test("the App's single task subscription updates the queue through the shared store", async () => {
  render(<App />);
  await screen.findByText("live.pdf");
  await waitFor(() => expect(listenMock).toHaveBeenCalledTimes(6));
  const handlers = Object.fromEntries(
    listenMock.mock.calls.map(([name, handler]) => [name, handler]),
  );

  act(() => {
    handlers["task:failed"]({
      payload: { id: "live", kind: "Server", message: "gateway 503" },
    });
  });

  const list = screen.getByRole("list", { name: "任务队列" });
  expect(within(list).getByText("服务暂时不可用。")).toBeInTheDocument();
  expect(within(list).getByText("gateway 503")).toBeInTheDocument();

  act(() => handlers["task:done"]({ payload: { id: "live" } }));
  expect(
    within(list).getByText("已完成", { selector: ".status-pill" }),
  ).toBeInTheDocument();
  expect(within(list).queryByText("gateway 503")).not.toBeInTheDocument();
  expect(listenMock).toHaveBeenCalledTimes(6);
});
