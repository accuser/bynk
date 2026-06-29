#!/usr/bin/env bash
# Verify every local link in llms.txt still resolves.
#
# llms.txt is hand-authored (the curated index + primer), so it can drift from
# the Book when a page is renamed or moved. This is a lighter cousin of the
# build-llms-full.sh drift check: it does not regenerate anything, it only
# confirms each repo-relative link target (a file or directory under the repo)
# exists. External links (http/https/mailto) and pure #anchors are skipped, as
# are links inside fenced or inline code, which are not real links.
#
#   scripts/check-llms-links.sh        report broken local links; non-zero if any
#
# CI runs this in the docs job alongside the llms-full.txt drift check.
set -euo pipefail

cd "$(dirname "$0")/.."

FILE="llms.txt"

# Extract link targets while ignoring code. awk toggles a fenced-code flag on
# ``` lines (skipping their bodies), strips inline `code` spans from prose
# lines, then prints every ](target) target one per line.
targets="$(awk '
	/^[[:space:]]*```/ { fence = !fence; next }
	fence { next }
	{
		line = $0
		# Drop inline code spans so `](x)` inside backticks is not seen as a link.
		while (match(line, /`[^`]*`/)) {
			line = substr(line, 1, RSTART - 1) substr(line, RSTART + RLENGTH)
		}
		# Emit each ](target) target.
		rest = line
		while (match(rest, /\]\([^)]*\)/)) {
			t = substr(rest, RSTART + 2, RLENGTH - 3)
			print t
			rest = substr(rest, RSTART + RLENGTH)
		}
	}
' "$FILE")"

broken=0
checked=0
while IFS= read -r target; do
	[ -n "$target" ] || continue
	# Strip a trailing #anchor.
	path="${target%%#*}"
	# Skip external links and pure in-page anchors.
	case "$target" in
	http://* | https://* | mailto:* | "#"*) continue ;;
	esac
	[ -n "$path" ] || continue
	checked=$((checked + 1))
	if [ ! -e "$path" ]; then
		echo "broken link in $FILE: $target -> $path (no such file or directory)" >&2
		broken=$((broken + 1))
	fi
done <<EOF
$targets
EOF

if [ "$broken" -gt 0 ]; then
	echo "error: $broken broken local link(s) in $FILE." >&2
	echo "Fix the target paths, or update $FILE to match the Book's current layout." >&2
	exit 1
fi

echo "$FILE: $checked local link(s) OK."
