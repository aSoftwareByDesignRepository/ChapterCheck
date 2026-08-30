import { CoverImage } from "./CoverImage";
import { useI18n } from "../i18n/I18nContext";
import type { CollectionSummaryDto } from "../types/catalog";
import { collectionStatusKind } from "../utils/collectionStatus";
import { coverUrl } from "../utils/coverUrl";

type Props = {
  item: CollectionSummaryDto;
  onPlay: (id: number, mode: "continue" | "start") => void;
  onOpen: (id: number) => void;
};

export function MediaCard({ item, onPlay, onOpen }: Props) {
  const { t } = useI18n();
  const status = collectionStatusKind(item);
  const needsAttention = status !== "ok";
  const primaryLabel = item.in_progress && !item.listened ? t("home.continue") : t("home.play");
  const src = coverUrl(item.cover_path);

  const statusLabel = () => {
    if (status === "drive_away") return t("catalog.away");
    if (status === "tracks_missing") return t("catalog.tracksUnavailable");
    if (status === "empty") return t("catalog.emptyCollectionTitle");
    return null;
  };

  const cardClass = [
    "media-card",
    status === "drive_away" ? "media-card--away" : "",
    status === "tracks_missing" || status === "empty" ? "media-card--missing" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <article
      className={cardClass}
      aria-label={`${item.title}${item.subtitle ? `, ${item.subtitle}` : ""}${
        item.in_progress && !item.listened ? `, ${Math.round(item.progress_pct)}%` : ""
      }`}
    >
      <button type="button" className="media-card-hit" onClick={() => onOpen(item.id)}>
        <div className="media-card-cover" aria-hidden="true">
          <CoverImage src={src} kind={item.kind} className="media-card-cover-img" />
        </div>
        <div className="media-card-body">
          <h3 className="media-card-title">{item.title}</h3>
          {item.subtitle ? <p className="media-card-sub">{item.subtitle}</p> : null}
          {needsAttention ? (
            <p className="media-card-away">{statusLabel()}</p>
          ) : (
            <div className="media-card-progress" aria-hidden="true">
              <div
                className="media-card-progress-fill"
                style={{ width: `${Math.round(item.progress_pct)}%` }}
              />
            </div>
          )}
          <p className="media-card-meta">
            {item.location_hint === "external" ? t("catalog.onExternal") : t("catalog.onThisPc")}
            {item.listened ? ` · ${t("catalog.finished")}` : null}
          </p>
        </div>
      </button>
      {!needsAttention ? (
        <button
          type="button"
          className="btn btn-primary media-card-action"
          onClick={() => onPlay(item.id, item.in_progress ? "continue" : "start")}
        >
          {primaryLabel}
        </button>
      ) : null}
    </article>
  );
}
