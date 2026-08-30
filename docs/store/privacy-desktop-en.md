# Privacy policy — ChapterCheck (Linux desktop)

**Last updated:** 30 August 2026  
**App:** ChapterCheck  
**Package:** `chapter-check` · Flathub ID (target): `de.softwarebydesign.ChapterCheck`  
**Publisher:** Software by Design GbR, Husumer Baum 2, 24837 Schleswig, Germany  
**Contact:** info@software-by-design.de · datenschutz@software-by-design.de  
**General privacy (website):** https://software-by-design.de/datenschutz/

---

## 1. Scope

This policy covers the **ChapterCheck** desktop application for Linux. The app plays audio files from folders you choose on your computer. Software by Design does **not** operate a cloud library for ChapterCheck.

---

## 2. Controller

| Location | Controller |
|----------|------------|
| App binary & this policy | Software by Design GbR |
| Your audio files & listening progress | You (on your device) |

---

## 3. Data stored on the device

| Data | Purpose | Storage |
|------|---------|---------|
| Linked folder paths | Library scan / playback | Local SQLite under `~/.local/share/chaptercheck/` (typical) |
| Playback position, speed, listened flags | Resume | Same database |
| Cover image cache | Display | `…/chaptercheck/covers/` |
| UI preferences (language, theme) | Appearance | Local settings / webview storage |

---

## 4. Optional network requests

If you enable **online metadata**, the app may send **titles and artist names you already have locally** to:

- Open Library  
- MusicBrainz  

over **HTTPS**. This is **off by default** and requires an **operating-system confirmation** dialog. No audio file contents are uploaded for that feature.

---

## 5. What we do not do

- No sale of personal data  
- No third-party advertising or analytics SDKs  
- No Software by Design account for ChapterCheck  
- No transmission of your library to Software by Design servers  

---

## 6. Permissions (Linux)

| Access | Reason |
|--------|--------|
| Files you select / linked folders | Playback |
| Network (optional) | Metadata lookup when enabled |
| Audio output | Playback via mpv |
| Session D-Bus | Media keys (MPRIS) |

---

## 7. Your rights

Delete local data by removing the ChapterCheck data directory and uninstalling the package. Questions: datenschutz@software-by-design.de

---

## 8. Changes

The current version is published at the privacy URL listed in the store listing.
