#!/usr/bin/env bash
# Build tree-sitter-bynk to a wasm grammar for web-tree-sitter (the playground's
# syntax highlighting — in-browser track, slice 4). Uses the repo-local
# tree-sitter CLI; `build --wasm` compiles via emscripten if present, else docker.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
repo="$(cd "$here/.." && pwd)"
grammar="$repo/tree-sitter-bynk"
ts="$grammar/node_modules/.bin/tree-sitter"

mkdir -p "$here/src/vendor"
cd "$grammar"
# Emits tree-sitter-bynk.wasm in CWD.
"$ts" build --wasm
mv -f tree-sitter-bynk.wasm "$here/src/vendor/tree-sitter-bynk.wasm"
# The web-tree-sitter runtime wasm + the highlight query, shipped alongside.
# (0.26 renamed the runtime wasm file from tree-sitter.wasm to web-tree-sitter.wasm;
# we keep our own vendored name, referenced by tshighlight.ts's locateFile.)
cp -f "$here/node_modules/web-tree-sitter/web-tree-sitter.wasm" "$here/src/vendor/tree-sitter.wasm"
cp -f "$grammar/queries/highlights.scm" "$here/src/vendor/highlights.scm"
echo "grammar wasm + runtime + highlights.scm staged in playground/src/vendor/"
