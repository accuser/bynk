---
level: patch
changelog: `bynk-ir`/`bynk-lower` carved out of `bynk-emit` as new workspace crates (ADR 0332's deferred split, triggered per ADR 0385)
---

## ADR: p7d1-ir-lower-crate-carve
title: The IR crate split deferred by ADR 0332 lands at Arc D's P7.d1, carrying its content unchanged
summary: bynk-ir and bynk-lower are carved out of bynk-emit with no code deleted, once test callers were counted alongside production ones

**Context.** ADR 0332 held `bynk-ir`/`bynk-lower` inside `bynk-emit` (as `ir.rs`/`ir/lower.rs`)
for lack of a second consumer, deferring the crate split Part 10 of the reference names. ADR 0385
records that carving `bynk-ts` as its own crate up front manufactures that second consumer, so the
deferred split "can happen inside this phase once Arc B lands." Arc B landed at P7.9; this is that
slice.

Grounding this slice found `ir/lower.rs` (10,095 lines) is not uniformly reachable from production:
11 of its ~80 top-level functions have no production caller. The first cut at this classification
(recorded in `design/tracks/the-typescript-tree.md`) concluded these 11 were dead weight to trim
before the carve — and was wrong twice over before implementation began. First, it undercounted
real production reachability itself (a same-file call chain from `lower_event_subscriber_shapes_ir`
through `lower_service_item_ir` reaches the bulk of the recursive expression-lowering machinery for
real, verified panic-free across the whole e2e fixture corpus). Second, once that was fixed and 11
functions remained with genuinely zero production callers, attempting the actual deletion surfaced
that several are the primary test-harness entry points for the shared lowering machinery above (one
alone, `lower_fn_body_ir`, backs 51 test call sites), and the rest are explicit regression tests for
real historical bugs (review of #1238, #1189) or non-redundant coverage of their own assembly logic.

**Decision.** A function's real callers include its tests, not only its production call sites. None
of the 11 is dead in the sense that matters for a carve — all of `ir.rs` and `ir/lower.rs` move into
the new `bynk-ir`/`bynk-lower` crates unchanged, no deletion. The one real reverse dependency
(`ir/lower.rs`'s own `use crate::emitter::{ block_uses_emit, match_needs_if_chain, MUTATING_*_OPS,
... }`) is resolved by relocating those items into `bynk-ir`, the crate both `bynk-emit` and
`bynk-lower` already depend on, with every caller on both sides repointed — not a re-export.

**Consequences.** `bynk-ir` depends on `bynk-syntax`/`bynk-check` only; `bynk-lower` depends on
those plus `bynk-ir`; `bynk-emit` depends on both, with zero circular edges. Every previously
`pub(crate)` item in the moved files that crossed the new crate boundary became `pub` — the crate
boundary is what encapsulates now, matching `bynk-render`'s and `bynk-strip`'s own prospective-carve
precedent (ADR 0385's own grounding). No behaviour changes anywhere: full workspace `cargo check`/
`cargo test` clean before and after, every gated `greenfield-status.md` probe unchanged. P7.d7
(R10.2 verification of `bynk-lsp`'s own dependency surface) is unblocked by this landing.
