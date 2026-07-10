import { useTranslation } from "react-i18next";

import { useApp, type View } from "../stores/app";

const navigation: ReadonlyArray<{ view: View; key: string }> = [
  { view: "home", key: "nav.home" },
  { view: "viewer", key: "nav.viewer" },
  { view: "queue", key: "nav.queue" },
  { view: "history", key: "nav.history" },
  { view: "usage", key: "nav.usage" },
  { view: "settings", key: "nav.settings" },
];

export function Sidebar() {
  const { t } = useTranslation();
  const view = useApp((state) => state.view);
  const setView = useApp((state) => state.setView);

  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="brand-mark" aria-hidden="true">
          {t("app.mark")}
        </span>
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
        <strong>{t("usage.summary", { used: "0", quota: "20,000" })}</strong>
        <div
          className="usage-meter"
          role="progressbar"
          aria-label={t("usage.meterLabel", { used: 0, quota: 20000 })}
          aria-valuemin={0}
          aria-valuemax={20000}
          aria-valuenow={0}
        >
          <span />
        </div>
      </section>
    </aside>
  );
}
