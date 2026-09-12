#!/usr/bin/env bash
# Bundle the browser client into `dist/`, which the node embeds and serves (SPEC.md §3.2).
#
# The bundle is committed so that building the node needs nothing but cargo — a node operator
# should not have to install npm to serve a page. `dist/build.json` records the digest of
# every source file that went in, so CI can tell a stale bundle from a current one without
# depending on the bundler being byte-reproducible.
set -euo pipefail

cd "$(dirname "$0")"

[ -d node_modules ] || npm ci --silent

rm -rf dist
mkdir -p dist

node_modules/.bin/esbuild src/app.js \
    --bundle \
    --format=esm \
    --target=es2022 \
    --minify \
    --legal-comments=none \
    --log-level=warning \
    --define:process.env.NODE_ENV='"production"' \
    --outfile=dist/app.js

# The DOM-free half on its own, so the browser path can be tested without a browser.
node_modules/.bin/esbuild src/index.js \
    --bundle \
    --format=esm \
    --target=es2022 \
    --legal-comments=none \
    --log-level=warning \
    --define:process.env.NODE_ENV='"production"' \
    --outfile=dist/popcorn.mjs

cp public/index.html public/app.css dist/
cp ../assets/popcorn-logo.jpg dist/logo.jpg

# The digest of the inputs, so a source change without a rebuild is detectable.
node digest.mjs --write > /dev/null

printf "bundle: %s bytes\n" "$(wc -c < dist/app.js)"
