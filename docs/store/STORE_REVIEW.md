# Store review notes — ChapterCheck

For Flathub / Snap reviewers and internal QA before upload.

## Product scope

- **In scope:** Local library folders, mpv playback, resume, speed, sleep timer, MPRIS, optional metadata lookup.
- **Out of scope:** Nextcloud login, phone builds, multi-user server, DRM storefronts.

## Known honest limitations

| Topic | Note |
|-------|------|
| Engine | Requires **mpv** (`.deb` depends on it; Flatpak should bundle or require it) |
| Scan size | Soft fail-closed caps (~25k audio files / 50k dirs) — huge trees get a clear error |
| Online metadata | Off by default; OS confirm + one-shot grant |
| Identifier | Deb uses `com.chaptercheck` / `chapter-check`; Flathub ID planned as `de.softwarebydesign.ChapterCheck` |

## Security posture (summary)

Picker grants, destructive grants for delete / online enable, path policy, private mpv socket, IPC allow-list. See `documentation/chaptercheck/qa-report/zeus-architecture-audit.md`.

## Regression gate before store upload

```bash
npm run store:preflight
```
