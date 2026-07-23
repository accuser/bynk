#!/usr/bin/env bash
#
# Fetch the print manuscript PDF built by CI (the `bynk-manuscript` artifact)
# and open it. The CI `book` job runs only when book inputs change, so this
# finds the newest non-expired artifact for the branch — regardless of which
# run produced it — and falls back to `main`.
#
#   scripts/fetch-book-pdf.sh            # newest build for the current branch (or main)
#   scripts/fetch-book-pdf.sh main       # newest build on main
#   scripts/fetch-book-pdf.sh <branch>   # newest build on a branch
#   scripts/fetch-book-pdf.sh --watch    # wait for the branch's in-flight run first
#
# Requires the GitHub CLI (`gh auth login`).
set -euo pipefail

repo="${BYNK_BOOK_REPO:-accuser/bynk}"
out="${BYNK_BOOK_PDF:-output/pdf/bynk-manuscript.pdf}"

watch=0
branch=""
for arg in "$@"; do
  case "$arg" in
  --watch) watch=1 ;;
  -*) echo "unknown flag: $arg" >&2; exit 2 ;;
  *) branch="$arg" ;;
  esac
done
[ -n "$branch" ] || branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo main)"

# The workflow-run id of the newest non-expired manuscript artifact on a branch.
newest_run_for() {
  gh api --paginate "repos/$repo/actions/artifacts?per_page=100" \
    --jq "[.artifacts[]
           | select(.name == \"bynk-manuscript\" and .expired == false
                    and .workflow_run.head_branch == \"$1\")]
          | sort_by(.created_at) | last | .workflow_run.id // empty"
}

if [ "$watch" = 1 ]; then
  run_id="$(gh run list -R "$repo" --workflow=ci.yml --branch="$branch" --limit=1 \
    --json databaseId,status -q '.[0] | select(.status != "completed") | .databaseId')"
  if [ -n "$run_id" ]; then
    echo "Waiting for CI run $run_id on '$branch' to finish…"
    gh run watch "$run_id" -R "$repo" --exit-status || true
  fi
fi

run="$(newest_run_for "$branch")"
if [ -z "$run" ]; then
  echo "No manuscript artifact for '$branch'; falling back to main." >&2
  branch=main
  run="$(newest_run_for main)"
fi
[ -n "$run" ] || { echo "No non-expired bynk-manuscript artifact found." >&2; exit 1; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
gh run download "$run" -R "$repo" -n bynk-manuscript -D "$tmp"
mkdir -p "$(dirname "$out")"
mv "$tmp"/bynk-manuscript.pdf "$out"
echo "Wrote $out (from run $run on '$branch')."
case "$(uname)" in Darwin) open "$out" ;; esac
