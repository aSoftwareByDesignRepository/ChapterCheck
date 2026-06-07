import { invoke } from "@tauri-apps/api/core";
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
  onPlay: (id: number, mode: "continue" | "start") => void;
  onAddToQueue?: (id: number, position: "next" | "end") => void;
  onOpen: (id: number) => void;
  onChanged?: () => void;
  onRemoveCollection?: (id: number, title: string) => void;
  featured?: boolean;
};

export function MediaRow({
  item,
  onPlay,
  onAddToQueue,
  onOpen,
  onChanged,
  onRemoveCollection,
  featured,
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
    if (onRemoveCollection) {
      onRemoveCollection(item.id, item.title);
      return;
    }
    void invoke("remove_collection_from_library", { collectionId: item.id }).then(() => {
      onChanged?.();
    });
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
      items.push({
        id: "remove-collection",
        label: t("catalog.removeCollection"),
        danger: true,
        onClick: removeCollection,
      });
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

  const rowClass = [
    "media-row",
    featured ? "media-row--featured" : "",
    status === "drive_away" ? "media-row--away" : "",
    status === "tracks_missing" || status === "empty" ? "media-row--missing" : "",
    onAddToQueue ? "" : "media-row--no-queue",
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
      <button
        type="button"
        className="media-row-play"
        aria-label={needsAttention ? t("catalog.fixMissingFiles") : playLabel}
        onClick={handlePlay}
      >
        <IconPlaySm />
      </button>

      <button
        type="button"
        className="media-row-main"
        onClick={handlePlay}
      >
        <div className="media-row-cover" aria-hidden="true">
          <CoverImage src={src} kind={item.kind} className="media-row-cover-img" />
        </div>
        <div className="media-row-text">
          <span className="media-row-title">{item.title}</span>
          {item.subtitle ? <span className="media-row-sub">{item.subtitle}</span> : null}
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

      {onAddToQueue ? (
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
        aria-label={t("media.details", { title: item.title })}
        onClick={() => onOpen(item.id)}
      >
        <span aria-hidden="true">⋯</span>
      </button>
    </article>
  );
}
