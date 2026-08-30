import { describe, expect, it } from "vitest";
import {
  homeHasVisibleContent,
  parseCatalogFilter,
  shouldApplyAsyncResult,
  sleepPresetSucceeded,
  classifyHostError,
} from "./viewLogic";

describe("parseCatalogFilter", () => {
  it("accepts only the four legal filters", () => {
    expect(parseCatalogFilter("all")).toBe("all");
    expect(parseCatalogFilter("in-progress")).toBe("in-progress");
    expect(parseCatalogFilter("finished")).toBe("finished");
    expect(parseCatalogFilter("away")).toBe("away");
  });

  it("maps garbage, empty, and lookalikes to all", () => {
    expect(parseCatalogFilter("")).toBe("all");
    expect(parseCatalogFilter("ALL")).toBe("all");
    expect(parseCatalogFilter("in-progress ")).toBe("all");
    expect(parseCatalogFilter("in_progress")).toBe("all");
    expect(parseCatalogFilter("finished;drop")).toBe("all");
  });
});

describe("shouldApplyAsyncResult", () => {
  it("accepts only the matching generation", () => {
    expect(shouldApplyAsyncResult(3, 3)).toBe(true);
    expect(shouldApplyAsyncResult(2, 3)).toBe(false);
    expect(shouldApplyAsyncResult(4, 3)).toBe(false);
    expect(shouldApplyAsyncResult(0, 0)).toBe(true);
  });
});

describe("sleepPresetSucceeded", () => {
  it("requires a real future deadline", () => {
    expect(sleepPresetSucceeded(1)).toBe(true);
    expect(sleepPresetSucceeded(1_700_000_000_000)).toBe(true);
    expect(sleepPresetSucceeded(null)).toBe(false);
    expect(sleepPresetSucceeded(undefined)).toBe(false);
    expect(sleepPresetSucceeded(0)).toBe(false);
    expect(sleepPresetSucceeded(-1)).toBe(false);
    expect(sleepPresetSucceeded(Number.NaN)).toBe(false);
    expect(sleepPresetSucceeded(Number.POSITIVE_INFINITY)).toBe(false);
  });
});

describe("homeHasVisibleContent", () => {
  it("is empty when there is nothing playable", () => {
    expect(
      homeHasVisibleContent({ continueItem: null, inProgressCount: 0, musicCount: 0 }),
    ).toBe(false);
    expect(
      homeHasVisibleContent({
        continueItem: { unavailable: true },
        inProgressCount: 0,
        musicCount: 0,
      }),
    ).toBe(false);
  });

  it("is visible when continue, in-progress, or music exists", () => {
    expect(
      homeHasVisibleContent({
        continueItem: { unavailable: false },
        inProgressCount: 0,
        musicCount: 0,
      }),
    ).toBe(true);
    expect(
      homeHasVisibleContent({ continueItem: null, inProgressCount: 1, musicCount: 0 }),
    ).toBe(true);
    expect(
      homeHasVisibleContent({ continueItem: null, inProgressCount: 0, musicCount: 2 }),
    ).toBe(true);
  });
});

describe("classifyHostError", () => {
  it("treats cancel tokens as cancelled, not a failure banner", () => {
    expect(classifyHostError("CANCELLED_BY_USER")).toBe("cancelled");
    expect(classifyHostError("error: CANCELLED_BY_USER")).toBe("cancelled");
    expect(classifyHostError("")).toBe("cancelled");
    expect(classifyHostError(null)).toBe("cancelled");
    expect(classifyHostError(undefined)).toBe("cancelled");
    // Whitespace-only must not become a scary generic banner (needs .trim()).
    expect(classifyHostError("   \n\t  ")).toBe("cancelled");
  });

  it("maps scan and grant failures to specific kinds", () => {
    expect(classifyHostError("This folder has too many audio files to scan")).toBe("too-large");
    expect(classifyHostError("A library scan is already running. Try again in a moment.")).toBe(
      "scan-busy",
    );
    expect(classifyHostError("Use Add my folder so we know you chose it.")).toBe("need-pick");
    // Each phrase alone must map — OR not AND.
    expect(classifyHostError("Add my folder")).toBe("need-pick");
    expect(classifyHostError("so we know you chose it")).toBe("need-pick");
    expect(classifyHostError("The computer must ask you again before a file is deleted.")).toBe(
      "need-os-confirm",
    );
    expect(classifyHostError("sqlite busy")).toBe("generic");
  });
});
