import { convertFileSrc, isTauri } from "@tauri-apps/api/core";

/** Reject empty, NUL, and `..` path segments so asset URLs cannot walk out of the cover cache. */
export function isSafeCoverPath(path: string | null | undefined): path is string {
  if (!path?.trim()) return false;
  const trimmed = path.trim();
  if (trimmed.includes("\0")) return false;
  const parts = trimmed.split(/[/\\]+/).filter(Boolean);
  if (parts.some((p) => p === "..")) return false;
  return parts.includes("covers");
}

/** Turn a local cover path from the catalog into a URL the webview can load. */
export function coverUrl(path: string | null | undefined): string | null {
  if (!isSafeCoverPath(path) || !isTauri()) return null;
  try {
    return convertFileSrc(path.trim());
  } catch {
    return null;
  }
}
