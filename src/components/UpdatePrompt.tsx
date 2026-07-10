import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

export function UpdatePrompt() {
  const { t } = useTranslation();
  const [update, setUpdate] = useState<Update | null>(null);
  const [status, setStatus] = useState<"idle" | "installing" | "failed">("idle");

  useEffect(() => {
    let active = true;
    void check().then(
      (available) => {
        if (!available?.version) {
          void available?.close();
          return;
        }
        if (active) setUpdate(available);
        else void available.close();
      },
      () => {
        // startup check is best-effort; Settings offers a manual retry
      },
    );
    return () => {
      active = false;
    };
  }, []);

  if (!update) return null;

  const install = async () => {
    setStatus("installing");
    try {
      await update.downloadAndInstall();
      await relaunch();
    } catch {
      setStatus("failed");
    }
  };

  const delay = () => {
    void update.close();
    setUpdate(null);
  };

  return (
    <div className="modal-backdrop update-backdrop">
      <section className="update-card" role="dialog" aria-modal="true" aria-labelledby="update-title">
        <span className="update-badge">{t("update.badge")}</span>
        <h2 id="update-title">{t("update.title", { version: update.version })}</h2>
        <p>{update.body || t("update.bodyFallback")}</p>
        {status === "failed" ? <p role="alert">{t("update.failed")}</p> : null}
        <div className="onboarding-actions">
          <button className="text-button" type="button" onClick={delay}>
            {t("update.later")}
          </button>
          <button
            className="primary-button"
            type="button"
            disabled={status === "installing"}
            onClick={() => void install()}
          >
            {status === "installing" ? t("update.installing") : t("update.install")}
          </button>
        </div>
      </section>
    </div>
  );
}
