import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { MediaRow } from "../components/MediaRow";
import { useI18n } from "../i18n/I18nContext";
import type { HomeSummaryDto } from "../types/catalog";

type Props = {
  refreshKey: number;
  onPlayCollection: (id: number, mode: "continue" | "start") => void;
  onAddToQueue: (id: number, position: "next" | "end") => void;
  onOpenDetail: (id: number) => void;
  onShuffleRelax: () => void;
  onLinkFolder: () => void;
  onOpenFolder: () => void;
  onOpenFile: () => void;
  onBrowseAudiobooks: () => void;
  onBrowseMusic: () => void;
};

export function HomeView({
  refreshKey,
  onPlayCollection,
  onAddToQueue,
  onOpenDetail,
  onShuffleRelax,
  onLinkFolder,
  onOpenFolder,
  onOpenFile,
  onBrowseAudiobooks,
  onBrowseMusic,
}: Props) {
  const { t } = useI18n();
  const [home, setHome] = useState<HomeSummaryDto | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const data = await invoke<HomeSummaryDto>("get_home_summary");
      setHome(data);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void load();
    const id = window.setInterval(() => void load(), 30_000);
    return () => window.clearInterval(id);
  }, [load, refreshKey]);

  if (error) {
    return (
      <div className="view-panel view-panel--center">
        <p className="view-error" role="alert">
          {error}
        </p>
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
        <h1 className="view-title">{t("home.welcomeTitle")}</h1>
        <p className="view-lead">{t("home.welcomeSimple")}</p>
        <div className="home-welcome-actions">
          <button type="button" className="btn btn-primary btn-hero" onClick={onLinkFolder}>
            {t("library.linkFolder")}
          </button>
          <button type="button" className="btn btn-ghost btn-hero" onClick={onOpenFolder}>
            {t("sidebar.openFolder")}
          </button>
          <button type="button" className="btn btn-ghost btn-hero" onClick={onOpenFile}>
            {t("sidebar.openFile")}
          </button>
        </div>
      </div>
    );
  }

  const continueItem = home.continue_item;
  const hasShelves = home.in_progress.length > 0 || home.music_shelf.length > 0;

  return (
    <div className="view-panel home-view">
      <header className="home-head">
        <h1 className="view-title">{t("nav.home")}</h1>
        <div className="home-head-actions">
          <button type="button" className="btn btn-ghost btn-compact" onClick={onBrowseAudiobooks}>
            {t("nav.audiobooks")}
          </button>
          <button type="button" className="btn btn-ghost btn-compact" onClick={onBrowseMusic}>
            {t("nav.music")}
          </button>
          <button type="button" className="btn btn-ghost btn-compact" onClick={onShuffleRelax}>
            {t("home.shuffleRelax")}
          </button>
        </div>
      </header>

      {continueItem && !continueItem.unavailable ? (
        <section className="home-shelf home-shelf--continue" aria-labelledby="home-continue-heading">
          <h2 id="home-continue-heading" className="section-title">
            {t("home.continueBook")}
          </h2>
          <div className="media-list">
            <MediaRow
              item={continueItem}
              featured
              onPlay={(id, mode) => onPlayCollection(id, mode)}
              onAddToQueue={onAddToQueue}
              onOpen={onOpenDetail}
            />
          </div>
        </section>
      ) : null}

      {!hasShelves && !continueItem ? (
        <div className="view-empty view-empty--actions">
          <p className="view-empty-title">{t("home.emptyLibraryTitle")}</p>
          <p className="view-empty-body">{t("home.emptyLibraryBody")}</p>
          <div className="view-empty-actions">
            <button type="button" className="btn btn-primary" onClick={onBrowseAudiobooks}>
              {t("nav.audiobooks")}
            </button>
            <button type="button" className="btn btn-secondary" onClick={onBrowseMusic}>
              {t("nav.music")}
            </button>
          </div>
        </div>
      ) : null}

      {home.in_progress.length > 0 ? (
        <section className="home-shelf" aria-labelledby="home-inprogress-heading">
          <div className="section-header section-header--row">
            <h2 id="home-inprogress-heading" className="section-title">
              {t("home.inProgress")}
            </h2>
            <button type="button" className="btn btn-ghost btn-compact" onClick={onBrowseAudiobooks}>
              {t("home.seeAll")}
            </button>
          </div>
          <div className="media-list">
            {home.in_progress.map((item) => (
              <MediaRow
                key={item.id}
                item={item}
                onPlay={(id, mode) => onPlayCollection(id, mode)}
                onAddToQueue={onAddToQueue}
                onOpen={onOpenDetail}
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
            <button type="button" className="btn btn-ghost btn-compact" onClick={onBrowseMusic}>
              {t("home.seeAll")}
            </button>
          </div>
          <div className="media-list">
            {home.music_shelf.map((item) => (
              <MediaRow
                key={item.id}
                item={item}
                onPlay={(id, mode) => onPlayCollection(id, mode)}
                onAddToQueue={onAddToQueue}
                onOpen={onOpenDetail}
              />
            ))}
          </div>
        </section>
      ) : null}
    </div>
  );
}
