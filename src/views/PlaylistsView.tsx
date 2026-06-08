import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";
import { MetadataGroupPicker } from "../components/MetadataGroupPicker";
import { IconPlaySm } from "../components/PlayerIcons";
import { useContextMenu, type ContextMenuEntry } from "../context/ContextMenuContext";
import { useAddToPlaylist } from "../context/AddToPlaylistContext";
import { useI18n } from "../i18n/I18nContext";
import type { PlaylistSummaryDto } from "../types/catalog";
import { PlaylistDetailView } from "./PlaylistDetailView";

type ConfirmCfg = {
  title: string;
  body: string;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void | Promise<void>;
};

type Props = {
  onPlayPlaylist: (id: number, shuffle?: boolean) => void;
  openConfirm: (cfg: ConfirmCfg) => void;
  onLibraryChanged?: () => void;
};

export function PlaylistsView({ onPlayPlaylist, openConfirm, onLibraryChanged }: Props) {
  const { t } = useI18n();
  const { openContextMenu } = useContextMenu();
  const { invalidatePlaylists } = useAddToPlaylist();
  const [playlists, setPlaylists] = useState<PlaylistSummaryDto[]>([]);
  const [newName, setNewName] = useState("");
  const [search, setSearch] = useState("");
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [showMetadataPicker, setShowMetadataPicker] = useState(false);

  const load = useCallback(async () => {
    const list = await invoke<PlaylistSummaryDto[]>("list_playlists");
    setPlaylists(list);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return playlists;
    return playlists.filter((pl) => pl.name.toLowerCase().includes(q));
  }, [playlists, search]);

  const createPlaylist = async () => {
    const name = newName.trim();
    if (!name) return;
    await invoke("create_playlist", { name, pin: false });
    invalidatePlaylists();
    setNewName("");
    await load();
  };

  const deletePlaylist = (id: number, name: string) => {
    openConfirm({
      title: t("playlists.deleteConfirmTitle"),
      body: t("playlists.deleteConfirmBody", { name }),
      confirmLabel: t("playlists.deleteConfirmBtn"),
      danger: true,
      onConfirm: async () => {
        await invoke("delete_playlist", { playlistId: id });
        invalidatePlaylists();
        if (selectedId === id) setSelectedId(null);
        await load();
      },
    });
  };

  const togglePin = async (id: number, pinned: boolean) => {
    await invoke("set_playlist_pinned", { playlistId: id, pinned: !pinned });
    await load();
  };

  const contextItems = (pl: PlaylistSummaryDto): ContextMenuEntry[] => [
    {
      id: "open",
      label: t("playlists.open"),
      onClick: () => setSelectedId(pl.id),
    },
    {
      id: "play",
      label: t("playlists.playInOrder"),
      disabled: pl.track_count === 0,
      onClick: () => onPlayPlaylist(pl.id, false),
    },
    {
      id: "shuffle",
      label: t("catalog.shuffleAll"),
      disabled: pl.track_count < 2,
      onClick: () => onPlayPlaylist(pl.id, true),
    },
    {
      id: "pin",
      label: pl.is_pinned ? t("playlists.unpin") : t("playlists.pin"),
      onClick: () => void togglePin(pl.id, pl.is_pinned),
    },
    { type: "separator" },
    {
      id: "delete",
      label: t("playlists.delete"),
      danger: true,
      onClick: () => deletePlaylist(pl.id, pl.name),
    },
  ];

  if (selectedId != null) {
    return (
      <PlaylistDetailView
        playlistId={selectedId}
        onBack={() => setSelectedId(null)}
        onPlayPlaylist={onPlayPlaylist}
        onDeleted={() => {
          setSelectedId(null);
          void load();
        }}
        openConfirm={openConfirm}
        onLibraryChanged={onLibraryChanged}
        onPlaylistsChanged={() => void load()}
      />
    );
  }

  return (
    <div className="view-panel playlist-page">
      <header className="playlist-page-head">
        <h1 className="view-title">{t("nav.playlists")}</h1>
        <p className="playlist-page-lead">{t("playlists.intro")}</p>
      </header>

      <div className="playlist-page-toolbar">
        <input
          type="search"
          className="catalog-search playlist-page-search"
          placeholder={t("playlists.searchPlaceholder")}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          aria-label={t("playlists.searchPlaceholder")}
        />
        <button
          type="button"
          className={`btn btn-secondary btn-compact${showMetadataPicker ? " btn-compact--active" : ""}`}
          aria-pressed={showMetadataPicker}
          onClick={() => setShowMetadataPicker((v) => !v)}
        >
          {t("playlists.createFromMetadata")}
        </button>
      </div>

      <form
        className="playlist-page-create"
        onSubmit={(e) => {
          e.preventDefault();
          void createPlaylist();
        }}
      >
        <input
          id="new-playlist-name"
          type="text"
          className="catalog-search"
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          placeholder={t("playlists.namePlaceholder")}
          aria-label={t("playlists.namePlaceholder")}
        />
        <button type="submit" className="btn btn-primary" disabled={!newName.trim()}>
          {t("playlists.create")}
        </button>
      </form>

      {showMetadataPicker ? (
        <MetadataGroupPicker
          mode="create"
          onClose={() => setShowMetadataPicker(false)}
          onCreated={(id) => {
            invalidatePlaylists();
            setShowMetadataPicker(false);
            void load().then(() => setSelectedId(id));
          }}
        />
      ) : null}

      {playlists.length === 0 ? (
        <div className="playlist-page-empty view-empty view-empty--actions">
          <p className="view-empty-title">{t("playlists.emptyTitle")}</p>
          <p className="view-empty-body">{t("playlists.emptyBody")}</p>
          <div className="view-empty-actions">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setShowMetadataPicker(true)}
            >
              {t("playlists.createFromMetadata")}
            </button>
          </div>
        </div>
      ) : filtered.length === 0 ? (
        <p className="view-empty-body">{t("playlists.searchEmpty")}</p>
      ) : (
        <ul className="playlist-page-list">
          {filtered.map((pl) => (
            <li
              key={pl.id}
              className="playlist-page-row"
              onContextMenu={(e) => openContextMenu(e, contextItems(pl))}
            >
              <button
                type="button"
                className="playlist-page-row-play"
                disabled={pl.track_count === 0}
                aria-label={t("home.play")}
                onClick={() => onPlayPlaylist(pl.id)}
                title={t("playlists.playInOrder")}
              >
                <IconPlaySm />
              </button>
              <button
                type="button"
                className="playlist-page-row-main"
                onClick={() => setSelectedId(pl.id)}
              >
                <span className="playlist-page-row-icon" aria-hidden="true">
                  {pl.is_pinned ? "★" : "♫"}
                </span>
                <span className="playlist-page-row-text">
                  <span className="playlist-page-row-title">{pl.name}</span>
                  <span className="playlist-page-row-meta">
                    {t("playlists.trackCount", { count: pl.track_count })}
                    {pl.is_pinned ? ` · ${t("playlists.pinned")}` : ""}
                    {pl.unavailable_count > 0 ? ` · ${t("catalog.away")}` : ""}
                  </span>
                </span>
              </button>
              <button
                type="button"
                className="playlist-page-row-more"
                aria-label={t("playlists.moreActions")}
                onClick={(e) => openContextMenu(e, contextItems(pl))}
              >
                ⋯
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
