import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";
import { MetadataGroupPicker } from "../components/MetadataGroupPicker";
import { IconPlaySm } from "../components/PlayerIcons";
import { useContextMenu, type ContextMenuEntry } from "../context/ContextMenuContext";
import { useAddToPlaylist } from "../context/AddToPlaylistContext";
import { useI18n } from "../i18n/I18nContext";
import type { ImportFolderToPlaylistResult, PlaylistDetailDto } from "../types/catalog";
import { MissingFileRow } from "../components/MissingFileRow";
import { missingFileContextEntries } from "../utils/missingFileMenu";

type ConfirmCfg = {
  title: string;
  body: string;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void | Promise<void>;
};

type Props = {
  playlistId: number;
  onBack: () => void;
  onPlayPlaylist: (id: number, shuffle?: boolean) => void;
  onDeleted: () => void;
  openConfirm: (cfg: ConfirmCfg) => void;
  onLibraryChanged?: () => void;
  onPlaylistsChanged?: () => void;
};

export function PlaylistDetailView({
  playlistId,
  onBack,
  onPlayPlaylist,
  onDeleted,
  openConfirm,
  onLibraryChanged,
  onPlaylistsChanged,
}: Props) {
  const { t } = useI18n();
  const { openContextMenu } = useContextMenu();
  const { appendPlaylistContextEntries, invalidatePlaylists } = useAddToPlaylist();
  const [detail, setDetail] = useState<PlaylistDetailDto | null>(null);
  const [loading, setLoading] = useState(true);
  const [renameValue, setRenameValue] = useState("");
  const [renaming, setRenaming] = useState(false);
  const [importBusy, setImportBusy] = useState(false);
  const [importStatus, setImportStatus] = useState<string | null>(null);
  const [importError, setImportError] = useState<string | null>(null);
  const [playlistSpeed, setPlaylistSpeed] = useState<number | "">("");
  const [showMetadataPicker, setShowMetadataPicker] = useState(false);
  const [trackSearch, setTrackSearch] = useState("");

  const loadDetail = useCallback(async () => {
    setLoading(true);
    try {
      const d = await invoke<PlaylistDetailDto>("get_playlist_detail", { playlistId });
      setDetail(d);
      setRenameValue(d.name);
      setPlaylistSpeed(d.default_playback_speed ?? "");
    } finally {
      setLoading(false);
    }
  }, [playlistId]);

  useEffect(() => {
    setRenaming(false);
    setImportStatus(null);
    setImportError(null);
    setTrackSearch("");
    void loadDetail();
  }, [loadDetail]);

  const filteredItems = useMemo(() => {
    if (!detail) return [];
    const q = trackSearch.trim().toLowerCase();
    if (!q) return detail.items;
    return detail.items.filter(
      (item) =>
        item.display_title.toLowerCase().includes(q) ||
        item.collection_title.toLowerCase().includes(q),
    );
  }, [detail, trackSearch]);

  const savePlaylistSpeed = async (speed: number | null) => {
    if (!detail) return;
    const d = await invoke<PlaylistDetailDto>("set_playlist_default_speed", {
      playlistId: detail.id,
      speed,
    });
    setDetail(d);
    setPlaylistSpeed(d.default_playback_speed ?? "");
    onPlaylistsChanged?.();
  };

  const importFolder = async () => {
    if (!detail) return;
    setImportBusy(true);
    setImportStatus(null);
    setImportError(null);
    try {
      const result = await invoke<ImportFolderToPlaylistResult | null>(
        "pick_import_folder_to_playlist",
        { playlistId: detail.id },
      );
      if (!result) return;
      const status =
        result.tracks_skipped > 0
          ? t("playlists.importFolderDone", {
              added: result.tracks_added,
              skipped: result.tracks_skipped,
            })
          : t("playlists.importFolderDoneAll", { added: result.tracks_added });
      setImportStatus(
        result.library_linked ? `${status} ${t("playlists.importFolderLinked")}` : status,
      );
      if (result.library_linked) onLibraryChanged?.();
      await loadDetail();
      onPlaylistsChanged?.();
    } catch (e) {
      setImportError(String(e));
    } finally {
      setImportBusy(false);
    }
  };

  const deletePlaylist = () => {
    if (!detail) return;
    openConfirm({
      title: t("playlists.deleteConfirmTitle"),
      body: t("playlists.deleteConfirmBody", { name: detail.name }),
      confirmLabel: t("playlists.deleteConfirmBtn"),
      danger: true,
      onConfirm: async () => {
        await invoke("delete_playlist", { playlistId: detail.id });
        invalidatePlaylists();
        onDeleted();
      },
    });
  };

  const saveRename = async () => {
    if (!detail) return;
    const name = renameValue.trim();
    if (!name) return;
    await invoke("rename_playlist", { playlistId: detail.id, name });
    await loadDetail();
    setRenaming(false);
    onPlaylistsChanged?.();
  };

  const moveItem = async (itemId: number, direction: -1 | 1) => {
    if (!detail) return;
    const idx = detail.items.findIndex((i) => i.id === itemId);
    const target = idx + direction;
    if (idx < 0 || target < 0 || target >= detail.items.length) return;
    const ids = detail.items.map((i) => i.id);
    const tmp = ids[idx]!;
    ids[idx] = ids[target]!;
    ids[target] = tmp;
    await invoke("reorder_playlist_items", { playlistId: detail.id, itemIds: ids });
    await loadDetail();
    onPlaylistsChanged?.();
  };

  const relinkFile = async (fileId: number) => {
    const path = await invoke<string | null>("pick_relink_audio_file");
    if (!path) return;
    await invoke("relink_collection_file", { fileId, newPath: path });
    await loadDetail();
    onPlaylistsChanged?.();
    onLibraryChanged?.();
  };

  const removeFileFromLibrary = (fileId: number, title: string) => {
    openConfirm({
      title: t("catalog.removeFromLibraryConfirmTitle"),
      body: t("catalog.removeFromLibraryConfirmBody", { title }),
      confirmLabel: t("catalog.removeFromLibraryConfirmBtn"),
      danger: true,
      onConfirm: async () => {
        const result = await invoke<{ collection_removed: boolean }>(
          "remove_collection_file_from_library",
          { fileId },
        );
        if (result.collection_removed) onLibraryChanged?.();
        await loadDetail();
        onPlaylistsChanged?.();
      },
    });
  };

  const removeTrack = (itemId: number, title: string) => {
    openConfirm({
      title: t("playlists.removeTrackConfirmTitle"),
      body: t("playlists.removeTrackConfirmBody", { title }),
      confirmLabel: t("playlists.removeTrackConfirmBtn"),
      danger: true,
      onConfirm: async () => {
        if (!detail) return;
        await invoke("remove_from_playlist", { itemId });
        await loadDetail();
        onPlaylistsChanged?.();
      },
    });
  };

  const togglePin = async () => {
    if (!detail) return;
    await invoke("set_playlist_pinned", { playlistId: detail.id, pinned: !detail.is_pinned });
    await loadDetail();
    onPlaylistsChanged?.();
  };

  if (loading && !detail) {
    return (
      <div className="view-panel playlist-page">
        <p className="view-loading" aria-live="polite">
          {t("home.loading")}
        </p>
      </div>
    );
  }

  if (!detail) {
    return (
      <div className="view-panel playlist-page">
        <button type="button" className="playlist-breadcrumb" onClick={onBack}>
          {t("playlists.backToList")}
        </button>
        <p className="view-empty-body">{t("playlists.notFound")}</p>
      </div>
    );
  }

  const canPlay = detail.items.some((i) => !i.unavailable);
  const missingItems = detail.items.filter((i) => i.unavailable);

  return (
    <div className="view-panel playlist-page playlist-page--detail">
      <button type="button" className="playlist-breadcrumb" onClick={onBack}>
        {t("playlists.backToList")}
      </button>

      <header className="playlist-detail-hero">
        <div className="playlist-detail-hero-icon" aria-hidden="true">
          {detail.is_pinned ? "★" : "♫"}
        </div>
        <div className="playlist-detail-hero-text">
          {renaming ? (
            <form
              className="playlist-detail-rename-form"
              onSubmit={(e) => {
                e.preventDefault();
                void saveRename();
              }}
            >
              <input
                type="text"
                className="catalog-search"
                value={renameValue}
                onChange={(e) => setRenameValue(e.target.value)}
                aria-label={t("playlists.rename")}
                autoFocus
              />
              <button type="submit" className="btn btn-secondary btn-compact">
                {t("playlists.saveName")}
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-compact"
                onClick={() => {
                  setRenaming(false);
                  setRenameValue(detail.name);
                }}
              >
                {t("modal.close")}
              </button>
            </form>
          ) : (
            <>
              <h1 className="playlist-detail-hero-title">{detail.name}</h1>
              <p className="playlist-detail-hero-meta">
                {t("playlists.trackCount", { count: detail.items.length })}
                {detail.is_pinned ? ` · ${t("playlists.pinned")}` : ""}
              </p>
            </>
          )}
        </div>
        <div className="playlist-detail-hero-playback">
          <button
            type="button"
            className="btn btn-primary playlist-detail-hero-play"
            disabled={!canPlay}
            onClick={() => onPlayPlaylist(detail.id, false)}
          >
            <IconPlaySm />
            <span>{t("playlists.playInOrder")}</span>
          </button>
          <button
            type="button"
            className="btn btn-secondary playlist-detail-hero-play"
            disabled={!canPlay || detail.items.length < 2}
            onClick={() => onPlayPlaylist(detail.id, true)}
          >
            <span>{t("catalog.shuffleAll")}</span>
          </button>
        </div>
      </header>

      <section className="playlist-detail-add" aria-labelledby="playlist-add-heading">
        <h2 id="playlist-add-heading" className="playlist-detail-add-title">
          {t("playlists.addTracks")}
        </h2>
        <p className="playlist-detail-add-hint">{t("playlists.addTracksHint")}</p>
        <div className="playlist-detail-add-actions">
          <button
            type="button"
            className={`btn btn-secondary${showMetadataPicker ? " btn-compact--active" : ""}`}
            aria-pressed={showMetadataPicker}
            onClick={() => setShowMetadataPicker((v) => !v)}
          >
            {t("playlists.addByMetadata")}
          </button>
          <button
            type="button"
            className="btn btn-secondary"
            disabled={importBusy}
            onClick={() => void importFolder()}
          >
            {importBusy ? t("playlists.importFolderBusy") : t("playlists.importFolder")}
          </button>
        </div>
        {importStatus ? (
          <p className="playlist-import-status" role="status">
            {importStatus}
          </p>
        ) : null}
        {importError ? (
          <p className="view-error" role="alert">
            {importError}
          </p>
        ) : null}
      </section>

      {showMetadataPicker ? (
        <MetadataGroupPicker
          mode="add"
          playlistId={playlistId}
          onClose={() => setShowMetadataPicker(false)}
          onDone={() => {
            void loadDetail();
            onPlaylistsChanged?.();
            setShowMetadataPicker(false);
          }}
        />
      ) : null}

      <details className="playlist-detail-settings">
        <summary>{t("playlists.settings")}</summary>
        <div className="playlist-detail-settings-body">
          <div className="playlist-detail-settings-row">
            <button type="button" className="btn btn-ghost btn-compact" onClick={() => setRenaming(true)}>
              {t("playlists.rename")}
            </button>
            <button
              type="button"
              className="btn btn-ghost btn-compact"
              aria-pressed={detail.is_pinned}
              onClick={() => void togglePin()}
            >
              {detail.is_pinned ? t("playlists.pinned") : t("playlists.pin")}
            </button>
            <button type="button" className="btn btn-ghost btn-compact" onClick={deletePlaylist}>
              {t("playlists.delete")}
            </button>
          </div>
          <label className="playlist-speed-field" htmlFor="playlist-default-speed">
            <span className="field-label">{t("playlists.defaultSpeed")}</span>
            <p className="hint">{t("playlists.defaultSpeedHint")}</p>
            <div className="playlist-speed-pref-row">
              <input
                id="playlist-default-speed"
                type="range"
                className="slider slider--speed"
                min={0.5}
                max={4}
                step={0.05}
                value={playlistSpeed === "" ? 1 : playlistSpeed}
                onChange={(e) => setPlaylistSpeed(Number(e.target.value))}
              />
              <span className="prefs-speed-readout" aria-live="polite">
                {(playlistSpeed === "" ? 1 : playlistSpeed).toFixed(2)}×
              </span>
              <button
                type="button"
                className="btn btn-secondary btn-compact"
                onClick={() =>
                  void savePlaylistSpeed(playlistSpeed === "" ? null : Number(playlistSpeed))
                }
              >
                {t("playlists.saveDefaultSpeed")}
              </button>
              {detail.default_playback_speed != null ? (
                <button
                  type="button"
                  className="btn btn-ghost btn-compact"
                  onClick={() => void savePlaylistSpeed(null)}
                >
                  {t("playlists.clearDefaultSpeed")}
                </button>
              ) : null}
            </div>
          </label>
        </div>
      </details>

      <section className="playlist-detail-tracks" aria-labelledby="playlist-tracks-heading">
        <div className="playlist-detail-tracks-head">
          <h2 id="playlist-tracks-heading" className="playlist-detail-tracks-title">
            {t("playlists.tracksHeading")}
          </h2>
          {detail.items.length > 0 ? (
            <input
              type="search"
              className="catalog-search playlist-detail-tracks-search"
              placeholder={t("playlists.trackSearchPlaceholder")}
              value={trackSearch}
              onChange={(e) => setTrackSearch(e.target.value)}
              aria-label={t("playlists.trackSearchPlaceholder")}
            />
          ) : null}
        </div>

        {missingItems.length > 0 ? (
          <section className="missing-files-panel" aria-labelledby="playlist-missing-heading">
            <header className="missing-files-panel-head">
              <h3 id="playlist-missing-heading" className="missing-files-panel-title">
                {t("catalog.relinkHeading")}
              </h3>
              <p className="missing-files-panel-lead">{t("catalog.relinkHint")}</p>
              <p className="missing-files-panel-count" aria-live="polite">
                {t("catalog.missingFileCount", { count: missingItems.length })}
              </p>
            </header>
            <ul className="missing-files-panel-list">
              {missingItems.map((item) => (
                <MissingFileRow
                  key={item.id}
                  fileId={item.collection_file_id}
                  title={item.display_title}
                  subtitle={item.collection_title}
                  onRelink={relinkFile}
                  onRemove={removeFileFromLibrary}
                />
              ))}
            </ul>
          </section>
        ) : null}

        {detail.items.length === 0 ? (
          <div className="playlist-detail-empty">
            <p className="view-empty-body">{t("playlists.emptyPlaylist")}</p>
            <div className="playlist-detail-add-actions">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setShowMetadataPicker(true)}
              >
                {t("playlists.addByMetadata")}
              </button>
              <button
                type="button"
                className="btn btn-ghost"
                disabled={importBusy}
                onClick={() => void importFolder()}
              >
                {t("playlists.importFolder")}
              </button>
            </div>
          </div>
        ) : (() => {
          const availableFiltered = filteredItems.filter((item) => !item.unavailable);
          if (availableFiltered.length === 0) {
            if (trackSearch.trim() && detail.items.some((i) => !i.unavailable)) {
              return <p className="view-empty-body">{t("playlists.trackSearchEmpty")}</p>;
            }
            return null;
          }
          return (
          <ol className="playlist-track-list">
            {availableFiltered.map((item) => {
              const idx = detail.items.findIndex((i) => i.id === item.id);
              return (
                <li
                  key={item.id}
                  className="playlist-track"
                  onContextMenu={(e) => {
                    const items: ContextMenuEntry[] = [
                      {
                        id: "up",
                        label: t("catalog.moveUp"),
                        disabled: idx === 0,
                        onClick: () => void moveItem(item.id, -1),
                      },
                      {
                        id: "down",
                        label: t("catalog.moveDown"),
                        disabled: idx >= detail.items.length - 1,
                        onClick: () => void moveItem(item.id, 1),
                      },
                      { type: "separator" },
                      {
                        id: "remove",
                        label: t("playlists.removeTrack"),
                        danger: true,
                        onClick: () => removeTrack(item.id, item.display_title),
                      },
                    ];
                    openContextMenu(
                      e,
                      appendPlaylistContextEntries(items, { fileId: item.collection_file_id }),
                    );
                  }}
                >
                  <span className="playlist-track-index" aria-hidden="true">
                    {idx + 1}
                  </span>
                  <div className="playlist-track-body">
                    <span className="playlist-track-title">{item.display_title}</span>
                    <span className="playlist-track-album">{item.collection_title}</span>
                  </div>
                  <button
                    type="button"
                    className="playlist-track-remove"
                    aria-label={t("playlists.removeTrack")}
                    onClick={() => removeTrack(item.id, item.display_title)}
                  >
                    ×
                  </button>
                </li>
              );
            })}
          </ol>
          );
        })()}
      </section>
    </div>
  );
}
