import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { UsageRing } from "../components/UsageRing";
import { formatDate, formatNumber } from "../lib/format";
import { getUsage, type UsageRow } from "../lib/ipc";
import { useApp, type ServiceId } from "../stores/app";

const services: ReadonlyArray<{ id: ServiceId; key: string }> = [
  { id: "vl16", key: "services.vl16" },
  { id: "pp_ocr_v6", key: "services.ppOcrV6" },
  { id: "structure_v3", key: "services.structureV3" },
];

const localDateKey = (date: Date) => {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
};

export function Usage() {
  const { t, i18n } = useTranslation();
  const [rows, setRows] = useState<UsageRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const liveTotals = useApp((state) => state.todayPages);

  useEffect(() => {
    let active = true;
    void getUsage(7).then(
      (usage) => {
        if (!active) return;
        setRows(usage);
        setLoading(false);
        setFailed(false);
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
  }, []);

  const totals = useMemo(() => {
    const values: Record<ServiceId, number> = {
      vl16: 0,
      pp_ocr_v6: 0,
      structure_v3: 0,
    };
    const today = localDateKey(new Date());
    rows.forEach((row) => {
      if (row.date === today) values[row.service] += row.pages;
    });
    services.forEach(({ id }) => {
      values[id] = Math.max(values[id], liveTotals[id]);
    });
    return values;
  }, [liveTotals, rows]);
  const days = useMemo(() => {
    const byDate = new Map<string, number>();
    rows.forEach((row) => byDate.set(row.date, (byDate.get(row.date) ?? 0) + row.pages));
    byDate.set(
      localDateKey(new Date()),
      services.reduce((sum, { id }) => sum + totals[id], 0),
    );
    return Array.from({ length: 7 }, (_, index) => {
      const date = new Date();
      date.setDate(date.getDate() - (6 - index));
      const key = localDateKey(date);
      return { date, pages: byDate.get(key) ?? 0 };
    });
  }, [rows, totals]);
  const maxDay = Math.max(1, ...days.map(({ pages }) => pages));

  return (
    <div className="usage-view">
      <h1>{t("viewTitles.usage")}</h1>
      {loading ? <p>{t("common.loading")}</p> : null}
      {failed ? <p role="alert">{t("usage.loadFailed")}</p> : null}
      {!loading && !failed ? (
        <>
          <section className="usage-rings" aria-label={t("usage.byService")}>
            {services.map((service) => (
              <UsageRing label={t(service.key)} used={totals[service.id]} key={service.id} />
            ))}
          </section>
          <section className="usage-services">
            <h2>{t("usage.serviceBreakdown")}</h2>
            {services.map((service) => (
              <div className="usage-service" key={service.id}>
                <span>{t(service.key)}</span>
                <i style={{ width: `${Math.min(100, (totals[service.id] / 20_000) * 100)}%` }} />
                <strong>{t("usage.pages", { count: formatNumber(totals[service.id], locale) })}</strong>
              </div>
            ))}
          </section>
          <section className="usage-history">
            <h2>{t("usage.lastSevenDays")}</h2>
            <div className="usage-days">
              {days.map(({ date, pages }) => (
                <div className="usage-day" key={localDateKey(date)}>
                  <span>{formatDate(date, locale)}</span>
                  <i style={{ width: `${(pages / maxDay) * 100}%` }} />
                  <strong>{t("usage.pages", { count: formatNumber(pages, locale) })}</strong>
                </div>
              ))}
            </div>
          </section>
        </>
      ) : null}
    </div>
  );
}
