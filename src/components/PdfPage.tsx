import { useEffect, useRef, useState } from "react";
import {
  GlobalWorkerOptions,
  getDocument,
  type PDFDocumentProxy,
  type RenderTask,
} from "pdfjs-dist";
import pdfWorker from "pdfjs-dist/build/pdf.worker.min.mjs?url";

GlobalWorkerOptions.workerSrc = pdfWorker;

interface PdfPageProps {
  bytes: Uint8Array;
  pageNumber: number;
  onSize: (size: { width: number; height: number }) => void;
  onError?: () => void;
}

export function PdfPage({ bytes, pageNumber, onSize, onError }: PdfPageProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [document, setDocument] = useState<PDFDocumentProxy | null>(null);

  useEffect(() => {
    let active = true;
    setDocument(null);
    const loading = getDocument({ data: bytes });
    void loading.promise.then(
      (loaded) => {
        if (active) setDocument(loaded);
        else void loaded.destroy();
      },
      () => {
        if (active) onError?.();
      },
    );
    return () => {
      active = false;
      void loading.destroy();
    };
  }, [bytes, onError]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !document) return;
    let active = true;
    let renderTask: RenderTask | null = null;

    void document.getPage(pageNumber).then(
      (page) => {
        if (!active) return;
        const viewport = page.getViewport({ scale: 1.5 });
        canvas.width = Math.ceil(viewport.width);
        canvas.height = Math.ceil(viewport.height);
        onSize({ width: viewport.width, height: viewport.height });
        renderTask = page.render({ canvas, viewport });
        return renderTask.promise;
      },
      () => onError?.(),
    ).catch(() => {
      if (active) onError?.();
    });

    return () => {
      active = false;
      renderTask?.cancel();
    };
  }, [document, onError, onSize, pageNumber]);

  return (
    <canvas
      className="pdf-canvas"
      data-testid="pdf-page"
      data-page-number={pageNumber}
      ref={canvasRef}
    />
  );
}
