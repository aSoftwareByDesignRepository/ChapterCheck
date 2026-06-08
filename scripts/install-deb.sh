#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
deb_dir="$root/src-tauri/target/release/bundle/deb"
installed_bin="/usr/bin/audioplayer"

if [[ ! -d "$deb_dir" ]]; then
  echo "No .deb bundle found. Run: npm run tauri build" >&2
  exit 1
fi

mapfile -t debs < <(find "$deb_dir" -maxdepth 1 -name '*.deb' -type f | sort)
if [[ ${#debs[@]} -eq 0 ]]; then
  echo "No .deb file in $deb_dir. Run: npm run tauri build" >&2
  exit 1
fi

deb="${debs[-1]}"
name="$(basename "$deb")"
tmp_deb="$(mktemp "/tmp/chaptercheck.XXXXXX.deb")"
tmp_extract="$(mktemp -d "/tmp/chaptercheck.extract.XXXXXX")"
trap 'rm -f "$tmp_deb"; rm -rf "$tmp_extract"' EXIT

cp "$deb" "$tmp_deb"
chmod 644 "$tmp_deb"
dpkg-deb -x "$deb" "$tmp_extract"
deb_bin="$tmp_extract/usr/bin/audioplayer"

if [[ ! -f "$deb_bin" ]]; then
  echo "Could not find usr/bin/audioplayer inside $name" >&2
  exit 1
fi

echo "Installing $name"
echo "  package: $(stat -c '%y' "$deb_bin")"

export DEBIAN_FRONTEND=noninteractive
# Same deb version (0.1.0) is reused during development; plain "install" is a no-op.
if ! sudo apt-get -o APT::Install-Recommends=false install --reinstall -y "$tmp_deb"; then
  echo "Install failed. sudo/apt did not complete." >&2
  exit 1
fi

if [[ ! -f "$installed_bin" ]]; then
  echo "Install reported success but $installed_bin is missing." >&2
  exit 1
fi

deb_hash="$(md5sum "$deb_bin" | awk '{print $1}')"
installed_hash="$(md5sum "$installed_bin" | awk '{print $1}')"
if [[ "$deb_hash" != "$installed_hash" ]]; then
  echo "Install finished but $installed_bin does not match the .deb package." >&2
  echo "  package:   $deb_hash" >&2
  echo "  installed: $installed_hash" >&2
  exit 1
fi

echo "Installed OK: $installed_bin"
echo "  system: $(stat -c '%y' "$installed_bin")"
echo "Restart ChapterCheck completely (quit the app, then launch again)."
