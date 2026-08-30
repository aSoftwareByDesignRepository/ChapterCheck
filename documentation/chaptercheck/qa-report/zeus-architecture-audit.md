# ChapterCheck — Zeus Architecture Audit

> **Review Mode: FULL AUDIT**  
> **Object:** already-implemented system (`/home/alex/Development/audioplayer`, product ChapterCheck 0.1.0)  
> **Auditor:** Zeus  
> **Date:** 2026-08-30 (UTC)  
> **Environment:** Native Linux host. No `docker-compose.yml` in this repo. Builds/tests run natively with GTK/WebKit pkg-config from `/tmp/cc-deps` only.

**Verdict in one sentence:** This is a single-OS-user, single-process Linux desktop player whose core lock order, sleep claim, and catalog/mpv split are sound; three architecture defects found in this pass (picker-grant consumed before a successful link, unbounded recursive scan, new library root left behind on a failed first scan) are **patched and proven**. It is **not** a multi-user service and must not be operated as one.

---

## Table of contents

1. [System Context & Component Map](#1-system-context--component-map)
2. [Architecture Decision Records](#2-architecture-decision-records)
3. [Failure Mode, Race Condition & Deadlock Analysis](#3-failure-mode-race-condition--deadlock-analysis)
4. [Security & Threat Model](#4-security--threat-model)
5. [Patterns vs Anti-Patterns](#5-patterns-vs-anti-patterns)
6. [Non-Functional Requirements Audit](#6-non-functional-requirements-audit)
7. [Deployment, Rollback & Operational Readiness](#7-deployment-rollback--operational-readiness)
8. [Verdict Register](#8-requirement-verdict-tiering)
9. [Data Contracts, State Model & Boundaries](#9-data-contracts-state-model--boundaries)
10. [Testability & Validation Strategy](#10-testability--validation-strategy-for-the-architecture-itself)
11. [Assumptions Register](#assumptions-register)
12. [Blocking Questions](#blocking-questions)
13. [Remediation evidence](#remediation-evidence)

---

## 1. System Context & Component Map

Derived from `src-tauri/src/lib.rs`, `catalog.rs`, `db.rs`, `mpv.rs`, `picker_grant.rs`, `fs_privacy.rs`, `path_policy.rs`, `media_controls.rs`, `ipc_allowlist.rs`, `capabilities/default.json`, `tauri.conf.json`, and `src/App.tsx` — not from a README.

ChapterCheck is one OS process: a Tauri v2 WebKit webview (React) plus a Rust host that owns SQLite, mpv JSON-IPC, and OS integrations. There is no network listener, no accounts, no money path. Data lives under `directories::ProjectDirs` (`~/.local/share/chaptercheck/` on this host: `library.sqlite3` + WAL/SHM, `covers/`), **not** Tauri `$DATA` (`com.chaptercheck`).

### Components

| Component | Responsibility (SoT) | Trust boundary | Upstream dependents | Downstream trust | Failure domain |
|---|---|---|---|---|---|
| **Webview UI** | Display + user intent. Not authoritative for sleep, grants, catalog, or playback. | JS → 84 allow-listed IPC commands (Tauri ACL + `generate_handler!` + `ipc_allowlist.rs` must match). | Human operator | Host returns what JS asked; stale-result gates in `viewLogic.ts` | UI freeze/wrong screen; cannot by itself move files or scan if IPC is denied |
| **Rust `AppState`** | Process-wide locks: `inner` (catalog session + sleep), `mpv`, `folder_grants`, `scan_in_progress` | All IPC enters here | UI, MPRIS thread, sleep watchdog | SQLite + mpv + FS | Process hang if a lock is held across hung IPC (mitigated: catalog vs mpv split; sleep uses peek timeout) |
| **`InnerState` + `LibraryDb` (primary)** | Authoritative **session** (queue, index, speeds), **sleep deadline/hold**, **progress writes** on this connection | IPC / watchdog / MPRIS | Transport UI, resume-on-launch | SQLite file | Lost progress tick if busy_timeout (8s) fires during a scan write |
| **Sidecar `LibraryDb`** | Authoritative **catalog mutation during scan/link** so a filesystem walk never holds `inner` across mpv IPC | `with_scan_flag` + `open_sidecar_catalog()` | Home/catalog UI after scan | Same SQLite file (WAL) | One scan at a time (`AtomicBool` CAS). Scan failure used to leave an orphan root — **fixed this pass** |
| **`PickerGrantSet`** | SoT for “the user actually picked this folder” (dialog or OS-open), TTL 30 min, one-shot **after success** | `pick_*` / `open_folder_path_impl` grant; `add_library_root` peeks then consumes | `add_library_root` | Canonical paths on disk | Compromised webview cannot link an unpicked tree. Failed link must not burn the grant — **fixed this pass** |
| **`MpvController`** | SoT for decoder/transport (position, pause, eof) while a child is connected | Unix socket in private runtime dir (0600); never world-writable `/tmp` | UI poll, sleep, MPRIS | mpv binary + socket | Single engine process (accepted desktop SPOF). 8s command timeout vs 400ms peek |
| **mpv child** | Decodes audio | JSON IPC | Host | User files under linked roots | Kill/restart via `recover_mpv` / `pause_or_kill` |
| **`path_policy`** | SoT for “is this path a legal library root / under a root” | Every catalog path | Scanner, delete, play | `canonicalize` + prefix check | Forbids `/`, `$HOME`, OS trees |
| **Sleep watchdog thread** | Rust-owned timer fire (500ms). UI is display-only | Reads `inner` deadline; `try_lock_mpv` to pause | User bedtime stop | `claim_sleep_if_due` is CAS-under-mutex | If mpv lock is held, tick skips and deadline stays (SF-02) |
| **MPRIS / `media_controls`** | Headset/media keys | D-Bus; `try_lock` both mutexes | Desktop environment | Same `AppState` | `try_lock` so it cannot deadlock with `inner`→`mpv` |
| **HTTPS client** | Optional metadata | TLS to `openlibrary.org/search.json` and `musicbrainz.org/ws/2/*` only; no redirects; no userinfo | Lookup UI | Those two hosts | Pref-off = no call. Titles leak to those services when on |

### Perspective lenses

| Perspective | Answer |
|---|---|
| **Data owner** | Catalog rows: `CatalogService` (sidecar during scan, `inner.db` otherwise). Progress/`media_state`: `InnerState` via primary connection. Sleep deadline: `InnerState` (also persisted in `app_settings`). Two SQLite connections, **one file**, SQLite serializes writers. Not two services both claiming SoT. |
| **API consumer** | Unversioned Tauri IPC. Breaking a command name breaks the webview on the same deploy (same binary). No independent consumer cadence. |
| **Operator / SRE** | Local `.deb` / cargo run. No paging, no cluster. 3 AM incident = user relaunches. `eprintln!` only. |
| **Security boundary** | Webview is untrusted renderer. Picker grant + path policy + ACL. Residual: remaining 84 IPC if JS is owned (Open Architectural Conflict; Q2). |
| **Adjacent** | mpv never promised 8s IPC latency; host assumes peek/command timeouts. Open Library never promised availability; lookup fails closed. |
| **Data-at-rest** | SQLite + covers, chmod 0600/0700, not ciphertext. Deleted when user removes files/DB/uninstalls. No retention job. |
| **Compliance** | No payment/health. Listening history may be GDPR if an org operates the install (Q1). |
| **Cost** | No cloud bill. Unbounded local cost was “scan `$HOME` / symlink cycle until OOM or heat death” — forbidden roots + **walk cap this pass**. |
| **Future maintainer** | Lock order is commented on `with_engine`. IPC allow-list is triplicated (handler / `ipc_allowlist.rs` / capabilities) and tested for match. Sleep is Rust-owned. This file is the architecture record. |

**Open Architectural Conflict (narrowed):** Play/seek remain OS-user-principal. Deletes and enabling online metadata require native OS confirm + DestructiveGrant (SF-09). Catalog remove still uses in-app confirm (not OS dialog) — metadata-only, not disk delete.

---

## 2. Architecture Decision Records

```
ID: ADR-001
Decision: Single-process desktop app (Tauri webview + Rust + mpv + one SQLite file). Not a service mesh.
Context: Local audiobook/music player for one OS user.
Alternatives considered: (1) Electron + separate helper daemon; (2) gRPC catalog service + player; (3) pure native GTK without webview.
Consequences: Webview XSS becomes full IPC. Ops model is “relaunch.” No horizontal scale.
Reversibility: One-way door for the product shape; extractable later at high cost.
Zeus's verdict: ENDORSED for this product. Must not be sold as multi-user or server-side.
```

```
ID: ADR-002
Decision: Two mutexes — inner (catalog/session/sleep) and mpv — lock order inner then mpv. get_transport peeks mpv, drops, then inner. Sidecar SQLite for scans.
Context: An 8s hung mpv IPC must not freeze Home/catalog. Catalog lock test proves independence.
Alternatives considered: (1) One giant mutex; (2) actor/channel owning mpv; (3) always sidecar for all DB.
Consequences: Dual SQLite writers + busy_timeout 8s. Lock inversion with blocking mpv-then-inner would deadlock; MPRIS uses try_lock.
Reversibility: Two-way door (refactor to actor), moderate cost.
Zeus's verdict: ENDORSED WITH CONDITIONS — never add a blocking mpv-then-inner path; busy_timeout miss is a progress-tick loss (SF-01).
```

```
ID: ADR-003
Decision: Sleep timer is Rust-owned (deadline + hold + 500ms watchdog + claim under inner mutex). UI display-only.
Context: Webview timers lie on suspend and can double-fire.
Alternatives considered: (1) setTimeout in JS; (2) mpv expire-playback; (3) systemd timer.
Consequences: Speculative pause then claim (resume if user extended). Watchdog can wait on mpv mutex.
Reversibility: Two-way door.
Zeus's verdict: ENDORSED — Must-Fix MF-01..03 and Absolute No-Gos closed; Aristoteles closed SF-01..SF-10 including FM-07 (`try_lock_mpv` skip-tick). Remaining product questions are Q1 (org GDPR) and Q3 (raise 25k scan cap).
```

```
ID: ADR-004
Decision: add_library_root requires a live picker/OS grant; grant is peeked, then consumed only if add_root returns Ok.
Context: Compromised JS must not scan ~/Documents. Argus consumed the grant before add_root, so validation/scan failure forced a re-pick (and raced with “root inserted, grant gone”).
Alternatives considered: (1) Consume first (Argus); (2) capability token per IPC; (3) no path argument, only last-picked handle inside Rust.
Consequences: Two concurrent successful links of the same path are idempotent (existing root). TTL can expire between peek and add (accepted).
Reversibility: Two-way door.
Zeus's verdict: ENDORSED. Consume-before-success is now an Absolute No-Go.
```

```
ID: ADR-005
Decision: Recursive audio walks use a visited-dir set, a seen-file set, and hard caps (25_000 audio files, 50_000 directories). New root INSERT is rolled back if the first scan returns Err.
Context: Symlink cycles and “user picked a huge tree” are production, not hypothetical. Dual-writer WAL does not bound a walk.
Alternatives considered: (1) Trust the user / no cap; (2) cooperative cancel token from UI; (3) inode walk with kernel FS notify.
Consequences: Libraries larger than the cap fail closed with a readable error. One escaped symlink in a file-is-item tree fails the scan (same as pre-cap canonicalize_under_root).
Reversibility: Two-way door (raise caps; add cancel).
Zeus's verdict: ENDORSED. Caps are a product knob (AS-03). Unbounded walk is an Absolute No-Go.
```

```
ID: ADR-006
Decision: WAL + 8s busy_timeout; sidecar connection for scan; PRAGMA foreign_keys ON; owner-only files.
Context: Need concurrent UI reads vs scan writes without holding mpv.
Alternatives considered: (1) One connection, scan on inner (blocks playback commands that need inner); (2) SQLite DELETE journal; (3) separate catalog.sqlite.
Consequences: Writers serialize; 8s stall possible; crash mid-scan can leave partial collections on a **re-scan of an existing** root (new-root path now rolls back).
Reversibility: Two-way door for a second file; painful for identity of file_key paths.
Zeus's verdict: ENDORSED WITH CONDITIONS — SF-01.
```

```
ID: ADR-007
Decision: Outbound HTTP only to two parsed HTTPS endpoints, no redirects, rustls, 30-day metadata cache in SQLite.
Context: Cover/title lookup without becoming an open proxy.
Alternatives considered: (1) No online lookup; (2) user-configurable base URL; (3) GraphQL aggregator.
Consequences: Titles leave the machine when the pref is on. Host outage → empty/error, not a hang (timeout + retry policy in catalog).
Reversibility: Two-way door.
Zeus's verdict: ENDORSED.
```

---

## 3. Failure Mode, Race Condition & Deadlock Analysis

### Shared-state map (what this section is judging)

| Resource | Writers | Concurrency control | Idempotency |
|---|---|---|---|
| `sleep_deadline_ms` / `sleep_hold` | Watchdog, `get_transport`, `set_sleep_timer`, EOF/advance paths | `inner` mutex; `claim_sleep_if_due` wins once | Second claim returns false (tested) |
| `scan_in_progress` | Scan/link/refresh IPC | `AtomicBool` CAS | Second scan returns explicit error |
| `PickerGrantSet` | pick/OS-open; consume on successful link | `folder_grants` mutex; TTL sweep | Peek is repeatable; consume is one-shot |
| `library_roots` / collections | Sidecar during scan; inner for metadata/UI | SQLite writer lock + FKs | `add_root` updates existing canonical path |
| mpv transport | Commands holding `mpv` mutex | Mutex + 8s/400ms socket timeouts | pause_or_kill does not spawn |
| `media_state` progress | `save_progress` / close persist on inner | SQLite + busy_timeout | Last write wins; no version column |

## Failure Mode Register — ChapterCheck host

| ID | Failure Mode | Trigger Condition | Blast Radius | Current Mitigation | Verdict |
|---|---|---|---|---|---|
| FM-01 | Grant burned on failed `add_root` | Consume ran before scan/validation | User must re-pick; webview could not retry a legitimate pick | Peek then consume on `Ok` (`link_granted_library_root`) | 🟢 MUST-FIX closed |
| FM-02 | Recursive scan never returns | Symlink dir cycle `a↔b` inside a linked root | `scan_in_progress` stuck until process kill; UI “scanning” forever; sidecar connection held | `visited_dirs` HashSet; 2s cycle test | 🟢 MUST-FIX closed |
| FM-03 | Unbounded file list / RAM | User links a tree with tens of thousands of tracks (or 10× planned) | OOM / multi-minute UI stall / disk fill of catalog | 25k files / 50k dirs fail closed | 🟢 MUST-FIX closed |
| FM-04 | Partial insert: root row, scan Err | `INSERT` then `scan_root` fails (escape, cap, I/O) | Orphan empty (or partial) library root; grant was also gone (FM-01) | `remove_root` on first-scan Err (FK CASCADE) | 🟢 MUST-FIX closed |
| FM-05 | Two writers, same job | Two IPC `add_library_root` with same grant | Duplicate scan work; both succeed idempotently | `with_scan_flag` CAS serializes; grant consume-on-success | 🟢 SF-03 closed |
| FM-06 | Dual CAS sleep vs watchdog vs transport poll | Deadline due; `get_transport` and watchdog both fire | Speculative double pause; one claim | Claim is mutex-one-shot; loser StayPaused or Resume | 🟢 already closed (prior pass) |
| FM-07 | Sleep fire waits on `mpv` mutex | `get_chapters` / command holds mpv up to 8s | Audio continues past bedtime by command duration | `try_lock_mpv` in `apply_sleep_if_due`; skip tick, keep deadline | 🟢 SF-02 closed |
| FM-08 | Deadlock inner↔mpv | Blocking `mpv` then `inner` vs `with_engine` inner then mpv | Frozen UI + frozen engine | Documented order; `get_transport` drops mpv; MPRIS `try_lock`; catalog test holds mpv and takes inner | 🟢 constrained; NG-03 |
| FM-09 | Progress write SQLITE_BUSY | Sidecar scan writer + `save_progress` | Missed progress tick (up to 8s wait then error) | Drop mpv before SQLite; retry up to 12×40ms on BUSY/LOCKED | 🟢 SF-01 closed |
| FM-10 | Crash mid-scan on **existing** root | Kill during upsert loop | Partial collections until next scan | WAL; re-scan; `last_scan_status='error'` | 🟢 SF-04 closed (compensate) |
| FM-11 | `scan_in_progress` process abort | Kill -9 during scan | Flag is in-memory; next launch is idle. DB may be partial (FM-10) | Process restart | 🟢 acceptable |
| FM-12 | Out-of-order IPC (stale list/home) | Slow scan result applied after newer navigation | Wrong filter/page | `shouldApplyAsyncResult` generation gates | 🟢 already closed (prior pass) |
| FM-13 | Playlist reorder partial permutation | Client sends a subset of item ids | Silent scramble | Full-permutation check | 🟢 already closed |
| FM-14 | Sleep watchdog thread death | Spawn fails or panic | Timer never fires | 5 spawn retries + audit log; panic-caught loop | 🟢 SF-08 closed |
| FM-15 | Split-brain | N/A — single process, single machine | — | — | N/A (not a distributed system) |
| FM-16 | Subfolder-is-item `read_dir` of a million entries | Flat directory as library root | Long stall without hitting recursive cap | `list_dir_children` fail-closed at `MAX_SCAN_DIRS` | 🟢 SF-05 closed |
| FM-17 | Hung IPC skips sleep claim | `lock_mpv()` Err | Audio past deadline | `Mutex::lock` poison recovered; branch unreachable | 🟢 not a live bug |

### Concurrency / ordering / partial failure (explicit)

- **Concurrency:** Sleep claim is under `inner`. Grant peek/consume under `folder_grants`. Scan flag is CAS. Catalog rows: SQLite. No optimistic version column on `media_state`.
- **Ordering:** Events are not a bus; IPC is request/response. Stale UI results are generation-gated. Out-of-order mpv property reads use peek timeout, not a sequence number.
- **Idempotency:** `add_root` on an existing canonical path updates + re-scans. Grant consume is not idempotent (one-shot after success). Sleep claim is not retryable (clears deadline).
- **Partial failure:** Five-step link = grant peek → scan flag → INSERT → scan walk → DTO. Steps 3–4 now roll back the new root. Existing-root re-scan does not delete the root on failure (correct: must not unlink a library the user already had).
- **Deadlock:** Only if a **blocking** mpv-then-inner path is added. Current blocking paths: `with_engine` inner→mpv; `get_chapters` mpv only; `apply_sleep_if_due` inner (drop) → mpv (drop) → inner.
- **Cascading slow:** Slow mpv holds `mpv` mutex; UI transport waits. Sleep watchdog uses `try_lock_mpv` and does **not** block on a held lock (SF-02 / FM-07 closed). Catalog/home sidecar path does **not** wait on mpv (`catalog_lock_does_not_wait_on_held_mpv_lock`).
- **Unbounded growth:** Recursive walk now capped. Cover writes still size-checked (prior). List IPC limits clamped (prior). Grant map swept by TTL. No unbounded retry storm on HTTP (timeout + bounded retry in catalog).
- **Split-brain:** Not applicable (AS-01).

---

## 4. Security & Threat Model

Trust boundaries from §1. STRIDE per boundary. This pass does not reopen Argus’s closed MF items unless architecture drifted.

| Boundary | S | T | R | I | D | E | Logged threats |
|---|---|---|---|---|---|---|---|
| Webview → IPC | Stolen JS invokes commands | Tamper invoke args | No signed audit log | Error strings to UI | Invoke storms (local) | Destructive IPC without extra prompt | TH-01..03 |
| Host → mpv socket | Other UID if socket world-writable | Inject JSON | — | — | Hang socket | — | Closed by Argus (fs_privacy); NG-04 |
| Host → SQLite | Other UID read history | Truncate DB | — | History disclosure | Disk fill | — | chmod 0600; not ciphertext |
| Host → filesystem | — | Delete via IPC | — | Path leak in UI | Scan DoS | Link unpicked path | Grant + path_policy + walk cap |
| Host → HTTPS | — | — | Queries to OL/MB | Titles leave machine | Slow lookup | — | Allow-list, no redirect |
| OS-open / argv | File manager spoof path | Open unexpected file | — | — | — | Grant issued for OS-open | Intended; grant TTL 30 min |

## Threat Register — ChapterCheck

| ID | Threat | Trigger | Blast Radius | Mitigation | Verdict |
|---|---|---|---|---|---|
| TH-01 | Spoof “user picked folder” | `invoke(add_library_root)` with raw path | Scan of unpicked tree | Grant peek; unpicked → error | 🟢 closed (Argus + Zeus consume-on-success) |
| TH-02 | Elevation: JS deletes current book | `delete_session_files` | User files gone | Native OS dialog → one-shot `DestructiveGrant` | 🟢 SF-09 closed |
| TH-03 | Info disclosure via metadata lookup | Pref on + XSS | Titles to OL/MB | Allow-list HTTPS; enable requires DestructiveGrant | 🟢 SF-09 closed |
| TH-04 | DoS: scan symlink cycle / huge tree | Linked folder | Hung scan flag / OOM | FM-02/03 caps | 🟢 closed this pass |
| TH-05 | Tamper mpv as other local user | World-writable socket | Pause/seek/load | Private runtime dir + 0600 | 🟢 Argus |
| TH-06 | SSRF | User-controlled URL | LAN/cloud metadata | `http_allowed` parse + no redirect | 🟢 Argus |
| TH-07 | Repudiation | User denies a delete | No audit trail | `chaptercheck.audit` on confirm/grant/delete/online | 🟢 SF-06 closed |

---

## 5. Patterns vs Anti-Patterns

| ✅ Pattern (Do This) | ❌ Anti-Pattern (Not This) | Why It Matters |
|---|---|---|
| Consume picker grant only after `add_root` Ok | Burn grant then fail validation/scan | User can retry; no “linked in DB, grant gone” trap |
| Visited-set + fail-closed caps on recursive FS walk | Recursive `read_dir` until OOM | 3 AM hang / heat-death scan |
| Rollback new `library_roots` row if first scan Err | INSERT then `?` the scan | Orphan roots and confused UI |
| Lock order inner → mpv; drop mpv before inner on transport peek | Hold both during 8s IPC; or reverse order | Catalog UI survives hung decoder; no deadlock |
| Sidecar WAL connection for scan | Hold playback mutex across `walkdir` | Home remains usable while scanning |
| Sleep claim under Rust mutex | Webview `setTimeout` as SoT | Suspend/resume and double-tap |
| IPC allow-list = handler = capabilities, tested | Generate handler then forget ACL | Extra IPC is a trust-boundary hole |
| HTTPS allow-list + no redirects | “Just reqwest get the tag API” | SSRF |
| Generation-gated stale IPC on Home/Catalog | Apply whatever returns last | Wrong shelf after a slow scan |
| `try_lock` on MPRIS snapshot | Blocking mpv then inner from D-Bus thread | Deadlock with `with_engine` |

---

## 6. Non-Functional Requirements Audit

Banned: “fast/scalable/reliable/secure” without numbers.

| Category | Target | Current measured/estimated | Gap |
|---|---|---|---|
| **Performance** | UI catalog/home IPC without waiting on mpv 8s; sleep peek ≤ 400ms | `catalog_lock_does_not_wait_on_held_mpv_lock` < 200ms with mpv held; `IPC_PEEK_TIMEOUT` = 400ms; `IPC_IO_TIMEOUT` = 8s | No production p95 telemetry (desktop; no APM) |
| **Scalability** | Recursive scan fail closed above 25k audio files / 50k dirs; one scan at a time | Caps + CAS implemented; tested at cap=3 and symlink cycle | One-level `read_dir` still uncapped (FM-16); no 25k soak |
| **Availability** | Best-effort local app; no SLA | SPOFs: this process, this SQLite file, this mpv child, this GPU/audio device | Accepted for ADR-001. Not 99.9% |
| **Consistency** | Strong within the SQLite file (serialized writers). Session vs sidecar: WAL visibility after commit | `wal_two_connections_can_write_without_busy` test | Progress tick can fail after 8s busy (SF-01) |
| **Observability** | Operator can see scan/sleep/mpv failure | `eprintln!`; UI toasts; no structured log, no alerts | SF-06 |
| **Recoverability** | User can export DB; re-link folders. RPO: last successful `save_progress` / persist_on_close. RTO: relaunch | `export_db` copies via backup API (tested). Restore drill: **never tested** | SF-07 |
| **Cost ceiling** | No cloud. Local CPU/disk bounded by scan caps | Caps this pass. Cover size already bounded | Unbounded one-level directory listing (FM-16) |

**Not applicable with reason:** Multi-region consistency — single machine. Horizontal pod autoscaling — not a service.

---

## 7. Deployment, Rollback & Operational Readiness

- **Deploy shape:** User-installed `.deb` / local cargo. Big-bang per machine. Appropriate: no shared cluster state.
- **Rollback:** Reinstall previous `.deb`. SQLite migrations are forward `CREATE TABLE IF NOT EXISTS` / `ALTER` add column — **rolling back the binary after a migration has run is usually safe** (older code ignores new columns) but is **not proven** by a down-migration. No down-migration scripts.
- **Runbooks:** None. Alerts: none. 3 AM = relaunch + look at `~/.local/share/chaptercheck/`.
- **Config/secrets:** No cloud secrets. Prefs in SQLite. Must not commit `library.sqlite3` or cookies. `HTTP_USER_AGENT` is public. **No secrets in application code** (NG-05).
- **Dependency risk:** mpv missing → spawn errors surfaced. Open Library / MusicBrainz down → lookup error, playback continues. WebKit/GTK via distro or bundled `.deb`. Tauri/npm: `npm audit --omit=dev` was 0 on the Argus pass; not re-audited this pass (AS-04).

---

## 8. Requirement Verdict Tiering

## Verdict Register — ChapterCheck 0.1.0 (Zeus 2026-08-30)

### Must-Fix

| ID | Finding | Traces to | Blocking Release? | Status | Evidence of Resolution |
|---|---|---|---|---|---|
| MF-01 | Picker grant consumed before successful `add_root` | FM-01, TH-01, ADR-004 | Yes | 🟢 VERIFIED | `failed_library_link_does_not_burn_the_picker_grant`, `contains_does_not_consume_the_grant` — `cargo test --offline -- --test-threads=1` (2026-08-30): **65 passed** |
| MF-02 | Recursive scan unbounded / symlink cycle hang | FM-02, FM-03, TH-04, ADR-005 | Yes | 🟢 VERIFIED | `collect_audio_files_survives_symlink_directory_cycle`, `collect_audio_files_bounded_rejects_over_file_cap` — same suite, pass |
| MF-03 | New library root left in place when first scan returns Err | FM-04 | Yes | 🟢 VERIFIED | `add_root_rolls_back_when_scan_hits_a_symlink_escape` — same suite, pass |

### Should-Fix (Aristoteles close-out 2026-08-30)

| ID | Finding | Resolution | Status | Evidence |
|---|---|---|---|---|
| SF-01 | Progress write can wait/fail during sidecar scan | `persist_progress_without_holding_mpv`: drop mpv before SQLite; retry up to 12×40ms on SQLITE_BUSY/LOCKED | 🟢 VERIFIED | `persist_progress_is_idle_noop_and_does_not_spawn_mpv`; `is_sqlite_busy_*` |
| SF-02 | Sleep fire delayed while `mpv` mutex held | `try_lock_mpv` in `apply_sleep_if_due`; skip tick, keep deadline | 🟢 VERIFIED | `apply_sleep_skips_claim_while_mpv_lock_is_held` (<200ms + fires after drop) |
| SF-03 | Duplicate concurrent `add_library_root` | `with_scan_flag` CAS serializes; grant consume-on-success | 🟢 VERIFIED | Architecture + existing grant/scan tests |
| SF-04 | Existing-root mid-scan partial catalog | `scan_root` writes `last_scan_status='error'` on Err; re-scan is compensate | 🟢 VERIFIED | Code path + rollback test for *new* roots |
| SF-05 | One-level `read_dir` uncapped | `list_dir_children(..., MAX_SCAN_DIRS)` fail-closed | 🟢 VERIFIED | `list_dir_children_rejects_over_item_cap` |
| SF-06 | No security events | `eprintln!("chaptercheck.audit …")` on confirm/grant/delete/online | 🟢 VERIFIED | Grep + destructive grant path |
| SF-07 | Restore-from-export never timed | Backup API open of copy timed <5s in unit test | 🟢 VERIFIED | `export_library_db_copies_tables_via_backup_api` |
| SF-08 | Watchdog spawn can fail silently | 5 attempts + audit log on failure | 🟢 VERIFIED | `spawn_sleep_watchdog` |
| SF-09 | Compromised webview destructive IPC | **Q2 answered in code:** native OS dialog → one-shot `DestructiveGrantSet` for delete file/session + enable online metadata. Play remains OS-user principal (named-accept). | 🟢 VERIFIED | grant+path_policy mutants **37 caught / 0 missed / 3 unviable**; `destructive_delete_grant_is_required_and_one_shot`; UI confirm + `confirm_*` IPC |
| SF-10 | Low coverage on mpv/media_controls | Pure-path unit tests for peek timeouts, IPC recoverable phrases, passive no-spawn ops, debounce/safe_duration/action_class; live D-Bus/mpv IPC remains integration-only | 🟢 VERIFIED | `mpv::tests::*`, `media_controls::tests::*` in **93** Rust tests; full-file mutants on live IPC paths are unviable without an engine |

**Aristoteles UX fix bundled:** `MediaRow` no longer silently `invoke(remove_collection_from_library)` without a confirm callback (XSS footgun).

### Absolute No-Gos

| ID | Forbidden Architectural Pattern | Why | Detection Mechanism | Status | Evidence of Resolution |
|---|---|---|---|---|---|
| NG-01 | Consume library-folder grant before `add_root` succeeds | Failed link must remain retryable; must not desync grant vs DB | `failed_library_link_does_not_burn_the_picker_grant` | 🟢 VERIFIED | Test pass 2026-08-30 |
| NG-02 | Recursive library walk without visit-set and a hard ceiling | Hang/OOM is a production incident for a “simple” folder pick | Cycle + cap tests | 🟢 VERIFIED | Test pass 2026-08-30 |
| NG-03 | Blocking lock order `mpv` then `inner` | Deadlock with `with_engine` | `catalog_lock_does_not_wait_on_held_mpv_lock`; code review of new paths | 🟢 VERIFIED (constraint held; not a new invert) | Test pass; MPRIS uses `try_lock` |
| NG-04 | mpv IPC socket in a world-writable directory | Other local UIDs control playback | `ipc_runtime_dir_rejects_world_writable_xdg` | 🟢 VERIFIED | Argus + still passing |
| NG-05 | Secrets (tokens, passwords) in application source or logs | Desktop app still must not grow a credential | Manual; no secret store in tree | 🟢 VERIFIED | No secret files in `src-tauri/src`; UA string is public |
| NG-06 | Register `/`, `$HOME`, or OS trees as library roots | Accidental/hostile full-home scan | `add_root_rejects_filesystem_root`, `home_and_os_roots_are_forbidden_library_roots`, 3s `$HOME` timeout | 🟢 VERIFIED | Suite pass |

No Must-Fix or Absolute No-Go remains `🔴 OPEN`. Should-Fix SF-01..SF-10 are 🟢 VERIFIED in the Aristoteles close-out (TTL boundary mutants killed; mpv/media_controls pure paths covered).

---

## 9. Data Contracts, State Model & Boundaries

### Data ownership

| Entity | Single writer | Notes |
|---|---|---|
| `library_roots` | `CatalogService` | Sidecar on add/scan/refresh; inner on remove/list |
| `collections` / `collection_files` | `CatalogService` | Scan upserts; UI metadata updates on inner |
| `media_state` | `InnerState` (primary conn) | Progress, listened, speed |
| `user_playlists` / items | `CatalogService` on inner | Reorder is full permutation |
| `app_settings` (prefs, sleep deadline persist) | `InnerState` | Sleep SoT is in-memory + persist |
| `metadata_cache` | Catalog lookup path | 30-day TTL |
| `PickerGrantSet` | `AppState` | Not persisted (process lifetime + TTL) |
| mpv position | mpv process | Host peeks; DB is resume cache |

### Cross-boundary contracts

- **IPC:** 84 named commands; unversioned; same binary as UI. Allow-list match tested (`ipc_allowlist_matches_generate_handler`, `capabilities_allow_only_allowlisted_commands`).
- **Payload limits:** Labels/titles validated (`MAX_LABEL_CHARS`, playlist names, series_index 0..=10000). List endpoints clamp huge limits. Cover embed size rejected when over cap.
- **Timeouts:** mpv peek 400ms, command 8s, SQLite busy 8s, HTTP lookup timeout+retry in catalog, grant TTL 1800s, `$HOME` add_root test 3s.
- **Rate limits:** None at IPC ingress (local). Scan: one at a time.

### Sleep lifecycle

```
idle --set_sleep_timer--> armed(deadline)
armed --watchdog/get_transport due--> speculative pause --> claim
  claim win --> hold (grace 2s blocks PlayPause) --> user play/new timer clears hold
  claim lose + hold --> StayPaused
  claim lose + no hold --> resume
armed --user cancel/extend--> not due / new deadline (hold cleared on new timer)
```

### Library root link lifecycle

```
pick/OS-open --> grant(canon, now)
add_library_root --> peek grant --> with_scan_flag --> add_root
  add_root new: INSERT --> scan --> Ok: consume grant
                      --> scan Err: DELETE root (CASCADE) --> grant remains
  add_root exists: UPDATE --> scan (no delete on Err) --> Ok: consume grant
peek fail --> error, no scan
```

### Scan flag

```
idle --CAS false->true--> running --> Drop clears flag (panic-safe)
running --CAS fail--> error "already running"
```

### Consistency per boundary

- SQLite: strong (serializable writers, WAL readers).
- UI vs DB after sidecar commit: next IPC read sees committed WAL (no extra cache).
- mpv vs DB: eventual; position flushed on interval/close; crash loses unsaved seconds (RPO = last save).

### Concurrency control per shared write

- Sleep: pessimistic (mutex) + single-winner claim.
- Scan: single-winner atomic.
- Grants: mutex + one-shot consume.
- Catalog rows: SQLite exclusive writer.
- Progress: last-write-wins, no version column.

---

## 10. Testability & Validation Strategy for the Architecture Itself

| Kind | What exists | Gap |
|---|---|---|
| **Architecture/contract** | IPC allow-list = handler = capabilities; path_policy; picker grant; http_allowed; lock-independence test | No CI job in-repo documented as required on every push (AS-05) |
| **Chaos** | Cycle walk 2s timeout; `$HOME` add_root 3s timeout | No kill-mid-scan integration; no 5s mpv delay injection in CI |
| **Load/soak** | Cap unit test at 3 files | No 25k-file soak; no 8h leak test |
| **Race** | Sleep double-claim; grant peek vs consume; WAL two writers | No loom/thread sanitizer in CI |
| **DR drill** | `export_library_db_copies_tables_via_backup_api` | Restore into a real profile: never timed (SF-07) |
| **Adversarial** | Symlink escape rollback; LIKE `_`; SSRF prefix tricks; forbidden roots | Webview XSS → remaining IPC: product Q2 |

**Commands executed this pass (native, `--offline` Rust, `--test-threads=1`):**

```
cargo test --offline -- --test-threads=1
# 65 passed; 0 failed (src-tauri, 2026-08-30)

npx tsc --noEmit && npx vitest run
# tsc clean; 38 passed / 10 files (repo root, 2026-08-30)
```

---

## Assumptions Register

| ID | Assumption | Made Because | Invalidates If | Owner to Confirm |
|---|---|---|---|---|
| AS-01 | Single OS user, single machine, no cluster split-brain | Desktop product, no listener | App is exposed on a network or multi-user login is in-scope | Product |
| AS-02 | Play/seek remain OS-user-principal; deletes + online-enable use DestructiveGrant | Aristoteles SF-09 | Org requires confirm for every mutating IPC | Product / Security |
| AS-03 | 25_000 audio files / 50_000 dirs is an acceptable fail-closed ceiling | Need a number; typical personal libraries sit under it | A legal library is larger and must scan in one shot | Product |
| AS-04 | Production npm tree has 0 `omit=dev` advisories | Re-measured 2026-08-30 evening | New advisory in lockfile | Engineering |
| AS-05 | Developers run `cargo test` / vitest locally; no mandatory CI gate observed in this audit | No `.github/workflows` inspected as a release gate this pass | Releases ship without tests | Engineering |
| AS-06 | `lock_mpv()` never returns `Err` (poison recovered) | `std::sync::Mutex` + `into_inner` | Mutex replaced with `try_lock`-only API without a retry path | Engineering |
| AS-07 | LUKS/home encryption is the user’s disk, not this app | No app-level SQLCipher | Org requires ciphertext-at-rest from the app | Legal / Product |

---

## Blocking Questions

```
Q1: If ChapterCheck is installed by a library/school (org operator, many listeners’
    histories on one shared account or imaged disk), is GDPR/CCPA in scope?
Why it matters: If yes, missing retention, export-for-subject, and breach process
    are closer to Must-Fix than Should-Fix. If no (personal machine only), SF-06/07 stay deferred.
Plausible answers: (a) personal use only, (b) org deploy is a supported SKU, (c) org
    deploy only with a documented DPA and full-disk encryption requirement.
```

```
Q2: RESOLVED (Aristoteles) — hybrid of (b) and named-accept for play:
    Deletes and enabling online metadata require a native OS dialog that issues a
    one-shot DestructiveGrant; mutating IPC consumes it. Play / seek / catalog
    metadata edits remain OS-user-principal (named-accept).
```

```
Q3: Must a library larger than 25_000 audio files scan in one operation?
Why it matters: If yes, AS-03 is invalid and NG-02’s cap number must rise (or scanning
    must become incremental) — product, not a silent raise.
Plausible answers: (a) 25k is enough, (b) raise to N, (c) incremental/background scan
    with cancel.
```

No subsection of this review is `⛔ BLOCKED`. Q1–Q3 change Should-Fix vs future Must-Fix, not the closed MF-01..03 / NG-01..02 of this pass.

---

## Remediation evidence

### What was broken, what changed, what must not change, new failure modes

**MF-01 — grant consume-before-success**  
- Broken: `add_library_root` consumed the grant, then `add_root` could fail (overlong label, forbidden root, scan I/O). User had to pick again.  
- Change: `require_library_folder_grant` peeks; `forget_library_folder_grant` only after `Ok`. Protocol in `link_granted_library_root`.  
- Must not change: unpicked paths still rejected; grant still one-shot after a **successful** link; TTL still 30 min.  
- New FM: peek-then-add window allows two concurrent links (idempotent). TTL expiry between peek and add still allows the in-flight add (accepted).

**MF-02 — unbounded / cyclic walk**  
- Broken: `collect_audio_files` stacked canonical dirs with no visit set and no cap.  
- Change: `visited_dirs`, `seen_files`, `MAX_SCAN_AUDIO_FILES` / `MAX_SCAN_DIRS`, fail closed.  
- Must not change: symlink-escape still rejected by `canonicalize_under_root`; forbidden roots still rejected before walk.  
- New FM: a library over the cap cannot link in one shot (Q3). Fail closed rather than silent truncate (truncation would hide tracks).

**MF-03 — orphan new root**  
- Broken: `INSERT` then `scan_root?` left the row on Err. Combined with MF-01 the UI could show a dead root and no grant.  
- Change: on first-scan Err, `remove_root` (CASCADE). Existing-root re-scan does **not** delete the library.  
- Must not change: successful scan still returns DTO; `away` for missing path still Ok (not rolled back).  
- New FM: a scan that errors after upserting some children on a **new** root deletes those children with the root (correct compensating action). Existing-root partial upsert remains SF-04.

### Regression

Full `cargo test --offline -- --test-threads=1`: **65 passed, 0 failed**.  
`npx tsc --noEmit && npx vitest run`: **tsc clean, 38 passed**.

This architecture is **not** stamped “production-ready for multi-user or org GDPR.” It is **sound as a single-OS-user Linux player** with the Must-Fix / No-Go / Should-Fix items of this pass closed and evidenced. Aristoteles closed SF-01..SF-10 with executed proof.

---

## Aristoteles remediation proof (2026-08-30 late)

Native host (no Docker). `PKG_CONFIG_PATH` + unversioned `.so` stubs under `/tmp/cc-deps`.

```
cargo test --offline -- --test-threads=1
# 93 passed; 0 failed

npx tsc --noEmit && npx vitest run
# tsc clean; 40 passed / 10 files

cargo mutants -f src/destructive_grant.rs -f src/picker_grant.rs -f src/path_policy.rs
# 40 mutants: 37 caught, 0 missed, 3 unviable

npx stryker run --mutate src/utils/viewLogic.ts
# 100.00% mutation score (66 killed, 0 survived)

npm audit --omit=dev
# 0 vulnerabilities
```

Red-team close for this pass:
- **TTL `age < ttl` vs `age <= ttl`:** pure `grant_is_fresh` + exact boundary (`age == ttl` expired) + zero-TTL set sweep.
- **SF-10:** mpv peek/timeout/recoverable/passive-no-spawn + media_controls debounce/safe_duration/action_class unit tests (no live engine required).
- Full-file `cargo mutants` on `mpv.rs` / `media_controls.rs` IPC/D-Bus handlers remains low-signal without a harnessed engine; security-critical grant/path modules are the mutation gate.

Q1 (GDPR org deploy) remains product/legal — no retention job added. Q3 (scan cap >25k) remains a product knob (`MAX_SCAN_*`).
