# 0333 — `Callee` dispatch classification is new `bynk-check` work commissioned by this phase, not scope the retired phase-5 track missed

- **Status:** Accepted (v0.247.33)

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
