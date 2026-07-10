import { useState } from "react";
import { useTranslation } from "react-i18next";

import { setSettings, startCapture } from "../lib/ipc";
import { useApp, type ServiceId } from "../stores/app";

const services: ReadonlyArray<{ id: ServiceId; key: string }> = [
  { id: "vl16", key: "services.vl16" },
  { id: "pp_ocr_v6", key: "services.ppOcrV6" },
  { id: "structure_v3", key: "services.structureV3" },
];

export function TopBar() {
  const { t } = useTranslation();
  const service = useApp((state) => state.service);
  const setService = useApp((state) => state.setService);
  const setView = useApp((state) => state.setView);
  const tasks = useApp((state) => state.tasks);
  const [captureFailed, setCaptureFailed] = useState(false);
  const [switchFailed, setSwitchFailed] = useState(false);

  return (
    <header className="topbar">
      <div
        className="service-selector"
        role="group"
        aria-label={t("services.selectorLabel")}
      >
        {services.map((item) => (
          <button
            type="button"
            aria-pressed={service === item.id}
            onClick={() => {
              if (service === item.id) return;
              void setSettings({ current_service: item.id }).then(
                () => {
                  setService(item.id);
                  setSwitchFailed(false);
                },
                () => setSwitchFailed(true),
              );
            }}
            key={item.id}
          >
            <span>{t(item.key)}</span>
            {(() => {
              const scoped = tasks.filter((task) => task.service === item.id);
              const active = scoped.filter((task) =>
                ["pending", "uploading", "processing"].includes(
                  task.status ?? "pending",
                ),
              ).length;
              const failed = scoped.filter((task) => task.status === "failed").length;
              return (
                <span className="service-badges" aria-label={t("services.badges", { active, failed })}>
                  {active > 0 ? <b className="service-badge active">{active}</b> : null}
                  {failed > 0 ? <b className="service-badge failed">{failed}</b> : null}
                </span>
              );
            })()}
          </button>
        ))}
      </div>
      <button
        className="capture-button"
        type="button"
        title={t("actions.captureHint")}
        onClick={() =>
          void startCapture().then(
            () => {
              setCaptureFailed(false);
              setView("queue");
            },
            (error) => {
              setCaptureFailed(true);
              if (String(error).toLowerCase().includes("authentication")) {
                setView("settings");
              }
            },
          )
        }
      >
        {t("actions.capture")}
      </button>
      {captureFailed ? <span role="alert">{t("runtime.captureFailed")}</span> : null}
      {switchFailed ? <span role="alert">{t("runtime.serviceSwitchFailed")}</span> : null}
    </header>
  );
}
