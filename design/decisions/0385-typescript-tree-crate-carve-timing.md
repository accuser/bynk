# 0385 — `bynk-ts` is carved as a crate in the first slice, not built in-module and carved later

- **Status:** Accepted (v0.249.37)

**Context.** Phase 6 built its IR (`ir.rs`/`ir/lower.rs`) inside `bynk-emit` and deferred the `bynk-ir`/
`bynk-lower` crate split explicitly (ADR 0332), for lack of a second consumer. Whether `bynk-ts` should
follow the same in-module-first pattern, or be carved immediately, was open at this track's own opening.
Two real precedents exist in this codebase for carving a crate prospectively under R10.3: `bynk-strip`
(commit `868fda94`, #385 — created new, in the same PR as its only consumer, to keep `oxc` out of
`bynk-emit` and the LSP) and `bynk-render` (commit `b56f22de`, #251, `crate-decomposition` track slice 6
— created new, in the same PR that moved seven existing renderer functions out of `bynkc`). Both were
carved up front; neither was built in-module first. `bynk-render`'s own module doc states its load-bearing
invariant directly — `cargo tree -p bynk-render` is `bynk-syntax` + `ariadne` only, enforced structurally
by the crate graph. That is the same shape `bynk-ts` needs: R7.3's invariant ("the printer... is the only
code in the compiler that writes a character") is a boundary a `pub(crate)` module cannot enforce on
itself. The July review's finding #42 — 33 of 38 world-reachable `bynk_emit::emitter` items are `pub` only
to reach a sibling module — is direct, contemporaneous evidence that "enforce the boundary by convention,
carve the crate later" does not reliably happen in this codebase once code is already crate-internal.
Phase 6's own choice is not a counter-precedent: ADR 0332's stated reason was the absence of *any* second
consumer, not a preference for deferring boundary enforcement.

**Decision.** `bynk-ts` is carved as a new workspace crate in the first Arc B slice (P7.5), before any
conversion work begins. It depends on nothing but `bynk-syntax` (for `Span`); `bynk-emit` depends on it.
No circular-dependency risk exists in this shape.

**Consequences.** The crate boundary — not a `pub(crate)` convention — is what enforces R7.3/R7.4 from the
first slice that constructs a `TsProgram`. Carving `bynk-ts` immediately also manufactures the second IR
consumer ADR 0332 was waiting for, so phase 6's own deferred `bynk-ir`/`bynk-lower` split (P7.10, this
track's own §6) can happen inside this phase once Arc B lands, rather than needing a further, unscheduled
trigger later.
