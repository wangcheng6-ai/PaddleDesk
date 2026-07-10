import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { initI18n } from "../i18n";
import { useApp } from "../stores/app";
import { History } from "../views/History";
import { Settings } from "../views/Settings";
import { Usage } from "../views/Usage";

const doneTask = {
  id: "done",
  service: "vl16",
  status: "done",
  input_path: "C:/docs/done.png",
  created_at: 1783612800,
};

beforeEach(async () => {
  Object.defineProperty(navigator, "language", {
    configurable: true,
    value: "zh-CN",
  });
  invokeMock.mockReset().mockImplementation(async (command) => {
    if (command === "get_settings") {
      return {
        language: "zh-CN",
        theme: "system",
        default_service: "vl16",
        concurrency: "2",
        privacy_mode: "0",
        proxy_mode: "system",
        autostart: "0",
      };
    }
    if (command === "list_tasks") return [doneTask];
    if (command === "search_history") {
      return [
        {
          task_id: "done",
          file_name: "done.png",
          snippet: "卷积神经网络",
          created_at: 1783612800,
        },
      ];
    }
    if (command === "get_usage") {
      return [
        { date: "2026-07-10", service: "vl16", pages: 12 },
        { date: "2026-07-10", service: "pp_ocr_v6", pages: 3 },
      ];
    }
    if (command === "set_settings") return null;
    if (command === "validate_token") return true;
    throw new Error(`unexpected command: ${command}`);
  });
  useApp.setState({
    view: "home",
    service: "vl16",
    tasks: [],
    selectedTaskId: null,
  });
  await initI18n();
});

afterEach(cleanup);

test("debounces history search and opens a matching task", async () => {
  render(<History />);
  expect(await screen.findByText("done.png")).toBeInTheDocument();

  fireEvent.change(screen.getByRole("searchbox", { name: "搜索历史" }), {
    target: { value: "卷积" },
  });
  await waitFor(() =>
    expect(invokeMock).toHaveBeenCalledWith("search_history", { query: "卷积" }),
  );
  expect(screen.getByText("卷积神经网络")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: /done\.png/ }));
  expect(useApp.getState().selectedTaskId).toBe("done");
  expect(useApp.getState().view).toBe("viewer");
});

test("shows per-service quota rings and seven-day usage", async () => {
  render(<Usage />);

  expect(await screen.findByText("12 / 20,000 页")).toBeInTheDocument();
  expect(screen.getAllByRole("progressbar")).toHaveLength(3);
  expect(screen.getByText("近 7 日")).toBeInTheDocument();
});

test("persists language choices and applies them immediately", async () => {
  render(<Settings />);
  await screen.findByRole("heading", { name: "设置" });

  fireEvent.click(screen.getByRole("button", { name: "English" }));
  await waitFor(() => expect(i18next.language).toBe("en"));
  expect(invokeMock).toHaveBeenCalledWith("set_settings", {
    map: { language: "en" },
  });

  fireEvent.click(screen.getByRole("button", { name: "简体中文" }));
  await waitFor(() => expect(i18next.language).toBe("zh-CN"));
  fireEvent.click(screen.getByRole("button", { name: "跟随系统" }));
  await waitFor(() => expect(i18next.language).toBe("zh-CN"));
  expect(invokeMock).toHaveBeenCalledWith("set_settings", {
    map: { language: "system" },
  });
});
