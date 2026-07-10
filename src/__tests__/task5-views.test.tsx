import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { initI18n } from "../i18n";
import { useApp } from "../stores/app";
import { Settings } from "../views/Settings";
import { Usage } from "../views/Usage";
import { Viewer } from "../views/Viewer";

beforeEach(async () => {
  Object.defineProperty(navigator, "language", {
    configurable: true,
    value: "zh-CN",
  });
  invokeMock.mockReset().mockImplementation(async (command, args) => {
    if (command === "get_settings") {
      return {
        language: "zh-CN",
        theme: "system",
        concurrency: "2",
        save_history: "1",
        proxy_mode: "system",
        autostart: "0",
      };
    }
    if (command === "get_credential_status") {
      return { configured: true, last_four: "1234" };
    }
    if (command === "get_screenshot_hotkey") return "Ctrl+Alt+S";
    if (command === "set_screenshot_hotkey") return args.shortcut;
    if (command === "reveal_token") return "token-value-1234";
    if (command === "delete_token") return null;
    if (command === "list_results") {
      return [
        {
          task_id: "done",
          service: "vl16",
          file_name: "done.png",
          snippet: "卷积神经网络",
          created_at: 1783612800,
          temporary: false,
        },
      ];
    }
    if (command === "get_result") {
      return { markdown: "卷积神经网络", page_count: 1, pages: [] };
    }
    if (command === "get_usage") {
      return [
        { date: "2026-07-10", service: "vl16", pages: 12 },
        { date: "2026-07-10", service: "pp_ocr_v6", pages: 3 },
        { date: "2026-07-09", service: "vl16", pages: 5000 },
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
    todayPages: { vl16: 0, pp_ocr_v6: 0, structure_v3: 0 },
  });
  await initI18n();
});

afterEach(cleanup);

test("defaults to the newest service result and searches OCR content", async () => {
  render(<Viewer />);
  expect(await screen.findByText("done.png")).toBeInTheDocument();
  await waitFor(() => expect(useApp.getState().selectedTaskId).toBe("done"));

  fireEvent.change(screen.getByRole("searchbox", { name: "搜索历史" }), {
    target: { value: "卷积" },
  });
  await waitFor(() =>
    expect(invokeMock).toHaveBeenCalledWith("list_results", {
      service: "vl16",
      query: "卷积",
    }),
  );
  expect(screen.getAllByText("卷积神经网络")).toHaveLength(2);
});

test("shows quota rings for all three services and seven-day usage", async () => {
  render(<Usage />);

  expect(await screen.findByText("12 / 20,000 页")).toBeInTheDocument();
  expect(screen.getByText("3 / 20,000 页")).toBeInTheDocument();
  expect(screen.getByText("0 / 20,000 页")).toBeInTheDocument();
  expect(screen.getAllByRole("progressbar")).toHaveLength(3);
  expect(screen.getByText("近 7 日")).toBeInTheDocument();
});

test("persists language choices and applies them immediately", async () => {
  render(<Settings />);
  await screen.findByRole("heading", { name: "设置" });
  expect(screen.getByText("修改并发任务数后，重启应用生效。")).toBeInTheDocument();

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

test("reveals configured credentials and records hotkeys without losing the old value on failure", async () => {
  render(<Settings />);

  const tokenField = await screen.findByLabelText("当前 Token");
  expect(tokenField).toHaveValue("••••••••1234");
  fireEvent.click(screen.getByRole("button", { name: "显示" }));
  await waitFor(() => expect(tokenField).toHaveValue("token-value-1234"));
  fireEvent.click(screen.getByRole("button", { name: "隐藏" }));
  expect(tokenField).toHaveValue("••••••••1234");

  const hotkey = screen.getByLabelText("截图识别快捷键");
  fireEvent.focus(hotkey);
  fireEvent.keyDown(hotkey, { key: "F8" });
  await waitFor(() => expect(hotkey).toHaveValue("F8"));
  expect(invokeMock).toHaveBeenCalledWith("set_screenshot_hotkey", {
    shortcut: "F8",
  });

  invokeMock.mockRejectedValueOnce(new Error("shortcut conflict"));
  fireEvent.focus(hotkey);
  fireEvent.keyDown(hotkey, { key: "x", ctrlKey: true });
  await screen.findByRole("alert");
  expect(hotkey).toHaveValue("F8");
});
