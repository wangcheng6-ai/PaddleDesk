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

const { getCurrentWebviewMock, invokeMock, onDragDropEventMock, openMock } =
  vi.hoisted(() => ({
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

import { DropZone } from "../components/DropZone";
import { initI18n } from "../i18n";

beforeEach(async () => {
  invokeMock.mockReset().mockResolvedValue({ language: "zh-CN" });
  openMock.mockReset();
  onDragDropEventMock.mockReset();
  getCurrentWebviewMock.mockReset().mockReturnValue({
    onDragDropEvent: onDragDropEventMock,
  });
  await initI18n();
});

afterEach(cleanup);

test("opens the native multi-file dialog and submits only supported paths", async () => {
  onDragDropEventMock.mockResolvedValue(vi.fn());
  openMock.mockResolvedValue([
    "C:/docs/page.PNG",
    "C:/docs/notes.txt",
    "C:/docs/report.pdf",
  ]);
  const onPaths = vi.fn();

  render(<DropZone onPaths={onPaths} />);
  fireEvent.click(screen.getByRole("button", { name: "选择文件" }));

  await waitFor(() =>
    expect(onPaths).toHaveBeenCalledWith([
      "C:/docs/page.PNG",
      "C:/docs/report.pdf",
    ]),
  );
  expect(openMock).toHaveBeenCalledWith({
    multiple: true,
    filters: [
      {
        name: "支持的文件",
        extensions: ["png", "jpg", "jpeg", "webp", "pdf"],
      },
    ],
  });
});

test("filters native drop payloads and releases every StrictMode listener", async () => {
  const handlers: Array<(event: unknown) => void> = [];
  const unlisteners = [vi.fn(), vi.fn()];
  onDragDropEventMock.mockImplementation(async (handler) => {
    handlers.push(handler);
    return unlisteners[handlers.length - 1];
  });
  const onPaths = vi.fn();

  const { unmount } = render(
    <StrictMode>
      <DropZone onPaths={onPaths} />
    </StrictMode>,
  );
  await waitFor(() => expect(handlers).toHaveLength(2));

  handlers[1]({
    payload: {
      type: "drop",
      paths: ["C:/docs/image.webp", "C:/docs/archive.zip"],
    },
  });
  expect(onPaths).toHaveBeenCalledWith(["C:/docs/image.webp"]);

  unmount();
  await waitFor(() => {
    for (const unlisten of unlisteners) expect(unlisten).toHaveBeenCalledOnce();
  });
});

test("does nothing when the dialog is canceled or a drop has no supported paths", async () => {
  let handler: ((event: unknown) => void) | undefined;
  onDragDropEventMock.mockImplementation(async (nextHandler) => {
    handler = nextHandler;
    return vi.fn();
  });
  openMock.mockResolvedValue(null);
  const onPaths = vi.fn();

  render(<DropZone onPaths={onPaths} />);
  await waitFor(() => expect(handler).toBeDefined());
  fireEvent.click(screen.getByRole("button", { name: "选择文件" }));
  await waitFor(() => expect(openMock).toHaveBeenCalledOnce());
  handler!({ payload: { type: "drop", paths: ["C:/docs/notes.txt"] } });

  expect(onPaths).not.toHaveBeenCalled();
});

test("does not resubscribe on an ordinary rerender and still cleans up", async () => {
  const unlisten = vi.fn();
  onDragDropEventMock.mockResolvedValue(unlisten);
  const onPaths = vi.fn();
  const { rerender, unmount } = render(<DropZone onPaths={onPaths} />);
  await waitFor(() => expect(onDragDropEventMock).toHaveBeenCalledOnce());

  rerender(<DropZone onPaths={onPaths} />);

  expect(onDragDropEventMock).toHaveBeenCalledOnce();
  unmount();
  expect(unlisten).toHaveBeenCalledOnce();
});

test("keeps one drop listener while delivering paths to the latest callback", async () => {
  let handler: ((event: unknown) => void) | undefined;
  onDragDropEventMock.mockImplementation(async (nextHandler) => {
    handler = nextHandler;
    return vi.fn();
  });
  const firstOnPaths = vi.fn();
  const latestOnPaths = vi.fn();
  const { rerender } = render(<DropZone onPaths={firstOnPaths} />);
  await waitFor(() => expect(handler).toBeDefined());

  rerender(<DropZone onPaths={latestOnPaths} />);
  handler!({ payload: { type: "drop", paths: ["C:/docs/latest.png"] } });

  expect(onDragDropEventMock).toHaveBeenCalledOnce();
  expect(firstOnPaths).not.toHaveBeenCalled();
  expect(latestOnPaths).toHaveBeenCalledWith(["C:/docs/latest.png"]);
});

test("retries drop registration without hiding an independent submit error", async () => {
  let dropHandler: ((event: unknown) => void) | undefined;
  const unlisten = vi.fn();
  onDragDropEventMock
    .mockRejectedValueOnce(new Error("drag unavailable"))
    .mockImplementationOnce(async (handler) => {
      dropHandler = handler;
      return unlisten;
    });
  openMock.mockResolvedValue(["C:/docs/page.png"]);
  const onPaths = vi
    .fn()
    .mockRejectedValueOnce(new Error("submit failed"))
    .mockResolvedValueOnce(undefined);

  const { unmount } = render(<DropZone onPaths={onPaths} />);
  const registrationMessage = await screen.findByText("无法监听文件拖放。");
  const registrationAlert =
    registrationMessage.closest<HTMLElement>('[role="alert"]');
  expect(registrationAlert).not.toBeNull();
  expect(registrationAlert).toHaveClass("drop-zone-alert");

  fireEvent.click(screen.getByRole("button", { name: "选择文件" }));
  expect(await screen.findByText("无法添加文件。")).toBeInTheDocument();
  expect(registrationMessage).toBeInTheDocument();

  fireEvent.click(
    within(registrationAlert!).getByRole("button", { name: "重试" }),
  );
  await waitFor(() => expect(onDragDropEventMock).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(dropHandler).toBeDefined());
  expect(screen.queryByText("无法监听文件拖放。")).not.toBeInTheDocument();
  expect(screen.getByText("无法添加文件。")).toBeInTheDocument();

  dropHandler!({ payload: { type: "drop", paths: ["C:/docs/page.png"] } });
  await waitFor(() => expect(onPaths).toHaveBeenCalledTimes(2));
  await waitFor(() =>
    expect(screen.queryByText("无法添加文件。")).not.toBeInTheDocument(),
  );

  unmount();
  expect(unlisten).toHaveBeenCalledOnce();
});
