# ChapterCheck

Desktop audiobook and music player for **Linux** (e.g. Linux Mint). Remembers playback position, supports variable speed, and opens folders as playlists with sorting.

## Features

- **Resume** — SQLite-backed progress per file (long single-file audiobooks supported).
- **Library catalog** — Linked folders, collections, playlists, optional Open Library / MusicBrainz lookup.
- **Sleep timer** — Process-level timer (not just the window); pauses playback even if the UI is idle.
- **Speed** — Per-track speed; optional default for new tracks.
- **Playlists** — Open a folder or a single file; sort by name, date, or size.
- **Engine** — [mpv](https://mpv.io/) for broad codec support and reliable seeking.

## Data location

On Linux the library database and cover cache live under the XDG data directory, typically:

`~/.local/share/chaptercheck/library.sqlite3`  
`~/.local/share/chaptercheck/covers/`

If `XDG_DATA_HOME` is set, that directory is used instead (`$XDG_DATA_HOME/chaptercheck/…`). This is **not** Tauri’s `$APPDATA` path (`~/.local/share/com.chaptercheck/`). The webview may load cover images only from that covers folder (asset protocol). Audio files are played by mpv, not by the webview.

This README is a getting-started guide, not a security spec. IPC and invariants are in `documentation/chaptercheck/qa-report/`.

## Requirements

- [mpv](https://mpv.io/) on `PATH` (e.g. `sudo apt install mpv`), or set `MPV_PATH` to the binary.
- [Rust](https://rustup.rs/) and [Tauri v2 Linux prerequisites](https://v2.tauri.app/start/prerequisites/) (WebKitGTK, build tools, SSL dev libs, etc.).
- Node.js 18+ for the frontend toolchain.

## Quick start

```bash
git clone https://github.com/aSoftwareByDesignRepository/ChapterCheck.git
cd ChapterCheck
npm install
npm run tauri dev
```

Release build (Linux `.deb`):

```bash
npm run tauri build
```

Install the built package:

```bash
npm run install:deb
```

That copies the newest `.deb` from `src-tauri/target/release/bundle/deb/` to `/tmp` and installs it with `apt-get` (so apt’s `_apt` sandbox can read the file). Do not run `sudo apt install ./…/*.deb` directly from the repo — paths under `$HOME` trigger permission warnings even when the install succeeds.

## Author

**Alexander Mäule** — [info@software-by-design.de](mailto:info@software-by-design.de)  
Part of **Software by Design**; see also the [Nextcloud apps](http://nextcloud.software-by-design.de) in the same ecosystem (e.g. ArbeitszeitCheck, TicketCheck, ProjectCheck).

## License

MIT — see [LICENSE](LICENSE).
