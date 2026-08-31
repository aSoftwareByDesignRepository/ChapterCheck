# Flathub draft — ChapterCheck

App ID: `de.softwarebydesign.ChapterCheck`

## Local stage + build

```bash
# From ChapterCheck repo root (requires installed /usr/bin/audioplayer)
npm run store:flatpak-stage
flatpak-builder --user --force-clean /tmp/cc-fp \
  docs/store/flathub/de.softwarebydesign.ChapterCheck.yml
```

`staged-prefix/` is gitignored. For Flathub, replace the `dir` source with a GitHub Release tarball + `sha256`.

## Still needed before Flathub PR

1. Live privacy URLs (HTTP 200) — deploy website PHP pages
2. Hosted binary / source URL with checksum
3. Bundled or runtime **mpv** story (draft expects host/runtime ffmpeg; Flathub will reject a silent host dependency)
4. App ID rename from current Tauri `com.chaptercheck` if shipping Flatpak as primary
