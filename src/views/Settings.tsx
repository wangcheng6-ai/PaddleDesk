import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

import { useConfirm } from "../components/ConfirmDialog";
import { resolveLanguage } from "../i18n";
import {
  deleteToken,
  getCredentialStatus,
  getScreenshotHotkey,
  getSettings,
  revealToken,
  setScreenshotHotkey,
  setSettings,
  validateToken,
  type CredentialStatus,
} from "../lib/ipc";

type SettingsMap = Record<string, string>;

const defaults: SettingsMap = {
  language: "system",
  theme: "system",
  concurrency: "2",
  save_history: "1",
  proxy_mode: "system",
  proxy_address: "",
  autostart: "0",
};

interface Choice {
  value: string;
  label: string;
}

function Choices({
  label,
  value,
  choices,
  onChange,
}: {
  label: string;
  value: string;
  choices: Choice[];
  onChange: (value: string) => void;
}) {
  return (
    <div className="segmented-field">
      <span>{label}</span>
      <div className="segmented-control" role="group" aria-label={label}>
        {choices.map((choice) => (
          <button
            type="button"
            aria-pressed={choice.value === value}
            onClick={() => onChange(choice.value)}
            key={choice.value}
          >
            {choice.label}
          </button>
        ))}
      </div>
    </div>
  );
}

export function Settings({ onOpenOnboarding }: { onOpenOnboarding?: () => void }) {
  const { t, i18n } = useTranslation();
  const { confirm, confirmDialog } = useConfirm();
  const [settings, setLocalSettings] = useState(defaults);
  const [token, setToken] = useState("");
  const [credential, setCredential] = useState<CredentialStatus>({
    configured: false,
    last_four: null,
  });
  const [revealedToken, setRevealedToken] = useState<string | null>(null);
  const revealTimer = useRef<number | null>(null);
  const [hotkey, setHotkey] = useState("Ctrl+Alt+S");
  const [recordingHotkey, setRecordingHotkey] = useState(false);
  const hotkeyInput = useRef<HTMLInputElement>(null);
  const [tokenStatus, setTokenStatus] = useState<"idle" | "valid" | "invalid" | "failed">("idle");
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const [updateState, setUpdateState] = useState<
    "idle" | "checking" | "latest" | "failed" | "installing"
  >("idle");
  const [foundUpdate, setFoundUpdate] = useState<Update | null>(null);

  useEffect(() => {
    let active = true;
    void Promise.all([getSettings(), getCredentialStatus(), getScreenshotHotkey()]).then(
      ([values, status, shortcut]) => {
        if (!active) return;
        const merged = { ...defaults, ...values };
        setLocalSettings(merged);
        setCredential(status);
        setHotkey(shortcut);
        setLoading(false);
      },
      () => {
        if (!active) return;
        setLoading(false);
        setFailed(true);
      },
    );
    return () => {
      active = false;
      if (revealTimer.current !== null) window.clearTimeout(revealTimer.current);
      setRevealedToken(null);
    };
  }, []);

  const update = async (key: string, value: string) => {
    try {
      await setSettings({ [key]: value });
      setLocalSettings((current) => ({ ...current, [key]: value }));
      setFailed(false);
      if (key === "language") await i18n.changeLanguage(resolveLanguage(value));
      if (key === "theme") document.documentElement.dataset.theme = value;
    } catch {
      setFailed(true);
    }
  };

  const checkToken = async () => {
    try {
      const valid = await validateToken(token);
      setTokenStatus(valid ? "valid" : "invalid");
      if (valid) {
        setToken("");
        if (revealTimer.current !== null) window.clearTimeout(revealTimer.current);
        setRevealedToken(null);
        setCredential(await getCredentialStatus());
      }
    } catch {
      setTokenStatus("failed");
    }
  };

  const toggleReveal = async () => {
    if (revealedToken !== null) {
      if (revealTimer.current !== null) window.clearTimeout(revealTimer.current);
      revealTimer.current = null;
      setRevealedToken(null);
      return;
    }
    try {
      const value = await revealToken();
      setRevealedToken(value);
      if (revealTimer.current !== null) window.clearTimeout(revealTimer.current);
      revealTimer.current = window.setTimeout(() => {
        setRevealedToken(null);
        revealTimer.current = null;
      }, 30_000);
    } catch {
      setTokenStatus("failed");
    }
  };

  const copyToken = async () => {
    try {
      const value = revealedToken ?? (await revealToken());
      await navigator.clipboard.writeText(value);
    } catch {
      setTokenStatus("failed");
    }
  };

  const removeToken = async () => {
    if (!(await confirm(t("settings.account.confirmDelete")))) return;
    try {
      await deleteToken();
      setCredential({ configured: false, last_four: null });
      if (revealTimer.current !== null) window.clearTimeout(revealTimer.current);
      setRevealedToken(null);
      setToken("");
      setTokenStatus("idle");
    } catch {
      setTokenStatus("failed");
    }
  };

  const checkForUpdates = async () => {
    setUpdateState("checking");
    try {
      const available = await check();
      if (available?.version) {
        setFoundUpdate(available);
        setUpdateState("idle");
      } else {
        void available?.close();
        setFoundUpdate(null);
        setUpdateState("latest");
      }
    } catch {
      setFoundUpdate(null);
      setUpdateState("failed");
    }
  };

  const installFoundUpdate = async () => {
    if (!foundUpdate) return;
    setUpdateState("installing");
    try {
      await foundUpdate.downloadAndInstall();
      await relaunch();
    } catch {
      setUpdateState("failed");
    }
  };

  const recordHotkey = async (event: KeyboardEvent<HTMLInputElement>) => {
    if (!recordingHotkey) return;
    event.preventDefault();
    if (["Control", "Alt", "Shift", "Meta"].includes(event.key)) return;
    const modifiers = [
      event.ctrlKey ? "Ctrl" : null,
      event.altKey ? "Alt" : null,
      event.metaKey ? "Win" : null,
      event.shiftKey ? "Shift" : null,
    ].filter(Boolean) as string[];
    const key = event.key.length === 1 ? event.key.toUpperCase() : event.key.toUpperCase();
    const candidate = [...modifiers, key].join("+");
    try {
      const saved = await setScreenshotHotkey(candidate);
      setHotkey(saved);
      setLocalSettings((current) => ({
        ...current,
        screenshot_hotkey_available: "1",
      }));
      setRecordingHotkey(false);
      setFailed(false);
    } catch {
      setRecordingHotkey(false);
      setFailed(true);
    }
  };

  return (
    <div className="settings-view">
      {confirmDialog}
      <h1>{t("viewTitles.settings")}</h1>
      {loading ? <p>{t("common.loading")}</p> : null}
      {failed ? <p role="alert">{t("settings.saveFailed")}</p> : null}
      {!loading ? (
        <div className="settings-grid">
          <section className="settings-card">
            <h2>{t("settings.account.title")}</h2>
            <p>{t("settings.account.cloudDisclosure")}</p>
            {credential.configured ? (
              <div className="token-field">
                <span>{t("settings.account.currentToken")}</span>
                <div className="token-display">
                  <input
                    readOnly
                    type="text"
                    value={revealedToken ?? `••••••••${credential.last_four ?? ""}`}
                    aria-label={t("settings.account.currentToken")}
                  />
                  <button type="button" onClick={() => void toggleReveal()}>
                    {revealedToken !== null ? t("actions.hide") : t("actions.show")}
                  </button>
                  <button type="button" onClick={() => void copyToken()}>
                    {t("actions.copy")}
                  </button>
                  <button className="danger-button" type="button" onClick={() => void removeToken()}>
                    {t("actions.delete")}
                  </button>
                </div>
              </div>
            ) : (
              <p className="credential-status">{t("settings.account.notConfigured")}</p>
            )}
            <label>
              <span>
                {credential.configured
                  ? t("settings.account.replaceToken")
                  : t("settings.account.token")}
              </span>
              <input
                type="password"
                value={token}
                autoComplete="off"
                placeholder={t("settings.account.tokenPlaceholder")}
                onChange={(event) => {
                  setToken(event.currentTarget.value);
                  setTokenStatus("idle");
                }}
              />
            </label>
            <button className="primary-button" type="button" disabled={!token} onClick={checkToken}>
              {t("settings.account.validateToken")}
            </button>
            {tokenStatus !== "idle" ? (
              <span role={tokenStatus === "valid" ? "status" : "alert"}>
                {t(`settings.account.${tokenStatus}`)}
              </span>
            ) : null}
            <button className="secondary-button" type="button" onClick={onOpenOnboarding}>
              {t("settings.account.openOnboarding")}
            </button>
          </section>

          <section className="settings-card">
            <h2>{t("settings.recognition.title")}</h2>
            <label>
              <span>{t("settings.recognition.concurrency")}</span>
              <select
                value={settings.concurrency}
                onChange={(event) => void update("concurrency", event.currentTarget.value)}
              >
                {[1, 2, 3, 4].map((value) => <option value={value} key={value}>{value}</option>)}
              </select>
            </label>
            <p>{t("settings.recognition.concurrencyRestart")}</p>
            <label>
              <span>{t("settings.recognition.hotkeyLabel")}</span>
              <input
                ref={hotkeyInput}
                readOnly
                value={recordingHotkey ? t("settings.recognition.recording") : hotkey}
                onKeyDown={(event) => void recordHotkey(event)}
                onFocus={() => setRecordingHotkey(true)}
                onBlur={() => setRecordingHotkey(false)}
              />
            </label>
            <div className="button-row">
              <button
                type="button"
                onClick={() => {
                  setRecordingHotkey(true);
                  hotkeyInput.current?.focus();
                }}
              >
                {t("settings.recognition.recordHotkey")}
              </button>
              <button
                type="button"
                onClick={() =>
                  void setScreenshotHotkey("Ctrl+Alt+S").then(
                    (saved) => {
                      setHotkey(saved);
                      setLocalSettings((current) => ({
                        ...current,
                        screenshot_hotkey_available: "1",
                      }));
                      setFailed(false);
                    },
                    () => setFailed(true),
                  )
                }
              >
                {t("settings.recognition.restoreHotkey")}
              </button>
            </div>
            {settings.screenshot_hotkey_available === "0" ? (
              <p role="alert">{t("settings.recognition.hotkeyUnavailable")}</p>
            ) : null}
          </section>

          <section className="settings-card">
            <h2>{t("settings.privacy.title")}</h2>
            <label className="toggle-field">
              <input
                type="checkbox"
                checked={settings.save_history === "1"}
                onChange={(event) => void update("save_history", event.currentTarget.checked ? "1" : "0")}
              />
              <span>{t("settings.privacy.saveHistory")}</span>
            </label>
            <p>{t("settings.privacy.privacyHint")}</p>
            <Choices
              label={t("settings.privacy.proxyMode")}
              value={settings.proxy_mode}
              choices={[
                { value: "system", label: t("settings.proxy.system") },
                { value: "custom", label: t("settings.proxy.custom") },
                { value: "direct", label: t("settings.proxy.direct") },
              ]}
              onChange={(value) => void update("proxy_mode", value)}
            />
            {settings.proxy_mode === "custom" ? (
              <label>
                <span>{t("settings.privacy.proxyAddress")}</span>
                <input
                  value={settings.proxy_address}
                  placeholder={t("settings.privacy.proxyAddressPlaceholder")}
                  onChange={(event) => setLocalSettings((current) => ({ ...current, proxy_address: event.currentTarget.value }))}
                  onBlur={(event) => void update("proxy_address", event.currentTarget.value)}
                />
              </label>
            ) : null}
          </section>

          <section className="settings-card">
            <h2>{t("settings.appearance.title")}</h2>
            <Choices
              label={t("settings.appearance.language")}
              value={settings.language}
              choices={[
                { value: "system", label: t("settings.language.system") },
                { value: "zh-CN", label: t("settings.language.zhCN") },
                { value: "en", label: t("settings.language.en") },
              ]}
              onChange={(value) => void update("language", value)}
            />
            <Choices
              label={t("settings.appearance.theme")}
              value={settings.theme}
              choices={[
                { value: "system", label: t("settings.theme.system") },
                { value: "light", label: t("settings.theme.light") },
                { value: "dark", label: t("settings.theme.dark") },
              ]}
              onChange={(value) => void update("theme", value)}
            />
            <label className="toggle-field">
              <input
                type="checkbox"
                checked={settings.autostart === "1"}
                onChange={(event) => void update("autostart", event.currentTarget.checked ? "1" : "0")}
              />
              <span>{t("settings.appearance.autostart")}</span>
            </label>
          </section>

          <section className="settings-card">
            <h2>{t("settings.about.title")}</h2>
            <p>{t("settings.about.updateHint")}</p>
            <div className="button-row">
              <button
                className="secondary-button"
                type="button"
                disabled={updateState === "checking" || updateState === "installing"}
                onClick={() => void checkForUpdates()}
              >
                {updateState === "checking"
                  ? t("settings.about.checking")
                  : t("settings.about.checkUpdates")}
              </button>
              {foundUpdate ? (
                <button
                  className="primary-button"
                  type="button"
                  disabled={updateState === "installing"}
                  onClick={() => void installFoundUpdate()}
                >
                  {updateState === "installing"
                    ? t("update.installing")
                    : t("settings.about.installFound", { version: foundUpdate.version })}
                </button>
              ) : null}
            </div>
            {updateState === "latest" ? <p role="status">{t("settings.about.latest")}</p> : null}
            {updateState === "failed" ? (
              <p role="alert">{t("settings.about.checkFailed")}</p>
            ) : null}
          </section>
        </div>
      ) : null}
    </div>
  );
}
