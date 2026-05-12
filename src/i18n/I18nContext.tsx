import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { bundles, interpolate } from "./bundles";
import { LOCALE_STORAGE_KEY, type Locale } from "./types";

type I18nValue = {
  locale: Locale;
  /** Updates UI language, `document.documentElement.lang`, and localStorage. */
  setLocale: (loc: Locale) => void;
  t: (key: string, vars?: Record<string, string | number>) => string;
};

const I18nContext = createContext<I18nValue | null>(null);

function readStoredLocale(): Locale | null {
  try {
    const s = localStorage.getItem(LOCALE_STORAGE_KEY);
    if (s === "de" || s === "en") return s;
  } catch {
    /* private mode */
  }
  return null;
}

function browserDefaultLocale(): Locale {
  if (typeof navigator !== "undefined" && navigator.language.toLowerCase().startsWith("de")) {
    return "de";
  }
  return "en";
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(() => readStoredLocale() ?? browserDefaultLocale());

  const setLocale = useCallback((loc: Locale) => {
    setLocaleState(loc);
    try {
      localStorage.setItem(LOCALE_STORAGE_KEY, loc);
    } catch {
      /* ignore */
    }
    document.documentElement.lang = loc === "de" ? "de" : "en";
    document.title = bundles[loc]["app.title"] ?? "ChapterCheck";
  }, []);

  useEffect(() => {
    document.documentElement.lang = locale === "de" ? "de" : "en";
    document.title = bundles[locale]["app.title"] ?? "ChapterCheck";
  }, [locale]);

  const t = useCallback(
    (key: string, vars?: Record<string, string | number>) => {
      const raw = bundles[locale][key] ?? bundles.en[key] ?? key;
      return interpolate(raw, vars);
    },
    [locale],
  );

  const value = useMemo(() => ({ locale, setLocale, t }), [locale, setLocale, t]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  const v = useContext(I18nContext);
  if (!v) throw new Error("useI18n must be used within I18nProvider");
  return v;
}
