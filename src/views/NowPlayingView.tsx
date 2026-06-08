import { isTauri } from "@tauri-apps/api/core";
import { useState } from "react";
import { CoverImage } from "../components/CoverImage";
import {
  IconPause,
  IconPlay,
  IconSkipNext,
  IconSkipPrev,
} from "../components/PlayerIcons";
import { useContextMenu, type ContextMenuEntry } from "../context/ContextMenuContext";
import { useAddToPlaylist } from "../context/AddToPlaylistContext";
import { useI18n } from "../i18n/I18nContext";

type ChapterDto = {
  index: number;
  title: string;
  time_sec: number;
};

type TransportDto = {
  position_sec: number;
  duration_sec: number | null;
  paused: boolean;
  speed: number;
  eof: boolean;
  idle: boolean;
  current_index: number | null;
  mpv_error: string | null;
};

type Props = {
  transport: TransportDto | null;
  chapters: ChapterDto[];
  isMusicSession: boolean;
  nowCoverSrc: string | null;
  currentTitle: string;
  liveStatus: string;
  statusTone: string;
  hasQueue: boolean;
  hasTrack: boolean;
  canSeekTransport: boolean;
  canSkipTransport: boolean;
  canTogglePlayback: boolean;
  allTracksListened: boolean;
  sliderValue: number;
  progressMax: number;
  seekUi: number | null;
  setSeekUi: (v: number | null) => void;
  formatClock: (sec: number) => string;
  onSeekTo: (sec: number) => void;
  onSeekDelta: (delta: number) => void;
  onTogglePause: () => void;
  onSkipPrev: () => void;
  onSkipNext: () => void;
  onSetSpeed: (speed: number) => void;
  onSetDefaultSpeed: (speed: number) => void;
  onResetTrackSpeed: () => void;
  onMarkSessionListened: (listened: boolean) => void;
  onDeleteSessionFiles: () => void;
  deleteSessionLabel?: string | null;
  onOpenDetails?: () => void;
  onShuffleQueue?: () => void;
  queueLength?: number;
  currentPath?: string | null;
  osMediaActive?: boolean;
};

export function NowPlayingView({
  transport,
  chapters,
  isMusicSession,
  nowCoverSrc,
  currentTitle,
  liveStatus,
  statusTone,
  hasQueue,
  hasTrack,
  canSeekTransport,
  canSkipTransport,
  canTogglePlayback,
  allTracksListened,
  sliderValue,
  progressMax,
  seekUi,
  setSeekUi,
  formatClock,
  onSeekTo,
  onSeekDelta,
  onTogglePause,
  onSkipPrev,
  onSkipNext,
  onSetSpeed,
  onSetDefaultSpeed,
  onResetTrackSpeed,
  onMarkSessionListened,
  onDeleteSessionFiles,
  deleteSessionLabel,
  onOpenDetails,
  onShuffleQueue,
  queueLength = 0,
  currentPath,
  osMediaActive = false,
}: Props) {
  const { t } = useI18n();
  const { openContextMenu } = useContextMenu();
  const { appendPlaylistContextEntries } = useAddToPlaylist();
  const [showMusicMore, setShowMusicMore] = useState(false);
  const position = transport?.position_sec ?? 0;
  const duration = transport?.duration_sec ?? null;
  const showSpeed = !isMusicSession || showMusicMore;

  return (
    <section className="panel panel-hero panel--stage" aria-labelledby="now-playing-title">
      <div className="panel-header panel-header--hero">
        <h2 className="panel-title" id="now-playing-title">
          {t("nowPlaying.title")}
        </h2>
        <span className={`status-pill status-pill--${statusTone}`} aria-live="polite">
          {liveStatus}
        </span>
      </div>

      <div className="panel-body now-playing">
        <div
          className="now-hero"
          onContextMenu={(e) => {
            const items: ContextMenuEntry[] = [
              {
                id: "toggle",
                label:
                  !hasTrack || transport?.paused || transport?.eof
                    ? t("nowPlaying.playAria")
                    : t("nowPlaying.pauseAria"),
                disabled: !canTogglePlayback,
                onClick: onTogglePause,
              },
              {
                id: "prev",
                label: t("nowPlaying.prevAria"),
                disabled: !canSkipTransport,
                onClick: onSkipPrev,
              },
              {
                id: "next",
                label: t("nowPlaying.nextAria"),
                disabled: !canSkipTransport,
                onClick: onSkipNext,
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
            if (hasQueue) {
              items.push({ type: "separator" });
              items.push({
                id: "listened",
                label: allTracksListened
                  ? t("library.markUnlistened")
                  : t("library.markListened"),
                disabled: !isTauri(),
                onClick: () => onMarkSessionListened(!allTracksListened),
              });
            }
            const merged = currentPath
              ? appendPlaylistContextEntries(items, { path: currentPath })
              : items;
            openContextMenu(e, merged);
          }}
        >
          <div className="now-hero-art" aria-hidden="true">
            <CoverImage
              src={nowCoverSrc}
              kind={isMusicSession ? "music" : "audiobook"}
              className="now-hero-cover"
            />
          </div>
          <div className="now-hero-text">
            <div className="meta-label">{t("nowPlaying.currentTitle")}</div>
            <div className="meta-value meta-value--title" id="current-track-name">
              {currentTitle}
            </div>
          </div>
        </div>

        <div className="progress" aria-label={t("nowPlaying.progressAria")}>
          <div className="progress-row">
            <span className="time-tag" aria-hidden="true">
              {formatClock(position)}
            </span>
            <span className="progress-divider" aria-hidden="true" />
            <span className="time-tag time-tag--dim" aria-hidden="true">
              {duration ? formatClock(duration) : t("nowPlaying.timeUnknown")}
            </span>
          </div>
          <input
            className="slider slider--seek"
            aria-valuemin={0}
            aria-valuemax={progressMax}
            aria-valuenow={Math.min(sliderValue, progressMax)}
            aria-valuetext={`${formatClock(sliderValue)} of ${duration ? formatClock(duration) : t("nowPlaying.timeUnknown")}`}
            aria-label={t("nowPlaying.seekAria")}
            type="range"
            min={0}
            max={progressMax}
            step={0.25}
            value={Math.min(sliderValue, progressMax)}
            disabled={!canSeekTransport}
            onPointerDown={() => setSeekUi(position)}
            onInput={(e) => setSeekUi(Number(e.currentTarget.value))}
            onPointerUp={(e) => {
              const v = Number((e.currentTarget as HTMLInputElement).value);
              setSeekUi(null);
              void onSeekTo(v);
            }}
            onPointerCancel={() => setSeekUi(null)}
            onKeyUp={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                const v = Number((e.currentTarget as HTMLInputElement).value);
                setSeekUi(null);
                void onSeekTo(v);
              }
            }}
            onBlur={(e) => {
              if (seekUi == null) return;
              const v = Number((e.currentTarget as HTMLInputElement).value);
              setSeekUi(null);
              void onSeekTo(v);
            }}
          />
        </div>

        <div className="transport-block">
          <p className="transport-block-label" id="transport-controls-label">
            {t("nowPlaying.transportLabel")}
          </p>
          <div className="transport" role="group" aria-labelledby="transport-controls-label">
            <button
              className="btn icon-btn"
              type="button"
              aria-label={t("nowPlaying.prevAria")}
              disabled={!canSkipTransport}
              onClick={() => void onSkipPrev()}
            >
              <IconSkipPrev />
            </button>
            <button
              className="btn btn-skip"
              type="button"
              aria-label={t("nowPlaying.rewindAria")}
              disabled={!canSeekTransport}
              onClick={() => void onSeekDelta(-30)}
            >
              −30s
            </button>
            <button
              className="btn btn-primary icon-btn icon-btn--play"
              type="button"
              aria-label={
                !hasTrack || transport?.paused || transport?.eof
                  ? t("nowPlaying.playAria")
                  : t("nowPlaying.pauseAria")
              }
              disabled={!canTogglePlayback}
              onClick={() => void onTogglePause()}
            >
              {!hasTrack || transport?.paused || transport?.eof ? <IconPlay /> : <IconPause />}
            </button>
            <button
              className="btn btn-skip"
              type="button"
              aria-label={t("nowPlaying.forwardAria")}
              disabled={!canSeekTransport}
              onClick={() => void onSeekDelta(30)}
            >
              +30s
            </button>
            <button
              className="btn icon-btn"
              type="button"
              aria-label={t("nowPlaying.nextAria")}
              disabled={!canSkipTransport}
              onClick={() => void onSkipNext()}
            >
              <IconSkipNext />
            </button>
            {onShuffleQueue ? (
              <button
                className="btn btn-ghost transport-shuffle-btn"
                type="button"
                title={t("nowPlaying.shuffleQueueTitle")}
                aria-label={t("nowPlaying.shuffleQueueTitle")}
                disabled={!hasQueue || queueLength < 2}
                onClick={() => void onShuffleQueue()}
              >
                {t("nowPlaying.shuffleQueue")}
              </button>
            ) : null}
          </div>
          {osMediaActive ? (
            <div className="transport-headphone" role="status" aria-live="polite">
              <p className="transport-hint">{t("nowPlaying.headphoneHint")}</p>
              <span className="transport-headphone-badge transport-headphone-badge--on">
                {t("nowPlaying.headphoneStatusOn")}
              </span>
            </div>
          ) : null}
        </div>

        {!isMusicSession && chapters.length > 0 ? (
          <div className="chapters-card" aria-label={t("chapters.title")}>
            <div className="speed-card-head">
              <span className="field-label">{t("chapters.title")}</span>
              <span className="chapters-badge">{chapters.length}</span>
            </div>
            <ul className="chapter-list">
              {chapters.map((ch) => (
                <li key={`${ch.index}-${ch.time_sec}`}>
                  <button
                    type="button"
                    className="chapter-item"
                    disabled={!canSeekTransport || !!transport?.mpv_error}
                    onClick={() => void onSeekTo(ch.time_sec)}
                  >
                    <span className="chapter-time">{formatClock(ch.time_sec)}</span>
                    <span className="chapter-title">{ch.title}</span>
                  </button>
                </li>
              ))}
            </ul>
            <p className="hint">{t("chapters.hint")}</p>
          </div>
        ) : null}

        {hasQueue || onOpenDetails ? (
          <div className="library-actions session-actions">
            {onOpenDetails ? (
              <button type="button" className="btn btn-secondary" onClick={onOpenDetails}>
                {t("nowPlaying.openDetails")}
              </button>
            ) : null}
            {hasQueue ? (
              <>
                <button
                  type="button"
                  className="btn btn-secondary"
                  disabled={!isTauri()}
                  onClick={() => void onMarkSessionListened(!allTracksListened)}
                >
                  {allTracksListened ? t("library.markUnlistened") : t("library.markListened")}
                </button>
                {deleteSessionLabel ? (
                  <button
                    type="button"
                    className="btn btn-danger"
                    disabled={!isTauri()}
                    onClick={() => void onDeleteSessionFiles()}
                  >
                    {deleteSessionLabel}
                  </button>
                ) : null}
              </>
            ) : null}
          </div>
        ) : null}

        {isMusicSession && !showMusicMore ? (
          <button
            type="button"
            className="btn btn-ghost music-more-btn"
            onClick={() => setShowMusicMore(true)}
          >
            {t("nowPlaying.moreControls")}
          </button>
        ) : null}

        {showSpeed ? (
          <div className="speed-card" aria-label={t("speed.label")}>
            <div className="speed-card-head">
              <label className="field-label" htmlFor="speed-slider">
                {t("speed.label")}
              </label>
              <div className="speed-readout" aria-live="polite">
                {transport ? `${transport.speed.toFixed(2)}×` : t("speed.readoutEmpty")}
              </div>
            </div>
            <input
              id="speed-slider"
              className="slider slider--speed"
              type="range"
              min={0.5}
              max={4}
              step={0.05}
              value={transport?.speed ?? 1}
              disabled={!transport}
              onChange={(e) => void onSetSpeed(Number(e.target.value))}
            />
            <p className="hint">
              {isMusicSession ? t("speed.hintMusic") : t("speed.hintAudiobook")}
            </p>
            <div className="speed-actions">
              <button
                className="btn btn-secondary speed-actions-btn"
                type="button"
                disabled={!transport || !!transport.mpv_error}
                onClick={() => void onSetDefaultSpeed(transport?.speed ?? 1)}
              >
                {isMusicSession ? t("speed.saveDefaultMusic") : t("speed.saveDefaultAudiobook")}
              </button>
              <button
                className="btn btn-ghost speed-actions-btn"
                type="button"
                disabled={!transport || !!transport.mpv_error}
                onClick={() => void onResetTrackSpeed()}
              >
                {t("speed.resetDefault")}
              </button>
            </div>
          </div>
        ) : null}
      </div>
    </section>
  );
}
