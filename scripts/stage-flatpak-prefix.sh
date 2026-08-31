#!/usr/bin/env bash
# Stage a Flatpak prefix from the installed .deb / release binary for local flatpak-builder.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STAGE="$ROOT/docs/store/flathub/staged-prefix"
BIN="${CHAPTERCHECK_BIN:-/usr/bin/audioplayer}"
ICON_SRC="$ROOT/docs/store/assets/store-icon-512.png"
ICON_256="$ROOT/src-tauri/icons/256x256.png"
ICON_128="$ROOT/src-tauri/icons/128x128.png"
META_SRC="$ROOT/docs/store/flathub/de.softwarebydesign.ChapterCheck.metainfo.xml"
DESKTOP_SRC="$ROOT/docs/store/flathub/de.softwarebydesign.ChapterCheck.desktop"

if [[ ! -x "$BIN" ]]; then
  echo "Missing binary: $BIN (build/install first)" >&2
  exit 1
fi

rm -rf "$STAGE"
mkdir -p \
  "$STAGE/bin" \
  "$STAGE/share/applications" \
  "$STAGE/share/metainfo" \
  "$STAGE/share/icons/hicolor/512x512/apps" \
  "$STAGE/share/icons/hicolor/256x256/apps" \
  "$STAGE/share/icons/hicolor/128x128/apps"

install -Dm755 "$BIN" "$STAGE/bin/audioplayer"
install -Dm644 "$DESKTOP_SRC" "$STAGE/share/applications/de.softwarebydesign.ChapterCheck.desktop"
install -Dm644 "$META_SRC" "$STAGE/share/metainfo/de.softwarebydesign.ChapterCheck.metainfo.xml"
install -Dm644 "$ICON_SRC" "$STAGE/share/icons/hicolor/512x512/apps/de.softwarebydesign.ChapterCheck.png"
install -Dm644 "$ICON_256" "$STAGE/share/icons/hicolor/256x256/apps/de.softwarebydesign.ChapterCheck.png"
install -Dm644 "$ICON_128" "$STAGE/share/icons/hicolor/128x128/apps/de.softwarebydesign.ChapterCheck.png"

echo "Staged $STAGE"
echo "Next: flatpak-builder --user --force-clean /tmp/cc-fp docs/store/flathub/de.softwarebydesign.ChapterCheck.yml"
