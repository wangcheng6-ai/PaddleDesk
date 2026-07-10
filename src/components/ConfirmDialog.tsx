import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

interface PendingConfirm {
  message: string;
  resolve: (value: boolean) => void;
}

export function useConfirm(): {
  confirm: (message: string) => Promise<boolean>;
  confirmDialog: ReactNode;
} {
  const { t } = useTranslation();
  const [pending, setPending] = useState<PendingConfirm | null>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const confirmButton = useRef<HTMLButtonElement>(null);

  const confirm = useCallback(
    (message: string) =>
      new Promise<boolean>((resolve) => setPending({ message, resolve })),
    [],
  );

  const close = useCallback(
    (value: boolean) => {
      pending?.resolve(value);
      setPending(null);
    },
    [pending],
  );

  useEffect(() => {
    if (pending) confirmButton.current?.focus();
  }, [pending]);

  const confirmDialog = pending ? (
    <div
      className="modal-backdrop"
      role="presentation"
      onClick={() => close(false)}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          close(false);
          return;
        }
        if (event.key !== "Tab") return;
        event.preventDefault();
        const next =
          document.activeElement === cancelButton.current
            ? confirmButton.current
            : cancelButton.current;
        next?.focus();
      }}
    >
      <div
        className="confirm-card"
        role="alertdialog"
        aria-modal="true"
        aria-describedby="confirm-dialog-message"
        onClick={(event) => event.stopPropagation()}
      >
        <p id="confirm-dialog-message">{pending.message}</p>
        <div className="confirm-actions">
          <button
            className="secondary-button"
            type="button"
            ref={cancelButton}
            onClick={() => close(false)}
          >
            {t("actions.cancel")}
          </button>
          <button
            className="confirm-danger"
            type="button"
            ref={confirmButton}
            onClick={() => close(true)}
          >
            {t("actions.confirm")}
          </button>
        </div>
      </div>
    </div>
  ) : null;

  return { confirm, confirmDialog };
}
