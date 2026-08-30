import { describe, expect, it } from "vitest";
import { isSafeCoverPath } from "./coverUrl";
import { clampSleepMinutes, formatSleepRemaining } from "./sleepDisplay";

describe("formatSleepRemaining", () => {
  it("returns null when the timer is off", () => {
    expect(formatSleepRemaining(null, 1_000)).toBeNull();
    expect(formatSleepRemaining(0, 1_000)).toBeNull();
  });

  it("formats minutes and seconds", () => {
    expect(formatSleepRemaining(90_000, 0)).toBe("1:30");
  });

  it("formats hours when needed", () => {
    expect(formatSleepRemaining(3_661_000, 0)).toBe("1:01:01");
  });

  it("does not go negative after the deadline", () => {
    expect(formatSleepRemaining(500, 2_000)).toBe("0:00");
  });
});

describe("clampSleepMinutes", () => {
  it("rejects empty and oversized values", () => {
    expect(clampSleepMinutes(0)).toBeNull();
    expect(clampSleepMinutes(181)).toBeNull();
    expect(clampSleepMinutes(15)).toBe(15);
    expect(clampSleepMinutes(1)).toBe(1);
    expect(clampSleepMinutes(180)).toBe(180);
    expect(clampSleepMinutes(15.9)).toBe(15);
    expect(clampSleepMinutes(Number.NaN)).toBeNull();
  });
});

describe("isSafeCoverPath", () => {
  it("rejects empty, traversal, and NUL", () => {
    expect(isSafeCoverPath(null)).toBe(false);
    expect(isSafeCoverPath("")).toBe(false);
    expect(isSafeCoverPath("../etc/passwd")).toBe(false);
    expect(isSafeCoverPath("/covers/foo\0.png")).toBe(false);
    expect(isSafeCoverPath("/home/user/.local/share/chaptercheck/library.sqlite3")).toBe(false);
    expect(isSafeCoverPath("/home/user/.local/share/chaptercheck/covers/../library.sqlite3")).toBe(
      false,
    );
    expect(isSafeCoverPath("/home/user/.local/share/chaptercheck/covers/1.jpg")).toBe(true);
  });
});
