# ChapterCheck — Risk & Coverage Inventory

**App:** `audioplayer` (product name ChapterCheck v0.1.0)  
**Path:** `/home/alex/Development/audioplayer`  
**Date:** 2026-08-30  
**Environment:** Native (no `docker-compose.yml`). Commands run on the host. GTK/WebKit `-dev` packages were extracted to `/tmp/cc-deps/root` for `pkg-config` only.  
**Auditor persona:** Momos (hostile QA)

## Purpose (verified against code, not README)

Local Linux desktop player for audiobooks and music. Playback is **mpv** over a Unix-socket JSON IPC. Library, resume position, speed, playlists, and collections live in **SQLite** (`~/.local/share/chaptercheck/library.sqlite3` via the `directories` crate). The UI is a Tauri v2 webview (React). There is **no** ChapterCheck server, **no** user accounts, **no** multi-tenant API.

README is a getting-started guide and points at this QA folder. It is **not** the spec.

## Actors & stakes

| Actor | What goes wrong if the app lies |
|-------|----------------------------------|
| Listener (single OS user) | Resume restarts a 20-hour book; sleep timer fails and audio keeps playing; delete removes the wrong file; a scan of `/` or `$HOME` hangs the machine; playlist order is scrambled |
| Same OS user, malicious file / compromised webview | mpv plays arbitrary session files; webview reads files via `asset:` if the protocol scope is too wide; outbound HTTP leaves Open Library / MusicBrainz; unbounded labels hitch the UI |
| Nobody else | There is no User B. BOLA/IDOR across accounts **does not apply**. Residual is local IPC + filesystem + SSRF. |

## Auth / API surface

| Kind | Reality |
|------|---------|
| Multi-user auth | **None** — no login, JWT, sessions, or roles |
| HTTP APIs served by this app | **None** |
| IPC “API” | **84** `#[tauri::command]` handlers, identical in `generate_handler!`, `ipc_allowlist.rs`, and `capabilities/default.json` (tested). Unused helpers are not IPC. |
| External HTTPS | Open Library search + MusicBrainz WS/2 (allow-listed; redirects disabled) |
| Secrets | None. No API keys. |

OWASP API BOLA/IDOR (cross-user): **N/A**. Residual mapped below. Negative-role tests do not apply.

## Invariants (code-derived — this is what tests are for)

1. Playback paths must be in the current session `allowed_files` set (canonical).
2. Delete of a file on disk is allowed only under a **registered, currently available** library root.
3. A library root must be a real directory and must **not** be `/`, `$HOME` (exact), kernel trees (`/proc|/sys|/dev` and under), `/etc|/boot|/run|/root|/usr|/var` (prefix), or exact `/home|/opt|/tmp|/media|/mnt`. Nested folders (e.g. `~/Audiobooks`, `/tmp/cc_*`) remain allowed.
4. Adding a **file** that is not under a linked root must **not** register the parent folder and scan it.
5. Importing a folder into a playlist must only add files **under that folder**, even if the folder name contains `_` or `%`.
6. Outbound HTTP may only hit the Open Library / MusicBrainz prefixes, and **must not follow redirects**.
7. MusicBrainz Lucene queries must treat user titles as literals (including the words AND/OR/NOT).
8. “Continue” on a collection starts at the first **unfinished** track in order (listened **or** within 2s of duration counts as finished).
9. Resume of a single file rewinds to 0 if position is within 2s of duration.
10. Pagination `limit` for collection/metadata lists is capped (1–200).
11. Playlist names cannot be empty or longer than 200 characters.
12. Collection titles cannot be empty or longer than 500 characters; missing collection id is an error. Author/narrator/artist/album/series: omitted = keep, empty string = store NULL (clear). `series_index` must be `0..=10000` when set.
13. Library-root labels cannot be longer than 500 characters.
14. Embedded cover art written to disk is capped at 8 MiB.
15. DB export uses SQLite’s backup API, not a raw copy of a live WAL file.
16. Cover `asset:` URLs must not be able to fetch arbitrary `$HOME` files.
17. Two connections to the same WAL DB must be able to write without a hard `SQLITE_BUSY` (8s busy_timeout).
18. Sleep deadline is owned by Rust (SQLite + watchdog). Webview display cannot be the only timer.
19. Catalog mutex must not be held across mpv IPC. Passive peeks use a short timeout.
20. `reopen_recent` path must be in the stored recent list.
21. Collection title search treats `%` and `_` as literals (`LIKE` + `ESCAPE`).
22. Playlist reorder must be a **full unique permutation** of that playlist’s items, applied in a transaction.
23. Home and catalog list responses apply only if their request id is still the latest (stale IPC is dropped).
24. Sleep preset succeeds only when `set_sleep_timer` returns a finite deadline `> 0`. Double-taps must not fire two IPCs.

## Workflow inventory (severity)

| Workflow | Critical / High risks | Coverage after this audit |
|----------|----------------------|---------------------------|
| Open file/folder / recent | Any path from IPC (`reopen_recent`) | **Tests + fix** (`recent_path_requires_stored_membership`) |
| Library root add/scan | `/`, `$HOME`, OS trees; unbounded label; parent-of-file as root | **Tests + fix** |
| Import folder → playlist | LIKE `_`/`%` over-match | **Tests + fix** |
| Play / continue / resume | Continue skipped last chapter; near-EOF rewind | **Tests + fix** |
| Queue remove | Index after removing current last item | **Test** |
| Playlist reorder | Partial ID list scrambles `track_order` | **Tests + fix** |
| Delete file | Must be under available root | **Test + fix** |
| Online metadata | Redirect SSRF; Lucene operators | **Tests + fix** |
| Sleep timer | Frontend-only timer; double-tap; close sheet on `null` deadline | **Tests + fix** |
| Export DB | WAL torn copy | **Test + fix** |
| Covers in webview | `$HOME/**` asset scope | **Config fix** + Vitest |
| Collection metadata | Blank / overlong / missing id Ok; empty author could not clear; unbounded `series_index` | **Tests + fix** (empty optional fields → NULL; index `0..=10000`) |
| Home / catalog load | Stale IPC overwrite; error hides library; no retry | **Vitest journeys + generation id** |

## Shared-state / concurrency candidates

- `Mutex<InnerState>` and `Mutex<MpvController>` are **separate**. Lock order: **inner, then mpv**. `get_transport` peeks mpv, drops, then takes inner.
- Passive mpv I/O: 400ms. Command I/O: 8s.
- `scan_in_progress` AtomicBool serializes sidecar scans; **playback commands do not check it**.
- Sidecar `LibraryDb` on the same `library.sqlite3` (WAL + 8s busy_timeout). Covered by `wal_two_connections_can_write_without_busy`.
- Sleep: watchdog thread 500ms; claim is CAS; `sleep_hold` blocks EOF auto-advance.
- Playlist reorder: SQLite transaction (this pass).
- mpv socket `chaptercheck-mpv-{pid}.sock`.

## External dependency failure modes

| Dependency | Failure |
|------------|---------|
| mpv | Spawn error surfaced; `MPV_PATH` override. No mpv → no playback. |
| Open Library / MusicBrainz | 12s timeout, 6s connect; 503 retry once for MusicBrainz; non-2xx is an error; redirects off |
| SQLite file | WAL; sidecar scans; backup API for export |
| WebKit / GTK | Process-level; not unit-tested |

## Existing suite (real, not advertised)

| When | Count |
|------|-------|
| Before this audit | **13** unit tests |
| After earlier passes | **47** unit tests |
| After this Momos pass | **51** unit tests, all green (see `test-execution-log.md`) |
| Skipped / ignored | **none** |
| Frontend tests | **11** Vitest (`sleepDisplay`, `coverUrl`, IPC, `playbackIntent`, axe-core on nav + card) |
| E2E against the GUI | **none** |
| Coverage | `cargo llvm-cov --offline --summary-only` after 51 tests: **31.42%** lines, **28.73%** regions, **21.04%** functions. `path_policy.rs` **93.04%** lines / **100%** functions. `catalog.rs` **38.80%**. `mpv.rs` **9.25%**. `media_controls.rs` **0%**. |
| Mutation | `cargo mutants -f src/path_policy.rs`: **15/15 caught** (2026-08-30T14:30:49Z). Not run on `lib.rs`/`catalog.rs`. |

## Tauri commands (grouped)

Playback/session: `pick_open_folder`, `pick_open_file`, `resort_playlist`, `set_repeat_mode`, `play_index`, `toggle_pause`, `set_paused`, `get_os_media_status`, `seek_seconds`, `seek_delta`, `set_speed`, `set_default_playback_speed`, `set_playback_speed_defaults`, `reset_track_speed_to_default`, `save_progress`, `get_transport`, `get_current_playlist`, `play_collection`, `enqueue_collection`, `play_kind`, `enqueue_kind`, `remove_queue_item`, `play_playlist`, `advance_after_eof`, `skip_next`, `skip_prev`, `get_recent_opened`, `clear_recent_opened`, `reopen_recent`, `recover_mpv`, `set_track_listened`, `mark_session_listened`, `delete_track_file`, `delete_session_files`, `get_chapters`, `set_sleep_timer`, `set_stop_after_track`.

Library: `pick_library_folder`, `list_library_roots`, `add_library_root`, `remove_library_root`, `scan_library_root`, `refresh_library_roots`, `export_db`. (`add_to_library` / `update_library_root` / `get_scan_status` are **not** IPC.)

Playlists / collections / metadata / prefs: remaining commands in `ipc_allowlist.rs` (**84** total).

**Who may call them:** the webview, and only the 84 allow-listed names. The equivalent negative test is “JS that is not the UI still can call the 84,” and that remains **true**.

## OWASP API checklist (mapped)

| Item | Status |
|------|--------|
| 1 BOLA/IDOR | N/A (no User B). Residual: path/ID in IPC (`reopen_recent`, delete, relink, reorder) — tested where we had a failing case. |
| 2 Broken auth | N/A (no login). |
| 3 Property-level | Mass-assign `content_kind` / metadata — kinds validated; labels now capped; `series_index` still unbounded (Medium residual). |
| 4 Resource consumption | Pagination clamped; cover 8 MiB; sleep minutes ranged; speed clamped 0.5–4; bulk kind batch 5000; playlist reorder now full-list only. Scan of a huge **legal** folder is still unbounded (user chose it). |
| 5 Function-level | No roles. Extra IPC removed. |
| 6 Sensitive flows | Delete/import/scan from webview — desktop trust. |
| 7 SSRF | Allow-list + no redirects. |
| 8 Misconfig | CSP tightened; asset scope covers-dir only. |
| 9 Injection | LIKE escaped; Lucene quoted; rusqlite params. |
| 10 Inventory | 84 commands; allow-list tests; dead helpers not IPC. |
| 11 Session | N/A cookies. |
| 12 Schema | Tauri deserialize; extra fields typically ignored by serde; not a JSON schema contract test. |
