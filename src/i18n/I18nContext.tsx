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

export type Locale = "en" | "ko" | "ja" | "zh-CN" | "zh-TW" | "fr" | "es" | "de";

type Translations = Record<string, string>;

const locales: Record<Locale, Translations> = {
  en, ko, ja,
  "zh-CN": zhCN,
  "zh-TW": zhTW,
  fr, es, de,
};

export const LANGUAGE_OPTIONS: { id: Locale; label: string }[] = [
  { id: "en", label: "English" },
  { id: "ko", label: "한국어" },
  { id: "ja", label: "日本語" },
  { id: "zh-CN", label: "简体中文" },
  { id: "zh-TW", label: "繁體中文" },
  { id: "fr", label: "Français" },
  { id: "es", label: "Español" },
  { id: "de", label: "Deutsch" },
];

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
