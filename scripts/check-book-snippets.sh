#!/usr/bin/env bash
#
# Compile-gate for the print book's Bynk snippet projects.
#
# Working principle 7 of book/README.md ("Compile-test every listing presented
# as a complete program") is otherwise only a manual discipline: CI typesets
# the manuscript but never compiles book/snippets/. This script runs
# `bynkc check` over every snippet project and asserts each one's expected
# outcome:
#
#   * most projects must type-check cleanly (a valid program);
#   * the "rejected" projects that demonstrate a compiler refusal must fail
#     with the exact diagnostic code the chapter quotes;
#   * a project may instead be expected to compile with a specific warning.
#
# Expectations for the non-clean projects live in book/snippets/EXPECTATIONS.tsv.
# Any project not listed there must check cleanly, warnings included.
#
# Override the compiler with BYNKC=/path/to/bynkc; otherwise the debug build is
# used, and built first if absent.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SNIPPETS="$ROOT/book/snippets"
MANIFEST="$SNIPPETS/EXPECTATIONS.tsv"

BYNKC="${BYNKC:-$ROOT/target/debug/bynkc}"
if [ ! -x "$BYNKC" ]; then
  echo "Building bynkc (set BYNKC to skip)..."
  (cd "$ROOT" && cargo build -q -p bynkc) || exit 2
fi

fail=0
checked=0
while IFS= read -r toml; do
  dir="$(dirname "$toml")"
  rel="${dir#"$SNIPPETS"/}"
  checked=$((checked + 1))

  out="$("$BYNKC" check --format short "$dir" 2>&1)"
  rc=$?

  spec="$(awk -F'\t' -v p="$rel" '$1 == p { print $2 "\t" $3; exit }' "$MANIFEST")"
  if [ -z "$spec" ]; then
    kind="pass"
    code=""
  else
    kind="${spec%%$'\t'*}"
    code="${spec#*$'\t'}"
  fi

  ok=1
  msg=""
  case "$kind" in
  pass)
    if [ "$rc" -ne 0 ] || printf '%s' "$out" | grep -q 'error\['; then
      ok=0
      msg="expected a clean check"
    elif printf '%s' "$out" | grep -q 'warning\['; then
      ok=0
      msg="unexpected warning (add it to EXPECTATIONS.tsv if intended)"
    fi
    ;;
  fail)
    if [ "$rc" -eq 0 ]; then
      ok=0
      msg="expected refusal error[$code], but check passed"
    elif ! printf '%s' "$out" | grep -q "error\[$code\]"; then
      ok=0
      msg="expected error[$code]"
    fi
    ;;
  warn)
    if [ "$rc" -ne 0 ] || printf '%s' "$out" | grep -q 'error\['; then
      ok=0
      msg="expected a warning, got an error"
    elif ! printf '%s' "$out" | grep -q "warning\[$code\]"; then
      ok=0
      msg="expected warning[$code]"
    fi
    ;;
  *)
    ok=0
    msg="unknown expectation kind '$kind' in EXPECTATIONS.tsv"
    ;;
  esac

  if [ "$kind" = pass ]; then label="pass"; else label="$kind $code"; fi
  if [ "$ok" -eq 1 ]; then
    printf '  ok    %-42s %s\n' "$rel" "$label"
  else
    printf 'FAIL    %-42s %s\n' "$rel" "$msg"
    printf '%s\n' "$out" | sed 's/^/          | /'
    fail=1
  fi
done < <(find "$SNIPPETS" -name bynk.toml | sort)

echo
if [ "$fail" -eq 0 ]; then
  echo "All $checked snippet projects match their expected diagnostics."
else
  echo "Some snippet projects did not match their expected diagnostics."
fi
exit "$fail"
