# ChapterCheck — test execution log

All timestamps UTC unless noted. Host: Linux, native (no Docker).  
`PKG_CONFIG_PATH` / `LIBRARY_PATH` pointed at extracted GTK/WebKit headers under `/tmp/cc-deps/root` so `cargo test` can link.

---

## Environment

```
date -u
# 2026-08-30 (session)

# No docker-compose.yml in /home/alex/Development/audioplayer

rustc: 1.98.0 (via rustup)
npx tsc: 5.6.x from package.json
```

Coverage / mutation / axe (start of day vs close-out):

```
# 2026-08-30 morning:
command -v cargo-llvm-cov   # not found
command -v cargo-tarpaulin  # not found
command -v cargo-mutants    # not found
# No Playwright / axe-core in package.json

# 2026-08-30 afternoon (this close-out):
command -v cargo-llvm-cov   # /home/alex/.cargo/bin/cargo-llvm-cov
command -v cargo-mutants    # /home/alex/.cargo/bin/cargo-mutants
command -v cargo-tarpaulin  # not found
# Playwright / axe-core still not in package.json
```

WCAG contrast (Python relative luminance, 2026-08-30):

```
muted on bg: 10.10 AA AAA
muted-dim on bg: 6.52 AA
text-soft on bg: 11.77 AA AAA
text on bg: 17.24 AA AAA
muted-dim on elevated: 6.17 AA
muted on surface: 9.09 AA AAA
accent on bg: 12.66 AA AAA
accent-text on accent: 9.96 AA AAA
danger on bg: 7.17 AA AAA
```

---

## Baseline (before new tests/fixes)

Command: `cargo test --offline -- --nocapture`  
Time: 2026-08-30T10:25:52Z  
Result: **13 passed**, 0 failed

```
running 13 tests
test catalog::tests::show_in_progress_list_requires_one_percent ... ok
test catalog::tests::validate_playback_kind_rejects_mixed_and_unknown ... ok
test path_policy::tests::tracked_file_on_disk_rejects_missing ... ok
test path_policy::tests::tracked_file_on_disk_returns_canonical_file ... ok
test playback_tests::resume_start_seconds_rewinds_near_end ... ok
test path_policy::tests::canonicalize_under_root_accepts_nested_file ... ok
test playback_tests::remove_non_current_queue_item_keeps_playing_index ... ok
test playback_tests::enqueue_empty_queue_end_starts_session_without_autoplay ... ok
test playback_tests::enqueue_empty_playlist_with_session_root_starts_fresh_session ... ok
test playback_tests::enqueue_duplicate_collection_errors ... ok
test catalog::tests::list_collections_paginates_all_filter ... ok
test playback_tests::resolve_playlist_shuffle_pref ... ok
test catalog::tests::list_collections_filters_finished_and_away ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

---

## Red: LIKE underscore leak (test added, production still using `LIKE ?1 || '%'`)

Command: `cargo test --offline import_folder_like_underscore_does_not_match_sibling -- --nocapture`  
Time: 2026-08-30 (immediately after adding the test, before ESCAPE '!')

```
running 1 test

thread 'catalog::tests::import_folder_like_underscore_does_not_match_sibling' panicked at src/catalog.rs:5504:9:
assertion `left == right` failed: underscore in folder name must not LIKE-match a sibling folder; got [1, 2]
  left: 2
 right: 1
test catalog::tests::import_folder_like_underscore_does_not_match_sibling ... FAILED

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 13 filtered out
```

This is the required red run for that finding.

---

## Green: full suite after fixes

Command: `cargo test --offline -- --nocapture`  
Time: **2026-08-30T10:34:19Z**  
Result: **28 passed**, 0 failed, 0 ignored

```
running 28 tests
test catalog::tests::http_allowed_rejects_offlist_and_prefix_tricks ... ok
test catalog::tests::lucene_query_quotes_operator_words ... ok
test catalog::tests::show_in_progress_list_requires_one_percent ... ok
test catalog::tests::sql_like_literal_escapes_wildcards ... ok
test catalog::tests::validate_playback_kind_rejects_mixed_and_unknown ... ok
test path_policy::tests::filesystem_root_is_forbidden_library_root ... ok
test path_policy::tests::is_under_root_does_not_match_sibling_prefix ... ok
test path_policy::tests::tracked_file_on_disk_rejects_missing ... ok
test playback_tests::continue_start_index_skips_finished_chapters_in_order ... ok
test path_policy::tests::canonicalize_under_root_rejects_symlink_escape ... ok
test path_policy::tests::canonicalize_under_root_accepts_nested_file ... ok
test path_policy::tests::tracked_file_on_disk_returns_canonical_file ... ok
test playback_tests::resume_start_seconds_rewinds_near_end ... ok
test catalog::tests::add_to_library_file_outside_roots_does_not_scan_parent ... ok
test catalog::tests::import_folder_like_underscore_does_not_match_sibling ... ok
test catalog::tests::add_root_rejects_filesystem_root ... ok
test catalog::tests::create_playlist_rejects_empty_and_overlong_names ... ok
test playback_tests::remove_current_last_queue_item_selects_previous_index ... ok
test catalog::tests::list_metadata_groups_clamps_huge_limit ... ok
test playback_tests::remove_non_current_queue_item_keeps_playing_index ... ok
test playback_tests::enqueue_duplicate_collection_errors ... ok
test playback_tests::enqueue_empty_queue_end_starts_session_without_autoplay ... ok
test playback_tests::enqueue_empty_playlist_with_session_root_starts_fresh_session ... ok
test catalog::tests::list_collections_paginates_all_filter ... ok
test playback_tests::resolve_playlist_shuffle_pref ... ok
test catalog::tests::list_collections_filters_finished_and_away ... ok
test playback_tests::export_library_db_copies_tables_via_backup_api ... ok
test db::tests::wal_two_connections_can_write_without_busy ... ok

test result: ok. 28 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s

     Running unittests src/main.rs ... 0 tests
     Doc-tests audioplayer_lib ... 0 tests
```

Compiler warning on every run (not a test failure):

```
warning: method `read_transport_state` is never used
   --> src/mpv.rs:417:12
```

---

## TypeScript

Command: `npx tsc --noEmit`  
Time: **2026-08-30T10:34:22Z**  
Exit code: **0**

---

## Not run (tools missing)

- `cargo llvm-cov` / `cargo tarpaulin` — no coverage percentage
- `cargo mutants` — no mutation score
- axe-core / Playwright — no automated WCAG report

Do not invent numbers for these.

---

## High-item close-out (sleep / mutex / IPC)

Command: `cargo test --offline`  
Time: **2026-08-30T11:41:13Z** (llvm-cov instrumented run; same 40 tests also green on a non-coverage `cargo test --offline`)  
Result: **40 passed**, 0 failed, 0 ignored

Sleep / lock / ACL tests in that run include:
- `playback_tests::past_sleep_deadline_is_claimed_once`
- `playback_tests::future_sleep_deadline_is_not_claimed`
- `playback_tests::sleep_minutes_rejected_outside_range`
- `playback_tests::apply_sleep_if_due_clears_deadline_without_spawning_mpv`
- `playback_tests::new_sleep_timer_clears_hold_and_persists`
- `playback_tests::catalog_lock_does_not_wait_on_held_mpv_lock`
- `playback_tests::ipc_allowlist_matches_generate_handler`
- `playback_tests::capabilities_allow_only_allowlisted_commands`
- `playback_tests::recent_path_requires_stored_membership`
- `catalog::tests::list_collections_search_treats_percent_and_underscore_as_literals`
- `catalog::tests::path_delete_allowed_requires_available_registered_root`
- `path_policy::tests::is_under_any_root_requires_real_membership`
- `path_policy::tests::filesystem_root_is_forbidden_library_root` (now includes `/proc` and `/proc/self`)

`read_transport_state` unused warning is **gone** (helper deleted). Remaining compile warnings are unused catalog helpers that are no longer IPC (`update_root_path`, `list_album_groups`, …) and unused mpv `duration` / `duration_lenient`.

---

## TypeScript (close-out)

Command: `npx tsc --noEmit`  
Time: **2026-08-30T11:41:13Z**  
Exit code: **0**

---

## Vitest

Command: `npx vitest run`  
Time: **2026-08-30T11:22:39Z** (first green); suite unchanged after i18n-only QA docs  
Result: **7 passed**, 0 failed (`src/utils/sleepDisplay.test.ts`, `src/utils/ipcAllowlist.test.ts`)

```
RUN  v3.2.7
✓ src/utils/ipcAllowlist.test.ts (1 test)
✓ src/utils/sleepDisplay.test.ts (6 tests)
Test Files  2 passed (2)
Tests  7 passed (7)
```

---

## Coverage (real `cargo llvm-cov`, not invented)

Command: `cargo llvm-cov --offline --summary-only`  
Time: **2026-08-30T11:41:13Z**  
40 tests green under instrumentation.

```
Filename                      Regions    Missed Regions     Cover   Functions  Missed Functions  Executed       Lines      Missed Lines     Cover
catalog.rs                       7914              5581    29.48%         676               553    18.20%        4923              3330    32.36%
db.rs                             413               223    46.00%          39                24    38.46%         419               184    56.09%
lib.rs                           6498              5248    19.24%         466               391    16.09%        3824              3077    19.53%
main.rs                             3                 3     0.00%           1                 1     0.00%           3                 3     0.00%
media_controls.rs                 284               284     0.00%          19                19     0.00%         190               190     0.00%
mpv.rs                            796               776     2.51%          87                82     5.75%         490               473     3.47%
path_policy.rs                    230                 4    98.26%          18                 0   100.00%         122                 2    98.36%
TOTAL                           16138             12119    24.90%        1306              1070    18.07%        9971              7259    27.20%
```

`cargo-tarpaulin` was **not** used (not installed). `cargo-llvm-cov` **was** installed this session.

---

## Mutation (real `cargo mutants`, not invented)

Command: `cargo mutants -f src/path_policy.rs -t 60 -j 1 -- --offline`  
Time: **2026-08-30T11:40:04Z**  
Result: **13 mutants tested, 13 caught**, 0 missed

First run in this session (before `/proc` + `is_under_any_root` tests): 10 caught, 3 missed. Those three were closed with tests; the rerun is 13/13.

Not run on `lib.rs` / `catalog.rs` (would be hours). No whole-crate mutation score is claimed.

---

## GUI E2E / axe-core

**Tauri window E2E: not run.** No Playwright/WebDriver against the app window.

**axe-core:** run in jsdom on `AppNav` and `MediaCard` with tags `wcag2a`, `wcag2aa`, `wcag21a`, `wcag21aa` — **0 violations** (2026-08-30T14:20:58Z, Vitest). That is not a full-window certificate. Sleep UI, skip link, 44px targets, `focus-visible`, and `prefers-reduced-motion` were still reviewed in source. `--muted-dim` on `--bg` contrast **6.52** (AA, Python, earlier the same day).

---

## Cover-scope + no-spawn EOF (follow-up)

Command: `cargo test --offline -- --test-threads=1`  
Time: **2026-08-30T15:43:49Z**  
Result: **45 passed**, 0 failed, 0 ignored

New tests in that run:
- `catalog::tests::covers_live_beside_library_db_not_under_tauri_bundle_id`
- `playback_tests::eof_and_chapter_reads_do_not_spawn_mpv`

TypeScript: `npx tsc --noEmit` exit 0; `npx vitest run` **7 passed**.

---

## Sleep pause-or-kill + intended Pause (follow-up)

Command: `cargo test --offline -- --test-threads=1`  
Time: **2026-08-30T14:10:03Z**  
Result: **46 passed**, 0 failed, 0 ignored

New test: `playback_tests::pause_or_kill_does_not_spawn_mpv`

Code: `MpvController::pause_or_kill`; `apply_sleep_if_due` claims even if pause IPC fails (engine killed). UI Space/Play/Pause send `set_paused` with last known intended state.

TypeScript: `npx tsc --noEmit` exit 0; `npx vitest run` **7 passed**.

---

## Dead-helper removal + PlayPause grace + axe (follow-up)

Command: `cargo test --offline -- --test-threads=1`  
Time: **2026-08-30T14:20:58Z**  
Result: **47 passed**, 0 failed, 0 ignored

New test: `playback_tests::sleep_toggle_blocked_during_grace_not_after`

Removed unused catalog helpers (`update_root_path`, `list_album_groups`, `list_series_names`, `add_album_to_playlist`, `create_playlist_from_album`, `scan_status_all`).

Command: `cargo llvm-cov --offline --summary-only`  
Time: **2026-08-30T14:20:58Z**  
Result: **28.61%** lines, **26.29%** regions, **19.77%** functions. `mpv.rs` **9.25%**, `media_controls.rs` **0%**, `path_policy.rs` **98.36%**.

TypeScript: `npx tsc --noEmit` exit 0; `npx vitest run` **11 passed** (axe-core WCAG 2.1 A/AA on `AppNav` + `MediaCard`).

---

## Momos pass: HOME/OS roots + metadata labels (red → green)

### Red: `$HOME` still a legal library root

Command: `cargo test --offline home_and_os_roots_are_forbidden_library_roots -- --nocapture`  
Time: 2026-08-30 (same session, before expanding `is_forbidden_library_root`)  
Result: **FAILED**

```
thread 'path_policy::tests::home_and_os_roots_are_forbidden_library_roots' panicked at src/path_policy.rs:
the user's home directory must not be a library root (would scan everything in it)
```

`$HOME` itself was **not** passed to `add_root` on the unguarded build (that would scan the real home directory). The policy unit test is the proof.

### Red: blank / overlong / missing collection metadata

Command: `cargo test --offline update_collection_metadata_rejects_empty_missing_and_overlong -- --nocapture`  
Time: 2026-08-30 (before `validate_user_label`)  
Result: **FAILED**

```
blank title must not be stored: ()
```

### Green after those two fixes (49 tests)

Command: `cargo test --offline -- --test-threads=1`  
Result: **49 passed**, 0 failed, 0 ignored

### Vitest + tsc (this pass)

Command: `npx tsc --noEmit && npx vitest run`  
Time: 2026-08-30T14:27:36Z (Vitest “Start at 16:27:36” local CEST)  
Result: `tsc` exit 0; **11** Vitest passed (4 files), including axe-core on `AppNav` + `MediaCard`.

```
Test Files  4 passed (4)
     Tests  11 passed (11)
```

### Mutation: path_policy.rs after HOME/OS expansion

Command: `cargo mutants -f src/path_policy.rs -t 60 -j 1 -- --offline`  
Time: **2026-08-30T14:30:49Z** (`ended_at` in terminal)  
Result: **15 mutants tested in 3m: 15 caught** (unmutated baseline 98s build + 4s test)

### llvm-cov on 49-test suite (before +2 tests)

Command: `cargo llvm-cov --offline --summary-only`  
Time: immediately after the 49-test green run, before the reorder/label tests  
Result: **30.07%** lines, **27.40%** regions, **20.32%** functions. `path_policy.rs` **93.04%** lines. `catalog.rs` **36.24%**. `mpv.rs` **9.25%**. `media_controls.rs` **0%**.

---

## Momos pass: unbounded root label + partial playlist reorder (red → green)

### Red

Command:

```
cargo test --offline -- --test-threads=1 --nocapture \
  add_root_rejects_overlong_label_and_home_directory \
  reorder_playlist_rejects_partial_item_list
```

Result: **FAILED** (2 tests)

```
test catalog::tests::add_root_rejects_overlong_label_and_home_directory ... 
thread '…' panicked at src/catalog.rs: 501-character library label must be rejected
FAILED
test catalog::tests::reorder_playlist_rejects_partial_item_list ... 
thread '…' panicked at src/catalog.rs: partial reorder must be rejected: ()
FAILED
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 49 filtered out
```

### Green (same two tests, then full suite)

Command: same filter after `validate_user_label` on `add_root` and permutation+transaction on `reorder_playlist_items`  
Result: **2 passed**

Command: `cargo test --offline -- --test-threads=1`  
Result: **51 passed**, 0 failed, 0 ignored

```
test result: ok. 51 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s
```

### llvm-cov on 51-test suite

Command: `cargo llvm-cov --offline --summary-only`  
Time: started after `date -u` **2026-08-30T14:35:38Z**; compile+run ~23s  

```
catalog.rs                       …    38.80% lines
db.rs                            …    56.09% lines
lib.rs                           …    21.17% lines
main.rs                          …     0.00% lines
media_controls.rs                …     0.00% lines
mpv.rs                           …     9.25% lines
path_policy.rs                   …    93.04% lines
TOTAL                            …    31.42% lines  (28.73% regions, 21.04% functions)
```

51 tests in the llvm-cov run, all ok.

---

## Aristoteles (races, retry, metadata, mutation)

Native host (no Docker). `PKG_CONFIG_PATH` / `LIBRARY_PATH` as above.

### Frontend

Command: `npx tsc --noEmit && npx vitest run`  
Time: 2026-08-30T15:10:44Z  
Result: **exit 0**, **36 passed**, 0 failed (9 files)

Stryker (`npx stryker run`, mutate `src/utils/viewLogic.ts` only):

```
All files     | 100.00 |  100.00 | 35 killed | 0 timeout | 0 survived
viewLogic.ts  | 100.00 |  100.00 | 35 killed | 0 timeout | 0 survived
```

Time: 2026-08-30T15:10:26Z, ~30s.

### Rust

Command: `cargo test --offline -- --test-threads=1`  
Result: **52 passed**, 0 failed, 0 ignored (includes `optional_text_sql_keeps_clears_and_sets`; metadata clear + `series_index` bounds in `update_collection_metadata_rejects_empty_missing_and_overlong`)

Command: `cargo llvm-cov --offline --summary-only`  
Time: 2026-08-30T14:59Z window (51 tests in that run; one unit test added after)

```
catalog.rs                       …    39.59% lines
db.rs                            …    56.09% lines
lib.rs                           …    21.17% lines
main.rs                          …     0.00% lines
media_controls.rs                …     0.00% lines
mpv.rs                           …     9.25% lines
path_policy.rs                   …    93.04% lines
TOTAL                            …    31.85% lines  (29.14% regions, 21.10% functions)
```

Command: `cargo mutants -f src/path_policy.rs --timeout 15`  
Result: **15 mutants tested in 3m: 15 caught** (after `add_root($HOME)` test uses a 3s channel timeout so a mutated “allow home” fails instead of scanning `$HOME` until cargo-mutants times out)

---

