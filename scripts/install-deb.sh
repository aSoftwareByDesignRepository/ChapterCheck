#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
deb_dir="$root/src-tauri/target/release/bundle/deb"

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
trap 'rm -f "$tmp_deb"' EXIT

# apt's _apt sandbox user cannot read files under $HOME; install from /tmp instead.
cp "$deb" "$tmp_deb"
chmod 644 "$tmp_deb"

echo "Installing $name"

export DEBIAN_FRONTEND=noninteractive
sudo apt-get -qq -o APT::Install-Recommends=false install -y "$tmp_deb"
