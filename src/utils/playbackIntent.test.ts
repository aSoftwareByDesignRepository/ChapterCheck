import { describe, expect, it } from "vitest";
import { nextPausedIntent } from "./playbackIntent";

describe("nextPausedIntent", () => {
  it("Pause while playing means paused=true", () => {
    expect(nextPausedIntent(false)).toBe(true);
  });

  it("Play while paused means paused=false (resume), never a toggle", () => {
    expect(nextPausedIntent(true)).toBe(false);
  });
});
