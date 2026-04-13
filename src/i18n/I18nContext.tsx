import { createContext, useContext, useMemo } from "react";
import type { ReactNode } from "react";

import en from "./locales/en.json";
import ko from "./locales/ko.json";
import ja from "./locales/ja.json";
import zhCN from "./locales/zh-CN.json";
import zhTW from "./locales/zh-TW.json";
import fr from "./locales/fr.json";
import es from "./locales/es.json";
import de from "./locales/de.json";
import tr from "./locales/tr.json";
import it from "./locales/it.json";

export type Locale = "en" | "ko" | "ja" | "zh-CN" | "zh-TW" | "fr" | "es" | "de" | "tr" | "it";

type Translations = Record<string, string>;

const locales: Record<Locale, Translations> = {
  en, ko, ja,
  "zh-CN": zhCN,
  "zh-TW": zhTW,
  fr, es, de, tr, it,
};

export const LANGUAGE_OPTIONS: { id: Locale; label: string; englishName: string }[] = [
  { id: "en", label: "English", englishName: "English" },
  { id: "ko", label: "한국어", englishName: "Korean" },
  { id: "ja", label: "日本語", englishName: "Japanese" },
  { id: "zh-CN", label: "简体中文", englishName: "Simplified Chinese" },
  { id: "zh-TW", label: "繁體中文", englishName: "Traditional Chinese" },
  { id: "fr", label: "Français", englishName: "French" },
  { id: "es", label: "Español", englishName: "Spanish" },
  { id: "de", label: "Deutsch", englishName: "German" },
  { id: "tr", label: "Türkçe", englishName: "Turkish" },
  { id: "it", label: "Italiano", englishName: "Italian" },
];

export const LANGUAGE_NAMES: Record<string, string> = Object.fromEntries(
  LANGUAGE_OPTIONS.map((l) => [l.id, l.englishName]),
);

type TFunction = (key: string, params?: Record<string, string | number>) => string;

const I18nContext = createContext<TFunction>((key) => key);

interface Props {
  locale: Locale;
  children: ReactNode;
}

export function I18nProvider({ locale, children }: Props) {
  const t: TFunction = useMemo(() => {
    const messages = locales[locale] ?? en;
    const fallback: Translations = en;

    return (key: string, params?: Record<string, string | number>) => {
      let text = (messages as Translations)[key] ?? fallback[key] ?? key;
      if (params) {
        for (const [k, v] of Object.entries(params)) {
          text = text.replace(`{{${k}}}`, String(v));
        }
      }
      return text;
    };
  }, [locale]);

  return (
    <I18nContext.Provider value={t}>
      {children}
    </I18nContext.Provider>
  );
}

export function useI18n(): TFunction {
  return useContext(I18nContext);
}
