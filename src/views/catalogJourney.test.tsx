import { fireEvent, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import axe from "axe-core";
import { CatalogView } from "./CatalogView";
import { renderWithProviders } from "../test/renderWithProviders";
import type { CollectionSummaryDto } from "../types/catalog";
import "../styles.css";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
  isTauri: () => false,
  convertFileSrc: (p: string) => p,
}));

async function expectWcag21Aa(container: HTMLElement) {
  const result = await axe.run(container, {
    runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"] },
  });
  expect(result.violations, JSON.stringify(result.violations, null, 2)).toEqual([]);
}

const album: CollectionSummaryDto = {
  id: 3,
  root_id: 1,
  kind: "music",
  title: "Kind of Blue",
  subtitle: "Miles Davis",
  layout_kind: "album",
  cover_path: null,
  progress_pct: 0,
  listened: false,
  in_progress: false,
  unavailable: false,
  root_unavailable: false,
  playable_file_count: 5,
  missing_file_count: 0,
  location_hint: "local",
  track_count: 5,
  last_played_at: null,
};

const noop = () => undefined;

describe("Catalog user journeys", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_playlists") return [];
      if (cmd === "list_collections") {
        return { items: [], total: 0, offset: 0, limit: 50 };
      }
      return null;
    });
  });

  it("empty catalog: one add-folder CTA, no open-once fork, WCAG clean", async () => {
    const { container, findByRole, queryByRole } = renderWithProviders(
      <CatalogView
        kind="audiobook"
        onPlayCollection={noop}
        onOpenDetail={noop}
        onLinkFolder={noop}
      />,
    );
    expect(await findByRole("heading", { name: "Audiobooks" })).toBeTruthy();
    expect(await findByRole("button", { name: "Add my folder" })).toBeTruthy();
    expect(queryByRole("button", { name: /open folder once/i })).toBeNull();
    expect(queryByRole("button", { name: /queue all/i })).toBeNull();
    await expectWcag21Aa(container);
  });

  it("loading and error are announced", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_playlists") return [];
      if (cmd === "list_collections") throw new Error("scan failed");
      return null;
    });
    const { findByRole } = renderWithProviders(
      <CatalogView kind="music" onPlayCollection={noop} onOpenDetail={noop} />,
    );
    expect(await findByRole("alert")).toBeTruthy();
  });

  it("filter is a single listbox; play is one tap on a title", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_playlists") return [];
      if (cmd === "list_collections") {
        return { items: [album], total: 1, offset: 0, limit: 50 };
      }
      return null;
    });
    const onPlay = vi.fn();
    const { container, findByRole, getByRole } = renderWithProviders(
      <CatalogView
        kind="music"
        onPlayCollection={onPlay}
        onOpenDetail={noop}
        onAddToQueue={noop}
        onPlayAll={noop}
      />,
    );
    await waitFor(() => expect(getByRole("combobox")).toBeTruthy());
    const play = await findByRole("button", { name: /play kind of blue/i });
    play.click();
    expect(onPlay).toHaveBeenCalledWith(3, "start");
    expect(getByRole("button", { name: /play all/i })).toBeTruthy();
    expect(getByRole("button", { name: /shuffle/i })).toBeTruthy();
    await expectWcag21Aa(container);
  });

  it("maps a forged filter value to all, not a silent no-op kind", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_playlists") return [];
      if (cmd === "list_collections") {
        return { items: [album], total: 1, offset: 0, limit: 50 };
      }
      return null;
    });
    const { getByRole } = renderWithProviders(
      <CatalogView kind="music" onPlayCollection={noop} onOpenDetail={noop} />,
    );
    await waitFor(() => expect(getByRole("combobox")).toBeTruthy());
    fireEvent.change(getByRole("combobox"), { target: { value: "in-progress" } });
    await waitFor(() => {
      const last = invoke.mock.calls.filter((c) => c[0] === "list_collections").at(-1)?.[1] as {
        filter: string | null;
      };
      expect(last.filter).toBe("in-progress");
    });
    fireEvent.change(getByRole("combobox"), { target: { value: "hacked" } });
    await waitFor(() => {
      const last = invoke.mock.calls.filter((c) => c[0] === "list_collections").at(-1)?.[1] as {
        filter: string | null;
      };
      expect(last.filter).toBeNull();
    });
  });

  it("keeps search results when an older empty response arrives late", async () => {
    const resolvers: Array<(value: { items: typeof album[]; total: number; offset: number; limit: number }) => void> =
      [];
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_playlists") return [];
      if (cmd === "list_collections") {
        return new Promise((resolve) => {
          resolvers.push(resolve);
        });
      }
      return null;
    });
    const { findByRole, getByRole, queryByRole } = renderWithProviders(
      <CatalogView kind="music" onPlayCollection={noop} onOpenDetail={noop} onAddToQueue={noop} />,
    );
    await waitFor(() => expect(resolvers.length).toBe(1));
    fireEvent.change(getByRole("searchbox"), { target: { value: "miles" } });
    await waitFor(() => expect(resolvers.length).toBe(2), { timeout: 2000 });
    resolvers[1]({ items: [album], total: 1, offset: 0, limit: 50 });
    expect(await findByRole("button", { name: /play kind of blue/i })).toBeTruthy();
    resolvers[0]({ items: [], total: 0, offset: 0, limit: 50 });
    await waitFor(() => {
      expect(queryByRole("button", { name: /play kind of blue/i })).toBeTruthy();
    });
  });

  it("retry after a failed catalog load shows titles", async () => {
    let fail = true;
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_playlists") return [];
      if (cmd === "list_collections") {
        if (fail) throw new Error("scan failed");
        return { items: [album], total: 1, offset: 0, limit: 50 };
      }
      return null;
    });
    const { findByRole } = renderWithProviders(
      <CatalogView kind="music" onPlayCollection={noop} onOpenDetail={noop} onAddToQueue={noop} />,
    );
    const retry = await findByRole("button", { name: "Try again" });
    fail = false;
    retry.click();
    expect(await findByRole("button", { name: /play kind of blue/i })).toBeTruthy();
  });
});
