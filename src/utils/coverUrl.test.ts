import { describe, expect, it } from "vitest";
import { isSafeCoverPath } from "./coverUrl";

describe("isSafeCoverPath", () => {
  it("allows only real cover-cache paths", () => {
    expect(
      isSafeCoverPath("/home/alex/.local/share/chaptercheck/covers/1.jpg"),
    ).toBe(true);
  });

  it("rejects empty, NUL, parent segments, and paths outside covers", () => {
    expect(isSafeCoverPath(null)).toBe(false);
    expect(isSafeCoverPath("")).toBe(false);
    expect(isSafeCoverPath("/tmp/x\0covers/1.jpg")).toBe(false);
    expect(isSafeCoverPath("/home/alex/.local/share/chaptercheck/covers/../secret")).toBe(
      false,
    );
    expect(isSafeCoverPath("/etc/passwd")).toBe(false);
    expect(isSafeCoverPath("/tmp/notcovers/1.jpg")).toBe(false);
  });
});
