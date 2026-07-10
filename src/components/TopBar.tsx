import { useTranslation } from "react-i18next";

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
    </header>
  );
}
