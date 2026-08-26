---
level: patch
changelog: A Sum-variant payload or Record field of type `Int` now coerces its drawn `bigint` value to `number`, closing a `tsc --strict` type mismatch at the real constructor/object-literal call site
---

## ADR: property-int-field-number-coercion
title: gen_ts_for_ty/canon_ts_for_ty coerce a bare-Int payload/field value to number
summary: A Sum/Record's own recursive Int position needs a Number(...) coercion the top-level binding's own Int position must not have

**Context.** `gen_ts_for_ty`'s/`canon_ts_for_ty`'s own top-level `Base(Int)`
arms draw/emit a `bigint` (`rng.int(lo n, hi n)` / a bigint literal like
`0n`) — correct there, since a top-level `for all` binding's own
boundaries/shrink machinery (`__bynkShrinkInt`, the runtime `__bynkRng.int`)
is bigint-typed throughout, and mixing `number` in would break shrinking
(confirmed: `__bynkShrinkInt(v: bigint, floor: bigint)`'s own arithmetic
throws `TypeError: Cannot mix BigInt and other types` if handed a `number`).
But their `Sum`/`Record` arms recurse into each payload field's/record
field's own resolved type via the same functions, and when that field is a
bare `Int`, the recursive `bigint` result gets spliced directly into a real,
already-`number`-typed position: a Sum variant's real compiled constructor
parameter (`Hit: (score: number): Outcome => …`), or a Record's own
already-`number`-typed field. For a Sum variant this is a hard `tsc --strict`
`TS2345` error (`Argument of type 'bigint' is not assignable to parameter of
type 'number'`) — confirmed to reproduce for real via `bynkc test` against a
minimal `type Outcome = Hit(score: Int) | Miss` fixture, and confirmed to
stop reproducing with the fix applied (#1398). A record field's own
generator-descriptor object currently carries no contextual type annotation
constraining it, so the identical bigint-in-a-number-position mismatch does
**not** currently trip a `tsc` error there — verified empirically both ways
(reverting the fix and recompiling) rather than assumed from the issue's own
text, which named both cases — but the coercion is still the structurally
correct fix: it matches the record's own real field types and is not
contingent on the descriptor object staying untyped forever.

`bynk-check::test_suites::prop_binding_generable` already accepts any
sum/record whose payload/fields are themselves generable, so both shapes
were reachable production behavior, not dead paths — just never exercised,
since no existing fixture had an `Int`-typed sum-variant payload or record
field (the closest, #1395's `Outcome`, used a `Bool` payload; #1397's
`Point` predates this fix and is amended by it — see Consequences).

**Decision.** Add `coerce_int_field(t, types, tys, value)`: when a
payload/field's own resolved type `t` draws/emits as `bigint` — a bare
`checker::Ty::Base(BaseType::Int)`, **or** a `Ty::Named` whose declaration is
`TypeBody::Refined`/`TypeBody::Opaque` with `base: BaseType::Int` (review of
#1428, see below) — wrap `value` in a `Number(…)` call; otherwise pass it
through unchanged (including the `"undefined"` depth-exhaustion sentinel,
also review of #1428). Apply it at all four Sum/Record payload/field
recursion sites — `gen_ts_for_ty`'s and `canon_ts_for_ty`'s own
`TypeBody::Sum`/`TypeBody::Record` arms — rather than inside the shared
top-level `Base(Int)` arm itself, which stays bigint (top-level bindings
need it, per Context). Every other type's own already-number/string/bool/
object-shaped recursive result stays a no-op — zero diff for every existing
fixture with no bigint-drawing payload/field.

Review of this PR (#1428) raised two findings, both addressed:

1. **The original guard missed refined/opaque `Int`.** `refined_gen_ts`'s
   own `unchecked_construct_test`-wrapped cast (`(rng.int(...) as any)` for
   refined, `Pct.unsafe(rng.int(...))` for opaque) is compile-time only — it
   silences `tsc` but does nothing to the runtime value, so a refined-Int
   payload/field was still a real `bigint` without this coercion, confirmed
   to reproduce the exact `TypeError: Cannot mix BigInt and other types` this
   issue names (built a scratch `Pct = Int where InRange(0, 100)` record
   field, ran `bynkc test`, reverted the fix locally to confirm both
   directions). Extended `coerce_int_field`'s own type match to also cover
   `Refined`/`Opaque` with an `Int` base — `.unsafe`/`as any` are both
   identity at runtime, so wrapping either form in `Number(...)` is safe.
   Pinned by new fixture `1398_property_record_refined_int_field`.
2. **The guard would have wrapped the depth-exhaustion `undefined` sentinel
   into `Number(undefined)` (`NaN`) — a silent failure where the unfixed
   code at least produced a value `tsc` could reject.** Reachable: a
   self-recursive Sum whose *first* variant carries an `Int` payload passes
   `prop_binding_generable` (which only validates that variant) but exhausts
   `canon_ts_for_ty`'s own recursion budget partway down the chain,
   surfacing the `ident("undefined")` sentinel *inside* a real Int
   payload/field position. Confirmed by tracing the generated boundary text
   for `type T = Base(n: Int) | Cons(tail: T)` before and after: pre-guard,
   `T.Base(Number(undefined))`; with a guard that skips the sentinel,
   `T.Base(undefined)`. `coerce_int_field` now returns the sentinel
   unwrapped rather than wrapping it.

   No positive `.bynk` fixture pins this one, deliberately: every
   project-form positive fixture is required to pass real `tsc --strict`
   (`bynkc/tests/tsc_verify.rs::emitted_typescript_passes_tsc_strict`, a
   workspace-wide gate, no exclusion mechanism), and `T.Base(undefined)`
   itself still doesn't type-check against `Base`'s real `number` parameter
   — confirmed this is a distinct, pre-existing bug by testing the identical
   `type T = Base(n: Int) | Cons(tail: T)` fixture against the true
   pre-#1398 `main` baseline directly: it already failed there too, with a
   mix of `bigint`-not-assignable *and* `undefined`-not-assignable errors,
   so a fixture exercising this path can never be `tsc`-clean regardless of
   this fix. Filed separately as #1429 rather than folded into this fix —
   the depth-exhaustion fallback's own type is a distinct concern (general
   to any field type, not Int-specific) in the same recursion machinery.
   Pinned instead by five `#[cfg(test)]` unit tests directly on
   `coerce_int_field` (`bynk-emit/src/project/tests_emit.rs`, `mod tests`)
   — bare/refined/opaque Int wrapping, the `undefined` passthrough, and a
   non-Int no-op — each confirmed to fail against the pre-review version of
   the function before being confirmed to pass against the fixed one.

**Consequences.** Two new fixtures pin the fix:
`1398_property_sum_int_payload` (`Outcome.Hit(score: Int)`) confirms the
real `tsc TS2345` error without the fix and a clean `bynkc test` pass with
it; `1398_property_record_int_field` (`Point { x: Int, y: Int }`) pins the
coercion via golden-diff text and a predicate that does real arithmetic on
the coerced field (`p.x + 1 == p.x + 1`), proving the value is a genuine JS
`number` post-coercion — safe from #1426's own unrelated bigint/
number-literal-mixing gap precisely *because* this fix converts the value
before any arithmetic touches it. #1397's own existing `Point` fixture is
amended as a side effect (its `x`/`y` fields now emit `Number(rng.int(...))`
rather than bare `rng.int(...)`) — expected and correct, re-blessed here.

The Sum fixture's own predicate deliberately does not `match`/destructure
the drawn payload (`expect true`, matching #1395's own established
precedent) — doing so surfaced a second, unrelated, pre-existing bug during
development: a `for all` body's own `match` over a named single-payload sum
variant binds the field to a generic `.value` accessor instead of its real
name, confirmed general to `for all` bodies (reproduces even matching a
value locally constructed inside the body, not just a drawn one) and
confirmed absent from a plain `case` body's own identical match. Filed
separately as #1427 rather than folded into this fix — a distinct concern
(payload *destructuring* vs. payload *construction* typing) in a different
part of the pipeline, matching #1397's/#1426's own "tracked separately"
precedent.

Closes #1398.
