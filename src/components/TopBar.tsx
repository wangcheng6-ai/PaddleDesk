import { useState } from "react";
import { useTranslation } from "react-i18next";

import { startCapture } from "../lib/ipc";
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
  const [captureFailed, setCaptureFailed] = useState(false);

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
            onClick={() => setService(item.id)}
            key={item.id}
          >
            {t(item.key)}
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
            () => setCaptureFailed(true),
          )
        }
      >
        {t("actions.capture")}
      </button>
      {captureFailed ? <span role="alert">{t("runtime.captureFailed")}</span> : null}
    </header>
  );
}
