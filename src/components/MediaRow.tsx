import { invoke } from "@tauri-apps/api/core";
import type { MouseEvent } from "react";
import { IconPlaySm, IconQueueAdd } from "./PlayerIcons";
import { CoverImage } from "./CoverImage";
import { useContextMenu, type ContextMenuEntry } from "../context/ContextMenuContext";
import { useAddToPlaylist } from "../context/AddToPlaylistContext";
import { useI18n } from "../i18n/I18nContext";
import type { CollectionSummaryDto } from "../types/catalog";
import { collectionStatusKind } from "../utils/collectionStatus";
import { coverUrl } from "../utils/coverUrl";

type Props = {
  item: CollectionSummaryDto;
  onPlay: (id: number, mode: "continue" | "start", shuffle?: boolean) => void;
  onAddToQueue?: (id: number, position: "next" | "end") => void;
  onOpen: (id: number) => void;
  onChanged?: () => void;
  onRemoveCollection?: (id: number, title: string) => void;
  featured?: boolean;
  selectMode?: boolean;
  selected?: boolean;
  onSelectToggle?: (id: number) => void;
};

export function MediaRow({
  item,
  onPlay,
  onAddToQueue,
  onOpen,
  onRemoveCollection,
  featured,
  selectMode = false,
  selected = false,
  onSelectToggle,
}: Props) {
  const { t } = useI18n();
  const { openContextMenu } = useContextMenu();
  const { appendPlaylistContextEntries, invalidatePlaylists } = useAddToPlaylist();
  const src = coverUrl(item.cover_path);
  const status = collectionStatusKind(item);
  const needsAttention = status !== "ok";
  const playMode = item.in_progress && !item.listened ? "continue" : "start";
  const playLabel =
    playMode === "continue"
      ? t("media.playContinue", { title: item.title })
      : t("media.playTitle", { title: item.title });
  const rawProgress = item.progress_pct;
  const progress =
    rawProgress > 0 && rawProgress < 1 ? 1 : Math.round(rawProgress);
  const showProgress = !needsAttention && rawProgress > 0 && !item.listened;

  const statusBadge = () => {
    if (status === "drive_away") return t("catalog.away");
    if (status === "tracks_missing") return t("catalog.tracksUnavailable");
    if (status === "empty") return t("catalog.emptyCollectionTitle");
    return null;
  };

  const handlePlay = () => {
    if (needsAttention) {
      onOpen(item.id);
      return;
    }
    onPlay(item.id, playMode);
  };

  const removeCollection = () => {
    // Never call remove IPC without a confirm callback — silent remove is an XSS footgun.
    onRemoveCollection?.(item.id, item.title);
  };

  const contextItems = (): ContextMenuEntry[] => {
    const items: ContextMenuEntry[] = [];
    if (!needsAttention) {
      items.push({
        id: "play",
        label: playMode === "continue" ? t("home.continue") : t("home.play"),
        onClick: () => onPlay(item.id, playMode),
      });
      if (item.in_progress && !item.listened) {
        items.push({
          id: "start",
          label: t("contextMenu.playFromStart"),
          onClick: () => onPlay(item.id, "start"),
        });
      }
      if (item.kind === "music" && item.playable_file_count >= 2) {
        items.push({
          id: "shuffle",
          label: t("catalog.shuffleAll"),
          onClick: () => onPlay(item.id, "start", true),
        });
      }
      if (onAddToQueue) {
        items.push({ type: "separator" });
        items.push({
          id: "next",
          label: t("queue.playNext"),
          onClick: () => onAddToQueue(item.id, "next"),
        });
        items.push({
          id: "queue",
          label: t("queue.addToQueue"),
          onClick: () => onAddToQueue(item.id, "end"),
        });
      }
    } else {
      items.push({
        id: "fix-missing",
        label: t("catalog.fixMissingFiles"),
        onClick: () => onOpen(item.id),
      });
      if (onRemoveCollection) {
        items.push({
          id: "remove-collection",
          label: t("catalog.removeCollection"),
          danger: true,
          onClick: removeCollection,
        });
      }
    }
    items.push({ type: "separator" });
    items.push({
      id: "details",
      label: t("catalog.editTitle"),
      onClick: () => onOpen(item.id),
    });
    if (!needsAttention && item.kind === "music") {
      items.push({
        id: "create-playlist",
        label: t("playlists.createFromThisAlbum"),
        onClick: () => {
          void invoke<number>("create_playlist_from_collection", { collectionId: item.id }).then(
            () => invalidatePlaylists(),
          );
        },
      });
    }
    return items;
  };

  const openRowMenu = (event: MouseEvent<HTMLElement>) => {
    const base = contextItems();
    const items = needsAttention
      ? base
      : appendPlaylistContextEntries(base, {
          collectionId: item.id,
          title: item.title,
        });
    openContextMenu(event, items);
  };

  const rowClass = [
    "media-row",
    featured ? "media-row--featured" : "",
    status === "drive_away" ? "media-row--away" : "",
    status === "tracks_missing" || status === "empty" ? "media-row--missing" : "",
    selectMode
      ? "media-row--select"
      : needsAttention && onRemoveCollection
        ? "media-row--attention"
        : onAddToQueue
          ? ""
          : "media-row--no-queue",
    selectMode && selected ? "media-row--selected" : "",
  ]
    .filter(Boolean)
    .join(" ");

  const badgeClass = [
    "media-row-badge",
    status === "drive_away" ? "media-row-badge--away" : "",
    status === "tracks_missing" || status === "empty" ? "media-row-badge--missing" : "",
    showProgress ? "media-row-badge--progress" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <article className={rowClass} onContextMenu={(e) => {
        const base = contextItems();
        const items = needsAttention
          ? base
          : appendPlaylistContextEntries(base, {
              collectionId: item.id,
              title: item.title,
            });
        openContextMenu(e, items);
      }}
    >
      {selectMode ? (
        <label className="media-row-select">
          <input
            type="checkbox"
            className="media-row-select-input"
            checked={selected}
            aria-label={t("catalog.selectItem", { title: item.title })}
            onChange={() => onSelectToggle?.(item.id)}
          />
        </label>
      ) : (
        <button
          type="button"
          className="media-row-play"
          aria-label={
            needsAttention ? t("catalog.openToFixMissing", { title: item.title }) : playLabel
          }
          onClick={handlePlay}
        >
          <IconPlaySm />
        </button>
      )}

      <button
        type="button"
        className="media-row-main"
        aria-label={t("catalog.openDetails", { title: item.title })}
        onClick={() => {
          if (selectMode) {
            onSelectToggle?.(item.id);
            return;
          }
          onOpen(item.id);
        }}
      >
        <div className="media-row-cover" aria-hidden="true">
          <CoverImage src={src} kind={item.kind} className="media-row-cover-img" />
        </div>
        <div className="media-row-text">
          <span className="media-row-title">{item.title}</span>
          {item.subtitle ? <span className="media-row-sub">{item.subtitle}</span> : null}
          {item.kind === "music" && item.playable_file_count > 0 ? (
            <span className="media-row-sub media-row-sub--tracks">
              {t("catalog.trackCount", { count: item.playable_file_count })}
            </span>
          ) : null}
          {needsAttention ? (
            <span className={badgeClass}>{statusBadge()}</span>
          ) : item.listened ? (
            <span className="media-row-badge">{t("catalog.finished")}</span>
          ) : showProgress ? (
            <span className="media-row-badge media-row-badge--progress">
              {t("media.progress", { pct: progress })}
            </span>
          ) : null}
        </div>
        {showProgress ? (
          <div className="media-row-progress" aria-hidden="true">
            <div className="media-row-progress-fill" style={{ width: `${progress}%` }} />
          </div>
        ) : null}
      </button>

      {!selectMode && needsAttention && onRemoveCollection ? (
        <button
          type="button"
          className="media-row-remove"
          aria-label={t("catalog.removeCollection")}
          title={t("catalog.removeCollection")}
          onClick={() => onRemoveCollection(item.id, item.title)}
        >
          {t("catalog.removeShort")}
        </button>
      ) : !selectMode && onAddToQueue ? (
        <button
          type="button"
          className="media-row-queue"
          disabled={needsAttention}
          aria-label={t("queue.addTitle", { title: item.title })}
          title={t("queue.addTitle", { title: item.title })}
          onClick={() => onAddToQueue(item.id, "end")}
        >
          <IconQueueAdd />
        </button>
      ) : null}

      <button
        type="button"
        className="media-row-more"
        aria-label={t("media.moreActions", { title: item.title })}
        aria-haspopup="menu"
        onClick={(e) => openRowMenu(e)}
      >
        <span aria-hidden="true">⋯</span>
      </button>
    </article>
  );
}
