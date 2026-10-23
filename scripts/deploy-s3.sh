#!/usr/bin/env bash
# Upload a published folder to an Amazon S3 bucket, so that somebody who is not you can
# play.
#
# `gatedspark publish` writes the folder. This puts it on a web host. Nothing here
# decides anything about a puzzle: every check still happens in the browser that opens
# it, which is why an ordinary bucket of files is enough and there is no program running
# on the server.
#
# Usage: scripts/deploy-s3.sh <folder> <bucket> [region]
#
#   scripts/deploy-s3.sh ./site jbravo-gated-spark us-east-2
#
# What the bucket needs first is in docs/deploying-to-s3.md. Run that once; this script
# is the part you run every time.
set -euo pipefail

site="${1:?which folder to upload, as written by: gatedspark publish --out <dir>}"
bucket="${2:?which bucket}"
region="${3:-us-east-2}"

if [ ! -f "$site/index.html" ]; then
    echo "There is no page in $site. Run 'gatedspark publish --out $site --page crates/gs-web/www' first." >&2
    exit 1
fi
if [ ! -f "$site/store/schedule.gspk" ]; then
    echo "There is no schedule in $site, so a visitor would have nothing to open." >&2
    exit 1
fi
# Publishing the answers is the one mistake that cannot be taken back: once they are on
# a public host they have been seen. `gatedspark publish` never copies them, and this is
# the second lock on the same door, because this script is pointed at a folder somebody
# chose and nothing stops that folder being a store.
if find "$site" -name 'witness.bin' -print -quit | grep -q .; then
    echo "REFUSING: $site contains answer files (witness.bin)." >&2
    echo "That looks like a store rather than a published folder. Uploading it would" >&2
    echo "publish the answer to every puzzle. Use 'gatedspark publish --out' instead." >&2
    exit 1
fi

echo "==> uploading $site to s3://$bucket ($region)"

# Three kinds of file, three sets of rules.
#
# **The puzzles never change.** A puzzle's name is its own fingerprint, so the file at
# that name can never be a different file: if the bytes changed, the name would change.
# That is what lets them be cached forever, and it is a property of how they are named
# rather than a promise anybody has to keep.
#
# The browser module and the page's own code are the same shape of thing per build, but
# their names do NOT carry a fingerprint, so they get an ordinary short cache.
#
# **The schedule and the withdrawal list must not be cached for long.** Both are the
# files that say what has changed. A cached withdrawal list is a withdrawal that has not
# arrived yet, and that is the one delay in this whole design that actually hurts
# somebody: a player carries on with a puzzle that cannot be finished. The signature and
# the serial number stop an old file being *forced* on a device; only a short cache stops
# it arriving late.

# The puzzles: immutable, and named by their own fingerprint.
aws s3 sync "$site/store" "s3://$bucket/store" \
    --region "$region" \
    --exclude "schedule.gspk" --exclude "withdrawals.gspk" \
    --content-type "application/octet-stream" \
    --cache-control "public, max-age=31536000, immutable" \
    --no-progress

# The two files that say what has changed. Uploaded after the puzzles, so a schedule
# never names a day whose puzzle is not up yet.
aws s3 cp "$site/store/schedule.gspk" "s3://$bucket/store/schedule.gspk" \
    --region "$region" \
    --content-type "application/octet-stream" \
    --cache-control "no-cache" \
    --no-progress
if [ -f "$site/store/withdrawals.gspk" ]; then
    aws s3 cp "$site/store/withdrawals.gspk" "s3://$bucket/store/withdrawals.gspk" \
        --region "$region" \
        --content-type "application/octet-stream" \
        --cache-control "no-cache" \
        --no-progress
fi

# The page. `.wasm` is the one that has to be right rather than merely tidy: a browser
# refuses to start a WebAssembly module that does not arrive as `application/wasm`, and
# the failure reads as a broken page rather than a wrong file type.
aws s3 sync "$site" "s3://$bucket" \
    --region "$region" \
    --exclude "store/*" --exclude "*.wasm" --exclude "*.js" --exclude "*.html" \
    --cache-control "public, max-age=300" \
    --no-progress
aws s3 sync "$site" "s3://$bucket" \
    --region "$region" \
    --exclude "*" --include "*.wasm" \
    --content-type "application/wasm" \
    --cache-control "public, max-age=300" \
    --no-progress
aws s3 sync "$site" "s3://$bucket" \
    --region "$region" \
    --exclude "*" --include "*.js" \
    --content-type "text/javascript; charset=utf-8" \
    --cache-control "public, max-age=300" \
    --no-progress
aws s3 sync "$site" "s3://$bucket" \
    --region "$region" \
    --exclude "*" --include "*.html" \
    --content-type "text/html; charset=utf-8" \
    --cache-control "no-cache" \
    --no-progress

echo
echo "==> done. The page is at:"
echo "    http://$bucket.s3-website.$region.amazonaws.com/"
echo
echo "If that address says 404, static website hosting is not turned on for the bucket"
echo "yet. See docs/deploying-to-s3.md."
