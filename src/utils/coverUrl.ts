import { convertFileSrc, isTauri } from "@tauri-apps/api/core";

/** Turn a local cover path from the catalog into a URL the webview can load. */
export function coverUrl(path: string | null | undefined): string | null {
  if (!path?.trim() || !isTauri()) return null;
  try {
    return convertFileSrc(path.trim());
  } catch {
    return null;
  }
}
