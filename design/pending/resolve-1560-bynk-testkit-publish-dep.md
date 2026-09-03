---
level: patch
changelog: `bynk-testkit` (a `publish = false` dev-dependency of `bynk`/`bynkc`/`bynk-lsp`) no longer carries a fake `version` in the workspace dependency table — that combination made `cargo publish` fail unconditionally for all three, since a versioned dependency must resolve against crates.io even with `--no-verify` (#1560)
---
