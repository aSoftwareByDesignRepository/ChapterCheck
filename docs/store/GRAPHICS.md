# Store graphics — ChapterCheck

| Asset | Size | Notes |
|-------|------|-------|
| App icon | 512×512 PNG | [assets/store-icon-512.png](./assets/store-icon-512.png) — from `src-tauri/icons/256x256.png` |
| Feature graphic | 1024×500 | [assets/feature-graphic-1024x500.png](./assets/feature-graphic-1024x500.png) |
| Screenshots | ≥ 2 | **You capture** — see below |

Regenerate icon + banner:

```bash
python3 scripts/generate-store-graphics.py
```

## Feature graphic brief (1024×500)

- Background: dark stage `#0e1014` (matches in-app dark theme)
- Left: app icon on elevated plate
- Right text: **ChapterCheck** · *Local audiobook & music player for Linux* · mint accent subline

## Screenshot content

Capture on a 1280×800 (or similar) Linux desktop window:

1. **home-empty** — welcome + one **Add my folder** CTA (dark theme)
2. **home-continue** — continue row + mini player playing
3. **catalog** (optional) — list of titles with clear play controls

Place files in `docs/store/assets/screenshots/` as:

- `desktop-01-home.png`
- `desktop-02-playing.png`
- `desktop-03-catalog.png` (optional)

See [assets/screenshots/README.md](./assets/screenshots/README.md).
