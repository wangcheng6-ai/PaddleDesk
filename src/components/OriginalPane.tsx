import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { convertFileSrc } from "@tauri-apps/api/core";

import { scaleBbox } from "../lib/bbox";
import type { RecognitionResult } from "../lib/ipc";
import { PdfPage } from "./PdfPage";

interface OriginalPaneProps {
  inputPath: string;
  result: RecognitionResult;
  sourceBytes: Uint8Array | null;
}

export function OriginalPane({ inputPath, result, sourceBytes }: OriginalPaneProps) {
  const { t } = useTranslation();
  const [pageIndex, setPageIndex] = useState(0);
  const [showBboxes, setShowBboxes] = useState(true);
  const [pdfSize, setPdfSize] = useState<{ width: number; height: number } | null>(null);
  const [pdfFailed, setPdfFailed] = useState(false);
  const page = result.pages[pageIndex];
  const isPdf = /\.pdf$/i.test(inputPath);
  const onPdfSize = useCallback((size: { width: number; height: number }) => {
    setPdfSize(size);
    setPdfFailed(false);
  }, []);
  const onPdfError = useCallback(() => setPdfFailed(true), []);

  return (
    <section className="viewer-panel original-pane" aria-label={t("viewer.original")}>
      <div className="panel-heading">
        <h2>{t("viewer.original")}</h2>
        <label className="bbox-toggle">
          <input
            type="checkbox"
            role="switch"
            checked={showBboxes}
            onChange={(event) => setShowBboxes(event.target.checked)}
          />
          {t("viewer.showBboxes")}
        </label>
      </div>

      <div className="original-stage">
        <div
          className="original-image-frame"
          style={pdfSize ? { aspectRatio: `${pdfSize.width} / ${pdfSize.height}` } : undefined}
        >
          {isPdf ? (
            sourceBytes ? (
              <PdfPage
                bytes={sourceBytes}
                pageNumber={pageIndex + 1}
                onSize={onPdfSize}
                onError={onPdfError}
              />
            ) : (
              <div className="pdf-placeholder">{t("common.loading")}</div>
            )
          ) : (
            <img src={convertFileSrc(inputPath)} alt={t("viewer.originalAlt")} />
          )}
          {showBboxes && page ? (
            <div className="bbox-layer" aria-hidden="true">
              {page.blocks.map((block) => {
                if (!block.bbox) return null;
                const box = scaleBbox(block.bbox, page, {
                  width: 100,
                  height: 100,
                });
                return (
                  <span
                    className={`bbox bbox-${block.kind}`}
                    data-testid="bbox"
                    key={block.id}
                    style={{
                      left: `${box.left}%`,
                      top: `${box.top}%`,
                      width: `${box.width}%`,
                      height: `${box.height}%`,
                    }}
                  />
                );
              })}
            </div>
          ) : null}
        </div>
        {pdfFailed ? <p role="alert">{t("viewer.pdfLoadFailed")}</p> : null}
      </div>

      {(isPdf || result.pages.length > 1) && (
        <div className="page-controls">
          <button
            type="button"
            disabled={pageIndex === 0}
            onClick={() => setPageIndex((index) => index - 1)}
          >
            {t("viewer.previousPage")}
          </button>
          <span>
            {t("viewer.page", {
              page: pageIndex + 1,
              total: Math.max(result.pages.length, result.page_count),
            })}
          </span>
          <button
            type="button"
            disabled={pageIndex >= result.pages.length - 1}
            onClick={() => setPageIndex((index) => index + 1)}
          >
            {t("viewer.nextPage")}
          </button>
        </div>
      )}
    </section>
  );
}
