export function formatNumber(value: number, locale: string) {
  return new Intl.NumberFormat(locale).format(value);
}

export function formatDate(value: Date | number, locale: string) {
  return new Intl.DateTimeFormat(locale, { dateStyle: "medium" }).format(value);
}
