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

**Decision.** Add `coerce_int_field(t, tys, value)`: when a payload/field's
own resolved type `t` is `checker::Ty::Base(BaseType::Int)`, wrap `value` in
a `Number(…)` call; otherwise pass it through unchanged. Apply it at all
four Sum/Record payload/field recursion sites — `gen_ts_for_ty`'s and
`canon_ts_for_ty`'s own `TypeBody::Sum`/`TypeBody::Record` arms — rather than
inside the shared top-level `Base(Int)` arm itself, which stays bigint
(top-level bindings need it, per Context). A refined `Int` (`Percent`, etc.)
already routes through `refined_gen_ts`'s/its `canon_ts_for_ty` sibling's own
`unchecked_construct_test`-wrapped cast, so the new guard is a no-op there,
and a no-op for every other type's own already-number/string/bool/
object-shaped recursive result — zero diff for every existing fixture with
no bare-Int payload/field.

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
