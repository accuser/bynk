# 0145 — `expect` replaces `assert`, and a failed expectation reports expected-vs-actual structure

- **Status:** Accepted (testing track, slice 1a; v0.112).
- **Provenance:** the testing feature track's first slice (DECISION B). The track's
  organising commitment — one predicate surface (ADR 0144) — is embodied here for the
  first time: the test-assertion keyword becomes `expect`, and because the predicate
  it takes is the invariant predicate, a failure can render the *structure* of that
  predicate rather than a single bit.
- **Realises:** the replacement of the one-bit `assert` with `expect`, a predicate
  position (ADR 0144) whose failures carry expected-vs-actual operands. No new
  capability beyond the report — the rest of the change is a keyword rename.
- **Relates:** ADR 0144 (one predicate surface — `expect` *is* the invariant
  predicate, so this ADR adds a *position*, not a second assertion grammar); ADR 0098
  (`bynkc test --format json` — the runner's pinned JSON document and its
  `path:line:col` failure location, which this slice enriches inside the existing
  `message` field, not reshapes); ADR 0146 (the `suite`/`case` container this `expect`
  lives inside, landed in the same slice).

## Context

Bynk's test assertion was `assert <bool>`: it reduced a checked claim to one bit and,
on failure, threw `AssertionError("assertion failed at <path:line:col>")` — the
location and nothing more. That is the weakest thing a test framework can report: a
reader sees *that* `net(100, 10) == 90` failed but not that the left side was `91`.
Every mature test surface answers "expected X, got Y"; Bynk did not, because `assert`
threw away the operator/operand structure the moment it evaluated to `false`.

The testing track (ADR 0144) reframes every checked claim as the same pure `Bool`
invariant predicate. Under that lens the deficiency is structural, not cosmetic: an
`assert` was never one bit *inherently* — the predicate it evaluated has an operator
and operands, and those are exactly what a good failure report needs. The keyword
`expect` was already reserved-but-unused (since v0.7), waiting for this.

## Decision

**D1 — The keyword `assert` is replaced by `expect`; `assert` is removed, not
deprecated.** Per the track's clean-slate posture, there is no transition period and
no alias: after this slice `assert` is not a keyword. `expect` keeps `assert`'s two
forms verbatim — a statement inside a `case` body and the expression form (in a
`match` arm etc.) — so both parse paths and both check sites move together.

**D2 — `expect` is the invariant predicate, checked identically.** The predicate is
the same pure `Bool` grammar as `invariant` / `ensures` — `is`, `implies`, the
operators, pure value methods — admitting nothing new. It must type to `Bool`
(`bynk.expect.not_bool`) and it is valid **only** inside a `case`
(`bynk.expect.outside_case`); an `invariant` predicate still may not contain an
`expect` (the impurity gate rejects `ExprKind::Expect` exactly as it rejected
`Assert`). This is ADR 0144 D1/D3 made concrete for the first position.

**D3 — A failed `expect` reports expected-vs-actual structure when the predicate is a
top-level comparison.** When the top level of the predicate is a binary comparison
(`==`, `!=`, `<`, `<=`, `>`, `>=`), the emitter lowers it so that on failure it
evaluates **both operands**, renders each (`__bynkShow`), and includes the
predicate's **source text** alongside the substituted values:

```text
commerce.money:
  ✗ deliberate failure
    expect total == 900
      expected: total == 900
      actual:   950 == 900
    at tests/commerce/money.test.bynk:8:12
```

For any other predicate (a bare `Bool`, a pure method call, a boolean connective) it
falls back to source-text + location — strictly no worse than `assert`.

**D4 — The report rides the existing runner protocol; only the `message` grows.** The
multi-line detail is carried in the runner's existing `message` field (ADR 0098), so
the pinned `--format json` document is unchanged in **shape** — the `location`
(`path:line:col`, ADR 0098 D0) is reused verbatim; the enrichment is additive. The
runtime `AssertionError` is renamed `ExpectationError`; a dedicated
`__bynkExpectFailure(location, start, end, detail)` builds the multi-line message.

**D5 — The diagnostic family is normalised onto `bynk.expect.*` (ADR 0144 D4).**
`bynk.assert.non_bool` → **`bynk.expect.not_bool`** (note `non_bool` → `not_bool`,
joining the `bynk.<position>.not_bool` family); `bynk.assert.outside_test` →
`bynk.expect.outside_case`. The legacy `bynk.assert.*` codes are retired, not carried
forward.

**D6 — Structural depth is bounded to top-level comparison for v1.** Double-evaluating
operands to display them is sound only because predicates are pure (ADR 0144 D3).
Rendering is scoped to a **top-level** comparison; `is`-narrowing display and deeper
rendering (operands of a nested connective, per-conjunct attribution) are named
follow-ons, not this slice. Over-reaching into nested rendering is where the slice
would bloat.

## Consequences

- Test failures are legible for the overwhelmingly common comparison case without a
  matcher library — the affordance the one-bit `assert` structurally could not offer,
  earned by ADR 0144's single predicate shape.
- Operand display is **only** sound because the predicate is pure; a future position
  that admitted effects in predicates would break the double-evaluation, which is why
  ADR 0144 D3's purity discipline is load-bearing here, not incidental.
- The runner's JSON contract (ADR 0098) is preserved — existing consumers see a
  richer `message` and an unchanged shape; no schema version bump.
- `bynk.assert.*` is gone from the registry; the `bynk.<position>.not_bool` family
  begins here and each later position (`ensures`, `transition`, observation) reuses
  the pair under its own segment.
- The `is`-narrowing structural report and deeper decomposition are the named
  follow-ons this ADR explicitly holds out of v1.
