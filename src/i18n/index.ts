import i18next from "i18next";
import { initReactI18next } from "react-i18next";

import { getSettings } from "../lib/ipc";
import en from "./en.json";
import zhCN from "./zh-CN.json";

export type Language = "zh-CN" | "en";

export function resolveLanguage(
  setting: unknown,
  systemLanguage = globalThis.navigator?.language ?? "en",
): Language {
  if (setting === "zh-CN" || setting === "en") return setting;
  return systemLanguage.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

i18next.use(initReactI18next);

export async function initI18n() {
  const settings = await getSettings();
  await i18next.init({
    resources: {
      "zh-CN": { translation: zhCN },
      en: { translation: en },
    },
    lng: resolveLanguage(settings.language),
    fallbackLng: "en",
    supportedLngs: ["zh-CN", "en"],
    load: "currentOnly",
    interpolation: { escapeValue: false },
  });
  return i18next;
}
