#!/usr/bin/env bash
# Build the bynk-wasm crate to wasm32 and generate the browser glue with
# wasm-bindgen (in-browser track, slice 4). Pass `--release` for a small,
# wasm-opt-ready artefact; default is debug (fast, large) for local iteration.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
repo="$(cd "$here/.." && pwd)"

profile="debug"
cargo_flags=()
if [[ "${1:-}" == "--release" ]]; then
  profile="release"
  cargo_flags=(--release)
fi

cd "$repo"
cargo build --target wasm32-unknown-unknown -p bynk-wasm "${cargo_flags[@]}"
wasm-bindgen --target web \
  --out-dir "$here/src/vendor" \
  "$repo/target/wasm32-unknown-unknown/$profile/bynk_wasm.wasm"

# Release: shrink with wasm-opt when available (the Q3 size budget). Optional —
# the artefact is correct without it.
if [[ "$profile" == "release" ]] && command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -Oz -o "$here/src/vendor/bynk_wasm_bg.wasm" "$here/src/vendor/bynk_wasm_bg.wasm"
  echo "wasm-opt -Oz applied"
fi
echo "wasm glue + module staged in playground/src/vendor/ ($profile)"
