# Publish privacy HTML — ChapterCheck

| Live URL (target) | This kit |
|-------------------|----------|
| `https://nextcloud.software-by-design.de/en/privacy-chaptercheck.html` | [publish/en/privacy-chaptercheck.html](./publish/en/privacy-chaptercheck.html) |
| `https://nextcloud.software-by-design.de/de/datenschutz-chaptercheck.html` | [publish/de/datenschutz-chaptercheck.html](./publish/de/datenschutz-chaptercheck.html) |

Markdown sources: [privacy-desktop-en.md](./privacy-desktop-en.md), [privacy-desktop-de.md](./privacy-desktop-de.md).

## Deploy

1. Copy the HTML files into the website repo (`website/en/`, `website/de/`) **or** serve these kit files as-is.
2. Confirm HTTP 200:

```bash
curl -sI 'https://nextcloud.software-by-design.de/en/privacy-chaptercheck.html' | head -1
curl -sI 'https://nextcloud.software-by-design.de/de/datenschutz-chaptercheck.html' | head -1
```

3. Paste the EN URL into Flathub / Snap privacy fields; add DE for the German listing.

Until deploy, URLs may 404 — do **not** submit to Flathub without live privacy pages.
