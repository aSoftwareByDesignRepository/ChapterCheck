import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { AddToPlaylistButton } from "./AddToPlaylistButton";
import { CoverImage } from "./CoverImage";
import { useAddToPlaylist } from "../context/AddToPlaylistContext";
import { useContextMenu, type ContextMenuEntry } from "../context/ContextMenuContext";
import { useDialogTrap } from "../hooks/useDialogTrap";
import { useI18n } from "../i18n/I18nContext";
import type { CollectionDetailDto, MetadataSuggestionDto } from "../types/catalog";
import { coverUrl } from "../utils/coverUrl";
import { MissingFilesPanel } from "./MissingFilesPanel";

type MetaForm = {
  title: string;
  author: string;
  narrator: string;
  artist: string;
  album: string;
};

function metaFromDetail(d: CollectionDetailDto): MetaForm {
  return {
    title: d.title,
    author: d.author ?? "",
    narrator: d.narrator ?? "",
    artist: d.artist ?? "",
    album: d.album ?? "",
  };
}

function metaIsDirty(detail: CollectionDetailDto, form: MetaForm): boolean {
  const base = metaFromDetail(detail);
  return (
    form.title !== base.title ||
    form.author !== base.author ||
    form.narrator !== base.narrator ||
    form.artist !== base.artist ||
    form.album !== base.album
  );
}

type ConfirmCfg = {
  title: string;
  body: string;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void | Promise<void>;
};

type Props = {
  collectionId: number | null;
  refreshKey?: number;
  onlineMetadataEnabled?: boolean;
  onClose: () => void;
  onPlayCollection: (id: number, mode: "continue" | "start", shuffle?: boolean) => void;
  onAddToQueue?: (id: number, position: "next" | "end") => void;
  onChanged?: () => void;
  openConfirm?: (cfg: ConfirmCfg) => void;
  dialogSuspended?: boolean;
};

export function CollectionDetailSheet({
  collectionId,
  refreshKey = 0,
  onlineMetadataEnabled = false,
  onClose,
  onPlayCollection,
  onAddToQueue,
  onChanged,
  openConfirm,
  dialogSuspended = false,
}: Props) {
  const { t } = useI18n();
  const { openContextMenu } = useContextMenu();
  const { openAddToPlaylist, appendPlaylistContextEntries } = useAddToPlaylist();
  const [detail, setDetail] = useState<CollectionDetailDto | null>(null);
  const [metaForm, setMetaForm] = useState<MetaForm | null>(null);
  const [editing, setEditing] = useState(false);
  const [metaSaved, setMetaSaved] = useState(false);
  const [editingTrackId, setEditingTrackId] = useState<number | null>(null);
  const [editTrackTitle, setEditTrackTitle] = useState("");
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [lookupBusy, setLookupBusy] = useState(false);
  const [suggestions, setSuggestions] = useState<MetadataSuggestionDto[]>([]);
  const [lookupError, setLookupError] = useState<string | null>(null);
  const [lookupDone, setLookupDone] = useState(false);
  const [suggestionApplied, setSuggestionApplied] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [kindError, setKindError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const formRef = useRef<HTMLDivElement>(null);

  const resetLookup = useCallback(() => {
    setSuggestions([]);
    setLookupError(null);
    setLookupDone(false);
    setSuggestionApplied(false);
    setLookupBusy(false);
  }, []);

  const closeDetail = useCallback(() => {
    setDetail(null);
    setMetaForm(null);
    setEditing(false);
    setMetaSaved(false);
    resetLookup();
    setLoadError(null);
    setKindError(null);
    setActionError(null);
    onClose();
  }, [onClose, resetLookup]);

  const requestClose = useCallback(() => {
    if (detail && metaForm && editing && metaIsDirty(detail, metaForm)) {
      const run = () => closeDetail();
      if (openConfirm) {
        openConfirm({
          title: t("catalog.discardEditsTitle"),
          body: t("catalog.discardEditsBody"),
          confirmLabel: t("catalog.discardEditsConfirm"),
          danger: true,
          onConfirm: run,
        });
      } else {
        run();
      }
      return;
    }
    closeDetail();
  }, [detail, metaForm, editing, closeDetail, openConfirm, t]);

  const { sheetRef, closeRef } = useDialogTrap(collectionId != null, requestClose);

  const refreshDetail = useCallback(async (id: number) => {
    const d = await invoke<CollectionDetailDto>("get_collection_detail", { collectionId: id });
    setDetail(d);
    setMetaForm(metaFromDetail(d));
    onChanged?.();
  }, [onChanged]);

  useEffect(() => {
    if (collectionId == null) {
      setDetail(null);
      setMetaForm(null);
      return;
    }
    let cancelled = false;
    setLoadError(null);
    resetLookup();
    void invoke<CollectionDetailDto>("get_collection_detail", { collectionId })
      .then((d) => {
        if (cancelled) return;
        setDetail(d);
        setMetaForm(metaFromDetail(d));
        setEditing(false);
        setMetaSaved(false);
      })
      .catch((e) => {
        if (!cancelled) setLoadError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [collectionId, resetLookup]);

  useEffect(() => {
    if (collectionId == null || refreshKey === 0) return;
    void refreshDetail(collectionId).catch((e) => setLoadError(String(e)));
  }, [collectionId, refreshKey, refreshDetail]);

  const saveMetadata = async () => {
    if (!detail || !metaForm) return;
    setActionError(null);
    try {
      await invoke("update_collection_metadata", {
        collectionId: detail.id,
        metadata: {
          title: metaForm.title.trim() || null,
          author: metaForm.author.trim() || null,
          narrator: metaForm.narrator.trim() || null,
          artist: metaForm.artist.trim() || null,
          album: metaForm.album.trim() || null,
          series: null,
          series_index: null,
        },
      });
      await refreshDetail(detail.id);
      setEditing(false);
      setMetaSaved(true);
      setSuggestionApplied(false);
    } catch (e) {
      setActionError(String(e));
    }
  };

  const moveTrack = async (fileId: number, direction: -1 | 1) => {
    if (!detail) return;
    const avail = detail.files.filter((f) => !f.unavailable);
    const idx = avail.findIndex((f) => f.id === fileId);
    const target = idx + direction;
    if (idx < 0 || target < 0 || target >= avail.length) return;
    const ids = avail.map((f) => f.id);
    const tmp = ids[idx]!;
    ids[idx] = ids[target]!;
    ids[target] = tmp;
    setActionError(null);
    try {
      await invoke("reorder_collection_files", { collectionId: detail.id, fileIds: ids });
      await refreshDetail(detail.id);
    } catch (e) {
      setActionError(String(e));
    }
  };

  const saveTrackTitle = async (fileId: number) => {
    const title = editTrackTitle.trim();
    if (!title || !detail) return;
    setActionError(null);
    try {
      await invoke("update_file_display_title", { fileId, displayTitle: title });
      setEditingTrackId(null);
      await refreshDetail(detail.id);
    } catch (e) {
      setActionError(String(e));
    }
  };

  const relinkFile = async (fileId: number) => {
    setBusyAction("relink");
    try {
      const path = await invoke<string | null>("pick_relink_audio_file");
      if (!path) return;
      await invoke("relink_collection_file", { fileId, newPath: path });
      if (detail) {
        await refreshDetail(detail.id);
        onChanged?.();
      }
    } finally {
      setBusyAction(null);
    }
  };

  const removeCollectionById = async (id: number) => {
    setActionError(null);
    try {
      await invoke("remove_collection_from_library", { collectionId: id });
      onChanged?.();
      closeDetail();
    } catch (e) {
      setActionError(String(e));
      onChanged?.();
    }
  };

  const removeCollectionFromLibrary = (id = detail?.id) => {
    if (id == null) return;
    const title = detail?.title ?? `#${id}`;
    const run = async () => removeCollectionById(id);
    if (openConfirm) {
      openConfirm({
        title: t("catalog.removeCollectionConfirmTitle"),
        body: t("catalog.removeCollectionConfirmBody", { title }),
        confirmLabel: t("catalog.removeCollectionConfirmBtn"),
        danger: true,
        onConfirm: run,
      });
    } else {
      void run();
    }
  };

  const removeFileFromLibrary = (fileId: number, title: string) => {
    const run = async () => {
      setActionError(null);
      try {
        const result = await invoke<{ collection_removed: boolean }>(
          "remove_collection_file_from_library",
          { fileId },
        );
        if (!detail) return;
        if (result.collection_removed) {
          onChanged?.();
          closeDetail();
        } else {
          await refreshDetail(detail.id);
          onChanged?.();
        }
      } catch (e) {
        setActionError(String(e));
        onChanged?.();
      }
    };
    if (openConfirm) {
      openConfirm({
        title: t("catalog.removeFromLibraryConfirmTitle"),
        body: t("catalog.removeFromLibraryConfirmBody", { title }),
        confirmLabel: t("catalog.removeFromLibraryConfirmBtn"),
        danger: true,
        onConfirm: run,
      });
    } else {
      void run();
    }
  };

  const setCollectionKind = async (newKind: "audiobook" | "music") => {
    if (!detail || detail.kind === newKind || busyAction) return;
    setBusyAction("kind");
    setKindError(null);
    try {
      await invoke("set_collection_kind", { collectionId: detail.id, kind: newKind });
      await refreshDetail(detail.id);
      onChanged?.();
    } catch (e) {
      setKindError(String(e));
    } finally {
      setBusyAction(null);
    }
  };

  const fixTrackOrder = async () => {
    if (!detail) return;
    setBusyAction("fix");
    try {
      await invoke("fix_collection_track_order", { collectionId: detail.id });
      await refreshDetail(detail.id);
    } finally {
      setBusyAction(null);
    }
  };

  const lookupOnline = async () => {
    if (!detail || !onlineMetadataEnabled || lookupBusy) return;
    setLookupBusy(true);
    setLookupError(null);
    setLookupDone(false);
    setSuggestionApplied(false);
    setSuggestions([]);
    try {
      const list = await invoke<MetadataSuggestionDto[]>("lookup_metadata_online", {
        collectionId: detail.id,
      });
      setSuggestions(list);
      setLookupDone(true);
    } catch (e) {
      setLookupError(String(e));
    } finally {
      setLookupBusy(false);
    }
  };

  const applySuggestion = (s: MetadataSuggestionDto) => {
    if (!metaForm) return;
    setMetaForm({
      ...metaForm,
      title: s.title ?? metaForm.title,
      author: s.author ?? metaForm.author,
      narrator: s.narrator ?? metaForm.narrator,
      artist: s.artist ?? metaForm.artist,
      album: s.album ?? metaForm.album,
    });
    setEditing(true);
    setMetaSaved(false);
    setSuggestionApplied(true);
    // Bring the now-filled editable fields into view so the cause and effect of
    // "Use this" is obvious even on small screens.
    requestAnimationFrame(() => {
      formRef.current?.scrollIntoView({ behavior: "smooth", block: "center" });
    });
  };

  if (collectionId == null) return null;

  const coverSrc = detail ? coverUrl(detail.cover_path) : null;
  const canPlay = detail != null && detail.playable_file_count > 0;
  const rootUnavailable = detail?.root_unavailable ?? false;
  const relinkDisabled = rootUnavailable;
  const relinkDisabledHint = rootUnavailable ? t("catalog.relinkRootUnavailable") : undefined;
  const showRemoveCollection = detail != null && !canPlay && detail.files.length > 0;
  const showEmptyCollection = detail != null && detail.files.length === 0;

  return (
    <div
      className="detail-sheet-backdrop"
      role="presentation"
      {...(dialogSuspended ? { inert: "" as const } : {})}
      onClick={dialogSuspended ? undefined : requestClose}
    >
      <div
        className="detail-sheet"
        ref={sheetRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="detail-sheet-title"
        onClick={(e) => e.stopPropagation()}
        onContextMenu={(e) => {
          if (!detail) return;
          e.preventDefault();
          e.stopPropagation();
          const items: ContextMenuEntry[] = [];
          if (canPlay) {
            const mode = detail.progress_pct > 0 ? "continue" : "start";
            items.push({
              id: "play",
              label: mode === "continue" ? t("home.continue") : t("home.play"),
              onClick: () => {
                onPlayCollection(detail.id, mode);
                closeDetail();
              },
            });
            if (detail.progress_pct > 0) {
              items.push({
                id: "start",
                label: t("contextMenu.playFromStart"),
                onClick: () => {
                  onPlayCollection(detail.id, "start");
                  closeDetail();
                },
              });
            }
            if (detail.kind === "music" && detail.playable_file_count >= 2) {
              items.push({
                id: "shuffle",
                label: t("catalog.shuffleAll"),
                onClick: () => {
                  onPlayCollection(detail.id, "start", true);
                  closeDetail();
                },
              });
            }
            if (onAddToQueue) {
              items.push({ type: "separator" });
              items.push({
                id: "next",
                label: t("queue.playNext"),
                onClick: () => {
                  onAddToQueue(detail.id, "next");
                  closeDetail();
                },
              });
              items.push({
                id: "queue",
                label: detail.kind === "music" ? t("catalog.queueAll") : t("queue.addToQueue"),
                onClick: () => {
                  onAddToQueue(detail.id, "end");
                  closeDetail();
                },
              });
            }
            items.push({ type: "separator" });
            items.push({
              id: "listened",
              label: detail.listened ? t("catalog.markUnlistened") : t("catalog.markListened"),
              onClick: () => {
                void (async () => {
                  await invoke("mark_collection_listened", {
                    collectionId: detail.id,
                    listened: !detail.listened,
                  });
                  await refreshDetail(detail.id);
                })();
              },
            });
          }
          if (!editing) {
            items.push({
              id: "edit",
              label: t("catalog.editTitle"),
              onClick: () => {
                setEditing(true);
                setMetaSaved(false);
              },
            });
          }
          if (showRemoveCollection || showEmptyCollection) {
            items.push({ type: "separator" });
            items.push({
              id: "remove-collection",
              label: t("catalog.removeCollection"),
              danger: true,
              onClick: removeCollectionFromLibrary,
            });
          }
          const merged = canPlay
            ? appendPlaylistContextEntries(items, {
                collectionId: detail.id,
                title: detail.title,
              })
            : items;
          openContextMenu(e, merged);
        }}
      >
        {loadError ? (
          <div className="ghost-collection-panel" role="alert">
            <p className="view-error">{loadError}</p>
            <p className="ghost-collection-panel-body">{t("catalog.staleEntryBody")}</p>
            <button
              type="button"
              className="btn btn-secondary ghost-collection-panel-action"
              onClick={() => removeCollectionFromLibrary(collectionId)}
            >
              {t("catalog.removeCollection")}
            </button>
          </div>
        ) : null}
        {actionError ? (
          <p className="view-error" role="alert">
            {actionError}
          </p>
        ) : null}
        {!loadError && (!detail || !metaForm) ? (
          <p className="view-loading" aria-live="polite">
            {t("home.loading")}
          </p>
        ) : !loadError && detail && metaForm ? (
          <>
            <header className="detail-sheet-head">
              <div className="detail-sheet-cover">
                <CoverImage
                  src={coverSrc}
                  kind={detail.kind}
                  className="detail-sheet-cover-img"
                />
              </div>
              <div className="detail-sheet-head-text">
                {editing ? (
                  <label className="detail-field">
                    <span className="field-label">{t("catalog.fieldTitle")}</span>
                    <input
                      type="text"
                      className="catalog-search"
                      value={metaForm.title}
                      onChange={(e) => setMetaForm({ ...metaForm, title: e.target.value })}
                    />
                  </label>
                ) : (
                  <h3 id="detail-sheet-title" className="detail-sheet-title">
                    {detail.title}
                  </h3>
                )}
                {!editing && (detail.author || detail.artist) ? (
                  <p className="detail-sheet-sub">{detail.author ?? detail.artist}</p>
                ) : null}
              </div>
            </header>

            <fieldset className="detail-kind-field" disabled={busyAction === "kind"}>
              <legend className="field-label">{t("catalog.kindLabel")}</legend>
              <div className="detail-kind-options" role="group" aria-label={t("catalog.kindLabel")}>
                <button
                  type="button"
                  className={`detail-kind-option${detail.kind === "audiobook" ? " detail-kind-option--active" : ""}`}
                  disabled={!!busyAction}
                  aria-pressed={detail.kind === "audiobook"}
                  onClick={() => void setCollectionKind("audiobook")}
                >
                  {t("catalog.kindAudiobook")}
                </button>
                <button
                  type="button"
                  className={`detail-kind-option${detail.kind === "music" ? " detail-kind-option--active" : ""}`}
                  disabled={!!busyAction}
                  aria-pressed={detail.kind === "music"}
                  onClick={() => void setCollectionKind("music")}
                >
                  {t("catalog.kindMusic")}
                </button>
              </div>
              {kindError ? (
                <p className="view-error detail-kind-error" role="alert">
                  {kindError}
                </p>
              ) : null}
              <p className="hint detail-kind-hint">
                {busyAction === "kind"
                  ? t("catalog.busyKind")
                  : detail.is_manual
                    ? t("catalog.kindManual")
                    : t("catalog.kindAuto")}
              </p>
            </fieldset>

            {editing ? (
              <div className="detail-meta-form" ref={formRef}>
                {suggestionApplied ? (
                  <p className="detail-meta-applied" role="status" aria-live="polite">
                    {t("catalog.suggestionApplied")}
                  </p>
                ) : null}
                {detail.kind === "audiobook" ? (
                  <>
                    <label className="detail-field">
                      <span className="field-label">{t("catalog.fieldAuthor")}</span>
                      <input
                        type="text"
                        className="catalog-search"
                        value={metaForm.author}
                        onChange={(e) => setMetaForm({ ...metaForm, author: e.target.value })}
                      />
                    </label>
                    <label className="detail-field">
                      <span className="field-label">{t("catalog.fieldNarrator")}</span>
                      <input
                        type="text"
                        className="catalog-search"
                        value={metaForm.narrator}
                        onChange={(e) => setMetaForm({ ...metaForm, narrator: e.target.value })}
                      />
                    </label>
                  </>
                ) : (
                  <>
                    <label className="detail-field">
                      <span className="field-label">{t("catalog.fieldArtist")}</span>
                      <input
                        type="text"
                        className="catalog-search"
                        value={metaForm.artist}
                        onChange={(e) => setMetaForm({ ...metaForm, artist: e.target.value })}
                      />
                    </label>
                    <label className="detail-field">
                      <span className="field-label">{t("catalog.fieldAlbum")}</span>
                      <input
                        type="text"
                        className="catalog-search"
                        value={metaForm.album}
                        onChange={(e) => setMetaForm({ ...metaForm, album: e.target.value })}
                      />
                    </label>
                  </>
                )}
              </div>
            ) : null}

            {metaSaved ? (
              <p className="detail-meta-saved" aria-live="polite">
                {t("catalog.metadataSaved")}
              </p>
            ) : null}

            {actionError ? (
              <p className="view-error detail-action-error" role="alert">
                {actionError}
              </p>
            ) : null}

            {canPlay ? (
              <section className="detail-playback-section" aria-labelledby="detail-playback-heading">
                <h4 id="detail-playback-heading" className="detail-section-heading">
                  {detail.kind === "music" ? t("catalog.playbackSectionMusic") : t("catalog.playbackSection")}
                </h4>
                <div className="detail-sheet-actions detail-sheet-actions--playback">
              <button
                type="button"
                className="btn btn-primary"
                disabled={!canPlay}
                onClick={() => {
                  onPlayCollection(detail.id, detail.progress_pct > 0 ? "continue" : "start");
                  closeDetail();
                }}
              >
                {detail.kind === "music"
                  ? t("catalog.playAll")
                  : detail.progress_pct > 0
                    ? t("home.continue")
                    : t("home.play")}
              </button>
              {detail.kind === "music" && canPlay ? (
                <button
                  type="button"
                  className="btn btn-secondary"
                  disabled={!canPlay || detail.playable_file_count < 2}
                  onClick={() => {
                    onPlayCollection(detail.id, "start", true);
                    closeDetail();
                  }}
                >
                  {t("catalog.shuffleAll")}
                </button>
              ) : null}
              {onAddToQueue ? (
                <>
                  <button
                    type="button"
                    className="btn btn-secondary"
                    disabled={!canPlay}
                    onClick={() => {
                      onAddToQueue(detail.id, "next");
                      closeDetail();
                    }}
                  >
                    {t("queue.playNext")}
                  </button>
                  <button
                    type="button"
                    className="btn btn-secondary"
                    disabled={!canPlay}
                    onClick={() => {
                      onAddToQueue(detail.id, "end");
                      closeDetail();
                    }}
                  >
                    {detail.kind === "music" ? t("catalog.queueAll") : t("queue.addToQueue")}
                  </button>
                </>
              ) : null}
                </div>
              </section>
            ) : null}

            <section className="detail-manage-section" aria-labelledby="detail-manage-heading">
              <h4 id="detail-manage-heading" className="detail-section-heading">
                {t("catalog.manageSection")}
              </h4>
              <div className="detail-sheet-actions detail-sheet-actions--manage">
              <button
                type="button"
                className="btn btn-secondary"
                disabled={!canPlay}
                onClick={() =>
                  openAddToPlaylist({ collectionId: detail.id, title: detail.title })
                }
              >
                {t("catalog.addToPlaylist")}
              </button>
              <button
                type="button"
                className="btn btn-secondary"
                disabled={!canPlay}
                onClick={() => {
                  void (async () => {
                    setActionError(null);
                    try {
                      await invoke("mark_collection_listened", {
                        collectionId: detail.id,
                        listened: !detail.listened,
                      });
                      await refreshDetail(detail.id);
                    } catch (e) {
                      setActionError(String(e));
                    }
                  })();
                }}
              >
                {detail.listened ? t("catalog.markUnlistened") : t("catalog.markListened")}
              </button>
              {editing ? (
                <button type="button" className="btn btn-secondary" onClick={() => void saveMetadata()}>
                  {t("catalog.saveMetadata")}
                </button>
              ) : (
                <button
                  type="button"
                  className="btn btn-ghost"
                  onClick={() => {
                    setEditing(true);
                    setMetaSaved(false);
                  }}
                >
                  {t("catalog.editTitle")}
                </button>
              )}
              {detail.kind === "audiobook" ? (
                <button
                  type="button"
                  className="btn btn-ghost"
                  disabled={!!busyAction}
                  title={t("catalog.fixTrackOrderHint")}
                  onClick={() => void fixTrackOrder()}
                >
                  {busyAction === "fix" ? t("catalog.busyFixOrder") : t("catalog.fixTrackOrder")}
                </button>
              ) : detail.kind === "music" ? (
                <button
                  type="button"
                  className="btn btn-ghost"
                  disabled={!!busyAction}
                  title={t("catalog.refreshTracksHint")}
                  onClick={() => void fixTrackOrder()}
                >
                  {busyAction === "fix" ? t("catalog.busyRefreshTracks") : t("catalog.refreshTracks")}
                </button>
              ) : null}
              <button type="button" className="btn btn-ghost" ref={closeRef} onClick={requestClose}>
                {t("modal.close")}
              </button>
              </div>
            </section>

            {onlineMetadataEnabled ? (
              <section className="detail-lookup" aria-labelledby="detail-lookup-heading">
                <div className="detail-lookup-head">
                  <h4 id="detail-lookup-heading" className="detail-lookup-title">
                    {t("catalog.lookupSectionTitle")}
                  </h4>
                  <p className="detail-lookup-desc">{t("catalog.lookupOnlineHint")}</p>
                </div>
                <button
                  type="button"
                  className="btn btn-secondary detail-lookup-btn"
                  disabled={lookupBusy}
                  aria-busy={lookupBusy}
                  onClick={() => void lookupOnline()}
                >
                  {lookupBusy ? t("catalog.busyLookup") : t("catalog.lookupOnline")}
                </button>

                <div className="detail-lookup-status" aria-live="polite">
                  {lookupBusy ? (
                    <p className="detail-lookup-msg detail-lookup-msg--loading">
                      {t("catalog.busyLookup")}
                    </p>
                  ) : lookupError ? (
                    <p className="detail-lookup-msg detail-lookup-msg--error" role="alert">
                      {lookupError}
                    </p>
                  ) : lookupDone && suggestions.length === 0 ? (
                    <p className="detail-lookup-msg detail-lookup-msg--empty">
                      {t("catalog.lookupEmpty")}
                    </p>
                  ) : lookupDone && suggestions.length > 0 ? (
                    <p className="detail-lookup-msg detail-lookup-msg--found field-label">
                      {t("catalog.lookupFound", { count: suggestions.length })}
                    </p>
                  ) : null}
                </div>

                {suggestions.length > 0 ? (
                  <>
                    <ul className="detail-lookup-results">
                      {suggestions.map((s, i) => {
                        const primary = s.title ?? s.album ?? t("catalog.lookupUntitled");
                        const secondary =
                          detail.kind === "audiobook"
                            ? [s.author, s.narrator].filter(Boolean).join(" · ")
                            : s.artist ?? "";
                        return (
                          <li key={`${s.source}-${i}`} className="detail-lookup-result">
                            <div className="detail-lookup-result-text">
                              <span className="detail-lookup-result-primary">{primary}</span>
                              {secondary ? (
                                <span className="detail-lookup-result-secondary">{secondary}</span>
                              ) : null}
                              <span className="detail-lookup-result-source">
                                {t("catalog.lookupSource", { source: s.source })}
                              </span>
                            </div>
                            <button
                              type="button"
                              className="btn btn-primary btn-compact detail-lookup-apply"
                              onClick={() => applySuggestion(s)}
                            >
                              {t("catalog.applySuggestion")}
                            </button>
                          </li>
                        );
                      })}
                    </ul>
                  </>
                ) : null}
              </section>
            ) : null}

            {rootUnavailable ? (
              <div className="missing-files-alert missing-files-alert--root" role="alert">
                <p className="missing-files-alert-title">{t("catalog.rootUnavailableTitle")}</p>
                <p className="missing-files-alert-body">{t("catalog.awayLong")}</p>
                <button
                  type="button"
                  className="btn btn-secondary btn-compact missing-files-alert-action"
                  disabled={!!busyAction}
                  onClick={() => removeCollectionFromLibrary()}
                >
                  {t("catalog.removeCollection")}
                </button>
              </div>
            ) : null}

            {showEmptyCollection ? (
              <section className="ghost-collection-panel" aria-labelledby="ghost-collection-heading">
                <h3 id="ghost-collection-heading" className="ghost-collection-panel-title">
                  {t("catalog.emptyCollectionTitle")}
                </h3>
                <p className="ghost-collection-panel-body">{t("catalog.emptyCollectionBody")}</p>
                <button
                  type="button"
                  className="btn btn-secondary ghost-collection-panel-action"
                  disabled={!!busyAction}
                  onClick={removeCollectionFromLibrary}
                >
                  {t("catalog.removeCollection")}
                </button>
              </section>
            ) : null}

            {showRemoveCollection ? (
              <section className="ghost-collection-panel" aria-labelledby="stale-collection-heading">
                <h3 id="stale-collection-heading" className="ghost-collection-panel-title">
                  {t("catalog.tracksUnavailable")}
                </h3>
                <p className="ghost-collection-panel-body">{t("catalog.tracksUnavailableLong")}</p>
                <button
                  type="button"
                  className="btn btn-secondary ghost-collection-panel-action"
                  disabled={!!busyAction}
                  onClick={removeCollectionFromLibrary}
                >
                  {t("catalog.removeCollection")}
                </button>
              </section>
            ) : null}

            <MissingFilesPanel
              files={detail.files}
              onRelink={relinkFile}
              onRemove={removeFileFromLibrary}
              busy={busyAction === "relink"}
              relinkDisabled={relinkDisabled}
              relinkDisabledHint={relinkDisabledHint}
            />

            {canPlay ? (
              <>
                <h4 className="detail-tracks-heading">
                  {t("catalog.tracksHeading")}
                  {detail.kind === "music" ? (
                    <span className="detail-tracks-count">
                      {" "}
                      · {t("catalog.trackCount", { count: detail.playable_file_count })}
                    </span>
                  ) : null}
                </h4>
                {(() => {
                  const avail = detail.files.filter((f) => !f.unavailable);
                  const total = avail.length;
                  const byDisc = new Map<number, typeof detail.files>();
                  for (const f of detail.files) {
                    const d = f.disc_index > 0 ? f.disc_index : 0;
                    const list = byDisc.get(d) ?? [];
                    list.push(f);
                    byDisc.set(d, list);
                  }
                  const discs = [...byDisc.keys()].sort((a, b) => a - b);
                  const showDiscGroups =
                    detail.layout_kind === "cd_nested" && discs.some((d) => d > 0);

                  const renderTrack = (f: (typeof detail.files)[0], indexInAvail: number) => (
                    <li
                      key={f.id}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        const items: ContextMenuEntry[] = [
                          {
                            id: "up",
                            label: t("catalog.moveUp"),
                            disabled: indexInAvail === 0,
                            onClick: () => void moveTrack(f.id, -1),
                          },
                          {
                            id: "down",
                            label: t("catalog.moveDown"),
                            disabled: indexInAvail >= total - 1,
                            onClick: () => void moveTrack(f.id, 1),
                          },
                          { type: "separator" },
                          {
                            id: "rename",
                            label: t("catalog.renameTrack"),
                            onClick: () => {
                              setEditingTrackId(f.id);
                              setEditTrackTitle(f.display_title);
                            },
                          },
                        ];
                        openContextMenu(
                          e,
                          appendPlaylistContextEntries(items, { fileId: f.id }),
                        );
                      }}
                    >
                      <div className="detail-track-main">
                        {editingTrackId === f.id ? (
                          <div className="detail-track-edit">
                            <input
                              type="text"
                              className="catalog-search"
                              value={editTrackTitle}
                              onChange={(e) => setEditTrackTitle(e.target.value)}
                              aria-label={t("catalog.renameTrack")}
                            />
                            <button
                              type="button"
                              className="btn btn-secondary btn-compact"
                              onClick={() => void saveTrackTitle(f.id)}
                            >
                              {t("catalog.saveTrackName")}
                            </button>
                          </div>
                        ) : (
                          <>
                            <span className="detail-track-title">{f.display_title}</span>
                            <span className="detail-track-of">
                              {t("catalog.trackOf", { n: indexInAvail + 1, total })}
                            </span>
                          </>
                        )}
                      </div>
                      <span className="detail-track-actions">
                        <button
                          type="button"
                          className="btn btn-ghost btn-compact"
                          aria-label={t("catalog.moveUp")}
                          disabled={indexInAvail === 0}
                          onClick={() => void moveTrack(f.id, -1)}
                        >
                          ↑
                        </button>
                        <button
                          type="button"
                          className="btn btn-ghost btn-compact"
                          aria-label={t("catalog.moveDown")}
                          disabled={indexInAvail >= total - 1}
                          onClick={() => void moveTrack(f.id, 1)}
                        >
                          ↓
                        </button>
                        <button
                          type="button"
                          className="btn btn-ghost btn-compact"
                          onClick={() => {
                            setEditingTrackId(f.id);
                            setEditTrackTitle(f.display_title);
                          }}
                        >
                          {t("catalog.renameTrack")}
                        </button>
                        <AddToPlaylistButton
                          target={{ fileId: f.id }}
                          className="btn btn-ghost btn-compact"
                        />
                      </span>
                    </li>
                  );

                  if (showDiscGroups) {
                    return discs.map((disc) => (
                      <div key={disc} className="detail-disc-group">
                        {disc > 0 ? (
                          <h5 className="detail-disc-heading">{t("catalog.cdGroup", { n: disc })}</h5>
                        ) : null}
                        <ul className="detail-track-list">
                          {(byDisc.get(disc) ?? []).map((f) => {
                            const idx = avail.findIndex((a) => a.id === f.id);
                            return renderTrack(f, idx >= 0 ? idx : 0);
                          })}
                        </ul>
                      </div>
                    ));
                  }

                  return (
                    <ul className="detail-track-list">
                      {avail.map((f, idx) => renderTrack(f, idx))}
                    </ul>
                  );
                })()}
              </>
            ) : detail.missing_file_count > 0 && !rootUnavailable && !showRemoveCollection ? (
              <p className="missing-files-all-gone" role="status">
                {t("catalog.allFilesMissing")}
              </p>
            ) : null}
          </>
        ) : null}
      </div>
    </div>
  );
}
