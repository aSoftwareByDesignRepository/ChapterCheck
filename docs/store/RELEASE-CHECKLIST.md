# Release checklist — ChapterCheck

App: **ChapterCheck** · version **0.1.0** · Linux desktop

---

## A. Accounts & legal

- [ ] Flathub (or Snap) publisher account for **Software by Design GbR**
- [ ] Deploy privacy HTML — [PUBLISH-PRIVACY.md](./PUBLISH-PRIVACY.md)
- [ ] Live privacy URLs return **HTTP 200**
- [ ] Support email monitored during review

---

## B. Store assets

- [ ] [assets/store-icon-512.png](./assets/store-icon-512.png)
- [ ] [assets/feature-graphic-1024x500.png](./assets/feature-graphic-1024x500.png)
- [ ] Listings [LISTING-en.txt](./LISTING-en.txt) / [LISTING-de.txt](./LISTING-de.txt)
- [ ] Categories + keywords per [ASO.md](./ASO.md)
- [ ] Screenshots ≥ 2 (Home + Playing) — **you capture** → `assets/screenshots/`
- [ ] AppStream file reviewed: [flathub/de.softwarebydesign.ChapterCheck.metainfo.xml](./flathub/de.softwarebydesign.ChapterCheck.metainfo.xml)

---

## C. App content

- [ ] [DATA-SAFETY.md](./DATA-SAFETY.md)
- [ ] [CONTENT-RATING.md](./CONTENT-RATING.md)
- [ ] [REVIEWER-ACCESS.md](./REVIEWER-ACCESS.md)
- [ ] [STORE_REVIEW.md](./STORE_REVIEW.md)
- [ ] Ads: **No** · License: **MIT** · Price: **Free**

---

## D. Preflight

```bash
cd /path/to/ChapterCheck   # this repo (audioplayer)
npm install
npm run store:preflight
```

---

## E. Build `.deb` (direct / GitHub Releases)

```bash
# Native Linux host with Tauri GTK/WebKit deps
npm run tauri build
# Artifact: src-tauri/target/release/bundle/deb/ChapterCheck_0.1.0_amd64.deb
npm run install:deb   # optional local install
```

- [ ] Attach `.deb` to GitHub Release `v0.1.0`
- [ ] Tag: `chaptercheck@0.1.0` or `v0.1.0`

---

## F. Flathub

- [ ] Open Flathub PR with Flatpak manifest + AppStream metainfo (ID `de.softwarebydesign.ChapterCheck`)
- [ ] Align runtime sandbox permissions with [DATA-SAFETY.md](./DATA-SAFETY.md)
- [ ] Paste listing EN + DE; link privacy URLs
- [ ] Release notes: [release-notes/0.1.0.txt](./release-notes/0.1.0.txt)

**Note:** Repo Tauri identifier is still `com.chaptercheck`. Flatpak ID should use `de.softwarebydesign.ChapterCheck`; migrate the desktop ID in a follow-up if Flathub requires an exact match.

---

## G. Snap Store (optional)

- [ ] `snapcraft.yaml` + store listing from LISTING-*  
- [ ] Same privacy URLs and screenshots

---

## H. Post-release

- [ ] Announce on product / GitHub
- [ ] Verify `chapter-check` installs and launches on a clean Linux Mint / Ubuntu VM
- [ ] Cross-link from AudioCheck store text if helpful
