import { CoverImage } from "./CoverImage";
import { IconPause, IconPlay, IconSkipNext, IconSkipPrev, IconSleep } from "./PlayerIcons";
import { useContextMenu, type ContextMenuEntry } from "../context/ContextMenuContext";
import { useAddToPlaylist } from "../context/AddToPlaylistContext";
import { useI18n } from "../i18n/I18nContext";

type Props = {
  title: string;
  paused: boolean;
  currentPath?: string | null;
  coverSrc?: string | null;
  coverKind?: string;
  position: number;
  duration: number | null;
  progressMax: number;
  sliderValue: number;
  setSeekUi: (v: number | null) => void;
  formatClock: (sec: number) => string;
  canSkip: boolean;
  canSeek: boolean;
  canToggle: boolean;
  onExpand: () => void;
  onToggle: () => void;
  onSkipPrev: () => void;
  onSkipNext: () => void;
  onSeekTo: (sec: number) => void;
  onOpenDetails?: () => void;
  onSleep?: () => void;
  sleepActive?: boolean;
  sleepRemainLabel?: string | null;
};

export function MiniPlayerBar({
  title,
  paused,
  currentPath,
  coverSrc,
  coverKind = "audiobook",
  position,
  duration,
  progressMax,
  sliderValue,
  setSeekUi,
  formatClock,
  canSkip,
  canSeek,
  canToggle,
  onExpand,
  onToggle,
  onSkipPrev,
  onSkipNext,
  onSeekTo,
  onOpenDetails,
  onSleep,
  sleepActive = false,
  sleepRemainLabel = null,
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
            disabled: !canToggle,
          },
        ];
        if (onSleep) {
          items.push({
            id: "sleep",
            label: t("menu.playback.sleepTimer"),
            onClick: onSleep,
          });
        }
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
      <div className="mini-player-progress">
        <span className="mini-player-time" aria-hidden="true">
          {formatClock(position)}
        </span>
        <input
          className="slider mini-player-seek"
          type="range"
          min={0}
          max={progressMax}
          step={0.1}
          value={Math.min(sliderValue, progressMax)}
          disabled={!canSeek}
          aria-valuemin={0}
          aria-valuemax={progressMax}
          aria-valuenow={Math.min(sliderValue, progressMax)}
          aria-valuetext={`${formatClock(sliderValue)} / ${duration ? formatClock(duration) : t("nowPlaying.timeUnknown")}`}
          aria-label={t("nowPlaying.seekAria")}
          onPointerDown={() => setSeekUi(position)}
          onInput={(e) => setSeekUi(Number(e.currentTarget.value))}
          onPointerUp={(e) => {
            const v = Number((e.currentTarget as HTMLInputElement).value);
            setSeekUi(null);
            onSeekTo(v);
          }}
          onPointerCancel={() => setSeekUi(null)}
          onKeyUp={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              const v = Number((e.currentTarget as HTMLInputElement).value);
              setSeekUi(null);
              onSeekTo(v);
            }
          }}
        />
        <span className="mini-player-time mini-player-time--end" aria-hidden="true">
          {duration ? formatClock(duration) : t("nowPlaying.timeUnknown")}
        </span>
      </div>
      <div className="mini-player-row">
        <button type="button" className="mini-player-main" onClick={onExpand}>
          <CoverImage
            src={coverSrc ?? null}
            kind={coverKind}
            className="mini-player-cover"
            alt=""
          />
          <span className="mini-player-copy">
            <span className="mini-player-kicker">{t("nav.nowPlaying")}</span>
            <span className="mini-player-title">{title}</span>
          </span>
        </button>
        <div className="mini-player-controls" role="group" aria-label={t("nowPlaying.transportLabel")}>
          {onSleep ? (
            <button
              type="button"
              className={`btn btn-ghost mini-player-skip${sleepActive ? " mini-player-sleep--on" : ""}`}
              aria-label={
                sleepActive && sleepRemainLabel
                  ? t("sleep.chip.aria", { time: sleepRemainLabel })
                  : t("sleep.chip.title")
              }
              onClick={onSleep}
            >
              <IconSleep />
            </button>
          ) : null}
          <button
            type="button"
            className="btn btn-ghost mini-player-skip"
            aria-label={t("nowPlaying.prevAria")}
            disabled={!canSkip}
            onClick={onSkipPrev}
          >
            <IconSkipPrev />
          </button>
          <button
            type="button"
            className="btn btn-primary mini-player-toggle"
            aria-label={paused ? t("nowPlaying.playAria") : t("nowPlaying.pauseAria")}
            disabled={!canToggle}
            onClick={onToggle}
          >
            {paused ? <IconPlay /> : <IconPause />}
          </button>
          <button
            type="button"
            className="btn btn-ghost mini-player-skip"
            aria-label={t("nowPlaying.nextAria")}
            disabled={!canSkip}
            onClick={onSkipNext}
          >
            <IconSkipNext />
          </button>
        </div>
      </div>
    </div>
  );
}
