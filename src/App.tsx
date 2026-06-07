import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AppNav } from "./components/AppNav";
import { LibrarySidebar } from "./components/LibrarySidebar";
import { MiniPlayerBar } from "./components/MiniPlayerBar";
import { useI18n } from "./i18n/I18nContext";
import type { Locale } from "./i18n/types";
import { normalizeLocale } from "./i18n/types";
import type {
  AppView,
  CollectionDetailDto,
  CollectionSummaryDto,
  LibraryRootDto,
} from "./types/catalog";
import { AddToPlaylistButton } from "./components/AddToPlaylistButton";
import { useAddToPlaylist } from "./context/AddToPlaylistContext";
import { useContextMenu, type ContextMenuEntry } from "./context/ContextMenuContext";
import { CollectionDetailSheet } from "./components/CollectionDetailSheet";
import { CatalogView } from "./views/CatalogView";
import { coverUrl } from "./utils/coverUrl";
import { HomeView } from "./views/HomeView";
import { NowPlayingView } from "./views/NowPlayingView";
import { LibraryView } from "./views/LibraryView";
import { PlaylistsView } from "./views/PlaylistsView";
import { missingFileContextEntries } from "./utils/missingFileMenu";

export type SortKey =
  | "name-asc"
  | "name-desc"
  | "modified-desc"
  | "modified-asc"
  | "size-desc"
  | "size-asc"
  | "random";

type PlaylistItemDto = {
  path: string;
  label: string;
  duration_sec: number | null;
  artist?: string | null;
  album?: string | null;
  listened: boolean;
  collection_file_id?: number | null;
  library_missing?: boolean;
};

type PlaylistDto = {
  root: string;
  items: PlaylistItemDto[];
  sort: SortKey;
  shuffled?: boolean;
};

type TransportDto = {
  position_sec: number;
  duration_sec: number | null;
  paused: boolean;
  speed: number;
  eof: boolean;
  idle: boolean;
  current_index: number | null;
  current_path: string | null;
  playlist_len: number;
  session_root: string | null;
  mpv_error: string | null;
  repeat_mode: string;
  playback_kind?: string | null;
  active_collection_id?: number | null;
  active_collection_kind?: string | null;
};

type RecentOpenDto = {
  path: string;
  kind: string;
  label: string;
};

type AppPrefsDto = {
  resume_playing_on_launch: boolean;
  scan_subfolders: boolean;
  online_metadata_enabled: boolean;
  ui_locale: string;
  default_speed_audiobook: number;
  default_speed_music: number;
};

type SetScanFoldersResult = {
  prefs: AppPrefsDto;
  playlist: PlaylistDto | null;
};

type ChapterDto = {
  index: number;
  title: string;
  time_sec: number;
};

const SORT_KEY_SET: ReadonlySet<SortKey> = new Set([
  "name-asc",
  "name-desc",
  "modified-desc",
  "modified-asc",
  "size-desc",
  "size-asc",
  "random",
]);

function isPlaylistDto(v: unknown): v is PlaylistDto {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  if (typeof o.root !== "string") return false;
  if (typeof o.sort !== "string" || !SORT_KEY_SET.has(o.sort as SortKey)) return false;
  if (!Array.isArray(o.items)) return false;
  if ("shuffled" in o && typeof (o as { shuffled?: unknown }).shuffled !== "boolean") return false;
  for (const raw of o.items) {
    if (!raw || typeof raw !== "object") return false;
    const it = raw as Record<string, unknown>;
    if (typeof it.path !== "string" || typeof it.label !== "string") return false;
    const d = it.duration_sec;
    if (d != null && (typeof d !== "number" || !Number.isFinite(d))) return false;
    if ("artist" in it && it.artist != null && typeof it.artist !== "string") return false;
    if ("album" in it && it.album != null && typeof it.album !== "string") return false;
    if (typeof it.listened !== "boolean") return false;
    if (
      "collection_file_id" in it &&
      it.collection_file_id != null &&
      typeof it.collection_file_id !== "number"
    ) {
      return false;
    }
    if ("library_missing" in it && it.library_missing != null && typeof it.library_missing !== "boolean") {
      return false;
    }
  }
  return true;
}

type UiAction =
  | "view.player"
  | "view.queue"
  | "help.shortcuts"
  | "help.about"
  | "app.preferences"
  | "playback.sleep_timer"
  | "library.link_folder";

type UserSessionOpenPayload = {
  playlist: PlaylistDto;
  suggest_library_link: boolean;
};

function isUiActionPayload(v: unknown): v is { action: UiAction } {
  if (!v || typeof v !== "object") return false;
  const a = (v as { action?: unknown }).action;
  return (
    a === "view.player" ||
    a === "view.queue" ||
    a === "help.shortcuts" ||
    a === "help.about" ||
    a === "app.preferences" ||
    a === "playback.sleep_timer" ||
    a === "library.link_folder"
  );
}

function isUserSessionOpenPayload(v: unknown): v is UserSessionOpenPayload {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return isPlaylistDto(o.playlist) && typeof o.suggest_library_link === "boolean";
}

function pathUnderAvailableRoot(filePath: string, roots: LibraryRootDto[]): boolean {
  for (const root of roots) {
    if (!root.is_available) continue;
    if (filePath === root.path) return true;
    const prefix = root.path.endsWith("/") ? root.path : `${root.path}/`;
    if (filePath.startsWith(prefix)) return true;
  }
  return false;
}

function pathsUnderRoot(items: PlaylistItemDto[], root: string): boolean {
  if (items.length === 0) return false;
  return items.every((it) => {
    if (it.path === root) return true;
    const prefix = root.endsWith("/") ? root : `${root}/`;
    return it.path.startsWith(prefix);
  });
}

type SessionDeleteCopy = {
  buttonLabel: string;
  confirmTitle: string;
  confirmBody: string;
  confirmLabel: string;
};

function buildSessionDeleteCopy(
  playlist: PlaylistDto,
  playbackKind: string | null | undefined,
  t: (key: string, vars?: Record<string, string | number>) => string,
): SessionDeleteCopy {
  const items = playlist.items;
  const count = items.length;
  const isOne = count === 1;
  const isFolderSession =
    !isOne && playbackKind === "session" && pathsUnderRoot(items, playlist.root);
  const label = isOne
    ? items[0]!.label
    : isFolderSession
      ? playlist.root
      : t("confirm.deleteSessionQueueLabel", { count });
  const confirmLabel = isOne ? t("confirm.deleteTrackBtn") : t("confirm.deleteSessionBtn");

  if (isOne) {
    return {
      buttonLabel: t("queue.deleteSessionOne"),
      confirmTitle: t("confirm.deleteSessionTitleOne"),
      confirmBody: t("confirm.deleteSessionOne", { label }),
      confirmLabel,
    };
  }

  return {
    buttonLabel: t("queue.deleteSessionMany", { count }),
    confirmTitle: t("confirm.deleteSessionTitleMany", { count }),
    confirmBody: isFolderSession
      ? t("confirm.deleteSessionManyFolder", { count, label })
      : t("confirm.deleteSessionMany", { count, label }),
    confirmLabel,
  };
}

function formatClock(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
  const s = Math.floor(seconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  if (h > 0) {
    return `${h}:${String(m).padStart(2, "0")}:${String(r).padStart(2, "0")}`;
  }
  return `${m}:${String(r).padStart(2, "0")}`;
}

function fileBasename(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] || path;
}

function normalizeQueueSearch(raw: string): string {
  return raw.trim().toLowerCase();
}

/** Each whitespace-separated token must appear in the title or full path (case-insensitive). */
function itemMatchesQueueSearch(queryNorm: string, it: PlaylistItemDto): boolean {
  if (!queryNorm) return true;
  const tokens = queryNorm.split(/\s+/).filter(Boolean);
  const hay = [it.label, it.path, it.artist ?? "", it.album ?? ""].join("\n").toLowerCase();
  return tokens.every((t) => hay.includes(t));
}

function formatQueueItemMeta(artist: string | null | undefined, album: string | null | undefined): string | null {
  const a = artist?.trim();
  const b = album?.trim();
  if (a && b) return `${a} — ${b}`;
  if (a) return a;
  if (b) return b;
  return null;
}

function IconPlay() {
  return (
    <svg className="ctrl-icon" viewBox="0 0 24 24" aria-hidden="true">
      <path fill="currentColor" d="M8 5.25v13.5L18.75 12 8 5.25Z" />
    </svg>
  );
}

function IconPause() {
  return (
    <svg className="ctrl-icon" viewBox="0 0 24 24" aria-hidden="true">
      <path fill="currentColor" d="M6 4.5h4.5v15H6v-15Zm7.5 0H18v15h-4.5v-15Z" />
    </svg>
  );
}

function IconSkipPrev() {
  return (
    <svg className="ctrl-icon" viewBox="0 0 24 24" aria-hidden="true">
      <path fill="currentColor" d="M4 6h2v12H4V6zm12-1L8 12l8 7V5z" />
    </svg>
  );
}

function IconSkipNext() {
  return (
    <svg className="ctrl-icon" viewBox="0 0 24 24" aria-hidden="true">
      <path fill="currentColor" d="M8 5l8 7-8 7V5zm9 1h2v12h-2V6z" />
    </svg>
  );
}

function IconCheck() {
  return (
    <svg className="qa-icon" viewBox="0 0 24 24" aria-hidden="true">
      <path
        fill="none"
        stroke="currentColor"
        strokeWidth="2.4"
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M5 12.5l4.5 4.5L19 7"
      />
    </svg>
  );
}

function IconTrash() {
  return (
    <svg className="qa-icon" viewBox="0 0 24 24" aria-hidden="true">
      <path
        fill="currentColor"
        d="M9 3h6a1 1 0 011 1v1h4v2H4V5h4V4a1 1 0 011-1zm1 5h2v10h-2V8zm4 0h2v10h-2V8zM6 8h2v10H6V8zm-1 12a2 2 0 002 2h10a2 2 0 002-2V8H5v12z"
      />
    </svg>
  );
}

export default function App() {
  const [playlist, setPlaylist] = useState<PlaylistDto | null>(null);
  const [transport, setTransport] = useState<TransportDto | null>(null);
  const [osMediaActive, setOsMediaActive] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const eofPrev = useRef(false);
  const lastAutoSave = useRef(0);
  const [seekUi, setSeekUi] = useState<number | null>(null);
  const menubarRef = useRef<HTMLDivElement>(null);
  const mainStageRef = useRef<HTMLElement>(null);
  const playlistPanelRef = useRef<HTMLElement>(null);
  const modalSheetRef = useRef<HTMLDivElement>(null);
  const modalCloseRef = useRef<HTMLButtonElement>(null);
  const [menuOpen, setMenuOpen] = useState<null | "file" | "playback" | "view" | "help">(null);
  const [modalSheet, setModalSheet] = useState<
    null | "shortcuts" | "about" | "preferences" | "sleep" | "confirm" | "addLibrary"
  >(null);
  const [activeView, setActiveView] = useState<AppView>("home");
  const [libraryRoots, setLibraryRoots] = useState<LibraryRootDto[]>([]);
  const [libraryRefreshKey, setLibraryRefreshKey] = useState(0);
  const [nowCoverSrc, setNowCoverSrc] = useState<string | null>(null);
  const [playingCollectionKind, setPlayingCollectionKind] = useState<string | null>(null);
  const [libraryPromptPath, setLibraryPromptPath] = useState<string | null>(null);
  const [driveNotice, setDriveNotice] = useState<string | null>(null);
  const [queueNotice, setQueueNotice] = useState<string | null>(null);
  const [collectionDetailId, setCollectionDetailId] = useState<number | null>(null);
  const [resolvedTrackCollectionId, setResolvedTrackCollectionId] = useState<number | null>(
    null,
  );
  const [finishBookPrompt, setFinishBookPrompt] = useState(false);
  const libraryRootsRef = useRef<LibraryRootDto[]>([]);
  const [confirmDialog, setConfirmDialog] = useState<{
    title: string;
    body: string;
    confirmLabel: string;
    danger?: boolean;
    onConfirm: () => void | Promise<void>;
  } | null>(null);
  const openConfirm = useCallback(
    (cfg: {
      title: string;
      body: string;
      confirmLabel: string;
      danger?: boolean;
      onConfirm: () => void | Promise<void>;
    }) => {
      setConfirmDialog(cfg);
      setModalSheet("confirm");
    },
    [],
  );
  const closeConfirm = useCallback(() => {
    setConfirmDialog(null);
    setModalSheet((m) => (m === "confirm" ? null : m));
  }, []);

  useEffect(() => {
    if (modalSheet !== "confirm" && confirmDialog) setConfirmDialog(null);
  }, [modalSheet, confirmDialog]);
  const [recent, setRecent] = useState<RecentOpenDto[]>([]);
  const [queueSearch, setQueueSearch] = useState("");
  const [appPrefs, setAppPrefs] = useState<AppPrefsDto | null>(null);
  const [chapters, setChapters] = useState<ChapterDto[]>([]);
  const [sleepDeadlineMs, setSleepDeadlineMs] = useState<number | null>(null);
  const [sleepPreset, setSleepPreset] = useState<string>("off");
  const [sleepTick, setSleepTick] = useState(0);
  const [stopAfterTrackUi, setStopAfterTrackUi] = useState(false);
  const stopAfterTrackRef = useRef(false);

  useEffect(() => {
    stopAfterTrackRef.current = stopAfterTrackUi;
  }, [stopAfterTrackUi]);

  const { t, locale, setLocale } = useI18n();
  const { openContextMenu } = useContextMenu();
  const { appendPlaylistContextEntries } = useAddToPlaylist();

  useEffect(() => {
    if (!appPrefs?.ui_locale) return;
    const next = normalizeLocale(appPrefs.ui_locale);
    if (next !== locale) setLocale(next);
  }, [appPrefs?.ui_locale, locale, setLocale]);

  useEffect(() => {
    setQueueSearch("");
  }, [playlist?.root]);

  useEffect(() => {
    if (!menuOpen) return;
    const close = (e: MouseEvent) => {
      if (menubarRef.current && !menubarRef.current.contains(e.target as Node)) {
        setMenuOpen(null);
      }
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menuOpen]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setMenuOpen(null);
        setModalSheet(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    eofPrev.current = false;
  }, [transport?.current_path]);

  const refreshTransport = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const t = await invoke<TransportDto>("get_transport");
      setTransport(t);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const syncPlaylistFromBackend = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const dto = await invoke<PlaylistDto | null>("get_current_playlist");
      if (isPlaylistDto(dto)) {
        setPlaylist(dto);
      }
    } catch {
      /* ignore transient IPC errors */
    }
  }, []);

  const refreshOsMedia = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const status = await invoke<{ available: boolean }>("get_os_media_status");
      setOsMediaActive(status.available);
    } catch {
      setOsMediaActive(false);
    }
  }, []);

  const loadRecent = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const r = await invoke<RecentOpenDto[]>("get_recent_opened");
      setRecent(r);
    } catch {
      setRecent([]);
    }
  }, []);

  const loadAppPrefs = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const p = await invoke<AppPrefsDto>("get_app_prefs");
      setAppPrefs(p);
    } catch {
      setAppPrefs({
        resume_playing_on_launch: false,
        scan_subfolders: false,
        online_metadata_enabled: false,
        ui_locale: "en",
      });
    }
  }, []);

  const loadLibraryRoots = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const roots = await invoke<LibraryRootDto[]>("list_library_roots");
      setLibraryRoots(roots);
      libraryRootsRef.current = roots;
    } catch {
      setLibraryRoots([]);
      libraryRootsRef.current = [];
    }
  }, []);

  const openManageLibrary = useCallback(() => {
    void loadLibraryRoots();
    setActiveView("library");
  }, [loadLibraryRoots]);

  const openPreferences = useCallback(() => {
    void loadAppPrefs();
    setModalSheet("preferences");
  }, [loadAppPrefs]);

  const linkLibraryFolder = async (knownPath?: string | null) => {
    if (!isTauri()) return;
    setBusy(t("library.busy.adding"));
    setError(null);
    try {
      const path = knownPath ?? (await invoke<string | null>("pick_library_folder"));
      if (!path) return;
      await invoke("add_library_root", {
        input: {
          path,
          label: null,
          content_kind: "mixed",
          scan_rule: "auto-classify",
          scan_subfolders: true,
        },
      });
      await loadLibraryRoots();
      setLibraryRefreshKey((k) => k + 1);
      setLibraryPromptPath(null);
      setModalSheet(null);
      setActiveView((prev) => (prev === "library" ? prev : "home"));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const handleUserSessionOpen = useCallback(
    async (dto: PlaylistDto, suggestLibraryLink: boolean) => {
      setPlaylist(dto);
      setActiveView("nowPlaying");
      await refreshTransport();
      void loadRecent();
      if (!suggestLibraryLink) return;
      try {
        const tr = await invoke<TransportDto>("get_transport");
        if (!tr.active_collection_id) {
          const alreadyLinked = libraryRootsRef.current.some((r) => r.path === dto.root);
          if (!alreadyLinked) setLibraryPromptPath(dto.root);
        }
      } catch {
        /* transport read is best-effort for the link banner */
      }
    },
    [refreshTransport, loadRecent],
  );

  useEffect(() => {
    void loadAppPrefs();
    void loadLibraryRoots();
  }, [loadAppPrefs, loadLibraryRoots]);

  useEffect(() => {
    if (transport?.active_collection_id != null) {
      setResolvedTrackCollectionId(null);
      return;
    }
    const path = transport?.current_path;
    if (!path || !isTauri()) {
      setResolvedTrackCollectionId(null);
      return;
    }
    let cancelled = false;
    void invoke<number | null>("find_collection_id_for_path", { path })
      .then((id) => {
        if (!cancelled) setResolvedTrackCollectionId(id);
      })
      .catch(() => {
        if (!cancelled) setResolvedTrackCollectionId(null);
      });
    return () => {
      cancelled = true;
    };
  }, [transport?.active_collection_id, transport?.current_path]);

  const playingCollectionId =
    transport?.active_collection_id ?? resolvedTrackCollectionId ?? null;

  useEffect(() => {
    const id = playingCollectionId;
    if (!id || !isTauri()) {
      setNowCoverSrc(null);
      setPlayingCollectionKind(null);
      return;
    }
    let cancelled = false;
    void invoke<CollectionDetailDto>("get_collection_detail", { collectionId: id })
      .then((d) => {
        if (!cancelled) {
          setNowCoverSrc(coverUrl(d.cover_path));
          setPlayingCollectionKind(d.kind);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setNowCoverSrc(null);
          setPlayingCollectionKind(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [playingCollectionId, libraryRefreshKey]);

  const scrollToPlayer = useCallback(() => {
    setActiveView("nowPlaying");
    const el = mainStageRef.current;
    if (!el) return;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    el.scrollIntoView({ behavior: reduce ? "auto" : "smooth", block: "start" });
    window.setTimeout(() => el.focus(), reduce ? 0 : 280);
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    const refreshDrives = async () => {
      const before = libraryRootsRef.current;
      try {
        await invoke("refresh_library_roots");
        const roots = await invoke<LibraryRootDto[]>("list_library_roots");
        setLibraryRoots(roots);
        libraryRootsRef.current = roots;
        const reconnected = roots.some(
          (r) => r.is_available && before.find((b) => b.id === r.id && !b.is_available),
        );
        if (reconnected) {
          setDriveNotice(t("library.driveReconnected"));
        }
      } catch {
        /* ignore background refresh errors */
      }
    };
    const interval = window.setInterval(() => void refreshDrives(), 60_000);
    const onFocus = () => void refreshDrives();
    window.addEventListener("focus", onFocus);
    return () => {
      window.clearInterval(interval);
      window.removeEventListener("focus", onFocus);
    };
  }, [t]);

  useEffect(() => {
    setSleepDeadlineMs(null);
  }, [transport?.current_path]);

  useEffect(() => {
    if (sleepDeadlineMs == null) return;
    const id = window.setInterval(() => {
      setSleepTick((x) => x + 1);
      if (Date.now() >= sleepDeadlineMs) {
        setSleepDeadlineMs(null);
        void invoke("set_paused", { paused: true })
          .then(() => refreshTransport())
          .catch((e) => setError(String(e)));
      }
    }, 500);
    return () => window.clearInterval(id);
  }, [sleepDeadlineMs, refreshTransport]);

  useEffect(() => {
    if (!isTauri() || !transport?.current_path || transport.mpv_error) {
      setChapters([]);
      return;
    }
    let cancelled = false;
    const tid = window.setTimeout(() => {
      void (async () => {
        try {
          const ch = await invoke<ChapterDto[]>("get_chapters");
          if (!cancelled) setChapters(ch);
        } catch {
          if (!cancelled) setChapters([]);
        }
      })();
    }, 350);
    return () => {
      cancelled = true;
      window.clearTimeout(tid);
    };
  }, [transport?.current_path, transport?.mpv_error]);

  const scrollToQueue = useCallback(() => {
    const el = playlistPanelRef.current;
    if (!el) return;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    el.scrollIntoView({ behavior: reduce ? "auto" : "smooth", block: "start" });
    window.setTimeout(() => el.focus(), reduce ? 0 : 280);
  }, []);

  const applyUiAction = useCallback(
    (action: UiAction) => {
      switch (action) {
        case "view.player":
          scrollToPlayer();
          break;
        case "view.queue":
          scrollToQueue();
          break;
        case "help.shortcuts":
          setModalSheet("shortcuts");
          break;
        case "help.about":
          setModalSheet("about");
          break;
        case "app.preferences":
          openPreferences();
          break;
        case "playback.sleep_timer":
          setModalSheet("sleep");
          break;
        default:
          break;
      }
    },
    [scrollToPlayer, scrollToQueue, loadAppPrefs, loadLibraryRoots],
  );

  const transportPollMs = useMemo(() => {
    const t = transport;
    if (!t) return 1200;
    const playing =
      !t.paused && !t.eof && !t.idle && t.current_index !== null && t.playlist_len > 0;
    if (playing) return 550;
    if (!t.idle && t.current_index !== null) return 750;
    return 1300;
  }, [
    transport?.idle,
    transport?.paused,
    transport?.eof,
    transport?.current_index,
    transport?.playlist_len,
  ]);

  useEffect(() => {
    void loadRecent();
    void syncPlaylistFromBackend();
  }, [loadRecent, syncPlaylistFromBackend]);

  useEffect(() => {
    if (!isTauri() || !transport) return;
    if (transport.playlist_len > 0 && (playlist?.items.length ?? 0) === 0) {
      void syncPlaylistFromBackend();
    }
  }, [transport?.playlist_len, playlist?.items.length, syncPlaylistFromBackend]);

  useEffect(() => {
    void refreshTransport();
    const id = window.setInterval(() => {
      void refreshTransport();
    }, transportPollMs);
    return () => window.clearInterval(id);
  }, [refreshTransport, transportPollMs]);

  useEffect(() => {
    if (!isTauri()) return;
    void refreshOsMedia();
    const id = window.setInterval(() => void refreshOsMedia(), 2500);
    return () => window.clearInterval(id);
  }, [refreshOsMedia]);

  useEffect(() => {
    if (!isTauri()) return;

    let mounted = true;
    let dispose: (() => void) | undefined;

    void Promise.all([
      listen<unknown>("abp:playlist-update", (ev) => {
        if (!mounted) return;
        const p = ev.payload;
        if (isPlaylistDto(p)) {
          setPlaylist(p);
          void refreshTransport();
          void loadRecent();
        }
      }),
      listen<unknown>("abp:user-session-open", (ev) => {
        if (!mounted) return;
        const p = ev.payload;
        if (isUserSessionOpenPayload(p)) {
          void handleUserSessionOpen(p.playlist, p.suggest_library_link);
        }
      }),
      listen<unknown>("abp:user-error", (ev) => {
        if (!mounted) return;
        const msg = ev.payload;
        if (typeof msg === "string") setError(msg);
      }),
      listen<unknown>("abp:ui-action", (ev) => {
        if (!mounted) return;
        if (!isUiActionPayload(ev.payload)) return;
        const action = ev.payload.action;
        if (action === "library.link_folder") {
          void linkLibraryFolder();
          return;
        }
        applyUiAction(action);
      }),
      listen("abp:transport-changed", () => {
        if (!mounted) return;
        void refreshTransport();
        void refreshOsMedia();
      }),
    ])
      .then((unlisteners) => {
        const all = () => unlisteners.forEach((u) => u());
        if (!mounted) all();
        else dispose = all;
      })
      .catch(() => {
        /* e.g. plain Vite without webview IPC */
      });

    return () => {
      mounted = false;
      dispose?.();
    };
  }, [refreshTransport, applyUiAction, loadRecent, handleUserSessionOpen]);

  useEffect(() => {
    if (!modalSheet) return;
    const savedFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const raf = window.requestAnimationFrame(() => modalCloseRef.current?.focus());

    const trapTab = (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      const sheet = modalSheetRef.current;
      if (!sheet) return;
      const nodes = [
        ...sheet.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ),
      ].filter((el) => !el.hasAttribute("disabled"));
      if (nodes.length === 0) return;
      const first = nodes[0];
      const last = nodes[nodes.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", trapTab, true);
    return () => {
      window.cancelAnimationFrame(raf);
      document.removeEventListener("keydown", trapTab, true);
      queueMicrotask(() => {
        if (savedFocus && savedFocus.isConnected) savedFocus.focus();
      });
    };
  }, [modalSheet]);

  useEffect(() => {
    const t = transport;
    if (!t) return;
    const playing = !t.paused && !t.eof && !t.idle && t.current_index !== null;
    if (!playing) return;
    const now = Date.now();
    if (now - lastAutoSave.current > 9000) {
      lastAutoSave.current = now;
      void invoke("save_progress").catch(() => {
        /* ignore transient IPC errors */
      });
    }
  }, [transport]);

  useEffect(() => {
    const t = transport;
    if (!t) return;
    const eof = !!t.eof;
    if (eof && !eofPrev.current) {
      if (stopAfterTrackRef.current) {
        stopAfterTrackRef.current = false;
        setStopAfterTrackUi(false);
        eofPrev.current = eof;
        return;
      }
      void invoke("advance_after_eof")
        .then(() => refreshTransport())
        .catch((e) => setError(String(e)));
    }
    eofPrev.current = eof;
  }, [transport, refreshTransport]);

  useEffect(() => {
    const tr = transport;
    const isMusic =
      playingCollectionKind === "music" ||
      tr?.active_collection_kind === "music" ||
      tr?.playback_kind === "music";
    if (!tr || isMusic || !tr.eof || !tr.active_collection_id) {
      setFinishBookPrompt(false);
      return;
    }
    const idx = tr.current_index;
    const len = tr.playlist_len;
    if (idx != null && len > 0 && idx === len - 1) {
      setFinishBookPrompt(true);
    } else {
      setFinishBookPrompt(false);
    }
  }, [
    transport?.eof,
    transport?.current_index,
    transport?.playlist_len,
    transport?.active_collection_id,
    transport?.playback_kind,
    transport?.active_collection_kind,
    playingCollectionKind,
  ]);

  useEffect(() => {
    const runTransport = (fn: () => Promise<void>) => {
      setError(null);
      void fn().catch((e) => setError(String(e)));
    };

    const onLinux =
      typeof navigator !== "undefined" && /linux/i.test(navigator.userAgent);

    const onKeyDown = (ev: KeyboardEvent) => {
      if (modalSheet) return;
      const target = ev.target as HTMLElement | null;
      const tag = target?.tagName?.toLowerCase();
      if (tag === "input" || tag === "select" || tag === "textarea" || target?.isContentEditable) {
        return;
      }

      if (ev.code === "Space") {
        ev.preventDefault();
        runTransport(async () => {
          await invoke("toggle_pause");
          await refreshTransport();
        });
        return;
      }

      const queueReady = (playlist?.items.length ?? 0) > 0;
      if (!queueReady) return;

      if (ev.code === "ArrowLeft" && !ev.shiftKey && !ev.altKey && !ev.ctrlKey && !ev.metaKey) {
        ev.preventDefault();
        runTransport(async () => {
          await invoke("seek_delta", { delta: -30 });
          await refreshTransport();
        });
        return;
      }

      if (ev.code === "ArrowRight" && !ev.shiftKey && !ev.altKey && !ev.ctrlKey && !ev.metaKey) {
        ev.preventDefault();
        runTransport(async () => {
          await invoke("seek_delta", { delta: 30 });
          await refreshTransport();
        });
        return;
      }

      if (
        ev.code === "ArrowLeft" &&
        ev.shiftKey &&
        !ev.altKey &&
        !ev.ctrlKey &&
        !ev.metaKey
      ) {
        ev.preventDefault();
        runTransport(async () => {
          await invoke("skip_prev");
          await refreshTransport();
        });
        return;
      }

      if (
        ev.code === "ArrowRight" &&
        ev.shiftKey &&
        !ev.altKey &&
        !ev.ctrlKey &&
        !ev.metaKey
      ) {
        ev.preventDefault();
        runTransport(async () => {
          await invoke("skip_next");
          await refreshTransport();
        });
        return;
      }

      // When MPRIS is active, Linux routes headset keys via D-Bus — skip here to
      // avoid double-firing. If MPRIS failed to register (Docker, no session bus),
      // fall through so focused-window media keys still work.
      if (
        onLinux &&
        osMediaActive &&
        (ev.code === "MediaTrackPrevious" ||
          ev.code === "MediaTrackNext" ||
          ev.code === "MediaPlayPause" ||
          ev.code === "MediaPlay" ||
          ev.code === "MediaPause")
      ) {
        return;
      }

      if (ev.code === "MediaTrackPrevious") {
        ev.preventDefault();
        runTransport(async () => {
          await invoke("skip_prev");
          await refreshTransport();
        });
        return;
      }

      if (ev.code === "MediaTrackNext") {
        ev.preventDefault();
        runTransport(async () => {
          await invoke("skip_next");
          await refreshTransport();
        });
        return;
      }

      if (ev.code === "MediaPlayPause") {
        ev.preventDefault();
        runTransport(async () => {
          await invoke("toggle_pause");
          await refreshTransport();
        });
        return;
      }

      if (ev.code === "MediaPlay") {
        ev.preventDefault();
        runTransport(async () => {
          await invoke("set_paused", { paused: false });
          await refreshTransport();
        });
        return;
      }

      if (ev.code === "MediaPause") {
        ev.preventDefault();
        runTransport(async () => {
          await invoke("set_paused", { paused: true });
          await refreshTransport();
        });
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [refreshTransport, modalSheet, playlist?.items.length, osMediaActive]);

  const playCollection = async (
    collectionId: number,
    mode: "continue" | "start",
    shuffle = false,
  ) => {
    if (!isTauri()) return;
    setBusy(t("busy.opening"));
    setError(null);
    try {
      const dto = await invoke<PlaylistDto>("play_collection", {
        collectionId,
        mode,
        shuffle,
      });
      setPlaylist(dto);
      setActiveView("nowPlaying");
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const openCollectionDetail = useCallback((id: number) => {
    setCollectionDetailId(id);
  }, []);

  const onCollectionDetailChanged = useCallback(() => {
    setLibraryRefreshKey((k) => k + 1);
    void refreshTransport();
    void loadLibraryRoots();
  }, [refreshTransport, loadLibraryRoots]);

  const confirmRemoveCollection = useCallback(
    (collectionId: number, title: string) => {
      openConfirm({
        title: t("catalog.removeCollectionConfirmTitle"),
        body: t("catalog.removeCollectionConfirmBody", { title }),
        confirmLabel: t("catalog.removeCollectionConfirmBtn"),
        danger: true,
        onConfirm: async () => {
          await invoke("remove_collection_from_library", { collectionId });
          onCollectionDetailChanged();
        },
      });
    },
    [openConfirm, onCollectionDetailChanged, t],
  );

  const enqueueCollection = async (
    collectionId: number,
    position: "next" | "end" = "end",
  ) => {
    if (!isTauri()) return;
    setError(null);
    try {
      const result = await invoke<{
        playlist: PlaylistDto;
        tracks_added: number;
        collection_title: string;
      }>("enqueue_collection", { collectionId, position });
      setPlaylist(result.playlist);
      const notice =
        position === "next"
          ? t("queue.addedNext", {
              count: result.tracks_added,
              title: result.collection_title,
            })
          : t("queue.addedEnd", {
              count: result.tracks_added,
              title: result.collection_title,
            });
      setQueueNotice(notice);
      window.setTimeout(() => setQueueNotice(null), 4000);
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const playPlaylistById = async (playlistId: number) => {
    if (!isTauri()) return;
    setBusy(t("busy.opening"));
    setError(null);
    try {
      const dto = await invoke<PlaylistDto>("play_playlist", {
        playlistId,
        shuffle: true,
      });
      setPlaylist(dto);
      setActiveView("nowPlaying");
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const shuffleRelax = async () => {
    if (!isTauri()) return;
    setError(null);
    try {
      const relaxId = await invoke<number | null>("find_relax_playlist");
      if (relaxId != null) {
        await playPlaylistById(relaxId);
        return;
      }
      const music = await invoke<CollectionSummaryDto[]>("list_collections", {
        kind: "music",
        filter: null,
        search: null,
        limit: 200,
        offset: 0,
      });
      const available = music.filter((m) => !m.unavailable);
      if (available.length === 0) {
        setError(t("home.noRelaxMusic"));
        return;
      }
      const pick = available[Math.floor(Math.random() * available.length)]!;
      await playCollection(pick.id, "start", true);
    } catch (e) {
      setError(String(e));
    }
  };

  const scanLibraryRoot = async (rootId: number) => {
    if (!isTauri()) return;
    setBusy(t("library.busy.scanning"));
    setError(null);
    try {
      await invoke("scan_library_root", { rootId });
      await loadLibraryRoots();
      setLibraryRefreshKey((k) => k + 1);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const exportLibraryDb = async () => {
    if (!isTauri()) return;
    setBusy(t("library.busy.exporting"));
    setError(null);
    try {
      await invoke<string | null>("export_db");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const removeLibraryRoot = (root: LibraryRootDto) => {
    if (!isTauri()) return;
    openConfirm({
      title: t("library.removeConfirmTitle"),
      body: t("library.removeConfirmBody"),
      confirmLabel: t("library.removeConfirmBtn"),
      danger: true,
      onConfirm: async () => {
        setError(null);
        try {
          await invoke("remove_library_root", { rootId: root.id });
          await loadLibraryRoots();
          setLibraryRefreshKey((k) => k + 1);
        } catch (e) {
          setError(String(e));
        }
      },
    });
  };

  const openFolder = async () => {
    setBusy(t("busy.openingFolder"));
    setError(null);
    try {
      const dto = await invoke<PlaylistDto | null>("pick_open_folder");
      if (dto) await handleUserSessionOpen(dto, true);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const openFile = async () => {
    setBusy(t("busy.openingFile"));
    setError(null);
    try {
      const dto = await invoke<PlaylistDto | null>("pick_open_file");
      if (dto) await handleUserSessionOpen(dto, false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const reopenRecent = async (entry: RecentOpenDto) => {
    setBusy(t("busy.opening"));
    setError(null);
    try {
      const dto = await invoke<PlaylistDto | null>("reopen_recent", {
        path: entry.path,
        kind: entry.kind,
      });
      if (dto) await handleUserSessionOpen(dto, entry.kind === "folder");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const clearRecentHistory = () => {
    if (!isTauri()) return;
    openConfirm({
      title: t("confirm.clearRecentTitle"),
      body: t("confirm.clearRecent"),
      confirmLabel: t("confirm.clearRecentBtn"),
      onConfirm: async () => {
        setError(null);
        try {
          await invoke("clear_recent_opened");
          await loadRecent();
        } catch (e) {
          setError(String(e));
        }
      },
    });
  };

  const changeSort = async (sort: SortKey) => {
    if (!playlist) return;
    setError(null);
    try {
      const dto = await invoke<PlaylistDto>("resort_playlist", { sort });
      setPlaylist(dto);
    } catch (e) {
      setError(String(e));
    }
  };

  const shuffleQueue = async () => {
    if (!playlist || playlist.items.length < 2) return;
    await changeSort("random");
  };

  const cycleRepeatMode = async () => {
    if (!isTauri()) return;
    const order = ["off", "one", "all"] as const;
    const cur: (typeof order)[number] =
      transport?.repeat_mode === "one"
        ? "one"
        : transport?.repeat_mode === "all"
          ? "all"
          : "off";
    const i = order.indexOf(cur);
    const next = order[(i + 1) % order.length]!;
    setError(null);
    try {
      await invoke("set_repeat_mode", { mode: next });
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const setResumePref = async (enabled: boolean) => {
    if (!isTauri()) return;
    setError(null);
    setAppPrefs((prev) => (prev ? { ...prev, resume_playing_on_launch: enabled } : prev));
    try {
      const p = await invoke<AppPrefsDto>("set_resume_playing_on_launch", { enabled });
      setAppPrefs(p);
    } catch (e) {
      setError(String(e));
      void loadAppPrefs();
    }
  };

  const setScanPref = async (enabled: boolean) => {
    if (!isTauri()) return;
    setError(null);
    try {
      const r = await invoke<SetScanFoldersResult>("set_scan_subfolders", { enabled });
      setAppPrefs(r.prefs);
      if (r.playlist) setPlaylist(r.playlist);
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const setOnlineMetadataPref = async (enabled: boolean) => {
    if (!isTauri()) return;
    setError(null);
    try {
      const p = await invoke<AppPrefsDto>("set_online_metadata_enabled", { enabled });
      setAppPrefs(p);
    } catch (e) {
      setError(String(e));
    }
  };

  const markActiveCollectionFinished = async () => {
    const cid = transport?.active_collection_id;
    if (!cid) return;
    setFinishBookPrompt(false);
    try {
      await invoke("mark_collection_listened", { collectionId: cid, listened: true });
    } catch (e) {
      setError(String(e));
    }
  };

  const changeUiLocale = async (loc: Locale) => {
    if (!isTauri()) {
      setLocale(loc);
      return;
    }
    setError(null);
    try {
      const p = await invoke<AppPrefsDto>("set_ui_locale", { locale: loc });
      setAppPrefs(p);
      setLocale(normalizeLocale(p.ui_locale));
    } catch (e) {
      setError(String(e));
    }
  };

  const recoverMpv = async () => {
    if (!isTauri()) return;
    setError(null);
    try {
      await invoke("recover_mpv");
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const startSleep = () => {
    const m = Number(sleepPreset);
    if (!Number.isFinite(m) || m <= 0) return;
    setSleepDeadlineMs(Date.now() + m * 60_000);
    setSleepTick((x) => x + 1);
  };

  const cancelSleep = () => {
    setSleepDeadlineMs(null);
  };

  const playIndex = async (index: number) => {
    setError(null);
    try {
      await invoke("play_index", { index });
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const toggleTrackListened = async (item: PlaylistItemDto) => {
    if (!isTauri()) return;
    setError(null);
    try {
      const dto = await invoke<PlaylistDto>("set_track_listened", {
        path: item.path,
        listened: !item.listened,
      });
      setPlaylist(dto);
    } catch (e) {
      setError(String(e));
    }
  };

  const relinkLibraryFile = async (fileId: number) => {
    if (!isTauri()) return;
    setError(null);
    try {
      const path = await invoke<string | null>("pick_relink_audio_file");
      if (!path) return;
      const result = await invoke<{ playlist: PlaylistDto | null }>("relink_collection_file", {
        fileId,
        newPath: path,
      });
      setLibraryRefreshKey((k) => k + 1);
      if (result.playlist) setPlaylist(result.playlist);
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const removeLibraryFile = (fileId: number, title: string) => {
    if (!isTauri()) return;
    openConfirm({
      title: t("catalog.removeFromLibraryConfirmTitle"),
      body: t("catalog.removeFromLibraryConfirmBody", { title }),
      confirmLabel: t("catalog.removeFromLibraryConfirmBtn"),
      danger: true,
      onConfirm: async () => {
        setError(null);
        try {
          await invoke("remove_collection_file_from_library", { fileId });
          setLibraryRefreshKey((k) => k + 1);
          await refreshTransport();
        } catch (e) {
          setError(String(e));
        }
      },
    });
  };

  const removeQueueItem = async (path: string) => {
    if (!isTauri()) return;
    setError(null);
    try {
      const dto = await invoke<PlaylistDto | null>("remove_queue_item", { path });
      setPlaylist(dto);
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const deleteTrackFile = (item: PlaylistItemDto) => {
    if (!isTauri()) return;
    openConfirm({
      title: t("confirm.deleteTrackTitle"),
      body: t("confirm.deleteTrack", { label: item.label }),
      confirmLabel: t("confirm.deleteTrackBtn"),
      danger: true,
      onConfirm: async () => {
        setError(null);
        try {
          const dto = await invoke<PlaylistDto | null>("delete_track_file", {
            path: item.path,
          });
          setPlaylist(dto);
          await refreshTransport();
          await loadRecent();
        } catch (e) {
          setError(String(e));
        }
      },
    });
  };

  const markSessionListened = async (listened: boolean) => {
    if (!isTauri() || !playlist) return;
    setError(null);
    try {
      const dto = await invoke<PlaylistDto>("mark_session_listened", { listened });
      setPlaylist(dto);
    } catch (e) {
      setError(String(e));
    }
  };

  const deleteSessionFiles = () => {
    if (!isTauri() || !playlist || !sessionDeleteCopy) return;
    openConfirm({
      title: sessionDeleteCopy.confirmTitle,
      body: sessionDeleteCopy.confirmBody,
      confirmLabel: sessionDeleteCopy.confirmLabel,
      danger: true,
      onConfirm: async () => {
        setError(null);
        try {
          await invoke("delete_session_files");
          setPlaylist(null);
          await refreshTransport();
          await loadRecent();
        } catch (e) {
          setError(String(e));
        }
      },
    });
  };

  const togglePause = async () => {
    setError(null);
    try {
      await invoke("toggle_pause");
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const seekTo = async (seconds: number) => {
    setError(null);
    try {
      await invoke("seek_seconds", { seconds });
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const seekDelta = async (delta: number) => {
    setError(null);
    try {
      await invoke("seek_delta", { delta });
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const setSpeed = async (speed: number) => {
    setError(null);
    try {
      await invoke<number>("set_speed", { speed });
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const setDefaultSpeed = async (speed: number) => {
    setError(null);
    try {
      await invoke<number>("set_default_playback_speed", { speed });
      const p = await invoke<AppPrefsDto>("get_app_prefs");
      setAppPrefs(p);
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const resetTrackSpeed = async () => {
    setError(null);
    try {
      await invoke<number>("reset_track_speed_to_default");
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const setPlaybackSpeedDefaults = async (audiobook: number, music: number) => {
    if (!isTauri()) return;
    setError(null);
    try {
      const p = await invoke<AppPrefsDto>("set_playback_speed_defaults", { audiobook, music });
      setAppPrefs(p);
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const skipNext = async () => {
    setError(null);
    try {
      await invoke("skip_next");
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const skipPrev = async () => {
    setError(null);
    try {
      await invoke("skip_prev");
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    }
  };

  const duration = transport?.duration_sec ?? null;
  const position = transport?.position_sec ?? 0;
  const progressMax = useMemo(() => {
    if (duration && duration > 1) return duration;
    return Math.max(1, position);
  }, [duration, position]);
  const sliderValue = seekUi ?? position;

  const currentTitle = useMemo(() => {
    if (transport?.current_path) return fileBasename(transport.current_path);
    return t("nowPlaying.nothingPlaying");
  }, [transport?.current_path, t]);

  const liveStatus = useMemo(() => {
    if (!transport) return t("status.starting");
    if (transport.mpv_error) return t("status.engineError", { detail: transport.mpv_error });
    if (transport.idle && transport.playlist_len === 0) return t("status.openToBegin");
    if (transport.idle) return t("status.ready");
    if (transport.eof) return t("status.eof");
    if (transport.paused) return t("status.paused");
    return t("status.playing");
  }, [transport, t]);

  const statusTone = useMemo(() => {
    const t = transport;
    if (!t) return "boot";
    if (t.mpv_error) return "error";
    if (t.idle && t.playlist_len === 0) return "empty";
    if (t.idle) return "ready";
    if (t.eof) return "eof";
    if (t.paused) return "paused";
    return "playing";
  }, [transport]);

  const rootDisplay = useMemo(() => {
    const r = playlist?.root;
    if (!r) return null;
    return r.length > 42 ? `…${r.slice(-40)}` : r;
  }, [playlist?.root]);

  const hasQueue = (playlist?.items.length ?? 0) > 0;
  const hasSession = hasQueue;
  const hasTrack = transport != null && transport.current_index !== null;
  const isPlaying =
    hasTrack && transport != null && !transport.paused && !transport.eof && !transport.idle;
  const showMiniPlayer = hasSession && activeView !== "nowPlaying";
  const showQueuePanel = activeView === "nowPlaying";
  const isMusicSession =
    playingCollectionKind === "music" ||
    transport?.active_collection_kind === "music" ||
    transport?.playback_kind === "music";
  const canSeekTransport =
    hasQueue && transport != null && !transport.mpv_error;
  const canSkipTransport = hasQueue && transport != null && !transport.mpv_error;
  const canTogglePlayback = hasQueue && transport != null && !transport.mpv_error;
  const allTracksListened = useMemo(() => {
    const items = playlist?.items;
    if (!items || items.length === 0) return false;
    return items.every((it) => it.listened);
  }, [playlist?.items]);

  const isCatalogSession = transport?.active_collection_id != null;
  const canDeleteQueueItem = useCallback(
    (path: string) => isCatalogSession || pathUnderAvailableRoot(path, libraryRoots),
    [isCatalogSession, libraryRoots],
  );

  const sessionDeleteCopy = useMemo((): SessionDeleteCopy | null => {
    const items = playlist?.items ?? [];
    if (items.length === 0) return null;
    if (!items.every((it) => canDeleteQueueItem(it.path))) return null;
    return buildSessionDeleteCopy(playlist!, transport?.playback_kind, t);
  }, [playlist, canDeleteQueueItem, transport?.playback_kind, t]);

  const queueSearchNorm = useMemo(() => normalizeQueueSearch(queueSearch), [queueSearch]);

  const repeatModeLabel = useMemo(() => {
    const m = (transport?.repeat_mode ?? "off").toLowerCase();
    if (m === "one") return t("queue.repeatOne");
    if (m === "all") return t("queue.repeatAll");
    return t("queue.repeatOff");
  }, [transport?.repeat_mode, t]);

  const repeatModeTitle = useMemo(() => {
    const m = (transport?.repeat_mode ?? "off").toLowerCase();
    if (m === "one") return t("queue.repeatTitleOne");
    if (m === "all") return t("queue.repeatTitleAll");
    return t("queue.repeatTitleOff");
  }, [transport?.repeat_mode, t]);

  const queueFilteredRows = useMemo(() => {
    const items = playlist?.items ?? [];
    const rows: { item: PlaylistItemDto; idx: number }[] = [];
    for (let idx = 0; idx < items.length; idx++) {
      const item = items[idx]!;
      if (itemMatchesQueueSearch(queueSearchNorm, item)) {
        rows.push({ item, idx });
      }
    }
    return { rows, total: items.length };
  }, [playlist?.items, queueSearchNorm]);

  const sleepRemainLabel = useMemo(() => {
    if (sleepDeadlineMs == null) return null;
    void sleepTick;
    const sec = Math.max(0, Math.ceil((sleepDeadlineMs - Date.now()) / 1000));
    const h = Math.floor(sec / 3600);
    const m = Math.floor((sec % 3600) / 60);
    const s = sec % 60;
    if (h > 0) return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
    return `${m}:${String(s).padStart(2, "0")}`;
  }, [sleepDeadlineMs, sleepTick]);

  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-stage">
        {activeView === "nowPlaying" ? t("skip.toPlayback") : t("nav.home")}
      </a>

      <div className="app-chrome">
        <header className="menubar" ref={menubarRef}>
          <div className="menubar-brand" data-tauri-drag-region>
            <img
              className="menubar-logo"
              src="/app-icon.png"
              width={22}
              height={22}
              alt=""
              decoding="async"
            />
            <span className="menubar-app-name">{t("app.title")}</span>
          </div>

          <nav className="menubar-menus" aria-label={t("menubar.aria")}>
            <div className="menubar-menu">
              <button
                type="button"
                className={`menubar-trigger${menuOpen === "file" ? " menubar-trigger--open" : ""}`}
                aria-expanded={menuOpen === "file"}
                aria-haspopup="menu"
                onClick={(e) => {
                  e.stopPropagation();
                  setMenuOpen((m) => (m === "file" ? null : "file"));
                }}
              >
                {t("menubar.file")}
              </button>
              {menuOpen === "file" ? (
                <div className="menubar-dropdown" role="menu">
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item menubar-item--primary"
                    onClick={() => {
                      setMenuOpen(null);
                      void linkLibraryFolder();
                    }}
                  >
                    {t("menu.file.linkFolder")}
                  </button>
                  <div className="menubar-sep" role="separator" />
                  <div className="menubar-group-label" role="presentation">
                    {t("sidebar.quickListen")}
                  </div>
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item"
                    onClick={() => {
                      setMenuOpen(null);
                      void openFolder();
                    }}
                  >
                    {t("menu.file.openFolder")}
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item"
                    onClick={() => {
                      setMenuOpen(null);
                      void openFile();
                    }}
                  >
                    {t("menu.file.openFile")}
                  </button>
                  <div className="menubar-sep" role="separator" />
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item"
                    onClick={() => {
                      setMenuOpen(null);
                      openPreferences();
                    }}
                  >
                    {t("menu.file.preferences")}
                  </button>
                </div>
              ) : null}
            </div>

            <div className="menubar-menu">
              <button
                type="button"
                className={`menubar-trigger${menuOpen === "playback" ? " menubar-trigger--open" : ""}`}
                aria-expanded={menuOpen === "playback"}
                aria-haspopup="menu"
                onClick={(e) => {
                  e.stopPropagation();
                  setMenuOpen((m) => (m === "playback" ? null : "playback"));
                }}
              >
                {t("menubar.playback")}
              </button>
              {menuOpen === "playback" ? (
                <div className="menubar-dropdown" role="menu">
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item"
                    disabled={!canTogglePlayback}
                    onClick={() => {
                      setMenuOpen(null);
                      void togglePause();
                    }}
                  >
                    {t("menu.playback.playPause")}
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item"
                    disabled={!canSkipTransport}
                    onClick={() => {
                      setMenuOpen(null);
                      void skipPrev();
                    }}
                  >
                    {t("menu.playback.prevTrack")}
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item"
                    disabled={!canSkipTransport}
                    onClick={() => {
                      setMenuOpen(null);
                      void skipNext();
                    }}
                  >
                    {t("menu.playback.nextTrack")}
                  </button>
                  <div className="menubar-sep" role="separator" />
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item"
                    disabled={!canSeekTransport}
                    onClick={() => {
                      setMenuOpen(null);
                      void seekDelta(-30);
                    }}
                  >
                    {t("menu.playback.back30")}
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item"
                    disabled={!canSeekTransport}
                    onClick={() => {
                      setMenuOpen(null);
                      void seekDelta(30);
                    }}
                  >
                    {t("menu.playback.forward30")}
                  </button>
                  <div className="menubar-sep" role="separator" />
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item"
                    onClick={() => {
                      setMenuOpen(null);
                      setModalSheet("sleep");
                    }}
                  >
                    {t("menu.playback.sleepTimer")}
                  </button>
                </div>
              ) : null}
            </div>

            <div className="menubar-menu">
              <button
                type="button"
                className={`menubar-trigger${menuOpen === "view" ? " menubar-trigger--open" : ""}`}
                aria-expanded={menuOpen === "view"}
                aria-haspopup="menu"
                onClick={(e) => {
                  e.stopPropagation();
                  setMenuOpen((m) => (m === "view" ? null : "view"));
                }}
              >
                {t("menubar.view")}
              </button>
              {menuOpen === "view" ? (
                <div className="menubar-dropdown" role="menu">
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item"
                    onClick={() => {
                      setMenuOpen(null);
                      scrollToPlayer();
                    }}
                  >
                    {t("menu.view.scrollPlayer")}
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item"
                    onClick={() => {
                      setMenuOpen(null);
                      scrollToQueue();
                    }}
                  >
                    {t("menu.view.scrollQueue")}
                  </button>
                </div>
              ) : null}
            </div>

            <div className="menubar-menu">
              <button
                type="button"
                className={`menubar-trigger${menuOpen === "help" ? " menubar-trigger--open" : ""}`}
                aria-expanded={menuOpen === "help"}
                aria-haspopup="menu"
                onClick={(e) => {
                  e.stopPropagation();
                  setMenuOpen((m) => (m === "help" ? null : "help"));
                }}
              >
                {t("menubar.help")}
              </button>
              {menuOpen === "help" ? (
                <div className="menubar-dropdown" role="menu">
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item"
                    onClick={() => {
                      setMenuOpen(null);
                      setModalSheet("shortcuts");
                    }}
                  >
                    {t("menu.help.shortcuts")}
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item"
                    onClick={() => {
                      setMenuOpen(null);
                      setModalSheet("about");
                    }}
                  >
                    {t("menu.help.about")}
                  </button>
                </div>
              ) : null}
            </div>
          </nav>

          {sleepDeadlineMs != null && sleepRemainLabel ? (
            <button
              type="button"
              className="menubar-sleep-chip"
              title={t("sleep.chip.title")}
              aria-label={t("sleep.chip.aria", { time: sleepRemainLabel })}
              onClick={() => setModalSheet("sleep")}
            >
              <span className="menubar-sleep-label">{t("sleep.chip.label")}</span>
              <span className="menubar-sleep-time" aria-hidden="true">
                {sleepRemainLabel}
              </span>
            </button>
          ) : null}

          {busy ? <span className="menubar-busy">{busy}</span> : <span className="menubar-spacer" aria-hidden="true" />}
        </header>

        {error ? (
          <div className="alert alert--banner" role="alert">
            <div className="alert-row">
              <p className="alert-message">{error}</p>
              <button className="btn btn-ghost btn-compact" type="button" onClick={() => setError(null)}>
                {t("alert.dismiss")}
              </button>
            </div>
          </div>
        ) : null}
        {driveNotice ? (
          <div className="library-prompt-banner library-prompt-banner--info" role="status">
            <div className="library-prompt-row">
              <p>{driveNotice}</p>
              <button
                type="button"
                className="btn btn-ghost btn-compact"
                aria-label={t("alert.dismiss")}
                onClick={() => setDriveNotice(null)}
              >
                {t("alert.dismiss")}
              </button>
            </div>
          </div>
        ) : null}
        {finishBookPrompt ? (
          <div className="library-prompt-banner" role="status">
            <div className="library-prompt-row">
              <p>{t("finishBook.prompt")}</p>
              <button
                type="button"
                className="btn btn-secondary btn-compact"
                onClick={() => void markActiveCollectionFinished()}
              >
                {t("finishBook.mark")}
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-compact"
                onClick={() => setFinishBookPrompt(false)}
              >
                {t("finishBook.dismiss")}
              </button>
            </div>
          </div>
        ) : null}
        {libraryPromptPath ? (
          <div className="library-prompt-banner" role="status">
            <div className="library-prompt-row">
              <p>{t("library.linkPrompt")}</p>
              <button
                type="button"
                className="btn btn-secondary btn-compact"
                onClick={() => {
                  const path = libraryPromptPath;
                  setLibraryPromptPath(null);
                  void linkLibraryFolder(path);
                }}
              >
                {t("library.linkFolder")}
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-compact"
                aria-label={t("alert.dismiss")}
                onClick={() => setLibraryPromptPath(null)}
              >
                {t("alert.dismiss")}
              </button>
            </div>
          </div>
        ) : null}
        {transport?.mpv_error ? (
          <div className="mpv-recover-banner" role="status">
            <div className="mpv-recover-row">
              <p className="mpv-recover-text">
                <strong>{t("mpv.disconnected")}</strong> {transport.mpv_error}
              </p>
              <button className="btn btn-secondary btn-compact" type="button" onClick={() => void recoverMpv()}>
                {t("mpv.restart")}
              </button>
            </div>
          </div>
        ) : null}
      </div>

      <div
        className={`app-body${showQueuePanel ? " app-body--playing" : " app-body--library"}`}
        {...(modalSheet ? { inert: "" as const } : {})}
      >
        <aside className="sidebar sidebar--left sidebar--nav" aria-label={t("nav.aria")}>
          <AppNav
            active={activeView}
            hasSession={hasSession}
            isPlaying={isPlaying}
            onNavigate={setActiveView}
          />
          <LibrarySidebar
            linkedFolderCount={libraryRoots.length}
            onLinkFolder={() => void linkLibraryFolder()}
            onManageLibrary={openManageLibrary}
            onOpenFolder={() => void openFolder()}
            onOpenFile={() => void openFile()}
          />
          {recent.length > 0 ? (
            <div className="sidebar-section sidebar-section--recent">
              <div className="recent-section-head">
                <h2 className="sidebar-heading sidebar-heading--compact" id="recent-opened-heading">
                  {t("sidebar.recent")}
                </h2>
                <button
                  type="button"
                  className="btn btn-ghost btn-compact recent-clear"
                  aria-label={t("sidebar.recentClearAria")}
                  title={t("sidebar.recentClearTitle")}
                  onClick={() => void clearRecentHistory()}
                >
                  {t("sidebar.recentClear")}
                </button>
              </div>
              <ul className="recent-list recent-list--compact" aria-label={t("sidebar.recent")}>
                {recent.slice(0, 5).map((it) => (
                  <li key={`${it.kind}:${it.path}`} className="recent-li">
                    <button
                      type="button"
                      className="recent-item"
                      title={it.path}
                      onClick={() => void reopenRecent(it)}
                    >
                      <span className="recent-label">{it.label}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </aside>

        <main ref={mainStageRef} className="main-stage" id="main-stage" tabIndex={-1}>
          {activeView === "home" ? (
            <HomeView
              refreshKey={libraryRefreshKey}
              onPlayCollection={(id, mode) => void playCollection(id, mode)}
              onAddToQueue={(id, position) => void enqueueCollection(id, position)}
              onOpenDetail={openCollectionDetail}
              onShuffleRelax={() => void shuffleRelax()}
              onLinkFolder={() => void linkLibraryFolder()}
              onOpenFolder={() => void openFolder()}
              onOpenFile={() => void openFile()}
              onBrowseAudiobooks={() => setActiveView("audiobooks")}
              onBrowseMusic={() => setActiveView("music")}
            />
          ) : activeView === "audiobooks" ? (
            <CatalogView
              kind="audiobook"
              refreshKey={libraryRefreshKey}
              onPlayCollection={(id, mode) => void playCollection(id, mode)}
              onOpenDetail={openCollectionDetail}
              onAddToQueue={(id, position) => void enqueueCollection(id, position)}
              onLinkFolder={() => void linkLibraryFolder()}
              onOpenFolder={() => void openFolder()}
              onRemoveCollection={confirmRemoveCollection}
            />
          ) : activeView === "music" ? (
            <CatalogView
              kind="music"
              refreshKey={libraryRefreshKey}
              onPlayCollection={(id, mode) => void playCollection(id, mode)}
              onOpenDetail={openCollectionDetail}
              onAddToQueue={(id, position) => void enqueueCollection(id, position)}
              onLinkFolder={() => void linkLibraryFolder()}
              onOpenFolder={() => void openFolder()}
              onRemoveCollection={confirmRemoveCollection}
            />
          ) : activeView === "library" ? (
            <LibraryView
              libraryRoots={libraryRoots}
              onLinkFolder={() => void linkLibraryFolder()}
              onScanRoot={(id) => void scanLibraryRoot(id)}
              onRemoveRoot={removeLibraryRoot}
              onExportDb={() => void exportLibraryDb()}
            />
          ) : activeView === "playlists" ? (
            <PlaylistsView
              onPlayPlaylist={(id) => void playPlaylistById(id)}
              openConfirm={openConfirm}
              onLibraryChanged={() => {
                void loadLibraryRoots();
                setLibraryRefreshKey((k) => k + 1);
              }}
            />
          ) : (
            <NowPlayingView
              transport={transport}
              chapters={chapters}
              isMusicSession={isMusicSession}
              nowCoverSrc={nowCoverSrc}
              currentTitle={currentTitle}
              liveStatus={liveStatus}
              statusTone={statusTone}
              hasQueue={hasQueue}
              hasTrack={hasTrack}
              canSeekTransport={canSeekTransport}
              canSkipTransport={canSkipTransport}
              canTogglePlayback={canTogglePlayback}
              allTracksListened={allTracksListened}
              sliderValue={sliderValue}
              progressMax={progressMax}
              seekUi={seekUi}
              setSeekUi={setSeekUi}
              formatClock={formatClock}
              onSeekTo={(v) => void seekTo(v)}
              onSeekDelta={(d) => void seekDelta(d)}
              onTogglePause={() => void togglePause()}
              onSkipPrev={() => void skipPrev()}
              onSkipNext={() => void skipNext()}
              onSetSpeed={(s) => void setSpeed(s)}
              onSetDefaultSpeed={(s) => void setDefaultSpeed(s)}
              onResetTrackSpeed={() => void resetTrackSpeed()}
              onMarkSessionListened={(l) => void markSessionListened(l)}
              onDeleteSessionFiles={() => void deleteSessionFiles()}
              deleteSessionLabel={sessionDeleteCopy?.buttonLabel ?? null}
              onOpenDetails={
                playingCollectionId != null
                  ? () => openCollectionDetail(playingCollectionId)
                  : undefined
              }
              currentPath={transport?.current_path ?? null}
              osMediaActive={osMediaActive}
            />
          )}
        </main>

        {showQueuePanel ? (
        <aside
          ref={playlistPanelRef}
          id="queue-panel"
          className="sidebar sidebar--right"
          aria-labelledby="playlist-title"
          tabIndex={-1}
        >
          <div className="sidebar-frame">
            <div className="sidebar-rail-head">
              <div>
                <h2 className="sidebar-rail-title" id="playlist-title">
                  {t("queue.title")}
                </h2>
                <p className="sidebar-rail-sub">{t("queue.subtitle")}</p>
                {queueNotice ? (
                  <p className="queue-notice" role="status">
                    {queueNotice}
                  </p>
                ) : null}
              </div>
              <span
                className="queue-badge"
                title={playlist?.root ?? ""}
                aria-label={playlist ? t("queue.badge.count", { count: playlist.items.length }) : t("queue.badge.none")}
              >
                {playlist ? playlist.items.length : "—"}
              </span>
            </div>
            <div className="queue-toolbar">
              {playingCollectionId != null ? (
                <div className="queue-toolbar-row">
                  <button
                    type="button"
                    className="btn btn-secondary queue-details-btn"
                    onClick={() => openCollectionDetail(playingCollectionId)}
                  >
                    {t("queue.openDetails")}
                  </button>
                </div>
              ) : null}
              <div className="queue-toolbar-row">
                <label className="field-label queue-toolbar-label" htmlFor="sort-select">
                  {t("queue.sortLabel")}
                </label>
                <select
                  id="sort-select"
                  className="select select-block queue-toolbar-select"
                  value={playlist?.sort ?? "name-asc"}
                  disabled={!playlist}
                  onChange={(e) => void changeSort(e.target.value as SortKey)}
                >
                  <option value="name-asc">{t("sort.nameAsc")}</option>
                  <option value="name-desc">{t("sort.nameDesc")}</option>
                  <option value="modified-desc">{t("sort.modifiedDesc")}</option>
                  <option value="modified-asc">{t("sort.modifiedAsc")}</option>
                  <option value="size-desc">{t("sort.sizeDesc")}</option>
                  <option value="size-asc">{t("sort.sizeAsc")}</option>
                  <option value="random">{t("sort.random")}</option>
                </select>
              </div>
              <div className="queue-toolbar-row queue-playback-modes">
                <button
                  type="button"
                  className="btn btn-ghost queue-mode-btn"
                  disabled={!isTauri() || !hasQueue || (playlist?.items.length ?? 0) < 2}
                  title={t("queue.shuffleTitle")}
                  onClick={() => void shuffleQueue()}
                >
                  {t("queue.shuffleBtn")}
                </button>
                <button
                  type="button"
                  className="btn btn-secondary queue-mode-btn"
                  disabled={!isTauri() || !hasQueue}
                  title={repeatModeTitle}
                  aria-label={repeatModeTitle}
                  onClick={() => void cycleRepeatMode()}
                >
                  {repeatModeLabel}
                </button>
              </div>
              <div className="queue-toolbar-row">
                <label className="field-label queue-toolbar-label" htmlFor="queue-search">
                  {t("queue.filterLabel")}
                </label>
                <input
                  id="queue-search"
                  type="search"
                  className="queue-search-input"
                  placeholder={t("queue.filterPlaceholder")}
                  value={queueSearch}
                  disabled={!hasQueue}
                  aria-controls={queueFilteredRows.rows.length > 0 ? "playlist-list" : undefined}
                  aria-describedby={queueSearchNorm && hasQueue ? "queue-search-meta" : undefined}
                  onChange={(e) => setQueueSearch(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape" && queueSearch) {
                      e.stopPropagation();
                      setQueueSearch("");
                    }
                  }}
                  autoComplete="off"
                  spellCheck={false}
                />
                {queueSearchNorm && hasQueue ? (
                  <p id="queue-search-meta" className="queue-search-meta" aria-live="polite">
                    {queueFilteredRows.rows.length === 0
                      ? t("queue.filterNoMatches")
                      : t("queue.filterSummary", {
                          shown: queueFilteredRows.rows.length,
                          total: queueFilteredRows.total,
                        })}
                  </p>
                ) : null}
              </div>
            </div>
            <div className="playlist" aria-label={t("queue.listAria")}>
            {!playlist || playlist.items.length === 0 ? (
              <div className="playlist-empty">
                <p className="playlist-empty-title">{t("queue.empty.title")}</p>
                <p className="playlist-empty-text">{t("queue.empty.body")}</p>
              </div>
            ) : queueFilteredRows.rows.length === 0 ? (
              <div className="playlist-empty playlist-empty--filter">
                <p className="playlist-empty-title">{t("queue.filterEmpty.title")}</p>
                <p className="playlist-empty-text">{t("queue.filterEmpty.body")}</p>
              </div>
            ) : (
              <ul id="playlist-list" className="playlist-list">
                {queueFilteredRows.rows.map(({ item: it, idx }) => {
                  const active = transport?.current_index === idx;
                  const dur =
                    typeof it.duration_sec === "number" && Number.isFinite(it.duration_sec)
                      ? formatClock(it.duration_sec)
                      : null;
                  const meta = formatQueueItemMeta(it.artist, it.album);
                  const listenedTitle = it.listened
                    ? t("queue.unmarkListenedTitle")
                    : t("queue.markListenedTitle");
                  return (
                    <li key={it.path} className="playlist-li">
                      <div
                        className={`playlist-row${it.listened ? " playlist-row--listened" : ""}${it.library_missing ? " playlist-row--missing" : ""}`}
                        onContextMenu={(e) => {
                          const items: ContextMenuEntry[] = [
                            {
                              id: "play",
                              label: t("home.play"),
                              disabled: it.library_missing,
                              onClick: () => void playIndex(idx),
                            },
                            {
                              id: "listened",
                              label: it.listened
                                ? t("contextMenu.unmarkListened")
                                : t("contextMenu.markListened"),
                              disabled: !isTauri(),
                              onClick: () => void toggleTrackListened(it),
                            },
                          ];
                          if (it.library_missing && it.collection_file_id != null) {
                            items.push(
                              ...missingFileContextEntries(
                                it.collection_file_id,
                                it.label,
                                {
                                  onRelink: relinkLibraryFile,
                                  onRemove: removeLibraryFile,
                                },
                                t,
                              ),
                            );
                          } else if (it.library_missing) {
                            items.push({ type: "separator" });
                            items.push({
                              id: "remove-queue",
                              label: t("queue.removeMissing"),
                              danger: true,
                              onClick: () => void removeQueueItem(it.path),
                            });
                          } else if (canDeleteQueueItem(it.path)) {
                            items.push({ type: "separator" });
                            items.push({
                              id: "delete",
                              label: t("contextMenu.deleteFile"),
                              danger: true,
                              disabled: !isTauri(),
                              onClick: () => void deleteTrackFile(it),
                            });
                          }
                          if (playingCollectionId != null) {
                            items.push({ type: "separator" });
                            items.push({
                              id: "details",
                              label: t("catalog.editTitle"),
                              onClick: () => openCollectionDetail(playingCollectionId),
                            });
                          }
                          openContextMenu(
                            e,
                            appendPlaylistContextEntries(items, { path: it.path }),
                          );
                        }}
                      >
                        <button
                          type="button"
                          className={`playlist-item${active ? " playlist-item--active" : ""}`}
                          aria-current={active ? "true" : undefined}
                          title={it.path}
                          onClick={() => void playIndex(idx)}
                        >
                          <span className="track-idx" aria-hidden="true">
                            {it.listened ? (
                              <span className="track-idx-check" aria-hidden="true">
                                <IconCheck />
                              </span>
                            ) : (
                              String(idx + 1).padStart(2, "0")
                            )}
                          </span>
                          <span className="track-body">
                            <span className="track-row track-row--main">
                              <span className="track-title">
                                {it.label}
                                {it.library_missing ? (
                                  <span className="track-missing-badge">{t("catalog.fileMissing")}</span>
                                ) : null}
                              </span>
                              <span
                                className="track-duration"
                                {...(dur
                                  ? { title: `${t("queue.durationColumn")}: ${dur}` }
                                  : {
                                      title: t("queue.durationUnknown"),
                                      "aria-label": t("queue.durationUnknown"),
                                    })}
                              >
                                {dur ?? "—"}
                              </span>
                            </span>
                            {meta ? (
                              <span className="track-meta" title={meta}>
                                {meta}
                              </span>
                            ) : null}
                          </span>
                        </button>
                        <div className="track-actions" aria-label={t("queue.itemActionsAria")}>
                          {it.library_missing && it.collection_file_id != null ? (
                            <>
                              <button
                                type="button"
                                className="track-action track-action--repair"
                                disabled={!isTauri()}
                                aria-label={t("catalog.relinkFile")}
                                title={t("catalog.relinkFile")}
                                onClick={() => void relinkLibraryFile(it.collection_file_id!)}
                              >
                                ↗
                              </button>
                              <button
                                type="button"
                                className="track-action track-action--danger"
                                disabled={!isTauri()}
                                aria-label={t("catalog.removeFromLibrary")}
                                title={t("catalog.removeFromLibrary")}
                                onClick={() => void removeLibraryFile(it.collection_file_id!, it.label)}
                              >
                                ×
                              </button>
                            </>
                          ) : it.library_missing ? (
                            <button
                              type="button"
                              className="track-action track-action--danger"
                              disabled={!isTauri()}
                              aria-label={t("queue.removeMissing")}
                              title={t("queue.removeMissing")}
                              onClick={() => void removeQueueItem(it.path)}
                            >
                              ×
                            </button>
                          ) : null}
                          {isCatalogSession ? (
                            <AddToPlaylistButton target={{ path: it.path }} compact />
                          ) : null}
                          <button
                            type="button"
                            className={`track-action${
                              it.listened ? " track-action--on" : ""
                            }`}
                            disabled={!isTauri()}
                            aria-pressed={it.listened}
                            aria-label={listenedTitle}
                            title={listenedTitle}
                            onClick={() => void toggleTrackListened(it)}
                          >
                            <IconCheck />
                          </button>
                          {canDeleteQueueItem(it.path) ? (
                            <button
                              type="button"
                              className="track-action track-action--danger"
                              disabled={!isTauri()}
                              aria-label={t("queue.deleteTrackTitle")}
                              title={t("queue.deleteTrackTitle")}
                              onClick={() => void deleteTrackFile(it)}
                            >
                              <IconTrash />
                            </button>
                          ) : null}
                        </div>
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
            </div>
          </div>
        </aside>
        ) : null}

        {showMiniPlayer ? (
          <MiniPlayerBar
            title={currentTitle}
            paused={!!transport?.paused || !!transport?.eof}
            currentPath={transport?.current_path ?? null}
            onExpand={() => setActiveView("nowPlaying")}
            onToggle={() => void togglePause()}
            onOpenDetails={
              playingCollectionId != null
                ? () => openCollectionDetail(playingCollectionId)
                : undefined
            }
          />
        ) : null}
      </div>

      {modalSheet ? (
        <div
          className="modal-backdrop"
          role="presentation"
          data-modal-open
          onClick={() => setModalSheet(null)}
          onKeyDown={(e) => {
            if (e.key === "Escape") setModalSheet(null);
          }}
        >
          <div
            className={`modal-sheet${
              modalSheet === "preferences" || modalSheet === "sleep" || modalSheet === "addLibrary"
                ? " modal-sheet--prefs"
                : ""
            }${modalSheet === "confirm" ? " modal-sheet--confirm" : ""}`}
            ref={modalSheetRef}
            role={modalSheet === "confirm" ? "alertdialog" : "dialog"}
            aria-modal="true"
            aria-labelledby={
              modalSheet === "shortcuts"
                ? "help-dialog-title"
                : modalSheet === "about"
                  ? "help-about-title"
                  : modalSheet === "preferences"
                    ? "preferences-dialog-title"
                    : modalSheet === "addLibrary"
                      ? "add-library-dialog-title"
                      : modalSheet === "confirm"
                        ? "confirm-dialog-title"
                        : "sleep-dialog-title"
            }
            aria-describedby={modalSheet === "confirm" ? "confirm-dialog-body" : undefined}
            onClick={(e) => e.stopPropagation()}
          >
            {modalSheet === "confirm" && confirmDialog ? (
              <>
                <h2 className="modal-title" id="confirm-dialog-title">
                  {confirmDialog.title}
                </h2>
                <p className="modal-body modal-body--confirm" id="confirm-dialog-body">
                  {confirmDialog.body}
                </p>
              </>
            ) : modalSheet === "shortcuts" ? (
              <>
                <h2 className="modal-title" id="help-dialog-title">
                  {t("shortcuts.title")}
                </h2>
                <ul className="modal-list">
                  <li>
                    <kbd className="kbd">Space</kbd> {t("shortcuts.spaceLine")}
                  </li>
                  <li>
                    <kbd className="kbd">←</kbd> / <kbd className="kbd">→</kbd> {t("shortcuts.seekLine")}
                  </li>
                  <li>
                    <kbd className="kbd">Shift</kbd> + <kbd className="kbd">←</kbd> /{" "}
                    <kbd className="kbd">→</kbd> {t("shortcuts.skipLine")}
                  </li>
                  <li>{t("shortcuts.headphoneLine")}</li>
                  <li>{t("shortcuts.menuLine")}</li>
                </ul>
              </>
            ) : modalSheet === "about" ? (
              <>
                <h2 className="modal-title" id="help-about-title">
                  {t("about.title")}
                </h2>
                <p className="modal-body">{t("about.body")}</p>
              </>
            ) : modalSheet === "preferences" ? (
              <>
                <h2 className="modal-title" id="preferences-dialog-title">
                  {t("prefs.title")}
                </h2>
                <p className="modal-body modal-body--tight">{t("prefs.intro")}</p>
                <div className="modal-prefs">
                  <fieldset className="prefs-section">
                    <legend className="prefs-section-title">{t("prefs.section.general")}</legend>
                    <label className="prefs-row prefs-row--select">
                      <span id="prefs-language-label">{t("prefs.language")}</span>
                      <select
                        className="select prefs-locale-select"
                        value={locale}
                        aria-labelledby="prefs-language-label"
                        onChange={(e) => void changeUiLocale(e.target.value as Locale)}
                      >
                        <option value="en">{t("prefs.lang.en")}</option>
                        <option value="de">{t("prefs.lang.de")}</option>
                      </select>
                    </label>
                  </fieldset>

                  <fieldset className="prefs-section">
                    <legend className="prefs-section-title">{t("prefs.section.playback")}</legend>
                    <label className="prefs-row prefs-row--toggle">
                      <input
                        type="checkbox"
                        className="prefs-checkbox"
                        checked={!!appPrefs?.resume_playing_on_launch}
                        disabled={!isTauri() || appPrefs == null}
                        aria-describedby="prefs-resume-hint"
                        onChange={(e) => void setResumePref(e.target.checked)}
                      />
                      <span className="prefs-row-text">
                        <span className="prefs-row-label">{t("prefs.resume")}</span>
                        <span className="hint prefs-hint" id="prefs-resume-hint">
                          {t("prefs.resumeHint")}
                        </span>
                      </span>
                    </label>
                  </fieldset>

                  <fieldset className="prefs-section">
                    <legend className="prefs-section-title">{t("prefs.section.library")}</legend>
                    <label className="prefs-row prefs-row--toggle">
                      <input
                        type="checkbox"
                        className="prefs-checkbox"
                        checked={!!appPrefs?.scan_subfolders}
                        disabled={!isTauri() || appPrefs == null}
                        aria-describedby="prefs-scan-hint"
                        onChange={(e) => void setScanPref(e.target.checked)}
                      />
                      <span className="prefs-row-text">
                        <span className="prefs-row-label">{t("prefs.scan")}</span>
                        <span className="hint prefs-hint" id="prefs-scan-hint">
                          {t("prefs.scanHint")}
                        </span>
                      </span>
                    </label>
                  </fieldset>

                  <fieldset className="prefs-section">
                    <legend className="prefs-section-title">{t("prefs.section.privacy")}</legend>
                    <label className="prefs-row prefs-row--toggle">
                      <input
                        type="checkbox"
                        className="prefs-checkbox"
                        checked={!!appPrefs?.online_metadata_enabled}
                        disabled={!isTauri() || appPrefs == null}
                        aria-describedby="prefs-metadata-hint"
                        onChange={(e) => void setOnlineMetadataPref(e.target.checked)}
                      />
                      <span className="prefs-row-text">
                        <span className="prefs-row-label">{t("prefs.onlineMetadata")}</span>
                        <span className="hint prefs-hint" id="prefs-metadata-hint">
                          {t("prefs.onlineMetadataHint")}
                        </span>
                      </span>
                    </label>
                  </fieldset>

                  <fieldset className="prefs-section prefs-section--speed">
                    <legend className="prefs-section-title">{t("prefs.section.speed")}</legend>
                    <p className="hint prefs-hint prefs-section-lead">{t("prefs.speedDefaultsHint")}</p>
                    <label className="prefs-speed-row">
                      <span className="prefs-speed-label">{t("prefs.speedAudiobook")}</span>
                      <input
                        type="range"
                        className="slider slider--speed"
                        min={0.5}
                        max={4}
                        step={0.05}
                        value={appPrefs?.default_speed_audiobook ?? 1.5}
                        disabled={!isTauri() || appPrefs == null}
                        onChange={(e) =>
                          void setPlaybackSpeedDefaults(
                            Number(e.target.value),
                            appPrefs?.default_speed_music ?? 1,
                          )
                        }
                      />
                      <span className="prefs-speed-readout" aria-live="polite">
                        {(appPrefs?.default_speed_audiobook ?? 1.5).toFixed(2)}×
                      </span>
                    </label>
                    <label className="prefs-speed-row">
                      <span className="prefs-speed-label">{t("prefs.speedMusic")}</span>
                      <input
                        type="range"
                        className="slider slider--speed"
                        min={0.5}
                        max={4}
                        step={0.05}
                        value={appPrefs?.default_speed_music ?? 1}
                        disabled={!isTauri() || appPrefs == null}
                        onChange={(e) =>
                          void setPlaybackSpeedDefaults(
                            appPrefs?.default_speed_audiobook ?? 1.5,
                            Number(e.target.value),
                          )
                        }
                      />
                      <span className="prefs-speed-readout" aria-live="polite">
                        {(appPrefs?.default_speed_music ?? 1).toFixed(2)}×
                      </span>
                    </label>
                  </fieldset>
                </div>
              </>
            ) : modalSheet === "addLibrary" ? (
              <>
                <h2 className="modal-title" id="add-library-dialog-title">
                  {t("addLibrary.title")}
                </h2>
                <p className="modal-body modal-body--tight">{t("addLibrary.body")}</p>
                <div className="add-library-actions">
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={!isTauri()}
                    onClick={() => void linkLibraryFolder()}
                  >
                    {t("library.linkFolder")}
                  </button>
                </div>
              </>
            ) : (
              <>
                <h2 className="modal-title" id="sleep-dialog-title">
                  {t("sleep.modalTitle")}
                </h2>
                <div className="sleep-card sleep-card--modal" aria-label={t("sleep.cardAria")}>
                  <div className="speed-card-head">
                    <span className="field-label">{t("sleep.status")}</span>
                    {sleepRemainLabel ? (
                      <div className="speed-readout" aria-live="polite">
                        {sleepRemainLabel}
                      </div>
                    ) : (
                      <span className="speed-readout speed-readout--dim">{t("sleep.off")}</span>
                    )}
                  </div>
                  <div className="sleep-row">
                    <label className="sr-only" htmlFor="sleep-preset">
                      {t("sleep.minutesLabel")}
                    </label>
                    <select
                      id="sleep-preset"
                      className="select sleep-select"
                      value={sleepPreset}
                      onChange={(e) => setSleepPreset(e.target.value)}
                    >
                      <option value="off">{t("sleep.preset.off")}</option>
                      <option value="15">{t("sleep.preset.15")}</option>
                      <option value="30">{t("sleep.preset.30")}</option>
                      <option value="45">{t("sleep.preset.45")}</option>
                      <option value="60">{t("sleep.preset.60")}</option>
                      <option value="90">{t("sleep.preset.90")}</option>
                    </select>
                    <button
                      className="btn btn-secondary"
                      type="button"
                      disabled={sleepPreset === "off" || sleepDeadlineMs != null}
                      onClick={() => startSleep()}
                    >
                      {t("sleep.start")}
                    </button>
                    <button
                      className="btn btn-ghost"
                      type="button"
                      disabled={sleepDeadlineMs == null}
                      onClick={() => cancelSleep()}
                    >
                      {t("sleep.cancel")}
                    </button>
                  </div>
                  <label className="prefs-row sleep-stop-row">
                    <input
                      type="checkbox"
                      checked={stopAfterTrackUi}
                      onChange={(e) => {
                        setStopAfterTrackUi(e.target.checked);
                        stopAfterTrackRef.current = e.target.checked;
                      }}
                    />
                    <span>{t("sleep.stopAfter")}</span>
                  </label>
                  <p className="hint">{t("sleep.hint")}</p>
                </div>
              </>
            )}
            {modalSheet === "confirm" && confirmDialog ? (
              <div className="modal-actions">
                <button
                  className="btn btn-ghost"
                  type="button"
                  ref={modalCloseRef}
                  onClick={() => closeConfirm()}
                >
                  {t("modal.cancel")}
                </button>
                <button
                  className={`btn ${confirmDialog.danger ? "btn-danger" : "btn-primary"}`}
                  type="button"
                  onClick={() => {
                    const action = confirmDialog.onConfirm;
                    closeConfirm();
                    void action();
                  }}
                >
                  {confirmDialog.confirmLabel}
                </button>
              </div>
            ) : (
              <button
                className="btn btn-primary modal-close"
                ref={modalCloseRef}
                type="button"
                onClick={() => setModalSheet(null)}
              >
                {t("modal.close")}
              </button>
            )}
          </div>
        </div>
      ) : null}

      {collectionDetailId != null ? (
        <CollectionDetailSheet
          collectionId={collectionDetailId}
          onlineMetadataEnabled={!!appPrefs?.online_metadata_enabled}
          onClose={() => setCollectionDetailId(null)}
          onPlayCollection={(id, mode) => void playCollection(id, mode)}
          onAddToQueue={(id, position) => void enqueueCollection(id, position)}
          onChanged={onCollectionDetailChanged}
          openConfirm={openConfirm}
        />
      ) : null}
    </div>
  );
}
