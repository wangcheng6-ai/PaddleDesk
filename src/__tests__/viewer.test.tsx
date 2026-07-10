import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { getDocumentMock, getPageMock, invokeMock, saveMock, writeTextMock } = vi.hoisted(
  () => ({
    getDocumentMock: vi.fn(),
    getPageMock: vi.fn(),
    invokeMock: vi.fn(),
    saveMock: vi.fn(),
    writeTextMock: vi.fn(),
  }),
);

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: saveMock }));
vi.mock("pdfjs-dist", () => ({
  GlobalWorkerOptions: { workerSrc: "" },
  getDocument: getDocumentMock,
}));
vi.mock("pdfjs-dist/build/pdf.worker.min.mjs?url", () => ({ default: "pdf.worker.js" }));

import { initI18n } from "../i18n";
import { useApp } from "../stores/app";
import { Viewer } from "../views/Viewer";

const result = {
  markdown: "# Mock 文档\n\n| 名称 | 数量 |\n| --- | --- |\n| 苹果 | 2 |",
  page_count: 1,
  pages: [
    {
      width: 100,
      height: 200,
      blocks: [
        { id: "text", kind: "text", bbox: [10, 20, 90, 60], content: "Mock 文档" },
        { id: "table", kind: "table", bbox: [10, 70, 90, 140], content: "名称,数量\n苹果,2" },
        { id: "formula", kind: "formula", bbox: [10, 150, 90, 180], content: "E=mc^2" },
      ],
    },
  ],
};

const pdfResult = {
  ...result,
  page_count: 2,
  pages: [result.pages[0], { width: 100, height: 200, blocks: [] }],
};

beforeEach(async () => {
  invokeMock.mockReset().mockImplementation(async (command, args) => {
    if (command === "get_settings") return { language: "zh-CN" };
    if (command === "list_results") {
      return [{
        task_id: "task-1",
        service: "vl16",
        file_name: "mock.png",
        snippet: "Mock 文档",
        created_at: 1,
        temporary: false,
      }];
    }
    if (command === "get_result") return result;
    if (command === "get_task_source") return new Uint8Array([1, 2, 3]).buffer;
    if (command === "export_result") return args.path;
    if (command === "clear_results") return null;
    throw new Error(`unexpected command: ${command}`);
  });
  saveMock.mockReset().mockResolvedValue("C:/exports/mock.md");
  writeTextMock.mockReset().mockResolvedValue(undefined);
  getPageMock.mockReset().mockImplementation(async () => ({
    getViewport: () => ({ width: 100, height: 200 }),
    render: () => ({ promise: Promise.resolve(), cancel: vi.fn() }),
  }));
  getDocumentMock.mockReset().mockReturnValue({
    promise: Promise.resolve({
      getPage: getPageMock,
      numPages: 2,
      destroy: vi.fn().mockResolvedValue(undefined),
    }),
    destroy: vi.fn().mockResolvedValue(undefined),
  });
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
  useApp.setState({
    view: "viewer",
    selectedTaskId: "task-1",
    tasks: [
      {
        id: "task-1",
        status: "done",
        input_path: "C:/docs/mock.png",
      },
    ],
  });
  await initI18n();
});

afterEach(cleanup);

test("loads a result, switches tabs, exports, and copies a formula", async () => {
  render(<Viewer />);

  expect(await screen.findByRole("heading", { name: "Mock 文档" })).toBeInTheDocument();
  expect(screen.getAllByTestId("bbox")).toHaveLength(3);

  fireEvent.click(screen.getByRole("tab", { name: "JSON" }));
  expect(screen.getByText(/"page_count": 1/)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("tab", { name: "预览" }));
  fireEvent.click(screen.getByRole("button", { name: "导出 Markdown" }));
  await waitFor(() =>
    expect(invokeMock).toHaveBeenCalledWith("export_result", {
      taskId: "task-1",
      format: "md",
      path: "C:/exports/mock.md",
      blockId: null,
    }),
  );

  fireEvent.click(screen.getByRole("button", { name: "复制 LaTeX" }));
  await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith("E=mc^2"));

  saveMock.mockRejectedValueOnce(new Error("dialog unavailable"));
  fireEvent.click(screen.getByRole("button", { name: "导出 JSON" }));
  expect(await screen.findByText("导出失败，请重试。")).toBeInTheDocument();
});

test("renders only the selected PDF page and advances to page two", async () => {
  useApp.setState({
    selectedTaskId: "pdf-task",
    tasks: [
      {
        id: "pdf-task",
        status: "done",
        input_path: "C:/docs/mock.pdf",
      },
    ],
  });
  invokeMock.mockImplementation(async (command) => {
    if (command === "list_results") {
      return [
        {
          task_id: "pdf-task",
          service: "vl16",
          file_name: "mock.pdf",
          snippet: "first",
          created_at: 2,
          temporary: false,
        },
        {
          task_id: "pdf-task-2",
          service: "vl16",
          file_name: "second.pdf",
          snippet: "second",
          created_at: 1,
          temporary: false,
        },
      ];
    }
    if (command === "get_result") return pdfResult;
    if (command === "get_task_source") return new Uint8Array([37, 80, 68, 70]).buffer;
    if (command === "get_settings") return { language: "zh-CN" };
    throw new Error(`unexpected command: ${command}`);
  });

  render(<Viewer />);

  const canvas = await screen.findByTestId("pdf-page", {}, { timeout: 3000 });
  await waitFor(() => expect(getPageMock).toHaveBeenCalledWith(1));
  expect(canvas).toHaveAttribute("data-page-number", "1");

  fireEvent.click(screen.getByRole("button", { name: "下一页" }));
  await waitFor(() => expect(getPageMock).toHaveBeenCalledWith(2));
  expect(screen.getByTestId("pdf-page")).toHaveAttribute("data-page-number", "2");

  getPageMock.mockClear();
  fireEvent.click(screen.getByRole("button", { name: /second\.pdf/ }));

  await waitFor(() => expect(getPageMock).toHaveBeenCalledWith(1));
  expect(screen.getByTestId("pdf-page")).toHaveAttribute("data-page-number", "1");
});

test("keeps markdown preview inert and keeps all readable block types in plain text", async () => {
  const unsafeResult = {
    markdown:
      "可见正文\n\n<script>window.pwned = true</script>\n\n![远程图](https://example.com/a.png)",
    page_count: 1,
    pages: [
      {
        width: 100,
        height: 100,
        blocks: [
          { id: "text", kind: "text", bbox: null, content: "正文" },
          { id: "seal", kind: "seal", bbox: null, content: "印章内容" },
          { id: "chart", kind: "chart", bbox: null, content: "图表内容" },
        ],
      },
    ],
  };
  invokeMock.mockImplementation(async (command) => {
    if (command === "list_results") {
      return [
        {
          task_id: "task-1",
          service: "vl16",
          file_name: "missing.png",
          snippet: "可见正文",
          created_at: 1,
          temporary: false,
        },
      ];
    }
    if (command === "get_result") return unsafeResult;
    if (command === "get_task_source") throw new Error("missing");
    throw new Error(`unexpected command: ${command}`);
  });

  const { container } = render(<Viewer />);

  expect(await screen.findByText("可见正文")).toBeInTheDocument();
  expect(container.querySelector("script")).toBeNull();
  expect(container.querySelector("img[src='https://example.com/a.png']")).toBeNull();
  expect(container.querySelector(".missing-markdown-image")).toHaveTextContent("远程图");
  expect(await screen.findByText("原文件不可用，识别内容仍可查看和导出。")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("tab", { name: "纯文本" }));
  expect(screen.getByText(/正文\s+印章内容\s+图表内容/)).toBeInTheDocument();
});

test("clearing current service results requires in-app confirmation", async () => {
  render(<Viewer />);
  await screen.findByRole("heading", { name: "Mock 文档" });

  fireEvent.click(screen.getByRole("button", { name: "清空当前服务结果" }));
  const dialog = await screen.findByRole("alertdialog");
  expect(dialog).toHaveTextContent("确定清空当前服务的全部识别结果吗？");
  fireEvent.click(screen.getByRole("button", { name: "取消" }));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(invokeMock).not.toHaveBeenCalledWith("clear_results", expect.anything());

  fireEvent.click(screen.getByRole("button", { name: "清空当前服务结果" }));
  await screen.findByRole("alertdialog");
  fireEvent.click(screen.getByRole("button", { name: "确定" }));
  await waitFor(() =>
    expect(invokeMock).toHaveBeenCalledWith("clear_results", { service: "vl16" }),
  );
});

test("clears the old result immediately when the global service changes", async () => {
  invokeMock.mockImplementation(async (command, args) => {
    if (command === "list_results") {
      return args.service === "pp_ocr_v6"
        ? [
            {
              task_id: "pp-task",
              service: "pp_ocr_v6",
              file_name: "pp.png",
              snippet: "PP result",
              created_at: 2,
              temporary: false,
            },
          ]
        : [
            {
              task_id: "task-1",
              service: "vl16",
              file_name: "mock.png",
              snippet: "VL result",
              created_at: 1,
              temporary: false,
            },
          ];
    }
    if (command === "get_result") return result;
    if (command === "get_task_source") return new Uint8Array([1, 2, 3]).buffer;
    throw new Error(`unexpected command: ${command}`);
  });
  render(<Viewer />);
  expect(
    await screen.findByRole("button", { name: /mock\.png/ }),
  ).toBeInTheDocument();

  act(() => useApp.getState().setService("pp_ocr_v6"));

  expect(await screen.findByText("pp.png")).toBeInTheDocument();
  expect(screen.queryAllByText("mock.png")).toHaveLength(0);
  expect(invokeMock).toHaveBeenCalledWith("list_results", {
    service: "pp_ocr_v6",
    query: "",
  });
});
