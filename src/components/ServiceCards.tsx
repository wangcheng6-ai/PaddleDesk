import { useTranslation } from "react-i18next";

import { useApp, type ServiceId } from "../stores/app";

const services: Array<{
  id: ServiceId;
  name: string;
  tag: string;
  description: string;
  fit: string;
}> = [
  {
    id: "vl16",
    name: "services.vl16",
    tag: "home.serviceCards.vl16.tag",
    description: "home.serviceCards.vl16.description",
    fit: "home.serviceCards.vl16.fit",
  },
  {
    id: "pp_ocr_v6",
    name: "services.ppOcrV6",
    tag: "home.serviceCards.ppOcrV6.tag",
    description: "home.serviceCards.ppOcrV6.description",
    fit: "home.serviceCards.ppOcrV6.fit",
  },
  {
    id: "structure_v3",
    name: "services.structureV3",
    tag: "home.serviceCards.structureV3.tag",
    description: "home.serviceCards.structureV3.description",
    fit: "home.serviceCards.structureV3.fit",
  },
];

export function ServiceCards() {
  const { t } = useTranslation();
  const service = useApp((state) => state.service);
  const setService = useApp((state) => state.setService);

  return (
    <div className="service-cards" role="group" aria-label={t("home.selectService")}>
      {services.map((item) => (
        <button
          className="service-card"
          data-selected={service === item.id}
          type="button"
          aria-pressed={service === item.id}
          key={item.id}
          onClick={() => setService(item.id)}
        >
          <span className="service-card-title">
            <strong>{t(item.name)}</strong>
            <span className="service-tag">{t(item.tag)}</span>
          </span>
          <span>{t(item.description)}</span>
          <small>{t(item.fit)}</small>
        </button>
      ))}
    </div>
  );
}
