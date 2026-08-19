---
level: patch
changelog: "P6.41: TypeShape::Refined::{base, refinement} investigated and ruled phase 7, not built. Both fields flow directly into emitter/emit.rs's emit_refined_type/emit_refined_checks, which generate TS validation source text (export type/export const, predicate checks) -- the identical \"how do I render this type as TS\" class of question P6.33 already ruled the JSON/wire codec renderer belongs to phase 7's eventual printer, not phase 6's IR construction. Building an IrRefinement/IrBaseType mirror now would mean building printer infrastructure for a consumer this track's own §2 scope already excludes, and per the plan's own finding the bounds (IntBound/FloatBound) deliberately keep source lexemes for byte-stable emission, so the mirror would be a near-copy rather than a clean IR abstraction -- weaker grounds for building it than the four fields P6.39 already deleted, which had a real redundant-metadata case for removal. No code changes. ast_importers unaffected -- invisible to the probe by construction."
---

## ADR: refined-shape-deferred

title: `TypeShape::Refined::{base, refinement}` ruled phase 7, not built — closes Phase F and this completion plan's own remaining rows

summary: Phase F of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.41) — the row the plan itself flagged as a phase-7 candidate; investigation confirms it

**Context.** `TypeShape::Refined::{base: BaseType, refinement: Option<Refinement>}` reuses the AST
types verbatim — the largest remaining IR-side item this plan named, flagged from the outset as
needing its own ADR arguing whether building a mirror is worth it at all, since `IntBound`/`FloatBound`
deliberately keep source lexemes for byte-stable emission (the mirror would be a near-copy, not a
clean abstraction).

**Traced directly: both fields flow into the phase-7 codec renderer, not into IR construction.**
`emitter.rs`'s `type_shape_for` passes `TypeShape::Refined { base, refinement, opaque }` straight into
`RefinedShape`, consumed by `emitter/emit.rs`'s `emit_refined_type`/`emit_refined_checks` — functions
that write `export type`/`export const`/predicate-check TS source text directly, matching `base`
against `BaseType::Int`/`Float` and `refinement` against `PredKind`'s own closed set to generate
validation strings. This is the identical "how do I render this checker type as TS source" class of
question P6.33 already ruled belongs to phase 7's eventual printer (`bynk-ts`), not phase 6's IR
construction — confirmed by inspection, not assumed from the plan's own framing.

**Decision: ruled phase 7, not built.** Building an `IrRefinement`/IR-native `BaseType` mirror now
would mean constructing printer infrastructure for a consumer this track's own §2 scope already
excludes (the `bynk-ts` printer, phase 7) — the same reasoning P6.33 applied to
`emitter/serialisation.rs`. The lexeme-preservation requirement makes the case for building this
mirror *weaker* than the four fields P6.39 already deleted (which had a genuine redundant-metadata
case): here there is no redundancy to remove, only a faithful AST reuse with no IR-native alternative
that would actually simplify anything. No source changes.

**Consequences.** `ast_importers` unaffected — invisible to the probe by construction (`ir.rs` is an
excluded file). No verification beyond `cargo xtask check-pending` and confirming the tree is
unchanged — this PR is doc-only. This closes the last named row in the completion plan's own Phase F;
see the corresponding track-doc update for what that means for this track's own retirement readiness.
