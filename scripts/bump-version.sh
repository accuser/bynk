#!/usr/bin/env bash
# Set the single repo version across every versioned manifest.
#
# The repo carries one version while everything lives together (see
# design/README.md "Versioning & release"). The sites that must agree:
#
#   - Cargo.toml  [workspace.package] version
#   - Cargo.toml  in-workspace dependency requirements (path + version)
#   - vscode-bynk/package.json        version + bynkServerVersion ("vX.Y.Z" —
#     the GitHub Release the extension downloads server binaries from)
#   - tree-sitter-bynk/package.json   version
#   - the lockfiles (Cargo.lock, both package-lock.json)
#   - the Bynk Book's current-version banners (docs/src/**, MAJOR.MINOR) —
#     guarded by bynkc/tests/doc_version.rs (Workstream 0)
#
# The release workflow's verify job refuses a tag that doesn't match all of
# them, so run this in the increment PR and land the bump with the increment.
#
# Usage: scripts/bump-version.sh X.Y.Z
set -euo pipefail

ver="${1:?usage: scripts/bump-version.sh X.Y.Z}"
if ! printf '%s' "$ver" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
	echo "error: '$ver' is not a bare X.Y.Z version" >&2
	exit 1
fi

cd "$(dirname "$0")/.."

# Cargo workspace: [workspace.package] version + the in-workspace dependency
# requirements ({ path = "...", version = "..." }).
sed -i.bak -E \
	-e 's/^version = "[^"]+"/version = "'"$ver"'"/' \
	-e 's/(path = "[^"]+", version = )"[^"]+"/\1"'"$ver"'"/' \
	Cargo.toml
rm Cargo.toml.bak
cargo update --workspace --quiet

# npm manifests. Targeted sed (not `npm version`/JSON rewrite) so the bump
# never reformats the files. bynkServerVersion is the extension's server pin —
# the GitHub Release tag it downloads binaries from.
sed -i.bak -E 's/^(  "version": )"[^"]+"/\1"'"$ver"'"/' \
	vscode-bynk/package.json tree-sitter-bynk/package.json
sed -i.bak -E 's/^(  "bynkServerVersion": )"[^"]+"/\1"v'"$ver"'"/' \
	vscode-bynk/package.json
rm vscode-bynk/package.json.bak tree-sitter-bynk/package.json.bak

# Sync the npm lockfiles to the new manifest versions.
(cd vscode-bynk && npm install --package-lock-only --ignore-scripts >/dev/null)
(cd tree-sitter-bynk && npm install --package-lock-only --ignore-scripts >/dev/null)

# The book's current-version banners track MAJOR.MINOR (patches are
# non-language). Only these "current version" banners move — never the
# historical "introduced in vX" feature markers. `bynkc/tests/doc_version.rs`
# fails CI if any banner drifts from the released version.
mm="${ver%.*}"
sed -i.bak -E "s/currently v[0-9]+\.[0-9]+/currently v$mm/" \
	docs/src/introduction.md docs/src/tooling/index.md
sed -i.bak -E "s/written against v[0-9]+\.[0-9]+/written against v$mm/" \
	docs/src/about/versioning-and-roadmap.md
sed -i.bak -E "s/written against \*\*v[0-9]+\.[0-9]+\*\*/written against **v$mm**/" \
	docs/src/reference/changelog.md
sed -i.bak -E "s/current version, v[0-9]+\.[0-9]+/current version, v$mm/" \
	docs/src/spec/scope.md docs/src/spec/appendix-version-history.md docs/src/spec/index.md
find docs/src -name '*.bak' -delete

# Regenerate the inlined Book (llms-full.txt) so its version banner tracks the
# bump. It reads the version straight from Cargo.toml, so this just re-stamps it.
"$(dirname "$0")/build-llms-full.sh" >/dev/null

echo "bumped to $ver:"
grep -m1 '^version' Cargo.toml
node -p '"vscode-bynk        " + require("./vscode-bynk/package.json").version + " (server pin " + require("./vscode-bynk/package.json").bynkServerVersion + ")"'
node -p '"tree-sitter-bynk   " + require("./tree-sitter-bynk/package.json").version'
echo "docs banners      v$mm"
