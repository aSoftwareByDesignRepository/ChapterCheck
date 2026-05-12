export type Locale = "en" | "de";

export const LOCALE_STORAGE_KEY = "chaptercheck.locale";

export function normalizeLocale(raw: string | null | undefined): Locale {
  if (raw && raw.toLowerCase().startsWith("de")) return "de";
  return "en";
}
