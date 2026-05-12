import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "./i18n/I18nContext";
import type { Locale } from "./i18n/types";
import { normalizeLocale } from "./i18n/types";

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
};

type RecentOpenDto = {
  path: string;
  kind: string;
  label: string;
};

type AppPrefsDto = {
  resume_playing_on_launch: boolean;
  scan_subfolders: boolean;
  ui_locale: string;
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
  }
  return true;
}

type UiAction =
  | "view.player"
  | "view.queue"
  | "help.shortcuts"
  | "help.about"
  | "app.preferences"
  | "playback.sleep_timer";

function isUiActionPayload(v: unknown): v is { action: UiAction } {
  if (!v || typeof v !== "object") return false;
  const a = (v as { action?: unknown }).action;
  return (
    a === "view.player" ||
    a === "view.queue" ||
    a === "help.shortcuts" ||
    a === "help.about" ||
    a === "app.preferences" ||
    a === "playback.sleep_timer"
  );
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

export default function App() {
  const [playlist, setPlaylist] = useState<PlaylistDto | null>(null);
  const [transport, setTransport] = useState<TransportDto | null>(null);
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
    null | "shortcuts" | "about" | "preferences" | "sleep"
  >(null);
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
    try {
      const t = await invoke<TransportDto>("get_transport");
      setTransport(t);
    } catch (e) {
      setError(String(e));
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
      setAppPrefs({ resume_playing_on_launch: false, scan_subfolders: false, ui_locale: "en" });
    }
  }, []);

  useEffect(() => {
    void loadAppPrefs();
  }, [loadAppPrefs]);

  const scrollToPlayer = useCallback(() => {
    const el = mainStageRef.current;
    if (!el) return;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    el.scrollIntoView({ behavior: reduce ? "auto" : "smooth", block: "start" });
    window.setTimeout(() => el.focus(), reduce ? 0 : 280);
  }, []);

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
          void loadAppPrefs();
          setModalSheet("preferences");
          break;
        case "playback.sleep_timer":
          setModalSheet("sleep");
          break;
        default:
          break;
      }
    },
    [scrollToPlayer, scrollToQueue, loadAppPrefs],
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
  }, [loadRecent]);

  useEffect(() => {
    void refreshTransport();
    const id = window.setInterval(() => {
      void refreshTransport();
    }, transportPollMs);
    return () => window.clearInterval(id);
  }, [refreshTransport, transportPollMs]);

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
      listen<unknown>("abp:user-error", (ev) => {
        if (!mounted) return;
        const msg = ev.payload;
        if (typeof msg === "string") setError(msg);
      }),
      listen<unknown>("abp:ui-action", (ev) => {
        if (!mounted) return;
        if (!isUiActionPayload(ev.payload)) return;
        applyUiAction(ev.payload.action);
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
  }, [refreshTransport, applyUiAction, loadRecent]);

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
    const onKeyDown = (ev: KeyboardEvent) => {
      if (modalSheet) return;
      const target = ev.target as HTMLElement | null;
      const tag = target?.tagName?.toLowerCase();
      if (tag === "input" || tag === "select" || tag === "textarea" || target?.isContentEditable) {
        return;
      }
      if (ev.code === "Space") {
        ev.preventDefault();
        void invoke("toggle_pause")
          .then(() => refreshTransport())
          .catch((e) => setError(String(e)));
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [refreshTransport, modalSheet]);

  const openFolder = async () => {
    setBusy(t("busy.openingFolder"));
    setError(null);
    try {
      const dto = await invoke<PlaylistDto | null>("pick_open_folder");
      if (dto) setPlaylist(dto);
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
      void loadRecent();
    }
  };

  const openFile = async () => {
    setBusy(t("busy.openingFile"));
    setError(null);
    try {
      const dto = await invoke<PlaylistDto | null>("pick_open_file");
      if (dto) setPlaylist(dto);
      await refreshTransport();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
      void loadRecent();
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
      if (dto) setPlaylist(dto);
      await refreshTransport();
      await loadRecent();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const clearRecentHistory = async () => {
    if (!isTauri()) return;
    if (
      !window.confirm(t("confirm.clearRecent"))
    ) {
      return;
    }
    setError(null);
    try {
      await invoke("clear_recent_opened");
      await loadRecent();
    } catch (e) {
      setError(String(e));
    }
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
    try {
      const p = await invoke<AppPrefsDto>("set_resume_playing_on_launch", { enabled });
      setAppPrefs(p);
    } catch (e) {
      setError(String(e));
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
  const hasTrack = transport != null && transport.current_index !== null;
  const canSeekTransport = hasQueue && transport != null && !transport.idle;

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
        {t("skip.toPlayback")}
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
                  <button
                    type="button"
                    role="menuitem"
                    className="menubar-item"
                    onClick={() => {
                      setMenuOpen(null);
                      void loadAppPrefs();
                      setModalSheet("preferences");
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
                    disabled={!canSeekTransport}
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
                    disabled={!hasTrack}
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
                    disabled={!hasTrack}
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

      <div className="app-body">
        <aside className="sidebar sidebar--left" aria-label={t("sidebar.library")}>
          <div className="sidebar-brand">
            <img
              className="sidebar-brand-mark"
              src="/app-icon.png"
              width={40}
              height={40}
              alt=""
              decoding="async"
            />
            <div className="sidebar-brand-text">
              <span className="sidebar-brand-name">{t("app.title")}</span>
              <span className="sidebar-brand-tag">{t("app.tagline")}</span>
            </div>
          </div>

          <div className="sidebar-section">
            <h2 className="sidebar-heading">{t("sidebar.library")}</h2>
            <div className="sidebar-actions">
              <button className="btn btn-primary btn-block" type="button" onClick={() => void openFolder()}>
                {t("sidebar.openFolder")}
              </button>
              <button className="btn btn-ghost btn-block" type="button" onClick={() => void openFile()}>
                {t("sidebar.openFile")}
              </button>
            </div>
          </div>

          <div className="sidebar-section">
            <div className="recent-section-head">
              <h2 className="sidebar-heading" id="recent-opened-heading">
                {t("sidebar.recent")}
              </h2>
              <button
                type="button"
                className="btn btn-ghost btn-compact recent-clear"
                aria-label={t("sidebar.recentClearAria")}
                title={t("sidebar.recentClearTitle")}
                disabled={recent.length === 0}
                onClick={() => void clearRecentHistory()}
              >
                <svg className="recent-clear-icon" viewBox="0 0 24 24" aria-hidden="true">
                  <path
                    fill="currentColor"
                    d="M9 3h6a1 1 0 011 1v1h4v2H4V5h4V4a1 1 0 011-1zm1 5h2v10h-2V8zm4 0h2v10h-2V8zM6 8h2v10H6V8zm-1 12a2 2 0 002 2h10a2 2 0 002-2V8H5v12z"
                  />
                </svg>
                <span className="recent-clear-label">{t("sidebar.recentClear")}</span>
              </button>
            </div>
            {recent.length === 0 ? (
              <p className="recent-empty">{t("sidebar.recentEmpty")}</p>
            ) : (
              <ul className="recent-list" aria-label={t("sidebar.recent")}>
                {recent.map((it) => (
                  <li key={`${it.kind}:${it.path}`} className="recent-li">
                    <button
                      type="button"
                      className="recent-item"
                      title={it.path}
                      onClick={() => void reopenRecent(it)}
                    >
                      <span className="recent-kind">
                        {it.kind === "file" ? t("sidebar.recentKind.file") : t("sidebar.recentKind.folder")}
                      </span>
                      <span className="recent-label">{it.label}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>

          {rootDisplay ? (
            <div className="sidebar-section sidebar-section--grow">
              <h2 className="sidebar-heading">{t("sidebar.sessionFolder")}</h2>
              <p className="sidebar-path" title={playlist?.root ?? ""}>
                {rootDisplay}
              </p>
              <p className="path-hint">{t("sidebar.pathHint")}</p>
            </div>
          ) : null}

          <div className="sidebar-foot">
            <p className="sidebar-tip">
              <strong>{t("sidebar.tipLabel")}</strong> {t("sidebar.tipBody")}
            </p>
          </div>
        </aside>

        <main ref={mainStageRef} className="main-stage" id="main-stage" tabIndex={-1}>
          <section className="panel panel-hero panel--stage" aria-labelledby="now-playing-title">
          <div className="panel-header panel-header--hero">
            <h2 className="panel-title" id="now-playing-title">
              {t("nowPlaying.title")}
            </h2>
            <span className={`status-pill status-pill--${statusTone}`} aria-live="polite">
              {liveStatus}
            </span>
          </div>

          <div className="panel-body now-playing">
            <div className="now-hero">
              <div className="now-hero-art" aria-hidden="true">
                <div className="now-hero-art-inner" />
              </div>
              <div className="now-hero-text">
                <div className="meta-label">{t("nowPlaying.currentTitle")}</div>
                <div className="meta-value meta-value--title" id="current-track-name">
                  {currentTitle}
                </div>
              </div>
            </div>

            <div className="progress" aria-label={t("nowPlaying.progressAria")}>
              <div className="progress-row">
                <span className="time-tag" aria-hidden="true">
                  {formatClock(position)}
                </span>
                <span className="progress-divider" aria-hidden="true" />
                <span className="time-tag time-tag--dim" aria-hidden="true">
                  {duration ? formatClock(duration) : t("nowPlaying.timeUnknown")}
                </span>
              </div>
              <input
                className="slider slider--seek"
                aria-valuemin={0}
                aria-valuemax={progressMax}
                aria-valuenow={Math.min(sliderValue, progressMax)}
                aria-label={t("nowPlaying.seekAria")}
                type="range"
                min={0}
                max={progressMax}
                step={0.25}
                value={Math.min(sliderValue, progressMax)}
                disabled={!transport || transport.playlist_len === 0 || transport.idle}
                onPointerDown={() => {
                  setSeekUi(position);
                }}
                onInput={(e) => {
                  setSeekUi(Number(e.currentTarget.value));
                }}
                onPointerUp={(e) => {
                  const v = Number((e.currentTarget as HTMLInputElement).value);
                  setSeekUi(null);
                  void seekTo(v);
                }}
                onPointerCancel={() => {
                  setSeekUi(null);
                }}
              />
            </div>

            <div className="transport" aria-label={t("nowPlaying.transportAria")}>
              <button className="btn icon-btn" type="button" aria-label={t("nowPlaying.prevAria")} onClick={() => void skipPrev()}>
                <IconSkipPrev />
              </button>
              <button
                className="btn btn-skip"
                type="button"
                aria-label={t("nowPlaying.rewindAria")}
                disabled={!canSeekTransport}
                onClick={() => void seekDelta(-30)}
              >
                −30s
              </button>
              <button
                className="btn btn-primary icon-btn icon-btn--play"
                type="button"
                aria-label={transport?.paused ? t("nowPlaying.playAria") : t("nowPlaying.pauseAria")}
                onClick={() => void togglePause()}
              >
                {transport?.paused ? <IconPlay /> : <IconPause />}
              </button>
              <button
                className="btn btn-skip"
                type="button"
                aria-label={t("nowPlaying.forwardAria")}
                disabled={!canSeekTransport}
                onClick={() => void seekDelta(30)}
              >
                +30s
              </button>
              <button className="btn icon-btn" type="button" aria-label={t("nowPlaying.nextAria")} onClick={() => void skipNext()}>
                <IconSkipNext />
              </button>
            </div>

            {chapters.length > 0 ? (
              <div className="chapters-card" aria-label={t("chapters.title")}>
                <div className="speed-card-head">
                  <span className="field-label">{t("chapters.title")}</span>
                  <span className="chapters-badge">{chapters.length}</span>
                </div>
                <ul className="chapter-list">
                  {chapters.map((ch) => (
                    <li key={`${ch.index}-${ch.time_sec}`}>
                      <button
                        type="button"
                        className="chapter-item"
                        disabled={!canSeekTransport || !!transport?.mpv_error}
                        onClick={() => void seekTo(ch.time_sec)}
                      >
                        <span className="chapter-time">{formatClock(ch.time_sec)}</span>
                        <span className="chapter-title">{ch.title}</span>
                      </button>
                    </li>
                  ))}
                </ul>
                <p className="hint">{t("chapters.hint")}</p>
              </div>
            ) : null}

            <div className="speed-card" aria-label={t("speed.label")}>
              <div className="speed-card-head">
                <label className="field-label" htmlFor="speed-slider">
                  {t("speed.label")}
                </label>
                <div className="speed-readout" aria-live="polite">
                  {transport ? `${transport.speed.toFixed(2)}×` : t("speed.readoutEmpty")}
                </div>
              </div>
              <input
                id="speed-slider"
                className="slider slider--speed"
                type="range"
                min={0.5}
                max={4}
                step={0.05}
                value={transport?.speed ?? 1}
                disabled={!transport}
                onChange={(e) => void setSpeed(Number(e.target.value))}
              />
              <p className="hint">{t("speed.hint")}</p>
              <div className="speed-actions">
                <button
                  className="btn btn-secondary speed-actions-btn"
                  type="button"
                  disabled={!transport || !!transport.mpv_error}
                  onClick={() => void setDefaultSpeed(transport?.speed ?? 1)}
                >
                  {t("speed.saveDefault")}
                </button>
                <button
                  className="btn btn-ghost speed-actions-btn"
                  type="button"
                  disabled={
                    !transport ||
                    !!transport.mpv_error ||
                    (transport.speed >= 0.995 && transport.speed <= 1.005)
                  }
                  onClick={() => void setSpeed(1)}
                >
                  {t("speed.resetOne")}
                </button>
              </div>
            </div>
          </div>
        </section>
        </main>

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
                  return (
                    <li key={it.path} className="playlist-li">
                      <button
                        type="button"
                        className={`playlist-item${active ? " playlist-item--active" : ""}`}
                        aria-current={active ? "true" : undefined}
                        title={it.path}
                        onClick={() => void playIndex(idx)}
                      >
                        <span className="track-idx" aria-hidden="true">
                          {String(idx + 1).padStart(2, "0")}
                        </span>
                        <span className="track-body">
                          <span className="track-row track-row--main">
                            <span className="track-title">{it.label}</span>
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
                    </li>
                  );
                })}
              </ul>
            )}
            </div>
          </div>
        </aside>
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
              modalSheet === "preferences" || modalSheet === "sleep" ? " modal-sheet--prefs" : ""
            }`}
            ref={modalSheetRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby={
              modalSheet === "shortcuts"
                ? "help-dialog-title"
                : modalSheet === "about"
                  ? "help-about-title"
                  : modalSheet === "preferences"
                    ? "preferences-dialog-title"
                    : "sleep-dialog-title"
            }
            onClick={(e) => e.stopPropagation()}
          >
            {modalSheet === "shortcuts" ? (
              <>
                <h2 className="modal-title" id="help-dialog-title">
                  {t("shortcuts.title")}
                </h2>
                <ul className="modal-list">
                  <li>
                    <kbd className="kbd">Space</kbd> {t("shortcuts.spaceLine")}
                  </li>
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
                  <label className="prefs-row prefs-row--select">
                    <span>{t("prefs.language")}</span>
                    <select
                      className="select prefs-locale-select"
                      value={locale}
                      onChange={(e) => void changeUiLocale(e.target.value as Locale)}
                    >
                      <option value="en">{t("prefs.lang.en")}</option>
                      <option value="de">{t("prefs.lang.de")}</option>
                    </select>
                  </label>
                  <label className="prefs-row">
                    <input
                      type="checkbox"
                      checked={!!appPrefs?.resume_playing_on_launch}
                      disabled={!isTauri() || appPrefs == null}
                      onChange={(e) => void setResumePref(e.target.checked)}
                    />
                    <span>{t("prefs.resume")}</span>
                  </label>
                  <label className="prefs-row">
                    <input
                      type="checkbox"
                      checked={!!appPrefs?.scan_subfolders}
                      disabled={!isTauri() || appPrefs == null}
                      onChange={(e) => void setScanPref(e.target.checked)}
                    />
                    <span>{t("prefs.scan")}</span>
                  </label>
                  <p className="hint prefs-hint">{t("prefs.scanHint")}</p>
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
            <button
              className="btn btn-primary modal-close"
              ref={modalCloseRef}
              type="button"
              onClick={() => setModalSheet(null)}
            >
              {t("modal.close")}
            </button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
