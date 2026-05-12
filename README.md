# ChapterCheck

Desktop audiobook and music player for **Linux** (e.g. Linux Mint). Remembers playback position, supports variable speed, and opens folders as playlists with sorting.

## Features

- **Resume** — SQLite-backed progress per file (long single-file audiobooks supported).
- **Speed** — Per-track speed; optional default for new tracks.
- **Playlists** — Open a folder or a single file; sort by name, date, or size.
- **Engine** — [mpv](https://mpv.io/) for broad codec support and reliable seeking.

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

Release build (Linux `.deb` when configured in Tauri):

```bash
npm run tauri build
```

Data (library DB) is stored under the OS user data directory (XDG on Linux), not inside the repo.

## License

MIT — see [LICENSE](LICENSE).
