# 0393 — bynk-check and bynk-emit read one shared `is_uses_commons_type`, not two independently maintained copies

- **Status:** Accepted (v0.289.2)

**Context.** R4.10's rationale names a concrete problem: `emit_context_rebrands`
(`bynk-emit/src/emitter.rs`) and `ResolvedCommons::is_uses_commons_type`/`prepare_unit_check_ctx`
(`bynk-check`) each independently inlined the identical two-condition check deciding which
`uses`-imported names get a context-rebranded TypeScript alias — linked only by a doc comment
promising the two matched exactly. ADR 0226 already recorded the cost of exactly this shape
diverging once (#655): a `tsc` failure in generated code the author never wrote. Review of this
change found a *third* independent inlining, sharper in failure mode than the pair above:
`emit_context_rebrands`'s own two-step pair — aliasing the `uses`-imported commons import, then
rebranding the type — must narrow the exact same set, or the generated module references an
undefined name or imports an alias never used.

**Decision.** Add one shared function, `bynk_check::resolver::compute_is_uses_commons_type(
imported_from_kind, types, name)` (named distinctly from the existing `ResolvedCommons::
is_uses_commons_type` method it's read through), and route all three real call sites through it
instead of each inlining its own copy. R8.2's rationale also mentions a second mechanism
(`brand_prefix`, `emitter/emit.rs:57`, a locally-declared type's own branded literal) — checked and
found no duplication risk there: `ctx.owning_context` is a single, non-duplicated project-level fact
with no checker-side mirror to diverge from, not the "two independently hand-maintained copies"
shape R4.10's own evidence is about. R4.10's own closing line ("One `Ty` carrying its brand, one
emitter reading it") is not implemented literally as a new field on `Ty`/`TypeDecl` — the evidenced
defect was the predicate mirror, not the absence of a stored string, and a shared function closes
the same structural-drift risk without a broader restructuring the evidence didn't call for.

**Consequences.** An edit to either condition of the predicate now updates every caller
structurally, not by doc-comment promise. No behaviour change: the logic itself is unchanged at all
three sites, only its location. A new direct unit test pins the predicate's four real combinations
(commons type, commons function — not rebranded per v0.20b, non-commons import, absent/local) —
nothing pinned it directly before. Full workspace test suite, strict `RUSTDOCFLAGS="-D warnings"
cargo doc`, and every gated `greenfield-status.md` probe confirm zero diff.
