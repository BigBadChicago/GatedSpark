#!/usr/bin/env bash
# Build the browser version of the core and put it where the page can load it.
#
# Two steps, and the second one is the part that is easy to forget. `cargo build`
# produces a WebAssembly file, which a browser can load but cannot talk to: the
# translation layer between Rust and a web page needs a matching piece of JavaScript,
# and `wasm-bindgen` is what writes it.
#
# The `wasm-bindgen` program's version must match the `wasm-bindgen` dependency in
# crates/gs-web/Cargo.toml. A mismatch is reported clearly by the program itself, which
# is the one piece of luck in this setup. Install it with:
#
#     cargo install wasm-bindgen-cli --version <the version in that Cargo.toml>
#
# Usage: scripts/build-web.sh [--debug]
set -euo pipefail

cd "$(dirname "$0")/.."

profile="release"
target_dir="release"
if [[ "${1:-}" == "--debug" ]]; then
    profile="dev"
    target_dir="debug"
fi

out="crates/gs-web/www/core"

echo "==> building the core for a browser (${profile})"
cargo build -p gs-web --target wasm32-unknown-unknown --profile "$profile"

echo "==> writing the translation layer into ${out}/"
rm -rf "$out"
wasm-bindgen \
    --target web \
    --no-typescript \
    --out-dir "$out" \
    "target/wasm32-unknown-unknown/${target_dir}/gs_web.wasm"

raw=$(stat -c%s "$out/gs_web_bg.wasm")
gz=$(gzip -c "$out/gs_web_bg.wasm" | wc -c)
echo "==> done: $((raw / 1024)) KB, $((gz / 1024)) KB compressed"
echo
echo "To look at it, serve the folder and open it in a browser:"
echo "    python3 -m http.server 8080 --directory crates/gs-web/www"
echo "    then open http://127.0.0.1:8080/?puzzle=<a puzzle name>"
