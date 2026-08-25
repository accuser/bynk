---
level: patch
changelog: The context-brand rebranding predicate (R4.10/R8.2) is one shared function, not two independently mirrored copies
---

## ADR: p7d2-brand-predicate-unification
title: bynk-check and bynk-emit read one shared `is_uses_commons_type`, not two independently maintained copies
summary: Closes R4.10/R8.2's predicate mirror, scoped to the evidenced defect rather than the rule's own maximal prose

**Context.** R4.10's rationale names a concrete problem: `emit_context_rebrands`
(`bynk-emit/src/emitter.rs`) and `ResolvedCommons::is_uses_commons_type`/`prepare_unit_check_ctx`
(`bynk-check`) each independently inlined the identical two-condition check deciding which
`uses`-imported names get a context-rebranded TypeScript alias — linked only by a doc comment
promising the two matched exactly. ADR 0226 already recorded the cost of exactly this shape
diverging once (#655): a `tsc` failure in generated code the author never wrote.

**Decision.** Add one shared function, `bynk_check::resolver::is_uses_commons_type(imported_from_kind,
types, name)`, and have both `prepare_unit_check_ctx` and `emit_context_rebrands` call it instead of
inlining their own copy. R8.2's rationale also mentions a second mechanism (`brand_prefix`,
`emitter/emit.rs:57`, a locally-declared type's own branded literal) — checked and found no
duplication risk there: `ctx.owning_context` is a single, non-duplicated project-level fact with no
checker-side mirror to diverge from, not the "two independently hand-maintained copies" shape R4.10's
own evidence is about. R4.10's own closing line ("One `Ty` carrying its brand, one emitter reading
it") is not implemented literally as a new field on `Ty`/`TypeDecl` — the evidenced defect was the
predicate mirror, not the absence of a stored string, and a shared function closes the same
structural-drift risk without a broader restructuring the evidence didn't call for.

**Consequences.** An edit to either condition of the predicate now updates both callers
structurally, not by doc-comment promise. No behaviour change: the logic itself is unchanged, only
its location. Full workspace test suite and every gated `greenfield-status.md` probe confirm zero
diff.
