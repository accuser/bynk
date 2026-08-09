---
level: patch
changelog: Settle phase 6 of the compiler trajectory (`design/tracks/the-ir.md`) — the IR and its CheckedProgram → Ir lowering pass land inside `bynk-emit` rather than as new `bynk-ir`/`bynk-lower` crates, Callee dispatch classification is commissioned as new `bynk-check` work (not scope the retired phase-5 track missed), and lowering driven from a certified CheckedProgram enforces IrExpr's total-by-construction type guarantee with a panic-not-fallback discipline rather than requiring expr_types to become an IndexVec first — scoped to that path only, since test-suite emission's own non-certified TypedCommons producer keeps its existing fallback
---

## ADR: the-ir-crate-location
title: The IR and its lowering pass land inside `bynk-emit`, not as new `bynk-ir`/`bynk-lower` crates
summary: The reference's own target crate graph names both crates, but only one consumer of the IR exists today — carving now would be prospective without the dependency R10.3 requires

**Context.** `bynk-greenfield-compiler.md`'s Part 10 (crate graph) names `bynk-ir` ("the checker's
output; the only thing lower consumes") and `bynk-lower` (`CheckedProgram → Ir`) as crates distinct
from a narrower, `Ir`-consuming `bynk-emit` that in the target graph "depends on `bynk-ts`, NOT on
`bynk-check`." Neither crate is mentioned anywhere in `bynk-compiler-trajectory.md`'s own phase-6
narrative — its stated invariant is that `bynk-emit` names no AST type, not that `bynk-emit` stops
being the lowering site — and the trajectory's own phase-6 reference-rule list omits R10.1/R10.2
entirely, meaning severing `bynk-emit`'s `bynk-check` dependency was never one of this phase's
commitments. R10.3 states plainly that a crate is carved "when a dependency arrives that only one
consumer needs — prospectively, at the moment the dependency appears," citing `bynk-strip` as "the
control case for this entire document" precisely because it was carved for a real, present dependency
(`oxc`), not a speculative future one. Today exactly one consumer of the IR exists: `bynk-emit` itself.
The reference's own second consumer, `bynk-ts`, is trajectory phase 7 and does not exist yet.

**Decision.** The IR's core types (`IrExpr`, `IrStmt`, `IrItem`, `Callee`, `IrPat`, …) and the
`CheckedProgram → Ir` lowering pass land as new modules inside the existing `bynk-emit` crate, not as
new `bynk-ir`/`bynk-lower` crates. `bynk-emit` continues depending on `bynk-check` exactly as it does
today. Carving `bynk-ir`/`bynk-lower` out — and, at the same or a later moment, reconsidering whether
`bynk-emit` still needs `bynk-check` once the IR is fully typed and resolved — is named as phase 7's
own entry condition (`design/tracks/the-ir.md` §7), not refused.

**Consequences.** This phase's own crate-graph footprint stays minimal: no new crate, no new manifest
edges, no CI dependency-graph gate to add. The reference's own target graph is not contradicted, only
deferred — a reviewer reading Part 10 cold could otherwise reasonably expect two new crates from this
phase and be surprised by their absence; this ADR is the record that the absence is deliberate. If a
second real consumer of the IR appears before phase 7 opens (unlikely, but not impossible — an
alternative backend, a standalone IR-level linter), that is new evidence for revisiting this ADR's
scope under its own review, not grounds to quietly build the split without one.

## ADR: the-ir-callee-in-bynk-check
title: `Callee` dispatch classification is new `bynk-check` work commissioned by this phase, not scope the retired phase-5 track missed
summary: R6.10's "resolved in phase 5" names the reference document's own Check phase, not the trajectory's phase-5 track — the two numbering schemes coincide by convention, not by identity

**Context.** R6.10 (`bynk-greenfield-compiler.md`) states "call dispatch is a closed enum resolved in
phase 5." Read against the trajectory's own phase numbering, this could misread as a claim that the
just-retired `semantics-in-the-checker.md` track (the trajectory's phase 5) should already have built a
`Callee` enum and did not. It did not, and that is not a missed-scope defect: `bynk-greenfield-compiler.md`
Part 3's own phase table defines a *different*, prior phase numbering — "5 | Check | `Resolved` |
`TypedProgram` | `bynk-check`" — that the reference document uses throughout, independent of any
trajectory track (R3.5's own text, "all semantic checking … happens in phase 5," uses the identical
convention, for the same reference-internal Check phase `semantics-in-the-checker.md` happened to close
under the trajectory's own, coincidentally same-numbered, phase-5 track). `semantics-in-the-checker.md`'s
own settled Q2 (`design/archive/retired-tracks.md`) scoped that track explicitly to relocating seven
named `bynk-check/src/analysis.rs` diagnostic categories plus two sites its own accounting missed — a
`Callee` enum was never named, argued, or implied in any of that track's five design questions.

**Decision.** `Callee` classification is checking work, belongs in `bynk-check`, and is commissioned as
this phase's own first slice (P6.0 in `design/tracks/the-ir.md` §6) — extending
`TypedCommons`/`CheckedProgram`'s checked output, not reopening the retired phase-5 track. Every later
lowering slice consumes the classification P6.0 produces rather than re-deriving it from the AST the
way `bynk-emit/src/emitter/lower.rs`'s `lower_method_call`/`lower_call` dispatchers do today.

**Consequences.** P6.0 becomes a load-bearing prerequisite nearly every other slice in this track
depends on, and it is the one slice in this track's decomposition that lands in a different crate
(`bynk-check`) than the rest of the phase's own work (`bynk-emit`). Getting its shape wrong is
expensive to unwind across the slices that build on it — named as this track's own top risk
(`design/tracks/the-ir.md` §9), not treated as a routine relocation. This ADR is also the correction
that keeps `R6.10`'s own text from being misread as a scope gap in an already-retired, already-closed
track.

## ADR: the-ir-lowering-totality-discipline
title: Lowering driven from a certified `CheckedProgram` enforces `IrExpr`'s total-by-construction type guarantee itself; `expr_types` stays a `HashMap`
summary: A single total walk that panics on a checker/lowerer disagreement closes R6.1 on the certified path without needing R4.9's `IndexVec` conversion first — scoped to that path only, since a second, non-certified `TypedCommons` producer already exists and must keep its fallback

**Context.** R6.1 requires every `IrExpr` to carry its type "by construction… there is no side table
and no fallible lookup." R4.9 is adjacent but distinct: it names `expr_types`'s own container type
(`HashMap<Span, Ty>` in its original rationale text) as the defect, gated on R2.4 and marked "large" in
`bynk-greenfield-compiler.md`'s Appendix D, still open at phase 3's own retirement ("functionally but
not structurally"). Checking the live code: `TypedCommons.expr_types` is already
`HashMap<ExprId, TypedExpr>` (T3.4, phase 3) — the position-keyed map R4.9's rationale describes was
already replaced with an identity-keyed one; only the container's own totality (a `HashMap` that can
miss, vs. an `IndexVec` that cannot) remains open, and nothing outside the checker's own internals
currently depends on that container shape directly — `TypedCommons::expr_ty(id)` is the one accessor,
already returning `Option`. `CheckedProgram` is constructible only via `certify`, which rejects on any
error-severity diagnostic (R3.10), so a certified program's `expr_types` should already be complete for
every real expression by the checker's own construction discipline. This codebase already has a
precedent for how a "should never happen" miss at exactly this kind of checker/lowerer boundary is
meant to be handled: `bynk-emit/src/emitter/emit.rs`'s `lower_workers_cross_context_call` panics on its
own `bynk.emit.unresolved_cross_context_signature` rather than silently degrading, reasoning explicitly
that an absent value the checker was supposed to have resolved is "the emitter disagreeing with the
checker — a compiler bug."

**`TypedCommons` has a second, non-certified producer today, and a blanket panic would break it.**
`bynk-emit/src/project/tests_emit.rs`'s `synthetic_typed_commons_for_target` hand-builds a
`TypedCommons` with `expr_types: HashMap::new()`, filled in by `bynk-check/src/test_suites.rs`'s
`let _ = checker::check_body(…)` — errors discarded by design, no `certify`, no R3.10 gate — and
test-suite emission then lowers case/property bodies through the same `lower.rs` functions this phase
replaces. That path's own partial typing is a modelled, expected state (`checker.rs`'s
`partial_expr_types` exists for exactly this reason across the codebase), not a should-never-happen —
so the "checker resolved this; a miss is a compiler bug" reasoning above does not hold there.

**Decision.** The panic discipline applies only to lowering reached from a real `CheckedProgram` — the
`CheckedProgram → Ir` lowering pass performs one total walk over the checked AST, minting each
`IrExpr.ty` from `TypedCommons::expr_ty(id)` as it goes, and `.expect()`s that lookup rather than
falling back on a miss on *that* path, the same discipline
`lower_workers_cross_context_call`'s own panic already uses. Test-suite emission's own lowering (driven
from `synthetic_typed_commons_for_target`, never certified) keeps its existing `Some(..) => …, _ => …`
fallback shape; routing it through `certify` first, so it too could adopt the panic discipline, is left
as a named, separate call for a future slice to make deliberately, not assumed here. R4.9's `IndexVec`
conversion is not attempted by this phase; it is filed as an optional, non-blocking follow-on, the same
treatment `content-ownership.md` gave `fs_below_driver`'s carve-outs and `semantics-in-the-checker.md`
gave `emit_diagnostics`'s 4/4-vs-0/0 floor.

**Consequences.** The certified lowering path becomes the one place a checker/lowerer disagreement is
loud rather than silent — closing the exact defect class R4.9 was written against (silent fallback on a
miss) at the site that matters most (the boundary that feeds real emission), without paying for a
container-type migration nothing outside the checker still needs there, and without forcing
test-suite emission's own, deliberately error-tolerant path to panic on ordinary partially-typed user
test bodies. If a future slice routes test-suite emission through `certify`, this ADR's panic
discipline extends to it naturally; until then, two lowering call sites legitimately behave
differently, and that difference is the point, not an inconsistency. If a future consumer needs
`expr_types` itself to be total (not just the certified lowering pass), that is new grounds to revisit
R4.9's own closure, not a reversal of this decision.
