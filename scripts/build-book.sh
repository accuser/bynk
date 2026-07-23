#!/usr/bin/env bash
# Build or continuously preview the print manuscript with its pinned toolchain.
set -euo pipefail

readonly TYPST_VERSION="0.15.0"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly FONT_DIR="$REPO_ROOT/book/fonts"
readonly FONT_MANIFEST="$FONT_DIR/SHA256SUMS"
readonly INPUT="$REPO_ROOT/book/main.typ"
readonly OUTPUT="${BYNK_BOOK_OUTPUT:-$REPO_ROOT/output/pdf/bynk-manuscript.pdf}"

temporary_dir=""

log() {
  printf '%s\n' "$*" >&2
}

die() {
  log "error: $*"
  exit 1
}

cleanup() {
  if [[ -n "$temporary_dir" && -d "$temporary_dir" ]]; then
    rm -rf -- "$temporary_dir"
  fi
}

trap cleanup EXIT HUP INT TERM

usage() {
  cat <<'EOF'
Usage: scripts/build-book.sh [build|watch]

  build  Compile output/pdf/bynk-manuscript.pdf (the default).
  watch  Recompile the PDF when manuscript sources change.

The script uses Typst 0.15.0. If that version is not on PATH, it downloads a
verified official binary into the ignored book/build/toolchain directory.

Environment:
  BYNK_TYPST_BIN   Use this Typst executable (it must be version 0.15.0).
  BYNK_BOOK_OUTPUT Override the generated PDF path.
  SOURCE_DATE_EPOCH
                   Override the PDF creation timestamp.
EOF
}

sha256_file() {
  local file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    die "sha256sum or shasum is required"
  fi
}

typst_version() {
  "$1" --version | awk 'NR == 1 {print $2}'
}

verify_typst() {
  local binary="$1"
  [[ -x "$binary" ]] || die "Typst executable not found: $binary"

  local version
  version="$(typst_version "$binary")"
  [[ "$version" == "$TYPST_VERSION" ]] ||
    die "Typst $TYPST_VERSION is required; $binary reports $version"
}

bootstrap_typst() {
  local platform asset checksum
  case "$(uname -s):$(uname -m)" in
    Darwin:arm64 | Darwin:aarch64)
      platform="aarch64-apple-darwin"
      checksum="fe53838737abf93a774495952a1a797b4686e9c4a21c2d99b9fdf77f46cc3572"
      ;;
    Darwin:x86_64)
      platform="x86_64-apple-darwin"
      checksum="30210c7c539c7954db94c063cd98b43fd0a0cad285d656dbbce2a40aee2e79be"
      ;;
    Linux:aarch64 | Linux:arm64)
      platform="aarch64-unknown-linux-musl"
      checksum="cdf50ffc7b8ba759ed02200632eda3d78eb8b99aacb6611f4f75684990647620"
      ;;
    Linux:x86_64)
      platform="x86_64-unknown-linux-musl"
      checksum="59b207df01be2dab9f13e80f73d04d7ff8273ffd46b3dd1b9eef5c60f3eeabea"
      ;;
    *)
      die "automatic Typst installation supports macOS and Linux on arm64 or x86_64; set BYNK_TYPST_BIN to an exact Typst $TYPST_VERSION executable"
      ;;
  esac

  asset="typst-$platform.tar.xz"
  local tool_dir="$REPO_ROOT/book/build/toolchain/typst-$TYPST_VERSION-$platform"
  local binary="$tool_dir/typst"

  if [[ -x "$binary" ]] && [[ "$(typst_version "$binary")" == "$TYPST_VERSION" ]]; then
    printf '%s\n' "$binary"
    return
  fi

  command -v curl >/dev/null 2>&1 || die "curl is required to download Typst"
  command -v tar >/dev/null 2>&1 || die "tar is required to unpack Typst"

  temporary_dir="$(mktemp -d "${TMPDIR:-/tmp}/bynk-typst.XXXXXX")"
  local archive="$temporary_dir/$asset"
  local url="https://github.com/typst/typst/releases/download/v$TYPST_VERSION/$asset"

  log "Downloading Typst $TYPST_VERSION for $platform..."
  curl --fail --location --retry 3 --silent --show-error \
    --output "$archive" "$url"

  local actual
  actual="$(sha256_file "$archive")"
  [[ "$actual" == "$checksum" ]] ||
    die "Typst archive checksum mismatch: expected $checksum, got $actual"

  tar -xJf "$archive" -C "$temporary_dir"
  mkdir -p "$tool_dir"
  install -m 0755 "$temporary_dir/typst-$platform/typst" "$binary"
  verify_typst "$binary"
  cleanup
  temporary_dir=""
  log "Installed Typst in book/build/toolchain."
  printf '%s\n' "$binary"
}

resolve_typst() {
  if [[ -n "${BYNK_TYPST_BIN:-}" ]]; then
    verify_typst "$BYNK_TYPST_BIN"
    printf '%s\n' "$BYNK_TYPST_BIN"
    return
  fi

  if command -v typst >/dev/null 2>&1; then
    local system_typst
    system_typst="$(command -v typst)"
    if [[ "$(typst_version "$system_typst")" == "$TYPST_VERSION" ]]; then
      printf '%s\n' "$system_typst"
      return
    fi
    log "Ignoring $system_typst because it is not Typst $TYPST_VERSION."
  fi

  bootstrap_typst
}

verify_fonts() {
  [[ -f "$FONT_MANIFEST" ]] || die "font checksum manifest not found: $FONT_MANIFEST"

  local expected filename actual count=0
  while read -r expected filename; do
    [[ -n "$expected" ]] || continue
    [[ "$expected" == \#* ]] && continue
    [[ -n "$filename" ]] || die "invalid entry in $FONT_MANIFEST"
    [[ -f "$FONT_DIR/$filename" ]] || die "vendored font not found: book/fonts/$filename"

    actual="$(sha256_file "$FONT_DIR/$filename")"
    [[ "$actual" == "$expected" ]] ||
      die "font checksum mismatch for $filename: expected $expected, got $actual"
    count=$((count + 1))
  done <"$FONT_MANIFEST"

  [[ "$count" -gt 0 ]] || die "no fonts listed in $FONT_MANIFEST"
}

command="${1:-build}"
case "$command" in
  build | watch) ;;
  -h | --help | help)
    usage
    exit 0
    ;;
  *)
    usage >&2
    die "unknown command: $command"
    ;;
esac
[[ "$#" -le 1 ]] || die "unexpected argument: $2"

verify_fonts
typst_bin="$(resolve_typst)"
verify_typst "$typst_bin"

creation_timestamp="${SOURCE_DATE_EPOCH:-}"
if [[ -z "$creation_timestamp" ]] && command -v git >/dev/null 2>&1; then
  creation_timestamp="$(
    git -C "$REPO_ROOT" log -1 --format=%ct -- \
      book scripts/build-book.sh 2>/dev/null || true
  )"
fi

common_args=(
  --root "$REPO_ROOT"
  --font-path "$FONT_DIR"
  --ignore-system-fonts
)
if [[ -n "$creation_timestamp" ]]; then
  common_args+=(--creation-timestamp "$creation_timestamp")
fi

mkdir -p "$(dirname "$OUTPUT")"
if [[ "$command" == "watch" ]]; then
  log "Watching the manuscript; writing $OUTPUT"
  exec "$typst_bin" watch "${common_args[@]}" "$INPUT" "$OUTPUT"
fi

log "Building the manuscript with Typst $TYPST_VERSION..."
"$typst_bin" compile "${common_args[@]}" "$INPUT" "$OUTPUT"
log "Built $OUTPUT"
