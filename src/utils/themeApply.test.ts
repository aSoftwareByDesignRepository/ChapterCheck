import { afterEach, describe, expect, it, vi } from "vitest";
import {
  applyThemePreference,
  resolveEffectiveTheme,
  THEME_STORAGE_KEY,
  writeStoredThemePreference,
} from "./theme";

describe("applyThemePreference against lying OS light", () => {
  afterEach(() => {
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.style.colorScheme = "";
    localStorage.removeItem(THEME_STORAGE_KEY);
    vi.restoreAllMocks();
  });

  it("keeps dark chrome when preference is dark even if OS prefers light", () => {
    vi.spyOn(window, "matchMedia").mockImplementation((query: string) => {
      return {
        matches: query.includes("prefers-color-scheme: light"),
        media: query,
        onchange: null,
        addListener: () => undefined,
        removeListener: () => undefined,
        addEventListener: () => undefined,
        removeEventListener: () => undefined,
        dispatchEvent: () => false,
      } as MediaQueryList;
    });

    const effective = applyThemePreference("dark");
    expect(effective).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.style.colorScheme).toBe("dark");
    expect(resolveEffectiveTheme("dark", true)).toBe("dark");
  });

  it("persists preference for the next launch", () => {
    writeStoredThemePreference("dark");
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
  });
});
