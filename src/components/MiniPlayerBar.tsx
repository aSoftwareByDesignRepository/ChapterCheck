import { useContextMenu, type ContextMenuEntry } from "../context/ContextMenuContext";
import { useAddToPlaylist } from "../context/AddToPlaylistContext";
import { useI18n } from "../i18n/I18nContext";

type Props = {
  title: string;
  paused: boolean;
  currentPath?: string | null;
  onExpand: () => void;
  onToggle: () => void;
  onOpenDetails?: () => void;
};

export function MiniPlayerBar({
  title,
  paused,
  currentPath,
  onExpand,
  onToggle,
  onOpenDetails,
}: Props) {
  const { t } = useI18n();
  const { openContextMenu } = useContextMenu();
  const { appendPlaylistContextEntries } = useAddToPlaylist();

  return (
    <div
      className="mini-player"
      role="region"
      aria-label={t("nav.nowPlaying")}
      onContextMenu={(e) => {
        const items: ContextMenuEntry[] = [
          {
            id: "now",
            label: t("nav.nowPlaying"),
            onClick: onExpand,
          },
          {
            id: "toggle",
            label: paused ? t("nowPlaying.playAria") : t("nowPlaying.pauseAria"),
            onClick: onToggle,
          },
        ];
        if (onOpenDetails) {
          items.push({ type: "separator" });
          items.push({
            id: "details",
            label: t("catalog.editTitle"),
            onClick: onOpenDetails,
          });
        }
        const merged = currentPath
          ? appendPlaylistContextEntries(items, { path: currentPath })
          : items;
        openContextMenu(e, merged);
      }}
    >
      <button type="button" className="mini-player-main" onClick={onExpand}>
        <span className="mini-player-title">{title}</span>
      </button>
      <button
        type="button"
        className="btn btn-primary mini-player-toggle"
        aria-label={paused ? t("nowPlaying.playAria") : t("nowPlaying.pauseAria")}
        onClick={onToggle}
      >
        {paused ? "▶" : "⏸"}
      </button>
    </div>
  );
}
