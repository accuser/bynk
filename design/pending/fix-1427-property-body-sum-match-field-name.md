---
level: patch
changelog: A `for all` property body (or `where` clause) that `match`es a sum value now binds a single named payload field to its own real name again, closing a case where it silently fell back to the generic `value` accessor and produced a `tsc TS2339` error (or, before recent adjacent changes, a silent `undefined` binding)
---

## ADR: property-body-sum-match-field-name-typecheck-gap

title: `emit_test_property_function` never re-type-checked its own body/`where` clause, so a `match`'s discriminant type was unresolvable and payload-field binding fell back to a generic name

summary: `emit_test_case_function` and `emit_test_history_property_function` both re-type-check their own body before lowering it; `emit_test_property_function` didn't — fixed by adding the same pass, with the `for all` bindings' own resolved types threaded into scope

**Context.** Given `type Outcome = Hit(score: Int) | Miss`, matching an `Outcome`
value inside a `for all` property-test body (or its `where` clause) generated
`const score = __d.value;` instead of `const score = __d.score;` —
`positional_field_name` (`bynk-emit/src/emitter.rs`) falls back to the
generic single-field `"value"` name whenever it can't resolve the match's own
discriminant type to a real declared `Sum` via `disc_ty`. Confirmed to
reproduce for the issue's own two cases: a `for all` body matching a value
**locally constructed inside the body** (not even a drawn value), and the
identical match inside a `where` clause — both via direct `bynkc test` runs
against `main`. A plain `case` body's identical match already binds
correctly, so the gap is specific to property/`where` lowering, not to `Sum`
matching in general.

Traced to the real cause: `emit_test_case_function` explicitly calls
`test_suites::typecheck_case_body` to populate `typed.expr_types`/
`typed.callees` with full inferred types before lowering the case body — the
doc comment on that function names exactly why (`"so the lowering has full
type information"`) — and `emit_test_history_property_function` does the
same for its own `run: List[Step]`-scoped body. `emit_test_property_function`
never did: it built `typed` via `synthetic_typed_commons_for_target` alone
and passed it straight to `emitter::lower_test_case_body`/
`lower_block_to_async_body` with no expression types populated at all. With
`typed.expr_types` empty, `cx.commons().expr_ty(id)` on the match scrutinee
resolves to `None`, `disc_ty` is `None`, and `positional_field_name`'s own
fallback ("the checker rejects mixed bindings already" — true, but silent
about *which* name to fall back to) takes over. Confirmed to reproduce as a
real `tsc TS2339` error against current `main` (`Property 'value' does not
exist on type '{ readonly tag: "Hit"; readonly score: number; }'`) — the
issue's own text predates a real `tsc` error appearing here (it described a
silent `undefined` binding); the underlying gap is the same regardless.

**Decision.** Add the same `test_suites::typecheck_case_body` pass to
`emit_test_property_function`, called twice — once for the synthesised
`where`-clause block (mirroring the existing `synth` construction there),
once for `prop.forall.body` — each extending (not replacing) `typed
.expr_types`/`typed.callees` before its own lowering call. Both calls pass an
`initial_scope` built from the property's own `for all` bindings' resolved
types (`binding_names` zipped with `binding_types`, already computed earlier
in the function for `destructure_vals`), the same shape
`emit_test_history_property_function`'s own `run: List[Step]` scope entry
already established, generalised to every binding a property declares.

**Consequences.** Two new fixtures pin both fixed call sites:
`1427_property_body_matches_local_sum` (the issue's own literal body-match
repro) and `1427_property_where_matches_local_sum` (the identical match
inside a `where` clause, a second, independently-fixed call site). Both are
covered by existing harnesses without a new behavioural test file: the bug
is a hard `tsc --strict` `TS2339` error today, so `tsc_verify`'s own
`emitted_typescript_passes_tsc_strict` gate (workspace-wide, no exclusion
mechanism) catches a regression on its own, and the exact wrong text
(`__d.value` vs `__d.score`) is directly visible to golden-text diffing
(`positive_fixtures`/`bless_positive_fixtures`) — unlike #1426's own
bigint/number gap, this one doesn't hide from either existing gate. No
existing fixture changes.

Closes #1427.
