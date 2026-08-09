# 0334 — Lowering driven from a certified `CheckedProgram` enforces `IrExpr`'s total-by-construction type guarantee itself; `expr_types` stays a `HashMap`

- **Status:** Accepted (v0.247.33)

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
