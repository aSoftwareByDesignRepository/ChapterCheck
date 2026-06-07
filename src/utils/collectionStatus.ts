import type { CollectionSummaryDto } from "../types/catalog";

export type CollectionStatusKind = "ok" | "drive_away" | "tracks_missing" | "empty";

export function collectionStatusKind(item: CollectionSummaryDto): CollectionStatusKind {
  if (item.root_unavailable) return "drive_away";
  if (item.playable_file_count > 0) return "ok";
  if (item.track_count === 0) return "empty";
  return "tracks_missing";
}

export function collectionNeedsAttention(item: CollectionSummaryDto): boolean {
  return collectionStatusKind(item) !== "ok";
}
