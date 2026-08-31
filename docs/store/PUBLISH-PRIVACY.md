# Publish privacy HTML — ChapterCheck

| Live URL (target) | This kit |
|-------------------|----------|
| `https://nextcloud.software-by-design.de/en/privacy-chaptercheck.html` | [publish/en/privacy-chaptercheck.html](./publish/en/privacy-chaptercheck.html) |
| `https://nextcloud.software-by-design.de/de/datenschutz-chaptercheck.html` | [publish/de/datenschutz-chaptercheck.html](./publish/de/datenschutz-chaptercheck.html) |

Markdown sources: [privacy-desktop-en.md](./privacy-desktop-en.md), [privacy-desktop-de.md](./privacy-desktop-de.md).

## Deploy

From the ChapterCheck repo root:

```bash
npm run store:privacy
# or: python3 scripts/publish-privacy-to-website.py
```

That writes:

- Kit mirrors under `docs/store/publish/{en,de}/` (self-contained HTML for reviewers)
- Live site PHP stubs + templates under `../nextcloud-dev/website/` (canonical `.html` URLs rewrite to `.php`)

Commit and deploy the **website** repo, then confirm HTTP 200:

```bash
curl -sI 'https://nextcloud.software-by-design.de/en/privacy-chaptercheck.html' | head -1
curl -sI 'https://nextcloud.software-by-design.de/de/datenschutz-chaptercheck.html' | head -1
```

Paste the EN URL into Flathub / Snap privacy fields; add DE for the German listing.

Until the website deploy is live, URLs may 404 — do **not** submit to Flathub without live privacy pages.
