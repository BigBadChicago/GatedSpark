#!/usr/bin/env bash
# Open the real page in a real browser, play a real puzzle to the end, and take
# pictures of it on the way.
#
# This is not the same thing as `cargo test -p gs-web --target wasm32-unknown-unknown`.
# That proves the core works inside a browser. This proves the *page* works: that a grid
# is drawn, that tapping a square changes it, that undo goes back, that the solved
# message appears when and only when the rules say the board is finished, and that a
# changed puzzle is refused in words a person can read.
#
# Not in CI yet. It needs Playwright's Chromium, which is not a declared dependency of
# this project, and adding one to run a browser is a decision worth making on purpose
# rather than in passing. Run it by hand before changing the page.
#
# Usage: scripts/check-page.sh [output directory for the pictures]
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
www="$here/crates/gs-web/www"
shots="${1:-$here/target/page-pictures}"
port="${PORT:-8099}"

if [ ! -f "$www/core/gs_web.js" ]; then
  echo "The core has not been built for a browser yet. Run scripts/build-web.sh first." >&2
  exit 1
fi
if [ ! -d "$www/store" ] || [ -z "$(ls -A "$www/store" 2>/dev/null)" ]; then
  echo "There are no puzzles to play. Run scripts/demo-web.sh first." >&2
  exit 1
fi

mkdir -p "$shots"
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$www" >/dev/null 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null || true' EXIT
sleep 1

node "$here/scripts/drive-page.mjs" "$shots" "$www" "http://127.0.0.1:$port/"
echo "==> pictures in $shots"
