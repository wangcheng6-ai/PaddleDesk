import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { resolveLanguage } from "../i18n";
import { getSettings, setSettings, validateToken } from "../lib/ipc";
import { useApp, type ServiceId } from "../stores/app";

type SettingsMap = Record<string, string>;

const defaults: SettingsMap = {
  language: "system",
  theme: "system",
  default_service: "vl16",
  concurrency: "2",
  privacy_mode: "0",
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

export function Settings() {
  const { t, i18n } = useTranslation();
  const [settings, setLocalSettings] = useState(defaults);
  const [token, setToken] = useState("");
  const [tokenStatus, setTokenStatus] = useState<"idle" | "valid" | "invalid" | "failed">("idle");
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const setService = useApp((state) => state.setService);

  useEffect(() => {
    let active = true;
    void getSettings().then(
      (values) => {
        if (!active) return;
        const merged = { ...defaults, ...values };
        setLocalSettings(merged);
        setService(merged.default_service as ServiceId);
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
    };
  }, [setService]);

  const update = async (key: string, value: string) => {
    try {
      await setSettings({ [key]: value });
      setLocalSettings((current) => ({ ...current, [key]: value }));
      setFailed(false);
      if (key === "language") await i18n.changeLanguage(resolveLanguage(value));
      if (key === "theme") document.documentElement.dataset.theme = value;
      if (key === "default_service") setService(value as ServiceId);
    } catch {
      setFailed(true);
    }
  };

  const checkToken = async () => {
    try {
      const valid = await validateToken(token);
      setTokenStatus(valid ? "valid" : "invalid");
      if (valid) setToken("");
    } catch {
      setTokenStatus("failed");
    }
  };

  return (
    <div className="settings-view">
      <h1>{t("viewTitles.settings")}</h1>
      {loading ? <p>{t("common.loading")}</p> : null}
      {failed ? <p role="alert">{t("settings.saveFailed")}</p> : null}
      {!loading ? (
        <div className="settings-grid">
          <section className="settings-card">
            <h2>{t("settings.account.title")}</h2>
            <p>{t("settings.account.cloudDisclosure")}</p>
            <label>
              <span>{t("settings.account.token")}</span>
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
          </section>

          <section className="settings-card">
            <h2>{t("settings.recognition.title")}</h2>
            <label>
              <span>{t("settings.recognition.defaultService")}</span>
              <select
                value={settings.default_service}
                onChange={(event) => void update("default_service", event.currentTarget.value)}
              >
                <option value="vl16">{t("services.vl16")}</option>
                <option value="pp_ocr_v6">{t("services.ppOcrV6")}</option>
                <option value="structure_v3">{t("services.structureV3")}</option>
              </select>
            </label>
            <label>
              <span>{t("settings.recognition.concurrency")}</span>
              <select
                value={settings.concurrency}
                onChange={(event) => void update("concurrency", event.currentTarget.value)}
              >
                {[1, 2, 3, 4].map((value) => <option value={value} key={value}>{value}</option>)}
              </select>
            </label>
            <p>{t("settings.recognition.hotkey", { hotkey: "Ctrl+Shift+P" })}</p>
          </section>

          <section className="settings-card">
            <h2>{t("settings.privacy.title")}</h2>
            <label className="toggle-field">
              <input
                type="checkbox"
                checked={settings.privacy_mode === "1"}
                onChange={(event) => void update("privacy_mode", event.currentTarget.checked ? "1" : "0")}
              />
              <span>{t("settings.privacy.privacyMode")}</span>
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
        </div>
      ) : null}
    </div>
  );
}
