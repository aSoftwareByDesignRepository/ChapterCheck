import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { MediaRow } from "../components/MediaRow";
import { useI18n } from "../i18n/I18nContext";
import type { CollectionSummaryDto } from "../types/catalog";

type Props = {
  kind: "audiobook" | "music";
  refreshKey?: number;
  onPlayCollection: (id: number, mode: "continue" | "start") => void;
  onOpenDetail: (id: number) => void;
  onAddToQueue?: (id: number, position: "next" | "end") => void;
  onLinkFolder?: () => void;
  onOpenFolder?: () => void;
  onRemoveCollection?: (id: number, title: string) => void;
};

type Filter = "all" | "in-progress" | "finished" | "away";

export function CatalogView({
  kind,
  refreshKey = 0,
  onPlayCollection,
  onOpenDetail,
  onAddToQueue,
  onLinkFolder,
  onOpenFolder,
  onRemoveCollection,
}: Props) {
  const { t } = useI18n();
  const [items, setItems] = useState<CollectionSummaryDto[]>([]);
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const list = await invoke<CollectionSummaryDto[]>("list_collections", {
        kind,
        filter: filter === "all" ? null : filter,
        search: search.trim() || null,
        limit: 500,
        offset: 0,
      });
      setItems(list);
    } finally {
      setLoading(false);
    }
  }, [kind, filter, search]);

  useEffect(() => {
    const tmr = window.setTimeout(() => void load(), search ? 200 : 0);
    return () => window.clearTimeout(tmr);
  }, [load, search, refreshKey]);

  const filters: { id: Filter; label: string }[] = [
    { id: "all", label: t("catalog.filterAll") },
    { id: "in-progress", label: t("catalog.filterProgress") },
    { id: "finished", label: t("catalog.filterFinished") },
    { id: "away", label: t("catalog.filterAway") },
  ];

  return (
    <div className="view-panel catalog-view">
      <header className="catalog-head">
        <h2 className="view-title">{kind === "audiobook" ? t("nav.audiobooks") : t("nav.music")}</h2>
        <div className="catalog-toolbar">
          <input
            type="search"
            className="catalog-search"
            placeholder={t("catalog.searchPlaceholder")}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            aria-label={t("catalog.searchPlaceholder")}
          />
          <div className="catalog-filters" role="group" aria-label={t("catalog.filters")}>
            {filters.map((f) => (
              <button
                key={f.id}
                type="button"
                className={`btn btn-ghost btn-compact${filter === f.id ? " btn-compact--active" : ""}`}
                aria-pressed={filter === f.id}
                onClick={() => setFilter(f.id)}
              >
                {f.label}
              </button>
            ))}
          </div>
        </div>
      </header>

      {loading ? (
        <p className="view-loading" aria-live="polite">
          {t("home.loading")}
        </p>
      ) : items.length === 0 ? (
        <div className="view-empty view-empty--actions">
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
        </div>
      ) : (
        <div className="media-list media-list--catalog">
          {items.map((item) => (
            <MediaRow
              key={item.id}
              item={item}
              onPlay={(id, mode) => onPlayCollection(id, mode)}
              onAddToQueue={onAddToQueue}
              onOpen={onOpenDetail}
              onChanged={() => void load()}
              onRemoveCollection={onRemoveCollection}
            />
          ))}
        </div>
      )}
    </div>
  );
}
