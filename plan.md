# ChapterCheck (Linux Mint) — Product & Engineering Plan

## Goals

- **Resume**: Persist accurate playback position per file (incl. 8h+ single files), with periodic and event-driven saves.
- **Speed**: Adjustable playback speed (audiobooks), persisted per file with a sane global default (clamped **0.5×–4.0×**).
- **Library workflow**: Open a **folder** as a playlist; **sort** by name (natural), modified time, or size; optional single-file open.
- **Optional subfolders**: Preference `pref.scan_subfolders` — when enabled, folder scans are **recursive** (still restricted to files whose canonical path stays under the session root). Toggling the pref **rescans** the open folder playlist. The toggle lives in **File → Preferences…** (modal), not the sidebar.
- **Session restore**: Restore last root, sort, last track, and resume position on launch. **Autoplay on launch** is **off by default**; optional preference `pref.resume_playing_on_launch` restores the previous “resume if it was playing when you quit” behavior (uses `session.last_playing`). Same modal as subfolders pref.
- **Queue UX**: Search filter, sort controls above the queue, scrollable queue column (grid row `minmax(0,1fr)`), clearable “recently opened” MRU.
- **mpv resilience**: If IPC fails, `get_transport` surfaces `mpv_error`; UI shows a **Restart playback engine** action (`recover_mpv`) that respawns mpv and reloads the current track **paused** at the saved position.
- **Chapters (M4B / metadata)**: When mpv exposes `chapter-list`, the UI lists chapters and seeks via existing `seek_seconds` (no extra backend beyond `get_chapters`).
- **Sleep timer**: Client-side countdown pauses playback at zero; optional **stop after this track** skips auto-advance on next EOF once. Timer clears when the current file path changes. Configure from **Playback → Sleep timer…** (modal sheet, same pattern as Preferences); a compact countdown chip appears in the menubar while active.
- **Moved-file hint**: In-app note under session folder: progress is keyed by **canonical file path**; renames/moves start a new bookmark (see limitations).
- **Localization**: UI strings in **English** and **German (Germany)** via flat message bundles; `pref.ui_locale` (`en`|`de`) persisted in SQLite, exposed on `AppPrefsDto`. In-app **Preferences → Language** and initial `navigator`/localStorage hint for the web build; **desktop menu bar** labels rebuild when the locale changes (`set_ui_locale`).
- **Dual use**: Optimized for long-form listening; still usable for music (shuffle is intentionally omitted unless added later).

## Stack (chosen)

| Layer | Choice | Rationale |
|-------|--------|-----------|
| Shell | **Tauri 2** | Small native binary, Rust backend, strict IPC, no remote code in renderer by default. |
| Audio engine | **mpv** (JSON IPC over Unix socket) | Excellent codec coverage (MP3, AAC, M4B, FLAC, Opus, …), battle-tested A/V sync, `playback-speed`, precise seek, `chapter-list` when present. |
| Persistence | **SQLite** (`rusqlite` bundled) | Durable, transactional, audit-friendly; single DB under XDG data dir. `app_settings` keys include `pref.resume_playing_on_launch`, `pref.scan_subfolders`, `pref.ui_locale`, session keys, MRU JSON. |
| UI | **React + TypeScript + Vite** | Accessible component patterns, responsive layout, fast dev loop. |

**Runtime dependency**: `mpv` must be on `PATH` (Linux Mint: `sudo apt install mpv`). Override with `MPV_PATH` if needed.

## Security model (threat-aware)

1. **No shell** when spawning mpv: argument vector only; no string interpolation into `sh -c`.
2. **Path handling**: Every audio path is `canonicalize()`d; playback is only allowed for files **under the current session root** (folder open) or the **explicit file** opened via the dialog flow (parent directory becomes root). Recursive scans additionally require each file’s canonical path to **start with** the canonical session root. This blocks drive-by `invoke('play', '/etc/passwd')` even if the webview were compromised, unless the attacker can also plant files under an already-selected root (local same-user threat).
3. **CSP** (Tauri): restrictive `default-src 'self'`; no remote scripts; no inline except controlled styles where required.
4. **Secrets**: None stored; no network stack in v1.
5. **Updates**: When adding auto-update or online metadata, pin TLS, signature-check, and supply-chain audit.

**Known limitations (document for auditors)**

- **Moved/renamed files**: State is keyed by canonical path; a rename appears as a new file (position not migrated). Mitigation later: optional content fingerprint (costly) or manual “link previous file”.
- **Symlinks**: `canonicalize` resolves them; ACL is based on resolved path under session root.
- **XSS in UI**: Treated as high impact; mitigated by React escaping, no `dangerouslySetInnerHTML`, strict CSP.

## UX / WCAG 2.1 AA

- **Landmarks**: `header`, `main`, `aside`, `nav` (transport), **skip link** to main content.
- **Contrast**: Body text vs surfaces ≥ 4.5:1; large controls ≥ 3:1 where applicable.
- **Focus**: Visible 2px focus ring, `:focus-visible` only for keyboard users.
- **Targets**: Controls sized ≥ 44×44 CSS px where feasible.
- **Keyboard**: Space play/pause when a field is not focused and no modal sheet is open; `?` shortcuts panel (optional phase 2); arrow seek (optional phase 2).
- **Screen readers**: `aria-live="polite"` for now playing / errors / sleep countdown; buttons have explicit `aria-label` where icon-only.
- **Motion**: `prefers-reduced-motion` respected for transitions.
- **Responsive**: Playlist collapses below breakpoint; minimum widths avoided where possible; horizontal scroll only for long filenames with ellipsis + `title` tooltip.

## Workflows & edge cases

| Scenario | Behaviour |
|----------|-----------|
| Crash / kill app | Last periodic save (≤10s drift) + save on pause/seek/speed/stop/close. |
| End of file | Auto-advance per **repeat mode** (`off` / `one` / `all`); **Stop after this track** still skips `advance_after_eof` once when set. |
| Near-end resume | If `position >= max(duration−2s, 0)` and duration known, **restart from 0** to avoid useless 2s of audio. |
| Unknown duration | Until mpv reports `duration`, treat as unknown; skip near-end logic. |
| mpv missing | Clear error banner + log; no panic. |
| mpv dies mid-play | IPC errors → `mpv_error` on transport; user can **Restart playback engine** (`recover_mpv`). |
| Empty / non-audio folder | Empty playlist message; no crash. |
| Huge folders | `read_dir` + optional recursion; extensions filter; paths confined under session root. |
| Very long path / unicode | Rust `Path` + lossy display only for UI; DB stores UTF-8 path string from canonical path. |
| Speed bounds | Clamped **0.5×–4.0×**. |
| Resume on launch pref off (default) | Session restore loads last track **paused** even if `session.last_playing` was true. |
| Resume on launch pref on | Restore uses `session.last_playing` to decide autoplay after launch. |
| Sleep timer | Countdown then `set_paused(true)`; cleared when `current_path` changes. |
| Chapters | Shown when `chapter-list` non-empty; seek jumps to chapter `time`. |
| Multiple windows | Single main window; state is global singleton in process. |

## IPC commands (Rust → UI)

| Command | Role |
|---------|------|
| `get_app_prefs` / `set_resume_playing_on_launch` / `set_scan_subfolders` / `set_ui_locale` | Preferences (`AppPrefsDto` includes `ui_locale`; scan toggle may refresh playlist). |
| `abp:ui-action` (`app.preferences`) | Opens the preferences modal (from the native **File → Preferences…** menu). |
| `recover_mpv` | Respawn mpv and reload current index paused. |
| `get_chapters` | Parse mpv `chapter-list` → `ChapterDto[]`. |
| `shuffle_playlist` | Fisher–Yates shuffle of in-memory queue; keeps the playing file; sets `PlaylistDto.shuffled` until the user picks a sort again (`resort_playlist` clears it). |
| `set_repeat_mode` | `off` \| `one` \| `all`: **one** loops the current track on EOF (seek 0); **all** loops the queue (`advance_after_eof` and prev/next wrap); **off** is default. |
| `get_transport` | Includes `repeat_mode` string for the queue repeat button. |

## Testing checklist (for you & the auditor)

- [ ] Open folder with mixed files → only audio extensions listed.
- [ ] Sort each mode → order correct (natural name).
- [ ] Play → pause → close app → reopen → resume position (with pref off: paused at position).
- [ ] Enable resume-on-launch → close while playing → reopen → plays if pref + last_playing agree.
- [ ] Change speed → switch file → per-file speed restored.
- [ ] Last file ends → auto-stop or advance per setting; stop-after-track prevents advance once.
- [ ] Remove mpv from PATH → friendly error; kill mpv → recover banner → restart engine.
- [ ] Keyboard-only navigation through controls.
- [ ] Zoom 200% layout still usable.
- [ ] Subfolder pref off/on with nested audio → count changes; path ACL still holds.
- [ ] Switch language to Deutsch → menus, modals, and native menubar (Tauri) match German strings.
- [ ] Shuffle queue → order changes, current track still plays; pick a sort → deterministic order again.
- [ ] Repeat one → track restarts from beginning at EOF; repeat all → last track advances to first; prev from first goes to last when repeat all.

## Build

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # if needed
sudo apt install mpv libwebkit2gtk-4.1-dev libssl-dev build-essential curl wget file libayatana-appindicator3-dev librsvg2-dev patchelf
cd /home/alex/Development/audioplayer
npm install
npm run tauri dev
```

Release: `npm run tauri build` (`.deb` on Linux).
