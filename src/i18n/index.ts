import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";

import enUS from "./locales/en-US.json";
import zhCN from "./locales/zh-CN.json";

/**
 * Detect the host operating system / browser language preference.
 *
 * @returns "zh-CN" if the system language is Chinese, otherwise "en-US".
 */
export function detectSystemLocale(): "zh-CN" | "en-US" {
  const navLang = typeof navigator !== "undefined" ? navigator.language || "" : "";
  if (navLang.toLowerCase().startsWith("zh")) {
    return "zh-CN";
  }
  return "en-US";
}

/**
 * Resolves configured preference ("auto" | "zh-CN" | "en-US") to an active locale.
 *
 * @param pref - Configured language preference.
 * @returns An active language code ("zh-CN" or "en-US").
 */
export function resolveLocale(pref: string | null | undefined): "zh-CN" | "en-US" {
  if (pref === "zh-CN" || pref === "zh") {
    return "zh-CN";
  }
  if (pref === "en-US" || pref === "en") {
    return "en-US";
  }
  return detectSystemLocale();
}

/**
 * Changes active language in i18next and notifies Tauri desktop shell.
 *
 * @param pref - Selected language setting ("auto", "zh-CN", "en-US").
 */
export async function setAppLanguage(pref: string): Promise<void> {
  const actual = resolveLocale(pref);
  await i18n.changeLanguage(actual);
  try {
    await invoke("set_locale", { locale: actual });
  } catch {
    // Ignore when running outside Tauri or if command is not yet registered
  }
}

i18n
  .use(initReactI18next)
  .init({
    resources: {
      "en-US": {
        translation: enUS,
      },
      "zh-CN": {
        translation: zhCN,
      },
    },
    lng: detectSystemLocale(),
    fallbackLng: "en-US",
    interpolation: {
      escapeValue: false, // React already safe from XSS
    },
  });

export default i18n;
