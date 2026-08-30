import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import {
  applyThemePreference,
  resolveEffectiveTheme,
} from "./theme";

/** Relative luminance (sRGB) for WCAG contrast checks. */
function luminance(hex: string): number {
  const h = hex.replace("#", "");
  const n = parseInt(h.length === 3 ? h.split("").map((c) => c + c).join("") : h, 16);
  const channel = (v: number) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  const r = channel((n >> 16) & 255);
  const g = channel((n >> 8) & 255);
  const b = channel(n & 255);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrastRatio(fg: string, bg: string): number {
  const L1 = luminance(fg);
  const L2 = luminance(bg);
  const lighter = Math.max(L1, L2);
  const darker = Math.min(L1, L2);
  return (lighter + 0.05) / (darker + 0.05);
}

describe("dark theme tokens (Bachus)", () => {
  const css = readFileSync(resolve(__dirname, "../styles.css"), "utf8");

  it("defines --text-muted so playlist hints never inherit garbage", () => {
    expect(css).toMatch(/--text-muted:\s*var\(--muted\)/);
  });

  it("keeps menus, modals, and mini-player on tokens (no night hex chrome)", () => {
    expect(css).not.toMatch(/\.menubar-dropdown\{[^}]*#1a1f2e/s);
    expect(css).not.toMatch(/\.modal-sheet\{[^}]*#1c2230/s);
    expect(css).not.toMatch(/\.mini-player\{[^}]*#12151f/s);
    expect(css).toMatch(/\.menubar-dropdown[\s\S]{0,200}background:\s*var\(--surface\)/);
    expect(css).toMatch(/\.modal-sheet[\s\S]{0,200}background:\s*var\(--surface\)/);
    expect(css).toMatch(/\.mini-player[\s\S]{0,220}background:\s*var\(--bg-elevated\)/);
  });

  it("meets WCAG AA for body text and muted labels on dark stage", () => {
    // Values from :root dark tokens (must stay in sync with styles.css).
    const bg = "#0e1014";
    const text = "#f5f7fb";
    const muted = "#c2c8d6";
    expect(contrastRatio(text, bg)).toBeGreaterThanOrEqual(7); // AAA body
    expect(contrastRatio(muted, bg)).toBeGreaterThanOrEqual(4.5); // AA normal text
  });

  it("applies dark preference even when the OS claims light", () => {
    window.matchMedia = ((query: string) =>
      ({
        matches: query.includes("prefers-color-scheme: light"),
        media: query,
        addEventListener: () => undefined,
        removeEventListener: () => undefined,
        onchange: null,
        addListener: () => undefined,
        removeListener: () => undefined,
        dispatchEvent: () => false,
      })) as typeof window.matchMedia;
    expect(resolveEffectiveTheme("dark", true)).toBe("dark");
    expect(applyThemePreference("dark")).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.style.colorScheme).toBe("dark");
  });
});
