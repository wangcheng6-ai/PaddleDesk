import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";

import { setSettings, validateToken } from "../lib/ipc";

const TOKEN_URL = "https://aistudio.baidu.com/paddleocr/task";

interface OnboardingProps {
  open: boolean;
  onClose: () => void;
}

export function Onboarding({ open, onClose }: OnboardingProps) {
  const { t } = useTranslation();
  const [step, setStep] = useState<1 | 2>(1);
  const [token, setToken] = useState("");
  const [status, setStatus] = useState<"idle" | "validating" | "invalid" | "failed">(
    "idle",
  );

  useEffect(() => {
    if (!open) return;
    setStep(1);
    setToken("");
    setStatus("idle");
  }, [open]);

  if (!open) return null;

  const finish = async () => {
    await setSettings({ onboarding_complete: "1" });
    onClose();
  };

  const skip = async () => {
    try {
      await finish();
    } catch {
      setStatus("failed");
    }
  };

  const checkToken = async () => {
    setStatus("validating");
    try {
      if (!(await validateToken(token))) {
        setStatus("invalid");
        return;
      }
      await finish();
      setToken("");
    } catch {
      setStatus("failed");
    }
  };

  return (
    <div className="modal-backdrop">
      <section
        className="onboarding-card"
        role="dialog"
        aria-modal="true"
        aria-labelledby="onboarding-title"
      >
        <div className="onboarding-progress" aria-label={t("onboarding.progress", { step })}>
          <span className="active" />
          <span className={step === 2 ? "active" : ""} />
        </div>

        {step === 1 ? (
          <>
            <img className="onboarding-logo" src="/paddledesk-icon.png" alt="" />
            <h1 id="onboarding-title">{t("onboarding.welcome.title")}</h1>
            <p className="onboarding-lead">{t("onboarding.welcome.subtitle")}</p>
            <div className="cloud-disclosure">
              <strong>{t("onboarding.welcome.cloudTitle")}</strong>
              <p>{t("onboarding.welcome.cloudDisclosure")}</p>
              <p>{t("onboarding.welcome.tokenStorage")}</p>
            </div>
            <div className="onboarding-actions">
              <button className="text-button" type="button" onClick={() => void skip()}>
                {t("onboarding.skip")}
              </button>
              <button className="primary-button" type="button" onClick={() => setStep(2)}>
                {t("onboarding.start")}
              </button>
            </div>
          </>
        ) : (
          <>
            <h1 id="onboarding-title">{t("onboarding.token.title")}</h1>
            <p className="onboarding-lead">{t("onboarding.token.subtitle")}</p>
            <ol className="token-steps">
              <li>{t("onboarding.token.step1")}</li>
              <li>{t("onboarding.token.step2")}</li>
              <li>{t("onboarding.token.step3")}</li>
            </ol>
            <button
              className="secondary-button"
              type="button"
              onClick={() => void openUrl(TOKEN_URL).catch(() => setStatus("failed"))}
            >
              {t("onboarding.token.openStudio")}
            </button>
            <label className="onboarding-token-field">
              <span>{t("settings.account.token")}</span>
              <input
                type="password"
                autoComplete="off"
                value={token}
                placeholder={t("settings.account.tokenPlaceholder")}
                onChange={(event) => {
                  setToken(event.currentTarget.value);
                  setStatus("idle");
                }}
              />
            </label>
            {status === "invalid" || status === "failed" ? (
              <p role="alert">{t(`onboarding.token.${status}`)}</p>
            ) : null}
            <div className="onboarding-actions">
              <button className="text-button" type="button" onClick={() => setStep(1)}>
                {t("actions.back")}
              </button>
              <button
                className="primary-button"
                type="button"
                disabled={!token || status === "validating"}
                onClick={() => void checkToken()}
              >
                {status === "validating"
                  ? t("onboarding.token.validating")
                  : t("onboarding.token.validate")}
              </button>
            </div>
          </>
        )}
      </section>
    </div>
  );
}
