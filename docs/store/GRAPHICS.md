# Store graphics — ChapterCheck

| Asset | Size | Notes |
|-------|------|-------|
| App icon | 512×512 PNG | [assets/store-icon-512.png](./assets/store-icon-512.png) |
| Feature graphic | 1024×500 | [assets/feature-graphic-1024x500.png](./assets/feature-graphic-1024x500.png) |
| Screenshots | 5 captures | [assets/screenshots/](./assets/screenshots/) — real Linux window |

Regenerate icon + banner:

```bash
python3 scripts/generate-store-graphics.py
```

## Feature graphic brief (1024×500)

- Background: dark stage `#0e1014` (matches in-app dark theme)
- Left: app icon on elevated plate
- Right text: **ChapterCheck** · *Local audiobook & music player for Linux* · mint accent subline

## Screenshots (captured)

| File | Content |
|------|---------|
| `desktop-01-home.png` | Home — continue / music shelf |
| `desktop-02-playing.png` | Session with transport visible |
| `desktop-03-music.png` | Music catalog |
| `desktop-04-playlists.png` | Playlists |
| `desktop-05-audiobooks.png` | Audiobooks catalog |

Target window ≈ 1280×800, dark theme. See [assets/screenshots/README.md](./assets/screenshots/README.md).
