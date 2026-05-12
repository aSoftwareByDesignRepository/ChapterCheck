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

Release build (Linux `.deb`):

```bash
npm run tauri build
```

Install the package (name matches **productName** + version + arch; use what `ls` shows):

```bash
sudo apt install ./src-tauri/target/release/bundle/deb/ChapterCheck_0.1.0_amd64.deb
```

If the version changes, run `ls src-tauri/target/release/bundle/deb/*.deb` and pass that path to `apt install`.

Data (library DB) is stored under the OS user data directory (XDG on Linux), not inside the repo.

## Author

**Alexander Mäule** — [info@software-by-design.de](mailto:info@software-by-design.de)  
Part of **Software by Design**; see also the [Nextcloud apps](http://nextcloud.software-by-design.de) in the same ecosystem (e.g. ArbeitszeitCheck, TicketCheck, ProjectCheck).

## License

MIT — see [LICENSE](LICENSE).
