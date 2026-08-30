# Data safety / permissions — ChapterCheck

**App:** ChapterCheck (Linux desktop)  
**Package:** `chapter-check` · target Flathub ID `de.softwarebydesign.ChapterCheck`

## Summary for users

> ChapterCheck plays audio files from folders you choose. Progress and library data stay on your computer. Optional title/cover lookup talks to Open Library and MusicBrainz only after you confirm in an OS dialog. No ads. No analytics.

## Collect or share data?

**Limited / optional.** Core use is offline. Online metadata is opt-in.

## Data types

### On device (always local)

| Type | Stored? | Shared? | Purpose |
|------|---------|---------|---------|
| Library folder paths | Yes | No | Scan and play your files |
| Playback progress / speed | Yes | No | Resume listening |
| Cover cache (files) | Yes | No | Show album art |
| UI prefs (locale, theme) | Yes | No | Appearance |

Location: typically `~/.local/share/chaptercheck/` (not uploaded).

### Optional network

| Type | Collected? | Shared with | Required? | Purpose |
|------|------------|-------------|-----------|---------|
| Book / artist titles you already have | Only if online lookup enabled | Open Library, MusicBrainz (HTTPS) | No | Covers and display names |

### Not collected

Advertising ID, analytics, contacts, location, financial data, health, photos (except covers you already have), account passwords.

## Encryption

| | Answer |
|---|--------|
| Data in transit (optional lookup) | **Yes** (HTTPS, allow-listed hosts, no redirects) |
| Data at rest | Local files under your user account (OS permissions) |

## Deletion

Delete the app data directory and uninstall the package. No Software by Design account holds your library.

## Account creation

**No** — single-OS-user desktop app.

## Flathub permission story (expected)

| Permission | Why |
|------------|-----|
| Home / documents access (user-selected) | Play files from linked folders |
| Network (optional) | Metadata lookup when enabled |
| Audio / PulseAudio / PipeWire | Playback via mpv |
| D-Bus (session) | MPRIS media keys |
