---
level: patch
changelog: The playground editor shows the inferred type of the expression under the cursor on hover (#397)
---

## ADR: playground-hover-397
title: Playground gains a hover tooltip for inferred types; completion split to a new issue
summary: Expose the checker's expr-types sink through a new `bynk_hover` wasm entry; defer completion

**Context.** Issue #397 was deferred from the in-browser track's slice 5d
(#393), which shipped live on-type diagnostics via a `bynk_analyze` wasm entry
over `bynk_emit::project::analyse_in_memory`. #397 asked for two richer LSP
features in the playground: hover (inferred types at the cursor) and
completion (context-aware suggestions).

The two are very different sizes of work. Hover only needs the checker's
existing per-expression type map (`bynk_check::expr_types`, ADR 0063's
`ExprTypeSink` + `type_at_offset`) exposed through the wasm boundary —
`analyse_in_memory` already ran the full `Analyse`-mode pipeline that
populates this sink but discarded everything except diagnostics. Completion's
pure logic lives in `bynk-lsp/src/completion.rs` (~2500 lines, no
`tower-lsp`/`tokio` dependency), but it is only reachable by extracting it and
several sibling modules out of the `bynk-lsp` crate — which pulls
`tower-lsp`/`tokio`, not viable on `wasm32-unknown-unknown` — into something
`bynk-wasm` can depend on. That is a real cross-crate refactor touching the
LSP's only end-to-end test coverage (the VS Code integration tests), not a
small increment.

**Decision.** Ship hover now; split completion into its own follow-up issue
rather than bundle the crate extraction into this increment.

`bynk-emit::project` gains `analyse_in_memory_with_types`, a variant of
`analyse_in_memory` that also drains the `exprs` sink for the analysed file
(the same sink `analyse_project_with` already drains) instead of discarding
it; `analyse_in_memory` itself is refactored to delegate to it, so there is
one code path instead of two. `bynk-wasm` gains a `bynk_hover(source, offset)`
entry following the existing `compile`/`analyze` pattern (inner fn →
`catch_panic` → `..._to_json` → `#[wasm_bindgen]`), returning
`{ "ty": string | null }` via `type_at_offset` + `Ty::display()`. The
playground wires this to a `@codemirror/view` `hoverTooltip` extension
alongside the existing `bynkLinter`.

Hover intentionally answers only "what is the type here", not the richer
`bynk-lsp` hover ladder (symbol/keyword/capability-op descriptions) — that
ladder's pure logic has the same crate-extraction blocker as completion, and
the bare inferred type is the value #397 named explicitly. Offsets are byte
offsets, matching the existing diagnostic `from`/`to` convention CodeMirror
already treats as its position units — an existing, accepted
UTF-16-vs-byte approximation this change doesn't newly introduce. Hover finds
nothing on a buffer that doesn't currently check clean (the `expr_types`
sink's own "clean-file ceiling", ADR 0063), the same limitation the LSP's
hover already lives with.

**Consequences.** The playground editor now shows inferred types on hover.
Completion remains open, split to a new tracked issue referencing this one
and #393 for the crate-extraction context.
