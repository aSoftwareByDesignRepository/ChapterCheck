# ChapterCheck — Argus Full Security Audit

> **Security Mode: FULL AUDIT**  
> **Object:** already-implemented system (`/home/alex/Development/audioplayer`, product ChapterCheck 0.1.0)  
> **Auditor:** Argus  
> **Date:** 2026-08-30 (UTC)  
> **Environment:** Native Linux host (no `docker-compose.yml` in this repo). GTK/WebKit via `/tmp/cc-deps` for `pkg-config` only.  
> **Active exploitation:** No evidence the running system is under attack right now. This is a white-box audit of code, ACL, CSP, IPC, filesystem, and dependency trees — not a live incident.

**Verdict in one sentence:** This is a local, single-OS-user Linux player. It is **not** a multi-user service and must not be sold as one. Three Must-Fix local/webview issues found in this pass are **patched and proven**. Residual: a compromised webview can still call the remaining IPC surface (delete-in-session, play, metadata lookup). That is an **Open Security Conflict** for the product owner, not a silent “probably fine.”

---

## Table of contents

1. [Stakeholder analysis](#1-multi-perspective-security-stakeholder-analysis)
2. [Asset and data classification](#2-asset--data-classification)
3. [Threat model](#3-threat-modeling)
4. [Vulnerability checklist](#4-vulnerability--weakness-enumeration)
5. [Patterns vs anti-patterns](#5-patterns-vs-anti-patterns)
6. [Verdict register](#6-security-requirement-tiering)
7. [Security NFRs](#7-security-non-functional-requirements)
8. [Secure SDLC](#8-secure-sdlc--testability)
9. [Data protection](#9-data-protection--privacy)
10. [Incident response](#10-incident-response--recoverability)
11. [Assumptions register](#assumptions-register)
12. [Blocking questions](#blocking-questions)
13. [Remediation evidence](#remediation-evidence)

---

## 1. Multi-perspective security stakeholder analysis

| Perspective | Finding |
|---|---|
| **Attacker (opportunistic)** | No internet listener, no login, no default creds. Cheapest abuse is a malicious audiobook tag (parser crash / huge cover) or a stolen `.deb` with a swapped binary. Dev-time `vite` CVEs do not ship in the production `omit=dev` tree (`npm audit --omit=dev` = **0**). |
| **Attacker (targeted)** | XSS or a malicious npm/Tauri dependency in the webview → `invoke("add_library_root")` used to scan `~/Documents` **until this audit**. Next: `lookup_metadata_online` leaks titles to Open Library; `delete_session_files` destroys the current book. Local other-user: world-writable mpv socket under `/tmp` **until this audit**. |
| **Data subject** | Listening history, file paths, and progress live in `~/.local/share/chaptercheck/library.sqlite3`. There is **no** in-app breach-notification path. The listener is also the OS user. |
| **Security / Legal** | No accounts, no payment, no health data. GDPR/CCPA **may** still apply to listening history if the operator is an org (library, school). **Open Security Conflict:** no DPA, no retention job, no export-for-subject besides `export_db`. See Q1. |
| **Engineering** | Blast radius of webview JS is the **full IPC allow-list** (84 commands). Blast radius of a local other-user was mpv IPC + readable SQLite; both tightened this pass. |
| **Ops / IR** | `eprintln!` and no security event log. Compromise at 3 AM is invisible. No alerting. |
| **Business** | Direct cost of a “breach” is local embarrassment + destroyed/exfiltrated listening library, not PCI fines. Trust cost if a `.deb` is backdoored is product-ending. |
| **Downstream** | Outbound HTTPS only to Open Library search + MusicBrainz `/ws/2/` (parsed host/path, no redirects). A compromised app can still **query** those services with user titles. |
| **Future auditor** | Evidence exists in this file + `qa-report/test-execution-log.md` + executed tests. There is **no** CI SAST/secrets gate, **no** pentest of the real Tauri window. |

**Open Security Conflict:** Engineering wants a single-window desktop app where the webview is trusted as “the UI.” Security’s position: the webview is an untrusted renderer. Picker grants close `add_library_root`. They do **not** close `delete_track_file` / `delete_session_files` / `lookup_metadata_online`. Product owner must **named-accept** “OS user = trusted for remaining IPC” or fund capability-token work for destructive commands.

---

## 2. Asset & data classification

## Data Inventory — ChapterCheck

| Data Type | Sensitivity Tier | Where Stored | Who/What Can Access | Encrypted At Rest | Encrypted In Transit | Retention Period |
|---|---|---|---|---|---|---|
| Listening progress, listened flags, speeds | High (behavioral PII) | `~/.local/share/chaptercheck/library.sqlite3` (+ WAL/SHM) | This OS user; was world-readable if umask 022 | No (chmod 0600 now; not ciphertext) | N/A (local) | Until user deletes DB / uninstalls |
| Library root paths, collection titles, authors | High (paths + tastes) | Same SQLite | Same | No (0600) | N/A | Until unlink/remove |
| Embedded cover images | Medium | `…/chaptercheck/covers/*` | OS user; webview via `asset:` scope | No (0600 files / 0700 dir) | N/A | Until collection gone |
| mpv JSON IPC | Critical (process control) | Unix socket in private runtime dir | This OS user only (after fix) | N/A | Unix socket, owner-only | Process lifetime |
| Online metadata cache | Medium | SQLite `metadata_cache` | OS user | No (0600) | TLS 1.2+ via rustls to allow-listed hosts | 30-day TTL in code |
| UI locale, sleep deadline, prefs | Medium | `app_settings` | OS user | No (0600) | N/A | Until changed |
| Passwords / payment / government ID | N/A | Not collected | N/A | N/A | N/A | N/A |
| Application logs | Low | stderr `eprintln!` only | Whoever sees the terminal | No | N/A | Not retained |

No blank cells. “Encrypted at rest = No” is an open residual for High-tier data on an unencrypted home directory (LUKS is the user’s disk, not this app).

---

## 3. Threat modeling

**Trust boundaries**

1. Webview JS ↔ Rust IPC (Tauri ACL + 84 commands).  
2. Rust ↔ mpv Unix socket.  
3. Rust ↔ SQLite files.  
4. Rust ↔ filesystem (library roots, delete, covers).  
5. Rust ↔ HTTPS (Open Library, MusicBrainz).  
6. OS file manager / argv → `open_path_from_os`.  
7. Local other UIDs on a shared machine.

**Attacker tiers:** unauthenticated remote (none facing); authenticated user N/A (no accounts); compromised webview; local other-user; supply-chain (npm/crate/CI).

## Threat Register — ChapterCheck

| ID | Threat | Attacker Tier | Entry Point | Impact | Likelihood | Current Mitigation | Verdict |
|---|---|---|---|---|---|---|---|
| TH-01 | `add_library_root` with raw path scans any non-forbidden folder | Compromised webview | IPC | Index `~/Documents`, then metadata lookup leaks names | High (was trivial) | One-shot picker/OS grant + forbid `$HOME`/OS trees | VERIFIED |
| TH-02 | mpv IPC socket in world-writable dir | Local other-user | `XDG_RUNTIME_DIR=/tmp` or old fallback | Control playback, `loadfile` arbitrary audio | Medium (shared machine) | Refuse other-writable dirs; 0600 socket; `~/.local/share/chaptercheck/run` fallback | VERIFIED |
| TH-03 | SQLite 0644 listening history | Local other-user | `library.sqlite3` | Read all paths/progress | High on multi-user hosts | chmod 0700 data dir, 0600 db/wal/shm/covers | VERIFIED |
| TH-04 | SSRF via metadata URL | Compromised webview / tag | `http_get_json` | Internal HTTP | Low after allow-list | Parsed `https` + host + path; no redirects; no userinfo | VERIFIED |
| TH-05 | SQL injection in catalog filters | Compromised webview | `list_collections` filter/search | DB read/write | Low | Bound params; filter is allow-listed fragments only | Pass (prior) |
| TH-06 | XSS via ID3 titles | Malicious file | React text | Webview RCE-equivalent via IPC | Low | React encoding; CSP `default-src 'self'`; no `dangerouslySetInnerHTML` | Should-watch |
| TH-07 | Destructive IPC (`delete_session_files`) | Compromised webview | IPC | Wipe current session files under library roots | Medium given XSS | Session + `path_delete_allowed`; **no** extra confirm at IPC | SHOULD-FIX |
| TH-08 | Supply-chain RCE in UI deps | Supply-chain | npm / crates | Full user-equivalent | Medium over time | `npm audit --omit=dev` 0 today; **no** CI SCA gate | SHOULD-FIX |
| TH-09 | Cover `asset:` path walk | Compromised webview | `convertFileSrc` | Read files outside covers | Low | Tauri scope `covers/**`; frontend `isSafeCoverPath` | Pass |
| TH-10 | LIKE `_` import sibling folders | User / IPC | playlist import | Wrong files in playlist | Was High | `ESCAPE '!'` | Pass (prior) |

STRIDE per boundary (summary): Spoofing N/A (no auth); Tampering of IPC payloads is the webview; Repudiation fail (no audit log); Disclosure of errors is local strings; DoS via huge scan (partially bounded by forbid roots + cover 8 MiB); EoP is webview → filesystem (TH-01 closed for link-folder).

---

## 4. Vulnerability & weakness enumeration

### 4.1 Authentication & session management

**N/A — no user accounts, passwords, MFA, or reset flow.** Desktop OS session is the authenticator. Residual: webview ≠ OS user intent (see TH-01/TH-07).

### 4.2 Authorization / access control

| Check | Result |
|---|---|
| Object-level checks on delete | Pass: session `allowed_files` **and** `path_delete_allowed` (registered available root) |
| `reopen_recent` | Pass: must match stored recent list (canonical) |
| `add_library_root` | **Was Fail.** Now consume picker/OS grant. |
| Function-level: hidden UI | Fail-as-designed: all 84 commands remain callable if JS runs |
| Least privilege IPC | Partial: ACL matches allow-list; `core:default` includes webview DevTools toggle |

### 4.3 Injection

| Check | Result |
|---|---|
| SQL | Pass: rusqlite `params!` / bound `ToSql`; dynamic SQL is allow-listed fragments |
| Command | Pass: `Command::new(mpv)` with fixed args; path via JSON `loadfile` not shell |
| XSS | Pass for React text; `style-src 'unsafe-inline'` remains |
| SSRF | Pass after URL parse (host/path/scheme/userinfo) |
| XXE | N/A — no XML parser on user documents (JSON APIs) |
| Deserialization | Pass: serde on IPC; JSON from mpv is property reads |

### 4.4 CSRF & clickjacking

**N/A / hardened:** not a cookie website. CSP now includes `frame-ancestors 'none'`. Tauri IPC is same-origin webview, not cross-site form posts.

### 4.5 Secrets management

| Check | Result |
|---|---|
| Secrets in git | Pass (grep): no API keys, no private keys. MusicBrainz/Open Library are keyless. |
| Vault/KMS | N/A — no credentials to store |
| Rotation | N/A |

Absolute No-Go “no secret in source” — **VERIFIED** for this tree. No pre-commit gitleaks in CI (detection gap, Section 8).

### 4.6 Cryptography

TLS to allow-listed hosts via reqwest `rustls-tls`. No app-level at-rest encryption. No home-rolled ciphers. Sleep deadline is a Unix-ms integer in SQLite, not a MAC.

### 4.7 Input validation & file handling

Server-side (Rust) validation on labels, playlist names, series index, sleep minutes, speeds, cover size (8 MiB), forbidden library roots. Audio type for open-file is extension-based (`AUDIO_EXT`) plus “is file”. **Not** magic-byte sniffing (SHOULD-FIX). Covers written only under `covers_data_dir()`.

### 4.8 Supply chain

| Check | Result |
|---|---|
| npm production | `npm audit --omit=dev` → **0** vulnerabilities (2026-08-30) |
| npm including dev | 6 issues (vite/postcss/nanoid high) — **dev server / build**, not the shipped renderer bundle |
| Cargo | No `cargo-audit`/`cargo-deny` in CI; not run this pass (tool not assumed installed) |
| SBOM | None |
| CI pin | No `.github/workflows` in this repo |

### 4.9 Logging, monitoring, data exposure

Fail for IR: no auth-event log, no append-only store, no alert. Errors returned to UI as strings (local). UA string includes project GitHub URL (intentional for MusicBrainz).

### 4.10 Denial of service

Partial: collection list limit 1–200; bulk kind cap 5000; HTTP 12s timeout; cover cap; forbid scanning `/` and `$HOME`. Unbounded recursive scan of a huge allowed folder still possible (user-picked). No IPC rate limit (local).

---

## 5. Patterns vs anti-patterns

| Do this (now in tree) | Not this (found / closed) | Why |
|---|---|---|
| Bound SQL parameters | String-concat user filter into SQL | Injection |
| Allow-list HTTPS host **and** path after `Url::parse` | `starts_with` on the raw string only | Userinfo / parser tricks |
| Picker/OS one-shot grant before `add_library_root` | Trust that the React UI used the dialog | Webview is not the OS |
| Owner-only data dir + db + mpv socket | Default umask 0644 / socket in `/tmp` | Other local UIDs |
| `path_delete_allowed` + session membership | Delete any path the UI sends | Accidental / XSS wipe |
| CSP `default-src 'self'` + no `dangerouslySetInnerHTML` | `eval` / `innerHTML` | XSS → IPC |

---

## 6. Security requirement tiering

## Verdict Register — ChapterCheck

### Must-Fix

| ID | Finding | CVSS (equiv.) | Traces to | Status | Evidence of Resolution |
|---|---|---|---|---|---|
| MF-01 | `add_library_root` accepted any non-forbidden path from IPC | 7.8 High (local, webview) | TH-01 | VERIFIED | `cargo test library_root_link_requires_picker_or_os_grant picker_grant --offline -- --test-threads=1` — pass. Commands: `pick_library_folder` / `open_folder_path_impl` grant; `add_library_root` consumes. |
| MF-02 | mpv IPC could land in a world-writable directory | 7.3 High (shared host) | TH-02 | VERIFIED | `cargo test ipc_runtime_dir_rejects_world_writable_xdg --offline` — pass. Socket chmod 0600 after create. |
| MF-03 | Library DB/covers created with default umask (often 0644/0755) | 6.2 Medium-High | TH-03 | VERIFIED | `restrict_dir_clears_group_and_other_bits`, `restrict_file_clears_group_and_other_bits` — pass. Applied on `LibraryDb::open`, `app_data_dir`, covers, cover writes. |
| MF-04 | SSRF allow-list was prefix-on-string only | 5.8 Medium (defense-in-depth) | TH-04 | VERIFIED | `http_allowed_rejects_offlist_and_prefix_tricks` including `evil@openlibrary.org` — pass. |

### Should-Fix

| ID | Finding | Rationale for deferral | Backlog | Status |
|---|---|---|---|---|
| SF-01 | Remaining 84 IPC commands have no per-action OS intent token | Needs product decision (Q2). Destructive delete is the next token candidate. | Owner: Engineering | Deferred — **named accept required** |
| SF-02 | No CI SAST / SCA / secrets scan | Process, not a live RCE in prod npm | Owner: Engineering | Deferred |
| SF-03 | No security event log / IR pack | Local app; still needed if org-deployed | Owner: Ops | Deferred |
| SF-04 | `mpv.rs` ~9% / `media_controls.rs` 0% line coverage | Engine hard to unit-test without mpv | Owner: Engineering | Deferred |
| SF-05 | `npm audit` high in **dev** (vite, postcss, nanoid) | Not in `omit=dev` production tree | Owner: Engineering | Deferred — run `npm audit fix` on a branch |
| SF-06 | Open-file type is extension-only | Malicious file with `.mp3` still hits lofty/mpv | Owner: Engineering | Deferred |
| SF-07 | No Playwright vs real Tauri window | jsdom axe only | Owner: QA | Deferred |
| SF-08 | `style-src 'unsafe-inline'` | CSS injection, not IPC | Owner: Engineering | Deferred |
| SF-09 | `cargo mutants -f src/picker_grant.rs`: 6/7 caught, 1 equivalent (`<` vs `<=` at TTL edge) | Equivalent mutant | — | Accepted |

### Absolute No-Gos

| ID | Forbidden behavior | Why | Detection | Status | Evidence |
|---|---|---|---|---|---|
| NG-01 | Secret committed to source | Full compromise if repo leaks | Manual grep this audit; **no** CI gitleaks yet | VERIFIED (tree) / detection incomplete | Grep `api_key\|BEGIN PRIVATE\|password=` in app sources — no credentials. GitHub URL in User-Agent is not a secret. |
| NG-02 | Unauthenticated network listener mutating library | Not this architecture | Code review: no bind/HTTP server | VERIFIED | Tauri desktop only |
| NG-03 | Home-rolled password KDF | N/A — no passwords | N/A | N/A | — |
| NG-04 | World-writable mpv control socket | Local EoP | Tests in `fs_privacy` | VERIFIED | MF-02 |
| NG-05 | Silent `add_library_root` of unpicked paths | Webview ≠ user | Tests in `picker_grant` + `library_root_link_requires_picker_or_os_grant` | VERIFIED | MF-01 |

No Must-Fix or Absolute No-Go remains `OPEN` except NG-01 **CI detection** (the tree is clean; the gate is missing → SF-02).

---

## 7. Security non-functional requirements

| Category | Required to specify | This product |
|---|---|---|
| Authentication | Mechanism, MFA, session | OS user session. No MFA. N/A for in-app login. |
| Authorization | Where enforced | Rust IPC: grants, session sets, path policy, recent-list. Not RBAC. |
| Encryption | Rest / transit / keys | Transit: TLS rustls to two hosts. Rest: filesystem mode 0600, not AES. Keys: none. |
| Vulnerability management | Scan cadence / SLA | **None today.** Recommend: `npm audit --omit=dev` + `cargo audit` on every release; Critical <7d, High <30d. |
| Audit logging | What / retention | **None.** Recommend: IPC destructive actions (delete, add root, export) to a local append-only log, 90 days. |
| Incident response SLA | Detect / notify | **None.** Local app: user notices broken playback. Org deploy: 72h GDPR clock is **not** meetable (Q1). |
| Penetration testing | Cadence | Never run against the real window. Recommend before any org/school deploy. |

---

## 8. Secure SDLC & testability

| Control | Status |
|---|---|
| SAST in CI | Missing (no `.github/workflows`) |
| DAST | N/A as website; missing as Tauri window automation |
| SCA | Manual `npm audit` this pass; no cargo-audit in CI |
| SBOM | Missing |
| Secrets scanning | Missing in CI |
| Fuzzing | Missing (lofty/mpv parsers) |
| Security regression tests | Mapped 1:1 to MF-01…MF-04 (see evidence) |
| Pentest scope | Out of scope this engagement: live `.deb`, malicious mp3 corpus, second UID on the box |

**This audit’s executed suites:** `cargo test --offline -- --test-threads=1` → **60 passed**; `npx tsc --noEmit && npx vitest run` → **38 passed**; `npm audit --omit=dev` → **0**; `cargo mutants -f src/picker_grant.rs` → **6 caught, 1 equivalent missed**.

---

## 9. Data protection & privacy

- **Minimization:** Titles/authors from tags and optional online lookup. No account email.  
- **Purpose:** Playback and resume only. Online lookup is optional (`set_online_metadata_enabled`).  
- **Retention:** No purge job. Data lives until uninstall.  
- **Erasure:** User can remove collections / delete DB file. No “delete all telemetry” button (there is no telemetry backend).  
- **Residency:** Local disk; optional HTTPS to US-based catalog APIs (Open Library, MusicBrainz).  
- **Processors:** Internet Archive / MetaBrainz if lookup enabled — no DPA in-app.  
- **Breach clock:** Not operationally meetable (no detection). See Q1.

---

## 10. Incident response & recoverability

| Capability | Reality |
|---|---|
| Detection | None |
| Containment | Kill the process; revoke by deleting `~/.local/share/chaptercheck/` |
| Forensics | WAL may have history; no hash-chained log |
| Credential rotation | N/A (no app secrets). Rotate OS user password if the account is stolen. |
| Communication | None |
| Post-incident | None |

If this were **actively exploited now**: stop the app, copy `library.sqlite3` off-box for forensics, check `ps` for unexpected `mpv --input-ipc-server`, inspect `~/.local/share/chaptercheck/run/` sockets, do not re-open untrusted webviews. **No such indicators were observed in this audit environment.**

---

## Assumptions register

| ID | Assumption | Made because | Invalidates if | Owner to confirm |
|---|---|---|---|---|
| AS-01 | Single personal Linux workstation; other UIDs are rare | Product is a local player | Multi-user kiosk / school image | Product owner |
| AS-02 | Production renderer does not include Vite (dev CVEs out of ship) | `npm audit --omit=dev` = 0; Tauri `frontendDist` is `../dist` | A future bundle inlines a vulnerable Vite runtime | Engineering |
| AS-03 | No Critical-tier secrets exist in the process | Grep + keyless APIs | A future MusicBrainz key or crash-reporter token | Engineering |
| AS-04 | User-picked folders are intentional library roots | Dialog / OS open is consent | Accessibility tools injecting dialog results | Engineering |
| AS-05 | Disk encryption (LUKS) is the user’s problem | App does not ship FDE | Org policy requires app-level at-rest crypto | Legal / IT |
| AS-06 | Not currently exploited | No runtime malware hunt beyond code | Unexpected outbound hosts in `ss`/`lsof` | Ops |

---

## Blocking questions

**Q1:** Does any deploying organization consider ChapterCheck listening history personal data of EU/CCPA residents (shared machines, classroom, library)?  
**Why it matters:** Flips Section 9 from “household app” to GDPR 72h, retention, and DPA with MusicBrainz/Open Library.  
**Plausible answers:** (a) personal use only, (b) org-managed devices, (c) unknown — do not ship to orgs until answered.

**Q2:** Does the product owner **named-accept** that after picker grants, a compromised webview may still call `delete_session_files`, `delete_track_file`, `lookup_metadata_online`, and playback commands?  
**Why it matters:** SF-01. Without acceptance this is an open High residual, not a closed audit.  
**Plausible answers:** (a) accept OS-user trust for remaining IPC, (b) require OS dialog/capability tokens on all destructive and network commands.

Until Q2 is answered, SF-01 stays **Deferred / conflict**, not VERIFIED-as-accepted.

---

## Remediation evidence

Patches in this audit (not suggestions):

- `src-tauri/src/picker_grant.rs` — one-shot, 30-minute grants  
- `src-tauri/src/lib.rs` — grant on pick/open-folder; consume on `add_library_root`  
- `src-tauri/src/fs_privacy.rs` — owner-only dirs/files; refuse other-writable XDG  
- `src-tauri/src/mpv.rs` — private runtime dir; chmod socket 0600  
- `src-tauri/src/db.rs` / `catalog.rs` — 0700/0600 on data, covers, sqlite sidecars  
- `http_allowed` — `reqwest::Url` host/path/scheme/userinfo  
- `tauri.conf.json` CSP — `base-uri`, `object-src`, `frame-ancestors`, `form-action`  
- `src/utils/coverUrl.test.ts` — cover path walk tests  

**Commands run (native, not Docker):**

```
cargo test --offline -- --test-threads=1
# 60 passed, 0 failed

npx tsc --noEmit && npx vitest run
# 38 passed, 0 failed

npm audit --omit=dev
# found 0 vulnerabilities

cargo mutants -f src/picker_grant.rs --timeout 20
# 6 caught, 1 equivalent missed (< vs <= on TTL)
```

---

## Final evaluation

Argus does **not** certify ChapterCheck as “secure against a compromised webview” or “safe for multi-user org deploy.” Argus **does** certify that the four Must-Fix items found in this full audit are closed with executed tests, and that production npm has no known CVEs today.

Ship as a **personal Linux player** only with Q2 explicitly accepted. Do not ship as a shared-kiosk or regulated processor until Q1/Q2 and SF-02/SF-03 are owned.
