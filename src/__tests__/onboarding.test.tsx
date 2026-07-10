import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { invokeMock, openUrlMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  openUrlMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: openUrlMock }));

import { Onboarding } from "../components/Onboarding";
import { initI18n } from "../i18n";

beforeEach(async () => {
  invokeMock.mockReset().mockImplementation(async (command) => {
    if (command === "get_settings") return { language: "zh-CN" };
    if (command === "set_settings") return undefined;
    if (command === "validate_token") return true;
    throw new Error(`unexpected command: ${command}`);
  });
  openUrlMock.mockReset().mockResolvedValue(undefined);
  await initI18n();
});

afterEach(cleanup);

test("discloses cloud recognition and lets the user skip then reopen later", async () => {
  const onClose = vi.fn();
  render(<Onboarding open onClose={onClose} />);

  expect(screen.getByRole("dialog")).toHaveTextContent("百度云端");
  fireEvent.click(screen.getByRole("button", { name: "暂时跳过" }));

  await waitFor(() =>
    expect(invokeMock).toHaveBeenCalledWith("set_settings", {
      map: { onboarding_complete: "1" },
    }),
  );
  expect(onClose).toHaveBeenCalledOnce();
});

test("opens the official token page and completes only after validation", async () => {
  const onClose = vi.fn();
  render(<Onboarding open onClose={onClose} />);

  fireEvent.click(screen.getByRole("button", { name: "开始设置" }));
  fireEvent.click(screen.getByRole("button", { name: "打开 AI Studio" }));
  expect(openUrlMock).toHaveBeenCalledWith("https://aistudio.baidu.com/paddleocr");

  fireEvent.change(screen.getByLabelText("Access Token"), {
    target: { value: "test-secret" },
  });
  fireEvent.click(screen.getByRole("button", { name: "验证并开始使用" }));

  await waitFor(() =>
    expect(invokeMock).toHaveBeenCalledWith("validate_token", {
      token: "test-secret",
    }),
  );
  expect(invokeMock).toHaveBeenCalledWith("set_settings", {
    map: { onboarding_complete: "1" },
  });
  expect(onClose).toHaveBeenCalledOnce();
});
