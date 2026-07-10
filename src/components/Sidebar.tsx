import { useTranslation } from "react-i18next";

import brandIcon from "../assets/paddledesk-icon.png";
import { formatNumber } from "../lib/format";
import { useApp, type ServiceId, type View } from "../stores/app";

const navigation: ReadonlyArray<{ view: View; key: string }> = [
  { view: "home", key: "nav.home" },
  { view: "viewer", key: "nav.viewer" },
  { view: "queue", key: "nav.queue" },
  { view: "usage", key: "nav.usage" },
  { view: "settings", key: "nav.settings" },
];

const serviceKeys: Record<ServiceId, string> = {
  vl16: "services.vl16",
  pp_ocr_v6: "services.ppOcrV6",
  structure_v3: "services.structureV3",
};

export function Sidebar() {
  const { t, i18n } = useTranslation();
  const view = useApp((state) => state.view);
  const setView = useApp((state) => state.setView);
  const service = useApp((state) => state.service);
  const used = useApp((state) => state.todayPages[state.service]);
  const quota = 20_000;
  const locale = i18n.resolvedLanguage ?? i18n.language;

  return (
    <aside className="sidebar">
      <div className="brand">
        <img className="brand-mark" src={brandIcon} alt="" aria-hidden="true" />
        <span>{t("app.name")}</span>
      </div>

      <nav className="navigation" aria-label={t("nav.label")}>
        {navigation.map((item) => (
          <button
            type="button"
            className="nav-button"
            data-active={view === item.view}
            aria-current={view === item.view ? "page" : undefined}
            onClick={() => setView(item.view)}
            key={item.view}
          >
            {t(item.key)}
          </button>
        ))}
      </nav>

      <section className="sidebar-usage" aria-label={t("usage.today")}>
        <span className="usage-label">{t("usage.today")}</span>
        <strong>
          {t("usage.summary", {
            used: formatNumber(used, locale),
            quota: formatNumber(quota, locale),
          })}
        </strong>
        <div
          className="usage-meter"
          role="progressbar"
          aria-label={t("usage.meterLabel", { used, quota })}
          aria-valuemin={0}
          aria-valuemax={quota}
          aria-valuenow={used}
        >
          <span style={{ width: `${Math.min(100, (used / quota) * 100)}%` }} />
        </div>
        <small>{t(serviceKeys[service])}</small>
      </section>
    </aside>
  );
}
