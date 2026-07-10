import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import Markdown from "react-markdown";
import rehypeKatex from "rehype-katex";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import "katex/dist/katex.min.css";

import type { ExportFormat, RecognitionResult } from "../lib/ipc";

type ResultTab = "markdown" | "json" | "text";

interface ResultPaneProps {
  result: RecognitionResult;
  onExport: (format: ExportFormat, blockId?: string) => Promise<void>;
}

const tabs: ResultTab[] = ["markdown", "json", "text"];

export function ResultPane({ result, onExport }: ResultPaneProps) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<ResultTab>("markdown");
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">(
    "idle",
  );
  const plainText = useMemo(
    () =>
      result.pages
        .flatMap((page) => page.blocks)
        .filter((block) => block.kind === "text")
        .map((block) => block.content)
        .join("\n\n"),
    [result],
  );
  const actionable = result.pages
    .flatMap((page) => page.blocks)
    .filter((block) => block.kind === "table" || block.kind === "formula");

  const copyFormula = async (content: string) => {
    try {
      await navigator.clipboard.writeText(content);
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    }
  };

  return (
    <section className="viewer-panel result-pane" aria-label={t("viewer.result")}>
      <div className="panel-heading result-heading">
        <div className="result-tabs" role="tablist" aria-label={t("viewer.resultTabs")}>
          {tabs.map((name) => (
            <button
              type="button"
              role="tab"
              aria-selected={tab === name}
              key={name}
              onClick={() => setTab(name)}
            >
              {t(`viewer.tabs.${name}`)}
            </button>
          ))}
        </div>
        <div className="export-actions">
          {(["md", "json", "txt"] as ExportFormat[]).map((format) => (
            <button type="button" key={format} onClick={() => void onExport(format)}>
              {t(`viewer.export.${format}`)}
            </button>
          ))}
        </div>
      </div>

      <div className="result-content">
        {tab === "markdown" ? (
          <div className="markdown-content">
            <Markdown remarkPlugins={[remarkGfm, remarkMath]} rehypePlugins={[rehypeKatex]}>
              {result.markdown}
            </Markdown>
          </div>
        ) : tab === "json" ? (
          <pre>{JSON.stringify(result, null, 2)}</pre>
        ) : (
          <pre>{plainText}</pre>
        )}

        {tab === "markdown" && actionable.length > 0 ? (
          <div className="block-actions">
            {actionable.map((block) => (
              <article className={`result-block result-block-${block.kind}`} key={block.id}>
                <pre>{block.content}</pre>
                {block.kind === "table" ? (
                  <button type="button" onClick={() => void onExport("csv", block.id)}>
                    {t("viewer.export.csv")}
                  </button>
                ) : (
                  <button type="button" onClick={() => void copyFormula(block.content)}>
                    {t("viewer.copyLatex")}
                  </button>
                )}
              </article>
            ))}
          </div>
        ) : null}
        {copyState !== "idle" ? (
          <p role="status">{t(`viewer.copy.${copyState}`)}</p>
        ) : null}
      </div>
    </section>
  );
}
