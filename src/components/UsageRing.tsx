import type { CSSProperties } from "react";
import { useTranslation } from "react-i18next";

import { formatNumber } from "../lib/format";

interface UsageRingProps {
  label: string;
  used: number;
  quota?: number;
}

export function UsageRing({ label, used, quota = 20_000 }: UsageRingProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const percent = Math.min(100, (used / quota) * 100);

  return (
    <article className="usage-card">
      <div
        className="usage-ring"
        role="progressbar"
        aria-label={t("usage.meterLabel", { used, quota })}
        aria-valuemin={0}
        aria-valuemax={quota}
        aria-valuenow={used}
        style={{ "--usage-percent": `${percent}%` } as CSSProperties}
      >
        <span>{Math.round(percent)}%</span>
      </div>
      <strong>{label}</strong>
      <span>{t("usage.summary", {
        used: formatNumber(used, locale),
        quota: formatNumber(quota, locale),
      })}</span>
    </article>
  );
}
