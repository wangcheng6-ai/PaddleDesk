import { StrictMode } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
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
  invokeMock.mockReset().mockResolvedValue({ language: "zh-CN" });
  listenMock.mockReset().mockImplementation(async () => vi.fn());
  useApp.setState({ view: "home", service: "vl16", tasks: [] });
  await initI18n();
});

afterEach(cleanup);

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

test("switches among the three service wire values", () => {
  render(<App />);

  fireEvent.click(screen.getByRole("button", { name: "PP-OCRv6" }));

  expect(useApp.getState().service).toBe("pp_ocr_v6");
  expect(screen.getByRole("button", { name: "PP-OCRv6" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});

test("StrictMode and unmount eventually release every async listener", async () => {
  const unlisteners = Array.from({ length: 8 }, () => vi.fn());
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

  await waitFor(() => expect(listenMock).toHaveBeenCalledTimes(8));
  await waitFor(() => {
    for (const unlisten of unlisteners) {
      expect(unlisten).toHaveBeenCalledOnce();
    }
  });
});
