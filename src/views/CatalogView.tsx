import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { MediaRow } from "../components/MediaRow";
import { IconPlaySm } from "../components/PlayerIcons";
import { useI18n } from "../i18n/I18nContext";
import type { CollectionListPageDto, SetCollectionsKindResult } from "../types/catalog";

const PAGE_SIZE = 50;

type ConfirmCfg = {
  title: string;
  body: string;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void | Promise<void>;
};

type Props = {
  kind: "audiobook" | "music";
  refreshKey?: number;
  onPlayCollection: (id: number, mode: "continue" | "start", shuffle?: boolean) => void;
  onOpenDetail: (id: number) => void;
  onAddToQueue?: (id: number, position: "next" | "end") => void;
  onPlayAll?: (opts: { filter: Filter; search: string; shuffle?: boolean }) => void;
  onAddAllToQueue?: (opts: { filter: Filter; search: string; position: "next" | "end" }) => void;
  onLinkFolder?: () => void;
  onOpenFolder?: () => void;
  onRemoveCollection?: (id: number, title: string) => void;
  onLibraryChanged?: () => void;
  openConfirm?: (cfg: ConfirmCfg) => void;
};

type Filter = "all" | "in-progress" | "finished" | "away";

export function CatalogView({
  kind,
  refreshKey = 0,
  onPlayCollection,
  onOpenDetail,
  onAddToQueue,
  onPlayAll,
  onAddAllToQueue,
  onLinkFolder,
  onOpenFolder,
  onRemoveCollection,
  onLibraryChanged,
  openConfirm,
}: Props) {
  const { t } = useI18n();
  const [page, setPage] = useState<CollectionListPageDto | null>(null);
  const [pageIndex, setPageIndex] = useState(0);
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [bulkErrors, setBulkErrors] = useState<string[]>([]);
  const [selectMode, setSelectMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<number>>(() => new Set());
  const [kindBusy, setKindBusy] = useState(false);
  const [selectAllBusy, setSelectAllBusy] = useState(false);
  const [kindNotice, setKindNotice] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<CollectionListPageDto>("list_collections", {
        kind,
        filter: filter === "all" ? null : filter,
        search: search.trim() || null,
        limit: PAGE_SIZE,
        offset: pageIndex * PAGE_SIZE,
      });
      setPage(result);
      const maxPage = Math.max(0, Math.ceil(result.total / PAGE_SIZE) - 1);
      if (pageIndex > maxPage) {
        setPageIndex(maxPage);
      }
    } catch (e) {
      setPage(null);
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [kind, filter, search, pageIndex]);

  useEffect(() => {
    setPageIndex(0);
    setSelectMode(false);
    setSelectedIds(new Set());
    setBulkErrors([]);
  }, [kind, refreshKey]);

  useEffect(() => {
    const tmr = window.setTimeout(() => void load(), search ? 200 : 0);
    return () => window.clearTimeout(tmr);
  }, [load]);

  const filters: { id: Filter; label: string }[] = [
    { id: "all", label: t("catalog.filterAll") },
    { id: "in-progress", label: t("catalog.filterProgress") },
    { id: "finished", label: t("catalog.filterFinished") },
    { id: "away", label: t("catalog.filterAway") },
  ];

  const title = kind === "audiobook" ? t("nav.audiobooks") : t("nav.music");
  const lead = kind === "audiobook" ? t("catalog.audiobooksLead") : t("catalog.musicLead");
  const items = page?.items ?? [];
  const total = page?.total ?? 0;
  const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE));
  const currentPage = Math.min(pageIndex + 1, totalPages);
  const rangeStart = total === 0 ? 0 : pageIndex * PAGE_SIZE + 1;
  const rangeEnd = Math.min(total, (pageIndex + 1) * PAGE_SIZE);
  const showPagination = total > PAGE_SIZE;
  const hasActiveQuery = search.trim() !== "" || filter !== "all";
  const showBulkPlayback = kind === "music" && total > 0 && !loading && !error && !selectMode;
  const showSelectToggle = total > 0 && !loading;
  const selectedCount = selectedIds.size;
  const pageIds = items.map((item) => item.id);
  const allPageSelected = pageIds.length > 0 && pageIds.every((id) => selectedIds.has(id));
  const oppositeKind = kind === "music" ? "audiobook" : "music";

  const toggleSelected = (id: number) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const selectPage = () => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      for (const id of pageIds) {
        next.add(id);
      }
      return next;
    });
  };

  const selectAllMatching = async () => {
    setSelectAllBusy(true);
    setError(null);
    try {
      const ids = await invoke<number[]>("list_collection_ids", {
        kind,
        filter: filter === "all" ? null : filter,
        search: search.trim() || null,
      });
      setSelectedIds(new Set(ids));
    } catch (e) {
      setError(String(e));
    } finally {
      setSelectAllBusy(false);
    }
  };

  const runApplyKind = async (targetKind: "audiobook" | "music") => {
    if (selectedCount === 0) return;
    setKindBusy(true);
    setError(null);
    setBulkErrors([]);
    setKindNotice(null);
    try {
      const result = await invoke<SetCollectionsKindResult>("set_collections_kind", {
        collectionIds: Array.from(selectedIds),
        kind: targetKind,
      });
      if (result.failures.length === 0) {
        setKindNotice(t("catalog.bulkKindDone", { count: result.updated }));
      } else {
        setKindNotice(
          t("catalog.bulkKindPartial", {
            updated: result.updated,
            failed: result.failures.length,
          }),
        );
        setBulkErrors(result.failures.map((failure) => `${failure.title}: ${failure.error}`));
      }
      setSelectMode(false);
      setSelectedIds(new Set());
      onLibraryChanged?.();
      await load();
      window.setTimeout(() => setKindNotice(null), 5000);
    } catch (e) {
      setError(String(e));
    } finally {
      setKindBusy(false);
    }
  };

  const applyKind = (targetKind: "audiobook" | "music") => {
    if (selectedCount === 0) return;
    const kindLabel =
      targetKind === "music" ? t("catalog.kindMusic") : t("catalog.kindAudiobook");
    const run = () => runApplyKind(targetKind);
    if (openConfirm) {
      openConfirm({
        title: t("catalog.bulkKindConfirmTitle", { count: selectedCount }),
        body: t("catalog.bulkKindConfirmBody", { count: selectedCount, kind: kindLabel }),
        confirmLabel: t("catalog.bulkKindConfirmBtn"),
        onConfirm: run,
      });
    } else {
      void run();
    }
  };

  const exitSelectMode = () => {
    setSelectMode(false);
    setSelectedIds(new Set());
    setBulkErrors([]);
  };

  return (
    <div className="view-panel catalog-view">
      <header className="catalog-head">
        <div className="catalog-head-text">
          <h2 className="view-title">{title}</h2>
          <p className="view-lead catalog-lead">{lead}</p>
        </div>

        {showBulkPlayback && onPlayAll ? (
          <section className="catalog-section" aria-labelledby="catalog-playback-heading">
            <h3 id="catalog-playback-heading" className="catalog-section-title">
              {t("catalog.playbackSectionMusic")}
            </h3>
            <div className="catalog-bulk-actions" role="group" aria-label={t("catalog.playbackSectionMusic")}>
              <button
                type="button"
                className="btn btn-primary btn-compact catalog-bulk-play"
                onClick={() => onPlayAll({ filter, search, shuffle: false })}
              >
                <IconPlaySm />
                <span>{t("catalog.playAll")}</span>
              </button>
              <button
                type="button"
                className="btn btn-secondary btn-compact"
                onClick={() => onPlayAll({ filter, search, shuffle: true })}
              >
                {t("catalog.shuffleAll")}
              </button>
              {onAddAllToQueue ? (
                <button
                  type="button"
                  className="btn btn-secondary btn-compact"
                  onClick={() => onAddAllToQueue({ filter, search, position: "end" })}
                >
                  {t("catalog.queueAll")}
                </button>
              ) : null}
            </div>
          </section>
        ) : null}

        <section className="catalog-section catalog-section--toolbar" aria-label={t("catalog.searchLabel")}>
          <div className="catalog-toolbar">
            <div className="catalog-toolbar-search">
              <label className="catalog-toolbar-search-label" htmlFor="catalog-search-input">
                <span className="field-label">{t("catalog.searchLabel")}</span>
              </label>
              <input
                id="catalog-search-input"
                type="search"
                className="catalog-search catalog-toolbar-search-input"
                placeholder={t("catalog.searchPlaceholder")}
                value={search}
                onChange={(e) => {
                  setSearch(e.target.value);
                  setPageIndex(0);
                }}
                aria-label={t("catalog.searchPlaceholder")}
              />
            </div>

            <div className="catalog-toolbar-filters" role="group" aria-label={t("catalog.sectionFilters")}>
              <span className="field-label">{t("catalog.sectionFilters")}</span>
              <div className="catalog-filters-buttons">
                {filters.map((f) => (
                  <button
                    key={f.id}
                    type="button"
                    className={`btn btn-ghost btn-compact${filter === f.id ? " btn-compact--active" : ""}`}
                    aria-pressed={filter === f.id}
                    onClick={() => {
                      setFilter(f.id);
                      setPageIndex(0);
                    }}
                  >
                    {f.label}
                  </button>
                ))}
              </div>
            </div>

            {showSelectToggle ? (
              <div className="catalog-toolbar-actions">
                <button
                  type="button"
                  className={`btn btn-ghost btn-compact${selectMode ? " btn-compact--active" : ""}`}
                  aria-pressed={selectMode}
                  onClick={() => {
                    if (selectMode) {
                      exitSelectMode();
                    } else {
                      setSelectMode(true);
                    }
                  }}
                >
                  {selectMode ? t("catalog.selectModeDone") : t("catalog.selectMode")}
                </button>
              </div>
            ) : null}
          </div>
        </section>
      </header>

      {error ? (
        <p className="view-error catalog-error" role="alert">
          {error}
        </p>
      ) : null}

      {bulkErrors.length > 0 ? (
        <div className="catalog-error-list" role="alert">
          <p className="catalog-error-list-title">{t("catalog.bulkKindFailuresTitle")}</p>
          <ul>
            {bulkErrors.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </div>
      ) : null}

      {kindNotice ? (
        <p className="catalog-kind-notice" aria-live="polite">
          {kindNotice}
        </p>
      ) : null}

      {selectMode && total > 0 ? (
        <div
          className="catalog-select-bar"
          role="toolbar"
          aria-label={t("catalog.kindLabel")}
          aria-busy={kindBusy || selectAllBusy}
        >
          <div className="catalog-select-summary">
            <span className="catalog-select-count">
              {t("catalog.selectedCount", { count: selectedCount })}
            </span>
            <p className="catalog-select-hint">{t("catalog.selectBarHint")}</p>
          </div>
          <div className="catalog-select-actions">
            <button
              type="button"
              className="btn btn-ghost btn-compact"
              disabled={allPageSelected || selectAllBusy || kindBusy}
              onClick={() => selectPage()}
            >
              {t("catalog.selectPage")}
            </button>
            {total > pageIds.length ? (
              <button
                type="button"
                className="btn btn-ghost btn-compact"
                disabled={selectAllBusy || kindBusy}
                aria-busy={selectAllBusy}
                onClick={() => void selectAllMatching()}
              >
                {selectAllBusy ? t("catalog.busySelectAll") : t("catalog.selectAllMatching", { count: total })}
              </button>
            ) : null}
            <button
              type="button"
              className="btn btn-primary btn-compact"
              disabled={selectedCount === 0 || kindBusy || selectAllBusy}
              aria-busy={kindBusy}
              onClick={() => applyKind(oppositeKind)}
            >
              {kindBusy
                ? t("catalog.busyBulkKind")
                : oppositeKind === "music"
                  ? t("catalog.setKindMusic")
                  : t("catalog.setKindAudiobook")}
            </button>
          </div>
        </div>
      ) : null}

      {loading ? (
        <p className="view-loading" aria-live="polite">
          {t("home.loading")}
        </p>
      ) : items.length === 0 && !error ? (
        <div className="view-empty view-empty--actions">
          {hasActiveQuery ? (
            <>
              <p className="view-empty-title">{t("catalog.noMatchesTitle")}</p>
              <p className="view-empty-body">{t("catalog.noMatchesBody")}</p>
            </>
          ) : (
            <>
              <p className="view-empty-title">{t("catalog.emptyTitle")}</p>
              <p className="view-empty-body">{t("catalog.emptyBody")}</p>
              <div className="view-empty-actions">
                {onLinkFolder ? (
                  <button type="button" className="btn btn-primary" onClick={onLinkFolder}>
                    {t("library.linkFolder")}
                  </button>
                ) : null}
                {onOpenFolder ? (
                  <button type="button" className="btn btn-ghost" onClick={onOpenFolder}>
                    {t("sidebar.openFolder")}
                  </button>
                ) : null}
              </div>
              <p className="view-empty-hint">{t("catalog.emptyQuick")}</p>
            </>
          )}
        </div>
      ) : !loading && items.length > 0 ? (
        <section className="catalog-results" aria-label={title}>
          <p className="catalog-results-meta" aria-live="polite">
            {showPagination
              ? t("catalog.resultsRange", {
                  start: rangeStart,
                  end: rangeEnd,
                  total,
                })
              : t("catalog.resultsCount", { count: total })}
          </p>
          <div className="media-list media-list--catalog">
            {items.map((item) => (
              <MediaRow
                key={item.id}
                item={item}
                onPlay={onPlayCollection}
                onAddToQueue={onAddToQueue}
                onOpen={onOpenDetail}
                onChanged={() => void load()}
                onRemoveCollection={onRemoveCollection}
                selectMode={selectMode}
                selected={selectedIds.has(item.id)}
                onSelectToggle={toggleSelected}
              />
            ))}
          </div>
          {showPagination ? (
            <nav className="catalog-pagination" aria-label={t("catalog.paginationLabel")}>
              <button
                type="button"
                className="btn btn-secondary btn-compact"
                disabled={pageIndex === 0}
                aria-label={t("catalog.pagePrev")}
                onClick={() => setPageIndex((p) => Math.max(0, p - 1))}
              >
                {t("catalog.pagePrev")}
              </button>
              <span className="catalog-pagination-status">
                {t("catalog.pageStatus", { current: currentPage, total: totalPages })}
              </span>
              <button
                type="button"
                className="btn btn-secondary btn-compact"
                disabled={pageIndex >= totalPages - 1}
                aria-label={t("catalog.pageNext")}
                onClick={() => setPageIndex((p) => p + 1)}
              >
                {t("catalog.pageNext")}
              </button>
            </nav>
          ) : null}
        </section>
      ) : null}
    </div>
  );
}
