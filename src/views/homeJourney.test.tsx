import { waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import axe from "axe-core";
import { HomeView } from "./HomeView";
import { renderWithProviders } from "../test/renderWithProviders";
import type { CollectionSummaryDto, HomeSummaryDto } from "../types/catalog";
import "../styles.css";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
  isTauri: () => false,
  convertFileSrc: (p: string) => p,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));

async function expectWcag21Aa(container: HTMLElement) {
  const result = await axe.run(container, {
    runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"] },
  });
  expect(result.violations, JSON.stringify(result.violations, null, 2)).toEqual([]);
}

const book: CollectionSummaryDto = {
  id: 7,
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

const emptyHome: HomeSummaryDto = {
  continue_item: null,
  in_progress: [],
  music_shelf: [],
  has_library: false,
  scan_in_progress: false,
};

const libraryHome: HomeSummaryDto = {
  continue_item: book,
  in_progress: [book],
  music_shelf: [],
  has_library: true,
  scan_in_progress: false,
};

const noop = () => undefined;

describe("Home user journeys", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_playlists") return [];
      if (cmd === "get_home_summary") return emptyHome;
      return null;
    });
  });

  it("empty library: one primary add-folder action, no competing open-folder button", async () => {
    const { container, getByRole, queryByRole } = renderWithProviders(
      <HomeView
        refreshKey={0}
        onPlayCollection={noop}
        onAddToQueue={noop}
        onOpenDetail={noop}
        onShuffleRelax={noop}
        onLinkFolder={noop}
        onOpenFile={noop}
        onBrowseAudiobooks={noop}
        onBrowseMusic={noop}
      />,
    );

    await waitFor(() => expect(getByRole("heading", { name: "ChapterCheck" })).toBeTruthy());
    expect(getByRole("button", { name: "Add my folder" })).toBeTruthy();
    expect(getByRole("button", { name: "Just play one file" })).toBeTruthy();
    expect(queryByRole("button", { name: /open folder once/i })).toBeNull();
    expect(container.querySelectorAll(".btn-hero").length).toBe(1);
    await expectWcag21Aa(container);
  });

  it("shows loading until home summary arrives", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_playlists") return [];
      if (cmd === "get_home_summary") {
        await new Promise(() => undefined);
      }
      return null;
    });
    const { getByText } = renderWithProviders(
      <HomeView
        refreshKey={0}
        onPlayCollection={noop}
        onAddToQueue={noop}
        onOpenDetail={noop}
        onShuffleRelax={noop}
        onLinkFolder={noop}
        onOpenFile={noop}
        onBrowseAudiobooks={noop}
        onBrowseMusic={noop}
      />,
    );
    expect(getByText("Loading…")).toBeTruthy();
  });

  it("shows an alert when home summary fails", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_playlists") return [];
      if (cmd === "get_home_summary") throw new Error("disk gone");
      return null;
    });
    const { findByRole } = renderWithProviders(
      <HomeView
        refreshKey={0}
        onPlayCollection={noop}
        onAddToQueue={noop}
        onOpenDetail={noop}
        onShuffleRelax={noop}
        onLinkFolder={noop}
        onOpenFile={noop}
        onBrowseAudiobooks={noop}
        onBrowseMusic={noop}
      />,
    );
    expect(await findByRole("alert")).toBeTruthy();
  });

  it("continue book is one play control, not a nav clone of Audiobooks/Music", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_playlists") return [];
      if (cmd === "get_home_summary") return libraryHome;
      return null;
    });
    const onPlay = vi.fn();
    const { container, findByRole, queryByRole } = renderWithProviders(
      <HomeView
        refreshKey={1}
        onPlayCollection={onPlay}
        onAddToQueue={noop}
        onOpenDetail={noop}
        onShuffleRelax={noop}
        onLinkFolder={noop}
        onOpenFile={noop}
        onBrowseAudiobooks={noop}
        onBrowseMusic={noop}
      />,
    );
    const play = await findByRole("button", { name: /continue dune/i });
    play.click();
    expect(onPlay).toHaveBeenCalledWith(7, "continue");
    expect(queryByRole("button", { name: "Audiobooks" })).toBeNull();
    expect(queryByRole("button", { name: "Music" })).toBeNull();
    await expectWcag21Aa(container);
  });

  it("failed home load offers Try again and recovers", async () => {
    let fail = true;
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_playlists") return [];
      if (cmd === "get_home_summary") {
        if (fail) throw new Error("disk gone");
        return libraryHome;
      }
      return null;
    });
    const { findByRole } = renderWithProviders(
      <HomeView
        refreshKey={0}
        onPlayCollection={noop}
        onAddToQueue={noop}
        onOpenDetail={noop}
        onShuffleRelax={noop}
        onLinkFolder={noop}
        onOpenFile={noop}
        onBrowseAudiobooks={noop}
        onBrowseMusic={noop}
      />,
    );
    const retry = await findByRole("button", { name: "Try again" });
    fail = false;
    retry.click();
    expect(await findByRole("button", { name: /continue dune/i })).toBeTruthy();
  });

  it("keeps the newer home summary when an older request finishes last", async () => {
    const resolvers: Array<(value: HomeSummaryDto) => void> = [];
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_playlists") return [];
      if (cmd === "get_home_summary") {
        return new Promise<HomeSummaryDto>((resolve) => {
          resolvers.push(resolve);
        });
      }
      return null;
    });
    const view = (
      refreshKey: number,
    ) => (
      <HomeView
        refreshKey={refreshKey}
        onPlayCollection={noop}
        onAddToQueue={noop}
        onOpenDetail={noop}
        onShuffleRelax={noop}
        onLinkFolder={noop}
        onOpenFile={noop}
        onBrowseAudiobooks={noop}
        onBrowseMusic={noop}
      />
    );
    const { rerender, findByRole, queryByRole } = renderWithProviders(view(0));
    await waitFor(() => expect(resolvers.length).toBe(1));
    rerender(view(1));
    await waitFor(() => expect(resolvers.length).toBe(2));
    resolvers[1](libraryHome);
    expect(await findByRole("button", { name: /continue dune/i })).toBeTruthy();
    resolvers[0](emptyHome);
    await waitFor(() => {
      expect(queryByRole("button", { name: /continue dune/i })).toBeTruthy();
      expect(queryByRole("heading", { name: "ChapterCheck" })).toBeNull();
    });
  });

  it("unavailable continue book still shows the add-folder empty path", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_playlists") return [];
      if (cmd === "get_home_summary") {
        return {
          ...libraryHome,
          continue_item: { ...book, unavailable: true },
          in_progress: [],
          music_shelf: [],
        };
      }
      return null;
    });
    const { findByRole, queryByRole } = renderWithProviders(
      <HomeView
        refreshKey={0}
        onPlayCollection={noop}
        onAddToQueue={noop}
        onOpenDetail={noop}
        onShuffleRelax={noop}
        onLinkFolder={noop}
        onOpenFile={noop}
        onBrowseAudiobooks={noop}
        onBrowseMusic={noop}
      />,
    );
    expect(await findByRole("button", { name: "Add my folder" })).toBeTruthy();
    expect(queryByRole("button", { name: /continue dune/i })).toBeNull();
    expect(await findByRole("heading", { name: "Home" })).toBeTruthy();
  });
});
