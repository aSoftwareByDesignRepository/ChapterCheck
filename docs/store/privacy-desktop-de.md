# Datenschutzerklärung — ChapterCheck (Linux-Desktop)

**Zuletzt aktualisiert:** 30. August 2026  
**App:** ChapterCheck  
**Paket:** `chapter-check` · Flathub-ID (Ziel): `de.softwarebydesign.ChapterCheck`  
**Anbieter:** Software by Design GbR, Husumer Baum 2, 24837 Schleswig, Deutschland  
**Kontakt:** info@software-by-design.de · datenschutz@software-by-design.de  
**Allgemeiner Datenschutz:** https://software-by-design.de/datenschutz/

---

## 1. Geltungsbereich

Diese Erklärung gilt für die **ChapterCheck**-Desktop-App unter Linux. Die App spielt Audiodateien aus von Ihnen gewählten Ordnern. Software by Design betreibt **keine** Cloud-Bibliothek für ChapterCheck.

---

## 2. Verantwortliche

| Ort | Verantwortung |
|-----|----------------|
| App und diese Erklärung | Software by Design GbR |
| Ihre Audiodateien und Hörfortschritt | Sie (auf Ihrem Gerät) |

---

## 3. Daten auf dem Gerät

| Daten | Zweck | Speicherung |
|-------|-------|-------------|
| Verknüpfte Ordnerpfade | Bibliothek / Wiedergabe | Lokale SQLite unter `~/.local/share/chaptercheck/` (typisch) |
| Position, Tempo, gehört-Markierungen | Fortsetzen | Dieselbe Datenbank |
| Cover-Cache | Anzeige | `…/chaptercheck/covers/` |
| UI-Einstellungen (Sprache, Design) | Darstellung | Lokale Einstellungen |

---

## 4. Optionale Netzwerkanfragen

Wenn Sie **Online-Metadaten** erlauben, kann die App **bereits lokal vorliegende Titel und Künstlernamen** an Open Library und MusicBrainz per **HTTPS** senden. Das ist **standardmäßig aus** und erfordert eine **Bestätigung im Betriebssystem**. Audiodateien werden dafür nicht hochgeladen.

---

## 5. Was wir nicht tun

- Kein Verkauf personenbezogener Daten  
- Keine Werbe- oder Analyse-SDKs Dritter  
- Kein Software-by-Design-Konto für ChapterCheck  
- Keine Übertragung Ihrer Bibliothek an Server von Software by Design  

---

## 6. Berechtigungen (Linux)

| Zugriff | Grund |
|---------|-------|
| Von Ihnen gewählte Dateien / Ordner | Wiedergabe |
| Netzwerk (optional) | Metadaten, wenn aktiviert |
| Audioausgabe | Wiedergabe über mpv |
| Session-D-Bus | Medientasten (MPRIS) |

---

## 7. Ihre Rechte

Lokale Daten löschen Sie durch Entfernen des ChapterCheck-Datenordners und Deinstallation. Fragen: datenschutz@software-by-design.de

---

## 8. Änderungen

Die aktuelle Fassung steht unter der Datenschutz-URL im Store-Eintrag.
