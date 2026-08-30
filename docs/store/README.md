# Linux app stores — ChapterCheck

Publication kit for the **ChapterCheck** desktop player (Tauri + mpv). Modeled on the ArbeitszeitCheck Terminal / AudioCheck Play kits, adapted for **Linux stores** (not Google Play / Apple App Store — this app is desktop Linux).

| Store | Status in this kit | Notes |
|-------|--------------------|-------|
| **Flathub** | Primary target | AppStream metainfo + listing copy + graphics |
| **Snap Store** | Optional | Same listing; snapcraft draft later |
| **Direct `.deb`** | Ready via `npm run tauri build` | Website / GitHub Releases |

| File | Use |
|------|-----|
| [LISTING-en.txt](./LISTING-en.txt) | Store listing English |
| [LISTING-de.txt](./LISTING-de.txt) | Store listing German (**DACH**) |
| [ASO.md](./ASO.md) | Discoverability — search phrases, categories |
| [DATA-SAFETY.md](./DATA-SAFETY.md) | Privacy / permissions summary (Flathub + site) |
| [CONTENT-RATING.md](./CONTENT-RATING.md) | Age / content questionnaire answers |
| [REVIEWER-ACCESS.md](./REVIEWER-ACCESS.md) | How Flathub / Snap reviewers test the app |
| [RELEASE-CHECKLIST.md](./RELEASE-CHECKLIST.md) | Build, tag, upload, post-release |
| [GRAPHICS.md](./GRAPHICS.md) | Icon + feature graphic + screenshots |
| [STORE_REVIEW.md](./STORE_REVIEW.md) | Reviewer notes / known limitations |
| [assets/store-icon-512.png](./assets/store-icon-512.png) | High-res icon |
| [assets/feature-graphic-1024x500.png](./assets/feature-graphic-1024x500.png) | Banner / social / Flathub hero |
| [privacy-desktop-en.md](./privacy-desktop-en.md) | Privacy source (EN) |
| [privacy-desktop-de.md](./privacy-desktop-de.md) | Privacy source (DE) |
| [PUBLISH-PRIVACY.md](./PUBLISH-PRIVACY.md) | Deploy HTML to website |
| [publish/en/privacy-chaptercheck.html](./publish/en/privacy-chaptercheck.html) | **Deploy (EN)** |
| [publish/de/datenschutz-chaptercheck.html](./publish/de/datenschutz-chaptercheck.html) | **Deploy (DE)** |
| [flathub/de.softwarebydesign.ChapterCheck.metainfo.xml](./flathub/de.softwarebydesign.ChapterCheck.metainfo.xml) | AppStream / Flathub draft |
| [release-notes/0.1.0.txt](./release-notes/0.1.0.txt) | First public release notes |

**Local check:** `npm run store:preflight`

## Privacy policy URL (required for Flathub)

| Language | Suggested URL |
|----------|----------------|
| EN | `https://nextcloud.software-by-design.de/en/privacy-chaptercheck.html` |
| DE | `https://nextcloud.software-by-design.de/de/datenschutz-chaptercheck.html` |

Deploy HTML from this kit (or the website repo) before submission — see [PUBLISH-PRIVACY.md](./PUBLISH-PRIVACY.md).

## Developer contact

| Field | Value |
|-------|--------|
| Developer | Software by Design GbR |
| Email | info@software-by-design.de |
| Privacy | datenschutz@software-by-design.de |
| Address | Husumer Baum 2, 24837 Schleswig, Germany |

## App identity

| Field | Current (repo) | Flathub recommendation |
|-------|----------------|------------------------|
| Product name | ChapterCheck | ChapterCheck |
| Desktop / package | `chapter-check` / binary `audioplayer` | Keep for `.deb` |
| Tauri identifier | `com.chaptercheck` | Migrate when packaging Flatpak |
| AppStream / Flathub ID | — | **`de.softwarebydesign.ChapterCheck`** |

## Not Google Play / App Store

ChapterCheck is **not** an Android or iOS client. Sister product **AudioCheck** covers phones. Do not file Play / App Store listings for this repo.
