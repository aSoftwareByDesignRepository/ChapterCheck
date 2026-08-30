import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { MediaRow } from "../components/MediaRow";
import { useI18n } from "../i18n/I18nContext";
import type { HomeSummaryDto } from "../types/catalog";
import { homeHasVisibleContent, shouldApplyAsyncResult } from "../utils/viewLogic";

type Props = {
  refreshKey: number;
  onPlayCollection: (id: number, mode: "continue" | "start", shuffle?: boolean) => void;
  onAddToQueue: (id: number, position: "next" | "end") => void;
  onOpenDetail: (id: number) => void;
  onShuffleRelax: () => void;
  onLinkFolder: () => void;
  onOpenFile: () => void;
  onBrowseAudiobooks: () => void;
  onBrowseMusic: () => void;
  onRemoveCollection?: (id: number, title: string) => void;
};

export function HomeView({
  refreshKey,
  onPlayCollection,
  onAddToQueue,
  onOpenDetail,
  onShuffleRelax,
  onLinkFolder,
  onOpenFile,
  onBrowseAudiobooks,
  onBrowseMusic,
  onRemoveCollection,
}: Props) {
  const { t } = useI18n();
  const [home, setHome] = useState<HomeSummaryDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const requestId = useRef(0);

  const load = useCallback(async () => {
    const id = ++requestId.current;
    try {
      const data = await invoke<HomeSummaryDto>("get_home_summary");
      if (!shouldApplyAsyncResult(id, requestId.current)) return;
      setHome(data);
      setError(null);
    } catch (e) {
      if (!shouldApplyAsyncResult(id, requestId.current)) return;
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void load();
    const pollMs = home?.scan_in_progress ? 2_000 : 30_000;
    const id = window.setInterval(() => void load(), pollMs);
    return () => window.clearInterval(id);
  }, [load, refreshKey, home?.scan_in_progress]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<boolean>("abp:scan-status", () => {
      void load();
    }).then((fn) => {
      if (cancelled) {
        fn();
        return;
      }
      unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [load]);

  if (error && !home) {
    return (
      <div className="view-panel view-panel--center home-error">
        <p className="view-error" role="alert">
          {error}
        </p>
        <button type="button" className="btn btn-primary" onClick={() => void load()}>
          {t("home.retry")}
        </button>
      </div>
    );
  }

  if (!home) {
    return (
      <div className="view-panel view-panel--center">
        <p className="view-loading" aria-live="polite">
          {t("home.loading")}
        </p>
      </div>
    );
  }

  if (!home.has_library) {
    return (
      <div className="view-panel view-panel--center home-welcome home-welcome--simple">
        {error ? (
          <p className="view-error" role="alert">
            {error}
          </p>
        ) : null}
        <h1 className="view-title home-welcome-brand">{t("app.title")}</h1>
        <p className="view-lead">{t("home.welcomeSimple")}</p>
        <div className="home-welcome-actions">
          <button type="button" className="btn btn-primary btn-hero" onClick={onLinkFolder}>
            {t("home.addFolderCta")}
          </button>
        </div>
        <button type="button" className="home-quiet-link" onClick={onOpenFile}>
          {t("home.playFileOnce")}
        </button>
      </div>
    );
  }

  const continueItem = home.continue_item;
  const continueId = continueItem && !continueItem.unavailable ? continueItem.id : null;
  const inProgress = home.in_progress.filter((item) => item.id !== continueId);
  const showContinue = continueId != null;
  const showEmpty = !homeHasVisibleContent({
    continueItem,
    inProgressCount: inProgress.length,
    musicCount: home.music_shelf.length,
  });

  return (
    <div className="view-panel home-view">
      {error ? (
        <div className="library-prompt-banner" role="alert">
          <div className="library-prompt-row">
            <p>{error}</p>
            <button type="button" className="btn btn-secondary btn-compact" onClick={() => void load()}>
              {t("home.retry")}
            </button>
          </div>
        </div>
      ) : null}
      {home.scan_in_progress ? (
        <div className="library-prompt-banner library-prompt-banner--info home-scan-banner" role="status">
          <p>{t("home.scanning")}</p>
        </div>
      ) : null}

      {showContinue && continueItem ? (
        <section className="home-shelf home-shelf--continue" aria-labelledby="home-continue-heading">
          <h1 id="home-continue-heading" className="section-title">
            {t("home.continueBook")}
          </h1>
          <div className="media-list">
            <MediaRow
              item={continueItem}
              featured
              onPlay={onPlayCollection}
              onAddToQueue={onAddToQueue}
              onOpen={onOpenDetail}
            />
          </div>
        </section>
      ) : (
        <header className="home-head">
          <h1 className="view-title">{t("nav.home")}</h1>
        </header>
      )}

      {showEmpty ? (
        <div className="view-empty view-empty--actions">
          <p className="view-empty-title">{t("home.emptyLibraryTitle")}</p>
          <p className="view-empty-body">{t("home.emptyLibraryBody")}</p>
          <div className="view-empty-actions">
            <button type="button" className="btn btn-primary" onClick={onLinkFolder}>
              {t("home.addFolderCta")}
            </button>
          </div>
        </div>
      ) : null}

      {inProgress.length > 0 ? (
        <section className="home-shelf" aria-labelledby="home-inprogress-heading">
          <div className="section-header section-header--row">
            <h2 id="home-inprogress-heading" className="section-title">
              {t("home.inProgress")}
            </h2>
            <button type="button" className="home-quiet-link" onClick={onBrowseAudiobooks}>
              {t("home.seeAll")}
            </button>
          </div>
          <div className="media-list">
            {inProgress.map((item) => (
              <MediaRow
                key={item.id}
                item={item}
                onPlay={onPlayCollection}
                onAddToQueue={onAddToQueue}
                onOpen={onOpenDetail}
                onRemoveCollection={onRemoveCollection}
              />
            ))}
          </div>
        </section>
      ) : null}

      {home.music_shelf.length > 0 ? (
        <section className="home-shelf" aria-labelledby="home-music-heading">
          <div className="section-header section-header--row">
            <h2 id="home-music-heading" className="section-title">
              {t("home.yourMusic")}
            </h2>
            <div className="home-shelf-tools">
              <button type="button" className="btn btn-secondary btn-compact" onClick={onShuffleRelax}>
                {t("home.shuffleRelax")}
              </button>
              <button type="button" className="home-quiet-link" onClick={onBrowseMusic}>
                {t("home.seeAll")}
              </button>
            </div>
          </div>
          <div className="media-list">
            {home.music_shelf.map((item) => (
              <MediaRow
                key={item.id}
                item={item}
                onPlay={onPlayCollection}
                onAddToQueue={onAddToQueue}
                onOpen={onOpenDetail}
                onRemoveCollection={onRemoveCollection}
              />
            ))}
          </div>
        </section>
      ) : null}
    </div>
  );
}
