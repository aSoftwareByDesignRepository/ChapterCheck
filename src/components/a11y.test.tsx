import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import axe from "axe-core";
import { AppNav } from "./AppNav";
import { MediaCard } from "./MediaCard";
import { MediaRow } from "./MediaRow";
import { MiniPlayerBar } from "./MiniPlayerBar";
import { I18nProvider } from "../i18n/I18nContext";
import { renderWithProviders } from "../test/renderWithProviders";
import type { CollectionSummaryDto } from "../types/catalog";
import "../styles.css";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "list_playlists") return [];
    return null;
  }),
  isTauri: () => false,
  convertFileSrc: (p: string) => p,
}));

async function expectWcag21Aa(container: HTMLElement) {
  const result = await axe.run(container, {
    runOnly: {
      type: "tag",
      values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"],
    },
  });
  expect(result.violations, JSON.stringify(result.violations, null, 2)).toEqual([]);
}

const sampleCard: CollectionSummaryDto = {
  id: 1,
  root_id: 1,
  kind: "audiobook",
  title: "Dune",
  subtitle: "Frank Herbert",
  layout_kind: "folder",
  cover_path: null,
  progress_pct: 42,
  listened: false,
  in_progress: true,
  unavailable: false,
  root_unavailable: false,
  playable_file_count: 3,
  missing_file_count: 0,
  location_hint: "local",
  track_count: 3,
  last_played_at: 1,
};

describe("WCAG 2.1 AA (axe-core, jsdom)", () => {
  it("primary navigation has no serious axe violations", async () => {
    const { container } = render(
      <I18nProvider>
        <AppNav active="home" hasSession isPlaying={false} onNavigate={() => undefined} />
      </I18nProvider>,
    );
    await expectWcag21Aa(container);
  });

  it("catalog card exposes title and play control", async () => {
    const { container, getByRole } = render(
      <I18nProvider>
        <MediaCard item={sampleCard} onPlay={() => undefined} onOpen={() => undefined} />
      </I18nProvider>,
    );
    expect(getByRole("button", { name: /continue/i })).toBeTruthy();
    await expectWcag21Aa(container);
  });

  it("catalog row exposes one play control and a separate open-details control", async () => {
    const onPlay = vi.fn();
    const onOpen = vi.fn();
    const { container, getByRole } = renderWithProviders(
      <MediaRow
        item={sampleCard}
        onPlay={onPlay}
        onOpen={onOpen}
        onAddToQueue={() => undefined}
      />,
    );
    const play = getByRole("button", { name: /continue dune/i });
    play.click();
    expect(onPlay).toHaveBeenCalledWith(1, "continue");
    const open = getByRole("button", { name: /open .dune/i });
    open.click();
    expect(onOpen).toHaveBeenCalledWith(1);
    await expectWcag21Aa(container);
  });

  it("mini player play, skip, and sleep controls are named", async () => {
    const { container, getByRole } = renderWithProviders(
      <MiniPlayerBar
        title="Dune"
        paused
        coverSrc={null}
        position={12}
        duration={100}
        progressMax={100}
        sliderValue={12}
        setSeekUi={() => undefined}
        formatClock={(s) => `${s}`}
        canSkip
        canSeek
        canToggle
        onExpand={() => undefined}
        onToggle={() => undefined}
        onSkipPrev={() => undefined}
        onSkipNext={() => undefined}
        onSeekTo={() => undefined}
        onSleep={() => undefined}
      />,
    );
    expect(getByRole("button", { name: "Play" })).toBeTruthy();
    expect(getByRole("button", { name: "Open sleep timer" })).toBeTruthy();
    await expectWcag21Aa(container);
  });
});
