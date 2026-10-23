#!/usr/bin/env bash
# Make a handful of real puzzles and put them where the demo page can load them.
#
# This is for looking at the page on your own machine. It is not how anything ships:
# it writes a development signing key, and the key it trusts is written into a file
# beside the page.
#
# The key belongs to the PAGE, not to the puzzles. A page that trusted whatever key
# list it found next to the content would trust whatever anybody put there, so in a
# real deployment this value is built into the page when the page is built. Here a
# script writes it, which is the same thing done by hand.
#
# Usage: scripts/demo-web.sh [how many per level]
set -euo pipefail

cd "$(dirname "$0")/.."

count="${1:-2}"
key="${TMPDIR:-/tmp}/gatedspark-demo.key"
www="crates/gs-web/www"

if [[ ! -f "$key" ]]; then
    echo "==> making a development signing key at $key"
    cargo run --quiet -p gs-cli -- keygen --out "$key"
fi

echo "==> making $count nonograms per level (this asks a real solver about each one)"
rm -rf "$www/store"
cargo run --quiet -p gs-cli -- fill \
    --store "$www/store" \
    --key "$key" \
    --count "$count"

# Both games into the one store, because that is the point: one store, one page, one
# key, two games. A page that needed a separate store per game would have needed a
# separate everything per game.
echo "==> making $count slitherlinks, 6 squares square"
cargo run --quiet --release -p gs-cli -- fill \
    --store "$www/store" \
    --key "$key" \
    --family slitherlink \
    --size 6 \
    --count "$count"

echo "==> writing the page's trusted key"
key_id=$(grep '^key-id: ' "$key" | cut -d' ' -f2)
public=$(grep '^public-key: ' "$key" | cut -d' ' -f2)
cat > "$www/trusted-key.js" <<JSEOF
// The key this page accepts puzzles from. Part of the PAGE, never part of the content:
// a page that trusted a key it found next to a puzzle would trust whatever anybody put
// there. Written by scripts/demo-web.sh for local use; in a real build this is baked in
// when the page is built.
export const TRUSTED_KEY_ID = "${key_id}";
export const TRUSTED_PUBLIC_KEY = "${public}";
JSEOF

echo "==> writing the list of puzzles the page can offer"
{
    echo "// Which puzzles this copy of the page knows about, newest build first."
    echo "// A real deployment gets this from wherever it lists content; the page only"
    echo "// needs names, because a name is a puzzle's own fingerprint."
    echo "export const PUZZLES = ["
    for dir in "$www"/store/*/; do
        name=$(basename "$dir")
        echo "  \"${name}\","
    done
    echo "];"
} > "$www/puzzles.js"

made=$(find "$www/store" -maxdepth 1 -type d | tail -n +2 | wc -l)
echo
echo "==> $made puzzles ready. To look at the page:"
echo "    python3 -m http.server 8080 --directory $www"
echo "    then open http://127.0.0.1:8080/"
echo
echo "==> to try withdrawing one, pick a name from the list above and run:"
echo "    cargo run -p gs-cli -- withdraw --store $www/store --key $key \\"
echo "        --add <name> --reason many-answers"
echo "    then press 'Check for withdrawn puzzles' on the page"
