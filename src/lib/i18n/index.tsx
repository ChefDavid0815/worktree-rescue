import { createContext, useCallback, useContext, useMemo, type ReactNode } from "react";
import { en } from "./en";
import { zh } from "./zh";

export type Locale = "en" | "zh-CN";
export type MessageKey = keyof typeof en;
export type Params = Record<string, string | number | null | undefined>;

const dictionaries: Record<Locale, Record<string, string>> = { en, "zh-CN": zh };

export function resolveLocale(pref: string | undefined | null): Locale {
  if (pref === "en" || pref === "zh-CN") return pref;
  const nav = typeof navigator !== "undefined" ? navigator.language : "en";
  return nav.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

function interpolate(template: string, params?: Params): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (_, k: string) => {
    const v = params[k];
    return v === undefined || v === null ? "" : String(v);
  });
}

export function translate(locale: Locale, key: string, params?: Params): string {
  const dict = dictionaries[locale];
  const raw = dict[key] ?? en[key as MessageKey] ?? key;
  return interpolate(raw, params);
}

interface I18nValue {
  locale: Locale;
  /** Typed lookup for static keys. */
  t: (key: MessageKey, params?: Params) => string;
  /** Count-aware lookup: uses `key_one` / `key_other`. */
  tn: (key: string, count: number, params?: Params) => string;
  /** Dynamic lookup with fallback (rule ids, event kinds, reasons). */
  td: (key: string, fallback?: string, params?: Params) => string;
  has: (key: string) => boolean;
}

const I18nContext = createContext<I18nValue | null>(null);

export function I18nProvider({ locale, children }: { locale: Locale; children: ReactNode }) {
  const t = useCallback((key: MessageKey, params?: Params) => translate(locale, key, params), [locale]);
  const has = useCallback(
    (key: string) => key in dictionaries[locale] || key in en,
    [locale],
  );
  const tn = useCallback(
    (key: string, count: number, params?: Params) => {
      const form = locale === "en" && count === 1 ? "one" : "other";
      return translate(locale, `${key}_${form}`, { count, ...params });
    },
    [locale],
  );
  const td = useCallback(
    (key: string, fallback?: string, params?: Params) =>
      has(key) ? translate(locale, key, params) : (fallback ?? key),
    [locale, has],
  );
  const value = useMemo(() => ({ locale, t, tn, td, has }), [locale, t, tn, td, has]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  const v = useContext(I18nContext);
  if (!v) throw new Error("useI18n outside I18nProvider");
  return v;
}
