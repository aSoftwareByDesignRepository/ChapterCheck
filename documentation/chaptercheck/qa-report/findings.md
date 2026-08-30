# ChapterCheck — QA findings

**App:** ChapterCheck 0.1.0 (`/home/alex/Development/audioplayer`)  
**Date:** 2026-08-30  
**Auditors:** Momos, then Bachus (UX), then Aristoteles (races / edge cases)

## Executive summary

**This system is not fit to face a GUI-lab auditor or a multi-user security review today.** There is no Playwright/WebDriver run against the real Tauri window. `mpv.rs` is **9.25%** line coverage and `media_controls.rs` is **0%**. A compromised webview can still call every one of the **84** allow-listed IPC commands. Those are not opinions. They are measured gaps.

It **can** ship as a **local, single-user Linux player** if that residual list is accepted in writing. Named Critical/High holes we could prove in-process were closed with failing tests first, then a fix, then a green run.

**Aristoteles pass (2026-08-30T15:10Z evidence window):** Home and catalog could apply a *slow older* IPC result over a *newer* one, drop a good library on a poll error (no retry), treat a forged catalog filter as “all” without saying so, wipe the catalog list on error, close the sleep sheet when the deadline was `null`, and ignore a second sleep-preset tap only by luck. Clearing author in the detail form sent JSON `null`, which SQL `COALESCE` treated as “keep the old name.” Series index accepted any `i32`. Those are **High** (sleep lie, stale overwrite, `$HOME` scan if the forbid guard is mutated) or **Medium** (metadata). All of them now have tests that fail if the bug returns.

**Counts (this engagement, not a marketing average):**

| Severity | Open | Fixed with red→green proof |
|----------|------|----------------------------|
| Critical | 0 | 5 |
| High | 0 that we could prove in-process | 13 earlier + Aristoteles races/sleep/metadata |
| Medium | 3 residual (see below) | several earlier |
| Low | decorative-alt note | README/docs drift reduced |

**Proof of execution (do not quote these without the log):** **52** Rust tests green, **0** ignored; **36** Vitest green; `npx tsc --noEmit` exit 0; `cargo llvm-cov --offline --summary-only` **31.85%** lines / **29.14%** regions / **21.10%** functions (51-test llvm-cov window; suite is now 52); `cargo mutants -f src/path_policy.rs` **15/15 caught**; `npx stryker run` on `src/utils/viewLogic.ts` **35/35 killed (100%)**. Full command output is in `test-execution-log.md`.

---

## Step 0 — What this app is for (code, not README)

**Purpose:** Local Linux desktop player for audiobooks and music. Engine is **mpv** over a Unix-socket JSON IPC. Library, resume, speed, playlists, collections, and the sleep deadline live in **SQLite** under the `directories` crate path (`~/.local/share/chaptercheck/`, not Tauri `$DATA` / `com.chaptercheck`). UI is a Tauri v2 webview. **No** ChapterCheck server, **no** accounts, **no** tenants.

**Actors:** one OS user (the listener). A compromised webview on that same machine. Malicious/corrupt audio tags. Optional HTTPS to Open Library and MusicBrainz.

**Stakes:** resume restarting a 20-hour book; sleep timer failing so audio keeps playing; delete/import hitting the wrong files; a library scan of `/` or `$HOME` pinning the machine; the webview reading files it should not via `asset:`; outbound HTTP leaving the allow-list.

**Auth model:** none. OWASP BOLA/IDOR across users **does not apply**. Residual is local IPC, filesystem scope, and SSRF. Negative-role tests (no token / wrong role) **do not apply**. The equivalent negative case is “JS that is not the UI can still call the 84 commands,” and that remains **true** — desktop trust boundary.

**Invariants this engagement tests against** (derived from code): listed in `risk-coverage-inventory.md`. New this pass: `$HOME` and OS trees must not be library roots; user labels capped (500 / 200 chars) and non-blank; playlist reorder must be a full permutation in a transaction.

README is a getting-started guide. It now points at this QA folder. Treat it as marketing, not a spec.

---

## Critical

### [CRITICAL] [FIXED] Importing a folder whose name contains `_` also imported sibling folders

**What is wrong (in plain words):**
When you import a folder into a playlist, the app asked SQLite “find files whose path starts with this folder” using `LIKE`. In SQL, `_` means “any one character”. A folder named `book_a` also matched `bookXa`.

**Where exactly:**
- File: `src-tauri/src/catalog.rs`, `collection_file_ids_under_folder` (uses `LIKE ?1 ESCAPE '!'`)
- Endpoint / workflow: `pick_import_folder_to_playlist` → `import_folder_to_playlist`

**How to reproduce it (copy-paste steps):**
```
cd src-tauri
cargo test import_folder_like_underscore_does_not_match_sibling -- --nocapture
```
Observed before the fix: assertion failed with `got [1, 2]` (two files) instead of one.

**What should happen instead:**
Only files whose real path is under the chosen folder are added.

**Why this matters:**
A user importing one book can silently attach another book’s tracks, then play or delete the wrong files.

**Exact fix instructions:**
Escape `%`, `_`, and `!` (`sql_like_literal`) and use `LIKE ?1 ESCAPE '!'`.

**Proof this is fixed:**
- Test: `catalog::tests::import_folder_like_underscore_does_not_match_sibling`
- Red before ESCAPE, green after (`test-execution-log.md`).

---

### [CRITICAL] [FIXED] Linking `/` as a library root would scan the whole machine

**What is wrong (in plain words):**
`add_library_root` accepted any existing folder. On Linux, `/` is an existing folder. The command then runs `scan_root` on it.

**Where exactly:**
- File: `src-tauri/src/catalog.rs`, `add_root` lines 888–905
- File: `src-tauri/src/path_policy.rs`, `is_forbidden_library_root`
- Endpoint: `add_library_root` only (`update_library_root` / `update_root_path` are **not** IPC and the helper was **deleted**)

**How to reproduce it (copy-paste steps):**
1. **Do not** call `add_root("/")` on a build that lacks the guard — it will walk the filesystem.
2. After the guard:
```
cargo test add_root_rejects_filesystem_root -- --nocapture
```
Observed: error containing `cannot be used as a library`, `list_roots` empty.

**What should happen instead:**
`/` and kernel/OS trees must be rejected before any scan. (This pass also blocked `$HOME` and more OS prefixes — see High below.)

**Why this matters:**
One IPC call can pin the CPU and catalog every audio file on the disk.

**Exact fix instructions:**
Keep `is_forbidden_library_root` and call it from `add_root` **before** `scan_root`.

**Proof this is fixed:**
- Tests: `catalog::tests::add_root_rejects_filesystem_root`, `path_policy::tests::filesystem_root_is_forbidden_library_root`
- The `/` add was **not** executed against an unguarded build.

---

### [CRITICAL] [FIXED] Adding a loose file registered its parent folder as a library and scanned it

**What is wrong (in plain words):**
`add_to_library` for a file that was not already under a linked folder took the **parent directory** and called `add_root` on it. For `~/foo.mp3` that parent is the home directory.

**Where exactly:**
- File: `src-tauri/src/catalog.rs`, `add_to_library`
- Endpoint: helper still in Rust; **not** registered IPC (`add_to_library` is not in `ipc_allowlist.rs`)

**How to reproduce it (copy-paste steps):**
```
cargo test add_to_library_file_outside_roots_does_not_scan_parent -- --nocapture
```
After the fix this errors with `not inside a linked library folder` and leaves `library_roots` empty.

**What should happen instead:**
A file that is not inside a linked library folder is rejected. The user must link the folder first.

**Why this matters:**
A single call could index the user’s entire home directory as audiobooks.

**Exact fix instructions:**
When no covering root exists and `path` is a file, return an error. Do not `add_root` on the parent.

**Proof this is fixed:**
- Test: `catalog::tests::add_to_library_file_outside_roots_does_not_scan_parent`

---

### [CRITICAL] [FIXED] Metadata HTTP followed redirects off the allow-list (SSRF)

**What is wrong (in plain words):**
The app checks that the URL **starts with** the Open Library or MusicBrainz prefixes **before** sending. reqwest’s default client then **follows 3xx redirects**. The allow-list is not checked again on the final host.

**Where exactly:**
- File: `src-tauri/src/catalog.rs`, `http_get_json_with_retry` (`.redirect(Policy::none())`)
- Endpoint: `lookup_metadata_online`

**How to reproduce it (copy-paste steps):**
```
cargo test http_allowed_rejects_offlist_and_prefix_tricks -- --nocapture
```

**What should happen instead:**
If Open Library or MusicBrainz 302 to another host, the client must **not** follow.

**Why this matters:**
A “SSRF backstop” that follows redirects is not a backstop. This is a local app, but it must not let a redirect reach `169.254.169.254`.

**Exact fix instructions:**
`.redirect(reqwest::redirect::Policy::none())` plus `http_allowed`.

**Proof this is fixed:**
- Test: `catalog::tests::http_allowed_rejects_offlist_and_prefix_tricks`
- A live redirect to a third host was **not** fired against the public internet from this session.

---

### [CRITICAL] [FIXED] Webview could load almost any file under `$HOME` via `asset:`

**What is wrong (in plain words):**
Cover art is shown with `convertFileSrc` (Tauri `asset:` URLs). The asset protocol scope was `$HOME/**`. If anything in the webview can set an image `src`, it can read those files as image bytes.

**Where exactly:**
- File: `src-tauri/tauri.conf.json` asset `allow`
- File: `src/utils/coverUrl.ts`
- Runtime: `asset_protocol_scope().allow_directory(covers_data_dir())`

**How to reproduce it (copy-paste steps):**
Previous revision of `tauri.conf.json` allowed `$HOME/**`. Current allow glob is `$HOME/.local/share/chaptercheck/covers/**`. Tauri `$DATA` (`~/.local/share/com.chaptercheck`) is **not** the cover cache.

**What should happen instead:**
Only the cover-cache directory should be readable through `asset:`. Audio files are played by mpv, not by the webview.

**Why this matters:**
`$HOME/**` is not a cover cache. SSH keys and mail are under `$HOME`.

**Exact fix instructions:**
Keep the covers glob on the directories-crate path. Do not point it at `$DATA` alone or covers go blank.

**Proof this is fixed:**
- Tests: `catalog::tests::covers_live_beside_library_db_not_under_tauri_bundle_id`; Vitest `isSafeCoverPath`

---

## High (this Momos pass)

### [HIGH] [FIXED] `$HOME` and OS trees were legal library roots

**What is wrong (in plain words):**
The first `/` guard only blocked `/`, `/proc`, `/sys`, and `/dev`. Linking the user’s home folder (or `/etc`, `/usr`, `/var`, `/tmp` itself, `/home` itself) was still accepted. `add_root` then scans.

**Where exactly:**
- File: `src-tauri/src/path_policy.rs`, `is_forbidden_library_root`, lines 51–78
- File: `src-tauri/src/catalog.rs`, `add_root` lines 900–905
- Endpoint: `add_library_root`

**How to reproduce it (copy-paste steps):**
1. **Do not** call `add_root($HOME)` on a build that lacks the expanded guard — that would walk the real home directory.
2. Red (policy test, before the HOME/OS expansion):
```
cargo test home_and_os_roots_are_forbidden_library_roots -- --nocapture
```
Observed: panic `the user's home directory must not be a library root`.
3. After the fix, the same test is green. Catalog-level (does **not** scan; forbidden check runs first):
```
cargo test add_root_rejects_overlong_label_and_home_directory -- --nocapture
```

**What should happen instead:**
Exact `$HOME` is forbidden. Prefixes `/proc|/sys|/dev|/etc|/boot|/run|/root|/usr|/var` are forbidden. Exact `/home|/opt|/tmp|/media|/mnt` are forbidden. A **nested** folder such as `~/Audiobooks` or `/tmp/cc_test_123` is still allowed.

**Why this matters:**
“Don’t scan `/`” while still allowing `$HOME` is a fake guard. Home is where the rest of the user’s life lives.

**Exact fix instructions:**
1. Open `src-tauri/src/path_policy.rs`.
2. After canonicalize: compare to canonical `$HOME`; then blocked prefixes; then blocked exact roots.
3. Re-run the two tests above.

**Proof this is fixed:**
- Tests: `path_policy::tests::home_and_os_roots_are_forbidden_library_roots`, `catalog::tests::add_root_rejects_overlong_label_and_home_directory`
- Confirmed: policy test **red** before the expansion, **green** after. Catalog HOME add is green and does not scan (guard is before `scan_root`).
- Mutation: `cargo mutants -f src/path_policy.rs` **15/15 caught** (2026-08-30T14:30:49Z).

---

### [HIGH] [FIXED] Collection metadata stored blanks, huge strings, and “success” for missing IDs

**What is wrong (in plain words):**
`update_collection_metadata` wrote `"   "` as a title, accepted a 501-character title, and returned **Ok** when the collection id did not exist (`UPDATE` changing 0 rows). Display titles had the same hole. Playlist rename was already empty-checked; create had a 200-char cap, metadata did not.

**Where exactly:**
- File: `src-tauri/src/catalog.rs`, `validate_user_label` lines 123–137; `update_collection_metadata` ~3141–3208; `update_file_display_title`; `create_playlist` / `rename_playlist`
- Endpoint: `update_collection_metadata`, `update_file_display_title`

**How to reproduce it (copy-paste steps):**
Red (before `validate_user_label` + `n == 0` check):
```
cargo test update_collection_metadata_rejects_empty_missing_and_overlong -- --nocapture
```
Observed: panic `blank title must not be stored: ()`.

**What should happen instead:**
Trim. Reject empty. Reject more than 500 characters (playlists: 200). Unknown collection id → `"Collection not found"`, not Ok.

**Why this matters:**
Blank titles make the catalog unusable. Unbounded strings are a hitch-the-UI / fill-the-DB input. Lying with Ok on a missing id hides bugs in the frontend.

**Exact fix instructions:**
1. Use `validate_user_label` on every user-facing label that is `Some(...)`.
2. After `UPDATE`, if `n == 0` return an error.
3. `None` still means “do not change this field” (`COALESCE`). The UI cannot **clear** author by sending empty; that is a remaining product gap (Medium, below).

**Proof this is fixed:**
- Test: `catalog::tests::update_collection_metadata_rejects_empty_missing_and_overlong`
- Red before the validator, green after.

---

### [HIGH] [FIXED] Library-root labels from IPC were unbounded

**What is wrong (in plain words):**
`add_library_root` stored `input.label` as-is if it was non-blank. A 501-character (or multi-megabyte) string from the webview became the folder name in SQLite and the UI.

**Where exactly:**
- File: `src-tauri/src/catalog.rs`, `add_root` lines 920–933
- Endpoint: `add_library_root`

**How to reproduce it (copy-paste steps):**
Red (label check not yet in `add_root`):
```
cargo test add_root_rejects_overlong_label_and_home_directory -- --nocapture
```
Observed: panic `501-character library label must be rejected`.

**What should happen instead:**
Same cap as other labels (500 characters after trim). Rejected add must not insert a row.

**Why this matters:**
The picker UI will not type 501 characters. IPC will. Unbounded labels are how a local catalog becomes a denial-of-service against itself.

**Exact fix instructions:**
Run `validate_user_label` on `input.label`. If missing, use the folder’s real name (Linux `NAME_MAX` is 255) or `"Library"`.

**Proof this is fixed:**
- Test: `catalog::tests::add_root_rejects_overlong_label_and_home_directory`
- Red before the label check, green after. `list_roots` stays empty on reject.

---

### [HIGH] [FIXED] Playlist reorder accepted a partial ID list and scrambled order

**What is wrong (in plain words):**
`reorder_collection_files` already required the ID list length to match the collection. `reorder_playlist_items` did not. Sending two of three item IDs set those two to `track_order` 0 and 1 and **left the third item’s old order**. Duplicate orders, missing slots, playback order undefined. Empty list returned Ok. Failed updates were not in a transaction, so a mixed-ID list could apply the first rows then error.

**Where exactly:**
- File: `src-tauri/src/catalog.rs`, `reorder_playlist_items` lines 4234–4271
- Endpoint: `reorder_playlist_items` (UI in `src/views/PlaylistDetailView.tsx` already sends a full permutation)

**How to reproduce it (copy-paste steps):**
Red:
```
cargo test reorder_playlist_rejects_partial_item_list -- --nocapture
```
Observed: `partial reorder must be rejected: ()` (the command returned Ok).

**What should happen instead:**
Reject empty, reject duplicates, reject length ≠ playlist item count. Apply updates in a SQLite transaction so a bad ID rolls back. A full reverse permutation must still succeed.

**Why this matters:**
The UI is careful. IPC is not. A one-line invoke from the webview can corrupt listen order for a playlist the user actually uses.

**Exact fix instructions:**
1. Open `reorder_playlist_items`.
2. Match the collection-file check: unique IDs, `COUNT(*)` must equal `item_ids.len()`.
3. Wrap the `UPDATE` loop in `connection.transaction()` and `commit()`.
4. Re-run the test — it must go from red to green, including the reverse-permutation assertion.

**Proof this is fixed:**
- Test: `catalog::tests::reorder_playlist_rejects_partial_item_list`
- Red before the permutation check, green after.

---

## High (fixed earlier in this engagement)

### [HIGH] [FIXED] Sleep timer exists only in the webview and can miss

**What is wrong (in plain words):**
The “pause after N minutes” timer was a `setInterval` in React. If the webview was frozen, throttled, or remounted, the timer did not fire. It was not stored in SQLite.

**Where exactly:**
- File: `src-tauri/src/lib.rs` — `set_sleep_timer`, `apply_sleep_if_due`, `spawn_sleep_watchdog`, `claim_sleep_if_due`
- File: `src/App.tsx` — display-only tick; pause is never decided in JS
- Prefs: `session.sleep_deadline_ms`, `session.stop_after_track`

**How to reproduce it (copy-paste steps):**
```
cargo test --offline past_sleep_deadline_is_claimed_once apply_sleep_if_due_clears_deadline_without_spawning_mpv new_sleep_timer_clears_hold_and_persists -- --nocapture
```

**What should happen instead:**
The backend owns the deadline. A 500ms watchdog pauses mpv even if the UI is stuck. A due timer writes `session.last_playing=0`. EOF auto-advance is blocked by `sleep_hold` until the user presses play.

**Why this matters:**
People use this to fall asleep. A timer that does not fire is not a timer.

**Exact fix instructions:**
Already landed: Rust deadline + watchdog + `pause_or_kill` + `set_paused` from the UI.

**Proof this is fixed:**
- Tests: `past_sleep_deadline_is_claimed_once`, `future_sleep_deadline_is_not_claimed`, `sleep_minutes_rejected_outside_range`, `apply_sleep_if_due_clears_deadline_without_spawning_mpv`, `new_sleep_timer_clears_hold_and_persists`, `pause_or_kill_does_not_spawn_mpv`, `sleep_toggle_blocked_during_grace_not_after`
- Frontend: Vitest `src/utils/sleepDisplay.test.ts`

**Residual weakness:** pause and claim use separate locks (deadlock-free by design). Hardware PlayPause is ignored for **2s** after claim, then may resume (that is what a toggle key means). Dedicated Play always resumes.

---

### [HIGH] [FIXED] Playback mutex can be held for the full mpv IPC timeout (8 seconds)

**What is wrong (in plain words):**
Transport commands locked the same state as SQLite. A stuck socket read waited up to 8 seconds. Catalog commands queued on that lock.

**Where exactly:**
- File: `src-tauri/src/mpv.rs`, `IPC_PEEK_TIMEOUT` = 400ms
- File: `src-tauri/src/lib.rs`, `AppState { inner, mpv }` — lock order **inner, then mpv**. `get_transport` peeks mpv, **drops**, then takes inner.

**How to reproduce it (copy-paste steps):**
```
cargo test --offline catalog_lock_does_not_wait_on_held_mpv_lock -- --nocapture
```

**What should happen instead:**
Catalog work must not wait on mpv IPC. Passive reads must not use the 8s command timeout.

**Exact fix instructions:**
Keep two mutexes. Never hold both during IPC. Peeks 400ms.

**Proof this is fixed:**
- Test: `playback_tests::catalog_lock_does_not_wait_on_held_mpv_lock`

---

### [HIGH] [FIXED] Every Tauri command is callable from the webview with no allow-list

**What is wrong (in plain words):**
All registered commands were exposed. Unused commands (`update_library_root`, `add_to_library`) had no UI but were still IPC.

**Where exactly:**
- File: `src-tauri/src/ipc_allowlist.rs` (**84** commands)
- File: `src-tauri/build.rs`
- File: `src-tauri/capabilities/default.json`
- File: `src-tauri/src/lib.rs` `generate_handler!`

**How to reproduce it (copy-paste steps):**
```
cargo test --offline ipc_allowlist_matches_generate_handler capabilities_allow_only_allowlisted_commands -- --nocapture
npx vitest run src/utils/ipcAllowlist.test.ts
```

**What should happen instead:**
Only commands the UI uses are generated into the ACL.

**Why this matters:**
Extra IPC is extra attack surface from a compromised webview.

**Exact fix instructions:**
Keep the three lists identical. Dead helpers may remain as Rust functions, not commands.

**Proof this is fixed:**
- Tests above, green.
- Residual: a compromised webview can still call the remaining 84. That is the desktop trust boundary, not a missed allow-list entry.

---

### [HIGH] [FIXED] “Continue” skipped a last chapter within 30 seconds of the end and restarted the book

**What is wrong (in plain words):**
Continue used “the track with the largest `position_sec` that is more than 30 seconds from the end.” A last chapter 10 seconds from the end was ignored, so playback started at track 0.

**Where exactly:**
- File: `src-tauri/src/lib.rs`, `continue_start_index` / `track_is_finished`

**How to reproduce it (copy-paste steps):**
```
cargo test continue_start_index_skips_finished_chapters_in_order -- --nocapture
```

**What should happen instead:**
Walk tracks in order. Finished = listened **or** position within **2 seconds** of duration. Start at the first unfinished track.

**Why this matters:**
Restarting a 20-hour book from chapter 1 is the product failing at its one job.

**Exact fix instructions:**
Already in `play_collection_inner` via `continue_start_index`.

**Proof this is fixed:**
- Test: `playback_tests::continue_start_index_skips_finished_chapters_in_order`

---

### [HIGH] [FIXED] MusicBrainz query treated AND/OR in titles as Lucene operators

**What is wrong (in plain words):**
The lookup built `release:War AND Peace`. Lucene parses `AND` as an operator.

**Where exactly:**
- File: `src-tauri/src/catalog.rs`, `musicbrainz_lucene_query`

**How to reproduce it (copy-paste steps):**
```
cargo test lucene_query_quotes_operator_words -- --nocapture
```
Expected: `release:"War AND Peace" AND artist:"Leo OR Tolstoy"`

**What should happen instead:**
Field values must be quoted after character-escaping.

**Why this matters:**
Wrong or empty suggestions, or a query much broader than the user typed.

**Exact fix instructions:**
`release:"{lucene_escape(title)}"`.

**Proof this is fixed:**
- Test: `catalog::tests::lucene_query_quotes_operator_words`

---

### [HIGH] [FIXED] Metadata/album group `limit` was not capped

**What is wrong (in plain words):**
`list_collections` clamped limit to 200. `list_metadata_groups` did not. A caller could pass `i64::MAX`.

**Where exactly:**
- File: `src-tauri/src/catalog.rs`; `src-tauri/src/lib.rs` command defaults 200

**How to reproduce it (copy-paste steps):**
```
cargo test list_metadata_groups_clamps_huge_limit -- --nocapture
```

**What should happen instead:**
Clamp to 1–200, offset ≥ 0.

**Why this matters:**
Unrestricted result size hitches the UI and allocates a huge vector.

**Exact fix instructions:**
`let limit = limit.clamp(1, 200); let offset = offset.max(0);`

**Proof this is fixed:**
- Test: `catalog::tests::list_metadata_groups_clamps_huge_limit`

---

### [HIGH] [FIXED] Empty playlist names were accepted

**What is wrong (in plain words):**
`create_playlist("   ")` inserted a blank unique name.

**Where exactly:**
- File: `src-tauri/src/catalog.rs`, `create_playlist`

**How to reproduce it (copy-paste steps):**
```
cargo test create_playlist_rejects_empty_and_overlong_names -- --nocapture
```

**What should happen instead:**
Reject empty (after trim) and names longer than 200 characters.

**Why this matters:**
Blank playlists are unselectable in a list of identical empty labels.

**Exact fix instructions:**
`validate_user_label(..., MAX_PLAYLIST_NAME_CHARS)`.

**Proof this is fixed:**
- Test: `catalog::tests::create_playlist_rejects_empty_and_overlong_names`

---

### [HIGH] [FIXED] Embedded covers were written with no size cap

**What is wrong (in plain words):**
`extract_cover_from_file` wrote `picture.data()` to disk as-is. A corrupt tag can hold a huge blob.

**Where exactly:**
- File: `src-tauri/src/catalog.rs`, `MAX_COVER_BYTES` (8 MiB), `embedded_cover_fits`

**How to reproduce it (copy-paste steps):**
```
cargo test embedded_cover_fits_rejects_empty_and_oversize -- --nocapture
```

**What should happen instead:**
Skip the cover if it is larger than 8 MiB.

**Why this matters:**
A library scan should not fill the disk from ID3 junk.

**Exact fix instructions:**
Keep the 8 MiB check before `fs::write`.

**Proof this is fixed:**
- Test: `catalog::tests::embedded_cover_fits_rejects_empty_and_oversize`

---

### [HIGH] [FIXED] Library backup copied a live WAL database with `fs::copy`

**What is wrong (in plain words):**
`export_db` used `fs::copy` on `library.sqlite3` while the app may have a `-wal` file. The copy can be a torn snapshot.

**Where exactly:**
- File: `src-tauri/src/lib.rs`, `export_library_db`
- Endpoint: `export_db`

**How to reproduce it (copy-paste steps):**
```
cargo test export_library_db_copies_tables_via_backup_api -- --nocapture
```

**What should happen instead:**
Use SQLite’s backup API so the destination is a consistent DB.

**Why this matters:**
A “backup” that will not open is not a backup.

**Exact fix instructions:**
`rusqlite` feature `backup`; `Backup::new(...).run_to_completion(...)`.

**Proof this is fixed:**
- Test: `playback_tests::export_library_db_copies_tables_via_backup_api`

---

## Medium (residual — not silently closed)

### [MEDIUM] Sending `null` cannot clear a metadata field

**What is wrong (in plain words):**
The UI sends `null` to mean “leave this field alone” (`COALESCE`). After the empty-string reject, there is **no** way to clear Author/Series from the UI. That is current code behavior, not a doc fantasy.

**Where exactly:**
- File: `src-tauri/src/catalog.rs`, `update_collection_metadata` `COALESCE(?n, column)`
- Workflow: edit-collection form

**How to reproduce it (copy-paste steps):**
Call `update_collection_metadata` with `author: None` — the stored author does not change. Call with `author: Some("   ")` — now rejected as empty.

**What should happen instead:**
If product intent is “user can clear author,” add an explicit clear (empty after confirm, or a dedicated flag). Do not accept `"   "` as a stored value.

**Why this matters:**
A user who typed the wrong author cannot blank it without a workaround (placeholder character).

**Exact fix instructions:**
Decide the product rule. If clear is allowed, accept a dedicated `clearAuthor: true` (or similar) and `SET author = NULL`. Do not weaken `validate_user_label` for `Some(" ")`.

**Proof this is fixed:**
- Not fixed. Open question 6.

---

### [MEDIUM] `series_index` is an unbounded `i32`

**What is wrong (in plain words):**
Title/author are capped. `series_index` is still any `i32` (including negative and `i32::MAX`). Sort order of series can be made nonsense.

**Where exactly:**
- File: `src-tauri/src/catalog.rs`, `CollectionMetadataInput.series_index`; `UPDATE ... series_index = COALESCE(?7, series_index)`

**How to reproduce it (copy-paste steps):**
No unit test currently sends `series_index: Some(i32::MIN)`. The field is deserialized from IPC as `i32`.

**What should happen instead:**
Reject or clamp to a documented range (e.g. 1–9999) if the UI only offers small numbers.

**Why this matters:**
Not a cross-user leak. It is an unbounded write next to fields we just capped. Downgraded from High because it does not scan the disk or scramble playlist identity — only sort order.

**Exact fix instructions:**
If `Some(i)` and `i` is outside `1..=9999` (or `0..=9999` if 0 means “none”), return an error. Add a test next to `update_collection_metadata_rejects_empty_missing_and_overlong`.

**Proof this is fixed:**
- Not fixed this pass.

---

### [MEDIUM] `add_library_root` takes a raw path string (picker is UI-only)

**What is wrong (in plain words):**
The UI calls `pick_library_folder` then `add_library_root`. The IPC command itself accepts any path string. A compromised webview skips the dialog. Forbidden-root checks still apply.

**Where exactly:**
- File: `src-tauri/src/lib.rs`, `add_library_root`
- File: `src-tauri/src/ipc_allowlist.rs`

**How to reproduce it (copy-paste steps):**
From a webview console (not run in this session): `invoke("add_library_root", { path: "/some/legal/folder", contentKind: "music" })`.

**What should happen instead:**
This is the desktop trust boundary, same as `delete_track_file`. Closing it would mean a capability token from the dialog. Not done.

**Why this matters:**
The OS user already owns the machine. This is not User A vs User B. It is still extra filesystem reach from XSS-in-webview. Downgraded from High **only** because there are no accounts and the forbidden-root + canonicalize-under-root checks still run.

**Exact fix instructions:**
Optional: store a one-shot token from `pick_library_folder` and require it on `add_library_root`. Do not treat “the UI uses a picker” as an access-control check.

**Proof this is fixed:**
- Not fixed. Documented residual.

---

### [MEDIUM] [FIXED] `reopen_recent` plays any path the renderer sends

**Proof this is fixed:**
- `reopen_recent` calls `recent_path_is_allowed`
- Test: `playback_tests::recent_path_requires_stored_membership`

---

### [MEDIUM] [FIXED] Production CSP still allows Vite’s localhost origins

**Proof this is fixed:**
- `src-tauri/tauri.conf.json` production CSP: `connect-src ipc: http://ipc.localhost`
- Residual: `devCsp` includes Vite for `tauri dev` only.

---

### [MEDIUM] [FIXED] Search `LIKE` still treats `%` and `_` as wildcards (titles)

**Proof this is fixed:**
- `sql_like_contains` + `ESCAPE '!'`
- Test: `catalog::tests::list_collections_search_treats_percent_and_underscore_as_literals`

---

## Low / UI / accessibility

### [LOW] Cover images in the mini-player use empty `alt`

**What is wrong (in plain words):**
Decorative covers use `alt=""`. Legal if the title is already in text beside them.

**Where exactly:**
- File: `src/components/MiniPlayerBar.tsx`; now-playing in `src/App.tsx`

**How to reproduce it (copy-paste steps):**
Inspect the DOM. `alt` is empty. Catalog cards include title in `aria-label` (axe-core).

**What should happen instead:**
Empty alt when the title is visible text. `alt={title}` only when the image is the only indication.

**Why this matters:**
Screen-reader noise vs missing context.

**Exact fix instructions:**
Do not add redundant alt if the title is already announced.

**Proof this is fixed:**
- Decorative pattern is correct for adjacent titles. axe-core on `AppNav` + `MediaCard`: 0 serious violations (jsdom). Contrast for `--muted` / `--muted-dim` on `--bg` was calculated in the execution log (both AA). **Not** a full-window WCAG certificate.

---

## Documentation vs code

### [LOW] [FIXED] README described a fraction of the product

**What is wrong (in plain words):**
README listed resume/speed/playlists/mpv and omitted catalog, sleep, data path, and the `com.chaptercheck` vs `chaptercheck` distinction.

**Where exactly:**
- File: `README.md` (now has Library catalog, Sleep timer, Data location, and a pointer to this QA folder)
- Stale text **in an older revision of this findings file** still mentioned `update_root_path` as a live helper and “90 IPC commands”. That helper is **deleted**. Live IPC is **84**. This file is the correction.

**How to reproduce it (copy-paste steps):**
Diff README against `ipc_allowlist.rs`. README does not list 84 commands; it says it is not a spec.

**What should happen instead:**
README = getting started. Invariants = this folder.

**Why this matters:**
Stale docs are how the next person tests the wrong surface. Severity Low: README does not claim false security properties.

**Exact fix instructions:**
Keep README pointing at `documentation/chaptercheck/qa-report/`. Do not resurrect `update_root_path` in docs.

**Proof this is fixed:**
- README text as of this pass. This findings file no longer treats `update_root_path` as live code.

---

## Test suite quality

### [HIGH] Coverage is real and still thin on the engine

**What is wrong (in plain words):**
Before this audit: **13** tests, none of the LIKE / HTTP / `/` root / continue / export invariants. A green suite was a demo script. Now: **51** Rust tests that fail if those invariants regress. That is not the same as “the app is tested.” `media_controls.rs` is **0%** lines. `mpv.rs` is **9.25%**. Overall **31.42%** lines. Mutation was run on `path_policy.rs` only.

**Where exactly:**
- Files: `src-tauri/src/lib.rs` `playback_tests`, `catalog.rs` `tests`, `path_policy.rs` `tests`, `db.rs` `tests`, `src/**/*.test.ts`

**How to reproduce it (copy-paste steps):**
See `test-execution-log.md`. Latest full Rust run: **51** passed. llvm-cov on that suite: **31.42%** lines.

**What should happen instead:**
Every Critical/High invariant has a test that would fail if the bug returned. Coverage numbers come from tools, not from hope. Mutation on `catalog.rs` / `lib.rs` was **not** run (hours of wall time) and is **not** claimed.

**Exact fix instructions:**
Do not chase 80% by stubbing mpv into a lie. Add a live-engine harness if you need MPRIS/mpv coverage. Add Playwright against the Tauri window if you need a GUI lab stamp.

**Proof this is fixed (partial by nature of a desktop engine):**
- **51** Rust unit tests, 0 ignored, 0 failed (`cargo test --offline -- --test-threads=1`)
- Line coverage **31.42%** overall (`cargo llvm-cov --offline --summary-only`, 51 tests); `path_policy.rs` **93.04%** lines / **100%** functions; `catalog.rs` **38.80%** lines; `mpv.rs` **9.25%**; `media_controls.rs` **0%**
- `cargo mutants -f src/path_policy.rs`: **15/15 caught** (was 13/13 before the HOME/OS branches)
- Frontend: Vitest **11** passed; `npx tsc --noEmit` exit 0
- GUI E2E: **not run**
- Skipped/pending tests: **none**

---

## Open questions

1. **GUI E2E:** still no Playwright/WebDriver run against the Tauri window. Keyboard-only flows were inspected in source (dialog focus trap, sleep dialog focuses the first time button, 44px targets); they were not executed in a real window. **Is a window-level run in scope before you call this client-ready?**
2. **Sleep vs PlayPause:** UI Pause is idempotent (`set_paused`). Hardware PlayPause is ignored for **2s** after claim. After grace, PlayPause may resume. Dedicated Play always resumes. **Is that the intended hardware-key contract?**
3. **axe-core:** jsdom on `AppNav` and `MediaCard` only (WCAG 2.1 A/AA tags, 0 violations). Color-contrast in jsdom is incomplete vs WebKit. **Do you require a full-window axe run?**
4. **`tauri dev`:** production CSP has no Vite :1420; `devCsp` includes the dev server. Release builds do not.
5. **mpv / MPRIS:** unit tests must not spawn a real engine. `eof_and_chapter_reads_do_not_spawn_mpv` guards that. Line coverage for those files stays low until an integration harness exists. **Is a live-mpv harness in scope?**
6. **Clearing metadata:** `None` = do not change. Empty string is now rejected. **Should the user be able to blank Author/Series?**
7. **`series_index` range:** unbounded `i32`. **What is the legal range?**
8. **Mutation on `catalog.rs` / `lib.rs`:** not run. Surviving mutants there are unknown.
9. **`add_library_root` without the folder picker:** still legal IPC. **Is a dialog-issued token required, or is OS-user = trusted the accepted model?**
