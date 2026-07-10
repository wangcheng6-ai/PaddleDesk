import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { getCurrentWebviewMock, invokeMock, listenMock } = vi.hoisted(() => ({
  getCurrentWebviewMock: vi.fn(),
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
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
  invokeMock.mockReset().mockImplementation(async (command) => {
    if (command === "get_settings") {
      return { language: "zh-CN", onboarding_complete: "1" };
    }
    if (command === "list_tasks" || command === "get_usage") return [];
    if (command === "create_task_from_clipboard") return "clipboard-task";
    throw new Error(`unexpected command: ${command}`);
  });
  listenMock.mockReset().mockResolvedValue(vi.fn());
  getCurrentWebviewMock.mockReturnValue({
    onDragDropEvent: vi.fn().mockResolvedValue(vi.fn()),
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

test("Ctrl+V creates a clipboard image task on Home but leaves inputs alone", async () => {
  render(<App />);
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("list_tasks", { status: null }));

  fireEvent.keyDown(window, { key: "v", ctrlKey: true });
  await waitFor(() =>
    expect(invokeMock).toHaveBeenCalledWith("create_task_from_clipboard", {
      service: "vl16",
    }),
  );
  expect(useApp.getState().view).toBe("queue");

  useApp.getState().setView("home");
  const input = document.createElement("input");
  document.body.append(input);
  input.focus();
  fireEvent.keyDown(input, { key: "v", ctrlKey: true });
  expect(
    invokeMock.mock.calls.filter(([command]) => command === "create_task_from_clipboard"),
  ).toHaveLength(1);
});
