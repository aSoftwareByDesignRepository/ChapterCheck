import { describe, expect, it } from "vitest";
import {
  normalizeThemePreference,
  resolveEffectiveTheme,
} from "./theme";

describe("theme preference", () => {
  it("defaults unknown values to dark (never OS-light by accident)", () => {
    expect(normalizeThemePreference(null)).toBe("dark");
    expect(normalizeThemePreference("")).toBe("dark");
    expect(normalizeThemePreference("auto")).toBe("dark");
    expect(normalizeThemePreference("dark")).toBe("dark");
    expect(normalizeThemePreference("light")).toBe("light");
    expect(normalizeThemePreference("system")).toBe("system");
  });

  it("resolves system from the media query without flipping dark preference", () => {
    expect(resolveEffectiveTheme("dark", true)).toBe("dark");
    expect(resolveEffectiveTheme("dark", false)).toBe("dark");
    expect(resolveEffectiveTheme("light", false)).toBe("light");
    expect(resolveEffectiveTheme("system", true)).toBe("light");
    expect(resolveEffectiveTheme("system", false)).toBe("dark");
  });
});
