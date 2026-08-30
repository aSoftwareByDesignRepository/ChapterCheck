#!/usr/bin/env bash
# Pre-submit checks for Linux store kits (Flathub / Snap / .deb) — ChapterCheck.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> Typecheck"
npx tsc --noEmit

echo "==> Unit / journey / a11y tests"
npm test

echo "==> Store kit files"
required=(
  docs/store/README.md
  docs/store/LISTING-en.txt
  docs/store/LISTING-de.txt
  docs/store/ASO.md
  docs/store/DATA-SAFETY.md
  docs/store/CONTENT-RATING.md
  docs/store/REVIEWER-ACCESS.md
  docs/store/RELEASE-CHECKLIST.md
  docs/store/GRAPHICS.md
  docs/store/STORE_REVIEW.md
  docs/store/privacy-desktop-en.md
  docs/store/privacy-desktop-de.md
  docs/store/PUBLISH-PRIVACY.md
  docs/store/publish/en/privacy-chaptercheck.html
  docs/store/publish/de/datenschutz-chaptercheck.html
  docs/store/assets/feature-graphic-1024x500.png
  docs/store/assets/store-icon-512.png
  docs/store/flathub/de.softwarebydesign.ChapterCheck.metainfo.xml
  docs/store/release-notes/0.1.0.txt
  docs/store/assets/screenshots/README.md
  scripts/generate-store-graphics.py
)
missing=0
for f in "${required[@]}"; do
  if [[ ! -f "$f" ]]; then
    echo "Missing: $f"
    missing=1
  fi
done
[[ "$missing" -eq 0 ]] || exit 1

echo "==> Graphics dimensions"
python3 - <<'PY'
from pathlib import Path
from PIL import Image
root = Path('.')
checks = {
    'docs/store/assets/store-icon-512.png': (512, 512),
    'docs/store/assets/feature-graphic-1024x500.png': (1024, 500),
}
for rel, size in checks.items():
    im = Image.open(root / rel)
    assert im.size == size, f'{rel}: expected {size}, got {im.size}'
    print(f'OK {rel} {im.size}')
PY

echo "==> Listing privacy URLs present"
rg -q 'privacy-chaptercheck\.html' docs/store/LISTING-en.txt
rg -q 'datenschutz-chaptercheck\.html' docs/store/LISTING-de.txt

echo "==> store:preflight OK"
