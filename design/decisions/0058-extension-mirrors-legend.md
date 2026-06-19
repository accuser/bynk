# 0058 — The extension mirrors the server's frozen semantic-token legend

- **Status:** Accepted (v0.29)
- **Spec:** `design/bynk-lsp-spec.md` §3.14

## Context
v0.28 (ADR 0057) shipped a frozen semantic-token legend on the server
with **custom** token types (`capability`/`service`/`agent`/`provider`)
and modifiers (`refined`/`opaque`/`platformNative`). Custom tokens do
not colour under default themes unless the *client* declares them — and
`vscode-bynk` declared none, so v0.28's Bynk-distinctive tokens were
invisible in the editor. The legend names now live in two languages:
the Rust legend (`semantic_tokens_legend()`) and, necessarily, the
extension's `package.json`.

## Decision
The extension **declares and themes the legend's custom entries**:
`contributes.semanticTokenTypes` / `semanticTokenModifiers` (each type
with a standard `superType` so semantic-highlighting themes colour it)
plus `contributes.semanticTokenScopes` fallbacks for TextMate-only
themes. The declared names are a **cross-component contract** with the
server legend — a mismatch silently un-themes those tokens.

The contract is enforced by **one source of truth, one cross-file
test**: a `bynk-lsp` test parses `vscode-bynk/package.json` and asserts
its declared custom types/modifiers equal `semantic_tokens_legend()`'s
custom entries (legend minus the LSP-standard `type`/`function` /
`declaration`). Two independently-pinned lists (a Rust legend test plus
a JS copy) would enforce *nothing* — a server change moves both Rust
copies while the extension drifts. The test reads a sibling file outside
the crate, so it is **`exclude`d from the published `bynk-lsp` tarball**
(it still runs in-repo via `cargo test --workspace`; a standalone
`cargo test` on the crates.io release must not fail on the missing
sibling).

Token *visibility* stays with the client built-ins — inlay hints via a
`provideInlayHints` middleware gated on `bynk.inlayHints.enable` (the
persistent per-language preference; `editor.inlayHints.enabled` is the
instant toggle), semantic tokens via `editor.semanticHighlighting.enabled`.
No server change.

## Consequences
v0.28's tokens colour out of the box, and the legend can't drift
silently — a new custom token type added server-side fails CI until
`package.json` matches. The cost is one extension-only increment under
the single-version posture (which republishes all four crates at the
new version, source effectively unchanged). Theme defaults for the
custom types and marketplace publishing remain B-2 / CI-Tier-4 follow-ups.
