/** Catalog filter values the UI may send. Anything else is treated as "all". */
export const CATALOG_FILTERS = ["all", "in-progress", "finished", "away"] as const;
export type CatalogFilter = (typeof CATALOG_FILTERS)[number];

export function parseCatalogFilter(raw: string): CatalogFilter {
  for (const allowed of CATALOG_FILTERS) {
    if (raw === allowed) return allowed;
  }
  return "all";
}

export function shouldApplyAsyncResult(requestId: number, latestId: number): boolean {
  return requestId === latestId;
}

/** True when a sleep-preset IPC actually armed a future deadline. */
export function sleepPresetSucceeded(deadlineMs: number | null | undefined): boolean {
  return Number.isFinite(deadlineMs) && Number(deadlineMs) > 0;
}

type ContinueLike = { unavailable: boolean } | null | undefined;

/** Home has something to tap besides the empty-library CTA. */
export function homeHasVisibleContent(args: {
  continueItem: ContinueLike;
  inProgressCount: number;
  musicCount: number;
}): boolean {
  const showContinue = !!args.continueItem && !args.continueItem.unavailable;
  return showContinue || args.inProgressCount > 0 || args.musicCount > 0;
}

export const HOST_USER_CANCELLED = "CANCELLED_BY_USER";

export type HostErrorKind =
  | "cancelled"
  | "too-large"
  | "scan-busy"
  | "need-pick"
  | "need-os-confirm"
  | "generic";

/** Map Rust host errors to a granny-readable kind. Never show raw IPC dumps. */
export function classifyHostError(raw: unknown): HostErrorKind {
  const s = String(raw ?? "").toLowerCase();
  if (!s.trim() || s.includes(HOST_USER_CANCELLED.toLowerCase())) return "cancelled";
  if (s.includes("too many")) return "too-large";
  if (s.includes("already running")) return "scan-busy";
  if (s.includes("add my folder") || s.includes("chose it")) return "need-pick";
  if (s.includes("must ask you")) return "need-os-confirm";
  return "generic";
}
