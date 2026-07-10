import { StrictMode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { getCurrentWebviewMock, invokeMock, listenMock, onDragDropEventMock } =
  vi.hoisted(() => ({
    getCurrentWebviewMock: vi.fn(),
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
    onDragDropEventMock: vi.fn(),
  }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: getCurrentWebviewMock,
}));

import App from "../App";
import { initI18n } from "../i18n";
import { useApp } from "../stores/app";

beforeEach(async () => {
  invokeMock.mockReset().mockImplementation(async (command) =>
    command === "get_settings" ? { language: "zh-CN" } : [],
  );
  listenMock.mockReset().mockImplementation(async () => vi.fn());
  onDragDropEventMock.mockReset().mockImplementation(async () => vi.fn());
  getCurrentWebviewMock.mockReset().mockReturnValue({
    onDragDropEvent: onDragDropEventMock,
  });
  useApp.setState({
    view: "home",
    service: "vl16",
    tasks: [],
    todayPages: { vl16: 0, pp_ocr_v6: 0, structure_v3: 0 },
  });
  await initI18n();
});

afterEach(cleanup);

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
});

const flushPromises = async () => {
  for (let index = 0; index < 10; index += 1) await Promise.resolve();
};

test("renders the approved brand icon instead of a letter placeholder", () => {
  render(<App />);

  const brand = screen.getByText("PaddleDesk").closest(".brand");
  const icon = brand?.querySelector("img.brand-mark");

  expect(icon).toBeInstanceOf(HTMLImageElement);
  expect(icon?.getAttribute("src")).toContain("paddledesk-icon.png");
  expect(icon).toHaveAttribute("alt", "");
  expect(icon).toHaveAttribute("aria-hidden", "true");
  expect(brand?.querySelector("span.brand-mark")).toBeNull();
});

test("renders six semantic nav buttons and switches the current view", () => {
  render(<App />);

  const nav = screen.getByRole("navigation", { name: "主导航" });
  expect(within(nav).getAllByRole("button")).toHaveLength(6);
  expect(within(nav).getByRole("button", { name: "主页" })).toHaveAttribute(
    "aria-current",
    "page",
  );

  fireEvent.click(within(nav).getByRole("button", { name: "设置" }));

  expect(screen.getByRole("heading", { name: "设置" })).toBeInTheDocument();
  expect(within(nav).getByRole("button", { name: "设置" })).toHaveAttribute(
    "aria-current",
    "page",
  );
});

test("switches among the three service wire values", async () => {
  render(<App />);

  fireEvent.click(await screen.findByRole("button", { name: "PP-OCRv6" }));

  expect(useApp.getState().service).toBe("pp_ocr_v6");
  expect(screen.getByRole("button", { name: "PP-OCRv6" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});

test("waits for all task listeners before mounting the task view", async () => {
  const resolveListeners: Array<(unlisten: () => void) => void> = [];
  listenMock.mockImplementation(
    () =>
      new Promise<() => void>((resolve) => {
        resolveListeners.push(resolve);
      }),
  );

  render(<App />);
  await waitFor(() => expect(resolveListeners).toHaveLength(1));
  expect(invokeMock).not.toHaveBeenCalledWith("list_tasks", { status: null });
  expect(onDragDropEventMock).not.toHaveBeenCalled();

  for (let index = 0; index < 4; index += 1) {
    await act(async () => {
      resolveListeners[index](vi.fn());
      await flushPromises();
    });
    await waitFor(() => expect(resolveListeners).toHaveLength(index + 2));
    expect(invokeMock).not.toHaveBeenCalledWith("list_tasks", { status: null });
  }

  await act(async () => {
    resolveListeners[4](vi.fn());
    await flushPromises();
  });

  await waitFor(() =>
    expect(invokeMock).toHaveBeenCalledWith("list_tasks", { status: null }),
  );
  expect(onDragDropEventMock).toHaveBeenCalledOnce();
});

test("StrictMode and unmount eventually release every async listener", async () => {
  const unlisteners = Array.from({ length: 10 }, () => vi.fn());
  listenMock.mockImplementation(async () => {
    const unlisten = unlisteners[listenMock.mock.calls.length - 1];
    return unlisten;
  });

  const { unmount } = render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
  unmount();

  await waitFor(() => expect(listenMock).toHaveBeenCalledTimes(10));
  await waitFor(() => {
    for (const unlisten of unlisteners) {
      expect(unlisten).toHaveBeenCalledOnce();
    }
  });
});

test("shows a localized retryable alert when listener registration fails", async () => {
  vi.useFakeTimers();
  const unlisteners = Array.from({ length: 5 }, () => vi.fn());
  listenMock
    .mockRejectedValueOnce(new Error("listen unavailable"))
    .mockResolvedValueOnce(unlisteners[0])
    .mockResolvedValueOnce(unlisteners[1])
    .mockResolvedValueOnce(unlisteners[2])
    .mockResolvedValueOnce(unlisteners[3])
    .mockResolvedValueOnce(unlisteners[4]);

  const { unmount } = render(<App />);
  await act(flushPromises);

  const alert = screen.getByRole("alert");
  expect(alert).toHaveTextContent("无法接收任务更新。");
  expect(invokeMock).not.toHaveBeenCalledWith("list_tasks", { status: null });

  await act(async () => {
    fireEvent.click(within(alert).getByRole("button", { name: "重试" }));
    await flushPromises();
  });

  expect(listenMock).toHaveBeenCalledTimes(6);
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  unmount();
  for (const unlisten of unlisteners) {
    expect(unlisten).toHaveBeenCalledOnce();
  }
});

test("initializes and refreshes the selected service usage", async () => {
  let pages = 12;
  invokeMock.mockImplementation(async (command) => {
    if (command === "get_settings") return { language: "zh-CN" };
    if (command === "get_usage") {
      return [{ date: "2026-07-10", service: "vl16", pages }];
    }
    return [];
  });

  render(<App />);
  expect(await screen.findByText("12 / 20,000 页")).toBeInTheDocument();

  pages = 14;
  const usageHandler = listenMock.mock.calls.find(
    ([name]) => name === "usage:updated",
  )?.[1];
  await act(async () => usageHandler({ payload: { today_pages: 14 } }));
  expect(await screen.findByText("14 / 20,000 页")).toBeInTheDocument();
});
