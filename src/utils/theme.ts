/** App appearance. Default is dark — OS "default" on Linux often maps to light. */
export type ThemePreference = "dark" | "light" | "system";

export type EffectiveTheme = "dark" | "light";

export const THEME_STORAGE_KEY = "chaptercheck.ui_theme";

export function normalizeThemePreference(raw: unknown): ThemePreference {
  if (raw === "light" || raw === "system" || raw === "dark") return raw;
  return "dark";
}

export function resolveEffectiveTheme(
  preference: ThemePreference,
  systemPrefersLight: boolean,
): EffectiveTheme {
  if (preference === "light") return "light";
  if (preference === "dark") return "dark";
  return systemPrefersLight ? "light" : "dark";
}

export function readStoredThemePreference(): ThemePreference {
  try {
    return normalizeThemePreference(localStorage.getItem(THEME_STORAGE_KEY));
  } catch {
    return "dark";
  }
}

export function writeStoredThemePreference(preference: ThemePreference): void {
  try {
    localStorage.setItem(THEME_STORAGE_KEY, preference);
  } catch {
    /* private mode / quota — appearance still applies for this session */
  }
}

/** Apply preference to <html> for CSS + native color-scheme. */
export function applyThemePreference(preference: ThemePreference): EffectiveTheme {
  const systemPrefersLight =
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: light)").matches;
  const effective = resolveEffectiveTheme(preference, systemPrefersLight);
  const root = document.documentElement;
  root.dataset.theme = preference;
  root.style.colorScheme = effective;
  return effective;
}
