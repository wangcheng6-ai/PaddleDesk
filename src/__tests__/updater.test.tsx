import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { checkMock, closeMock, downloadAndInstallMock, invokeMock, relaunchMock } = vi.hoisted(() => ({
  checkMock: vi.fn(),
  closeMock: vi.fn(),
  downloadAndInstallMock: vi.fn(),
  invokeMock: vi.fn(),
  relaunchMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: checkMock }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: relaunchMock }));

import { UpdatePrompt } from "../components/UpdatePrompt";
import { initI18n } from "../i18n";

beforeEach(async () => {
  invokeMock.mockReset().mockResolvedValue({ language: "zh-CN" });
  downloadAndInstallMock.mockReset().mockResolvedValue(undefined);
  closeMock.mockReset().mockResolvedValue(undefined);
  relaunchMock.mockReset().mockResolvedValue(undefined);
  checkMock.mockReset().mockResolvedValue({
    version: "0.2.0",
    body: "Faster OCR startup",
    close: closeMock,
    downloadAndInstall: downloadAndInstallMock,
  });
  await initI18n();
});

afterEach(cleanup);

test("offers a signed update and allows delaying it", async () => {
  render(<UpdatePrompt />);

  expect(await screen.findByText("PaddleDesk 0.2.0 可用")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "稍后" }));
  expect(screen.queryByText("PaddleDesk 0.2.0 可用")).not.toBeInTheDocument();
});

test("installs the update then relaunches the app", async () => {
  render(<UpdatePrompt />);
  fireEvent.click(await screen.findByRole("button", { name: "更新并重启" }));

  await waitFor(() => expect(downloadAndInstallMock).toHaveBeenCalledOnce());
  expect(relaunchMock).toHaveBeenCalledOnce();
});
