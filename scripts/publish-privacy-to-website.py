#!/usr/bin/env python3
"""Publish ChapterCheck privacy pages into the store kit + public website.

Expects sibling checkout: ../nextcloud-dev/website (or WEBSITE_ROOT).
Live site prefers PHP stubs (.html URLs rewrite to .php via .htaccess).

Run from ChapterCheck repo root:
  python3 scripts/publish-privacy-to-website.py
"""
from __future__ import annotations

import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
KIT = ROOT / "docs" / "store"
DEFAULT_WEBSITE = ROOT.parent / "nextcloud-dev" / "website"
WEBSITE = Path(os.environ.get("WEBSITE_ROOT", DEFAULT_WEBSITE))

BASE = "https://nextcloud.software-by-design.de"
EN_FILE = "privacy-chaptercheck.html"
DE_FILE = "datenschutz-chaptercheck.html"
EN_SLUG = "privacy-chaptercheck"
DE_SLUG = "datenschutz-chaptercheck"


def content_en() -> str:
    return """    <section class="page-head">
      <div class="container">
        <nav class="page-head__crumbs" aria-label="Breadcrumb"><a href="index.html">Home</a> · <span aria-current="page">ChapterCheck — Privacy</span></nav>
        <span class="eyebrow">Legal · Desktop app</span>
        <h1>Privacy policy — ChapterCheck</h1>
        <p class="lead">This notice covers the <strong>ChapterCheck</strong> Linux desktop player. Your audio files and listening progress stay on <strong>your computer</strong>.</p>
        <p class="muted"><small>Last updated: 31 August 2026</small></p>
      </div>
    </section>
    <section class="section">
      <div class="container">
        <div class="prose">
          <h2 id="publisher">Publisher</h2>
          <address>
            Software by Design GbR<br>
            Husumer Baum 2<br>
            24837 Schleswig, Germany<br>
            Email: <a href="mailto:info@software-by-design.de">info@software-by-design.de</a><br>
            Privacy: <a href="mailto:datenschutz@software-by-design.de">datenschutz@software-by-design.de</a>
          </address>
          <p>Package: <code>chapter-check</code> · Flathub ID (target): <code>de.softwarebydesign.ChapterCheck</code></p>
          <p>General website privacy: <a href="privacy.html">website privacy notice</a>.</p>

          <h2 id="scope">1. Scope</h2>
          <p>ChapterCheck plays audio from folders you choose. Software by Design does <strong>not</strong> operate a cloud library for this app.</p>

          <h2 id="device">2. Data on your device</h2>
          <div class="table-wrap"><table>
            <thead><tr><th scope="col">Data</th><th scope="col">Purpose</th><th scope="col">Storage</th></tr></thead>
            <tbody>
              <tr><td>Linked folder paths</td><td>Library / playback</td><td>Local SQLite under <code>~/.local/share/chaptercheck/</code> (typical)</td></tr>
              <tr><td>Progress, speed, listened flags</td><td>Resume</td><td>Same database</td></tr>
              <tr><td>Cover cache</td><td>Display</td><td><code>…/chaptercheck/covers/</code></td></tr>
              <tr><td>UI preferences</td><td>Appearance</td><td>Local settings</td></tr>
            </tbody>
          </table></div>

          <h2 id="network">3. Optional network</h2>
          <p>If you enable <strong>online metadata</strong>, titles and artist names you already have may be sent to Open Library and MusicBrainz over HTTPS. This is <strong>off by default</strong> and requires an OS confirmation. Audio files are not uploaded.</p>

          <h2 id="not">4. What we do not do</h2>
          <ul>
            <li>No sale of personal data</li>
            <li>No advertising or analytics SDKs</li>
            <li>No Software by Design account for ChapterCheck</li>
            <li>No transfer of your library to Software by Design servers</li>
          </ul>

          <h2 id="permissions">5. Permissions</h2>
          <div class="table-wrap"><table>
            <thead><tr><th scope="col">Access</th><th scope="col">Why</th></tr></thead>
            <tbody>
              <tr><td>Files / folders you select</td><td>Playback</td></tr>
              <tr><td>Network (optional)</td><td>Metadata when enabled</td></tr>
              <tr><td>Audio output</td><td>Playback via mpv</td></tr>
              <tr><td>Session D-Bus</td><td>Media keys (MPRIS)</td></tr>
            </tbody>
          </table></div>

          <h2 id="deletion">6. Deletion</h2>
          <p>Remove the ChapterCheck data directory and uninstall the package.</p>

          <h2 id="rights">7. Your rights (GDPR)</h2>
          <p>Contact <a href="mailto:datenschutz@software-by-design.de">datenschutz@software-by-design.de</a> for questions about this app.</p>

          <h2 id="changes">8. Changes</h2>
          <p>We may update this policy when the app changes. The “Last updated” date will change accordingly.</p>
        </div>
      </div>
    </section>
"""


def content_de() -> str:
    return """    <section class="page-head">
      <div class="container">
        <nav class="page-head__crumbs" aria-label="Brotkrumen-Navigation"><a href="index.html">Start</a> · <span aria-current="page">ChapterCheck — Datenschutz</span></nav>
        <span class="eyebrow">Rechtliches · Desktop-App</span>
        <h1>Datenschutzerklärung — ChapterCheck</h1>
        <p class="lead">Diese Erklärung gilt für den <strong>ChapterCheck</strong>-Linux-Desktop-Player. Ihre Audiodateien und Ihr Hörfortschritt bleiben auf <strong>Ihrem Rechner</strong>.</p>
        <p class="muted"><small>Zuletzt aktualisiert: 31. August 2026</small></p>
      </div>
    </section>
    <section class="section">
      <div class="container">
        <div class="prose">
          <h2 id="publisher">Anbieter</h2>
          <address>
            Software by Design GbR<br>
            Husumer Baum 2<br>
            24837 Schleswig, Deutschland<br>
            E-Mail: <a href="mailto:info@software-by-design.de">info@software-by-design.de</a><br>
            Datenschutz: <a href="mailto:datenschutz@software-by-design.de">datenschutz@software-by-design.de</a>
          </address>
          <p>Paket: <code>chapter-check</code> · Flathub-ID (Ziel): <code>de.softwarebydesign.ChapterCheck</code></p>
          <p>Allgemeiner Website-Datenschutz: <a href="datenschutz.html">Datenschutzerklärung der Website</a>.</p>

          <h2 id="scope">1. Geltungsbereich</h2>
          <p>ChapterCheck spielt Audio aus von Ihnen gewählten Ordnern. Software by Design betreibt <strong>keine</strong> Cloud-Bibliothek für diese App.</p>

          <h2 id="device">2. Daten auf dem Gerät</h2>
          <div class="table-wrap"><table>
            <thead><tr><th scope="col">Daten</th><th scope="col">Zweck</th><th scope="col">Speicherung</th></tr></thead>
            <tbody>
              <tr><td>Verknüpfte Ordnerpfade</td><td>Bibliothek / Wiedergabe</td><td>Lokale SQLite unter <code>~/.local/share/chaptercheck/</code> (typisch)</td></tr>
              <tr><td>Fortschritt, Tempo, gehört-Markierungen</td><td>Fortsetzen</td><td>Dieselbe Datenbank</td></tr>
              <tr><td>Cover-Cache</td><td>Anzeige</td><td><code>…/chaptercheck/covers/</code></td></tr>
              <tr><td>UI-Einstellungen</td><td>Darstellung</td><td>Lokale Einstellungen</td></tr>
            </tbody>
          </table></div>

          <h2 id="network">3. Optionales Netzwerk</h2>
          <p>Wenn Sie <strong>Online-Metadaten</strong> erlauben, können bereits lokale Titel und Künstlernamen an Open Library und MusicBrainz per HTTPS gesendet werden. Standardmäßig aus; OS-Bestätigung erforderlich. Audiodateien werden nicht hochgeladen.</p>

          <h2 id="not">4. Was wir nicht tun</h2>
          <ul>
            <li>Kein Verkauf personenbezogener Daten</li>
            <li>Keine Werbe- oder Analyse-SDKs</li>
            <li>Kein Software-by-Design-Konto für ChapterCheck</li>
            <li>Keine Übertragung Ihrer Bibliothek an Server von Software by Design</li>
          </ul>

          <h2 id="permissions">5. Berechtigungen</h2>
          <div class="table-wrap"><table>
            <thead><tr><th scope="col">Zugriff</th><th scope="col">Grund</th></tr></thead>
            <tbody>
              <tr><td>Von Ihnen gewählte Dateien / Ordner</td><td>Wiedergabe</td></tr>
              <tr><td>Netzwerk (optional)</td><td>Metadaten, wenn aktiviert</td></tr>
              <tr><td>Audioausgabe</td><td>Wiedergabe über mpv</td></tr>
              <tr><td>Session-D-Bus</td><td>Medientasten (MPRIS)</td></tr>
            </tbody>
          </table></div>

          <h2 id="deletion">6. Löschung</h2>
          <p>ChapterCheck-Datenordner entfernen und das Paket deinstallieren.</p>

          <h2 id="rights">7. Ihre Rechte (DSGVO)</h2>
          <p>Fragen zur App: <a href="mailto:datenschutz@software-by-design.de">datenschutz@software-by-design.de</a>.</p>

          <h2 id="changes">8. Änderungen</h2>
          <p>Wir können diese Erklärung aktualisieren. Das Datum „Zuletzt aktualisiert“ ändert sich entsprechend.</p>
        </div>
      </div>
    </section>
"""


def kit_html(lang: str, title: str, description: str, canonical: str, body: str, alt_href: str, alt_label: str) -> str:
    """Self-contained kit HTML for reviewers (no site chrome dependency)."""
    return f"""<!doctype html>
<html lang="{lang}">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{title}</title>
  <meta name="description" content="{description}" />
  <meta name="color-scheme" content="dark light" />
  <link rel="canonical" href="{canonical}" />
  <style>
    :root {{ color-scheme: light dark; --bg: #0e1014; --text: #f5f7fb; --muted: #c2c8d6; --accent: #6ef0bf; --card: #1a1d27; }}
    @media (prefers-color-scheme: light) {{
      :root {{ --bg: #eef2f7; --text: #12151c; --muted: #3a4454; --accent: #0b6b56; --card: #fff; }}
    }}
    body {{ margin: 0; font: 1.05rem/1.55 system-ui, sans-serif; background: var(--bg); color: var(--text); }}
    main {{ max-width: 42rem; margin: 0 auto; padding: 2rem 1.25rem 4rem; }}
    h1 {{ font-size: 1.75rem; letter-spacing: -0.02em; }}
    h2 {{ font-size: 1.15rem; margin-top: 1.75rem; }}
    .meta {{ color: var(--muted); font-size: 0.95rem; }}
    a {{ color: var(--accent); }}
    table {{ width: 100%; border-collapse: collapse; margin: 1rem 0; font-size: 0.95rem; }}
    th, td {{ border: 1px solid color-mix(in oklab, var(--muted) 35%, transparent); padding: 0.5rem 0.6rem; text-align: left; vertical-align: top; }}
    th {{ background: var(--card); }}
  </style>
</head>
<body>
  <main>
{body}
    <p class="meta"><a href="{alt_href}">{alt_label}</a></p>
  </main>
</body>
</html>
"""


def page_def_en() -> str:
    return f"""<?php
declare(strict_types=1);

return [
    'locale' => 'en',
    'navId' => '',
    'title' => 'Privacy policy — ChapterCheck',
    'description' => 'Privacy policy for ChapterCheck, the Linux desktop audiobook and music player by Software by Design GbR.',
    'canonicalUrl' => '{BASE}/en/{EN_FILE}',
    'ogLocale' => 'en',
    'robots' => 'noindex,nofollow',
    'depth' => 0,
    'content' => SBD_WEBSITE_ROOT . '/templates/content/en/{EN_SLUG}.php',
    'hreflang' => [
        ['href' => '{EN_FILE}', 'hreflang' => 'en', 'lang' => 'en', 'label' => 'English', 'current' => true],
        ['href' => '../de/{DE_FILE}', 'hreflang' => 'de', 'lang' => 'de', 'label' => 'Deutsch', 'current' => false],
    ],
    'headExtra' => <<<'HTML'
<link rel="alternate" type="text/plain" href="../llms.txt" title="Canonical machine-readable facts (GEO / retrieval)" />
HTML
,
    'bodyScripts' => [],
];
"""


def page_def_de() -> str:
    return f"""<?php
declare(strict_types=1);

return [
    'locale' => 'de',
    'navId' => '',
    'title' => 'Datenschutzerklärung — ChapterCheck',
    'description' => 'Datenschutzerklärung für ChapterCheck, den Linux-Desktop-Hörbuch- und Musikplayer von Software by Design GbR.',
    'canonicalUrl' => '{BASE}/de/{DE_FILE}',
    'ogLocale' => 'de',
    'robots' => 'noindex,nofollow',
    'depth' => 0,
    'content' => SBD_WEBSITE_ROOT . '/templates/content/de/{DE_SLUG}.php',
    'hreflang' => [
        ['href' => '../en/{EN_FILE}', 'hreflang' => 'en', 'lang' => 'en', 'label' => 'English', 'current' => false],
        ['href' => '{DE_FILE}', 'hreflang' => 'de', 'lang' => 'de', 'label' => 'Deutsch', 'current' => true],
    ],
    'headExtra' => <<<'HTML'
<link rel="alternate" type="text/plain" href="../llms.txt" title="Maschinenlesbare Fakten (GEO / Retrieval)" />
HTML
,
    'bodyScripts' => [],
];
"""


def stub(page_rel: str) -> str:
    return f"""<?php
declare(strict_types=1);

require_once __DIR__ . '/../lib/bootstrap.php';

use Sbd\\Website\\Template\\Page;

Page::renderFile(__DIR__ . '/../templates/pages/{page_rel}');
"""


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    print(f"wrote {path}")


def main() -> None:
    en_body = content_en()
    de_body = content_de()

    # Kit: self-contained HTML for store reviewers / offline
    write(
        KIT / "publish" / "en" / EN_FILE,
        kit_html(
            "en",
            "Privacy policy — ChapterCheck",
            "Privacy policy for ChapterCheck, the Linux desktop audiobook and music player by Software by Design GbR.",
            f"{BASE}/en/{EN_FILE}",
            en_body,
            f"../de/{DE_FILE}",
            "Deutsch",
        ),
    )
    write(
        KIT / "publish" / "de" / DE_FILE,
        kit_html(
            "de",
            "Datenschutzerklärung — ChapterCheck",
            "Datenschutzerklärung für ChapterCheck, den Linux-Desktop-Hörbuch- und Musikplayer von Software by Design GbR.",
            f"{BASE}/de/{DE_FILE}",
            de_body,
            f"../en/{EN_FILE}",
            "English",
        ),
    )

    if not WEBSITE.is_dir():
        print(f"website root not found ({WEBSITE}); kit mirrors only")
        return

    # Live site: PHP stubs + page defs + content (preferred over static HTML)
    write(WEBSITE / "templates" / "content" / "en" / f"{EN_SLUG}.php", en_body)
    write(WEBSITE / "templates" / "content" / "de" / f"{DE_SLUG}.php", de_body)
    write(WEBSITE / "templates" / "pages" / "en" / f"{EN_SLUG}.php", page_def_en())
    write(WEBSITE / "templates" / "pages" / "de" / f"{DE_SLUG}.php", page_def_de())
    write(WEBSITE / "en" / f"{EN_SLUG}.php", stub(f"en/{EN_SLUG}.php"))
    write(WEBSITE / "de" / f"{DE_SLUG}.php", stub(f"de/{DE_SLUG}.php"))

    # Remove leftover static HTML so rewrite always hits PHP
    for leftover in (
        WEBSITE / "en" / EN_FILE,
        WEBSITE / "de" / DE_FILE,
    ):
        if leftover.is_file():
            leftover.unlink()
            print(f"removed stale static {leftover}")


if __name__ == "__main__":
    main()
