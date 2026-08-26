# 0400 — `destructure_vals` coerces every `Int`-drawing top-level generative binding to `number` at the bind site, closing #1426's bigint/number arithmetic crash

- **Status:** Accepted (v0.289.9)

summary: `binding_gen`'s boundaries/shrink machinery stays `bigint`-typed by design (#1398's own ADR 0397), but the local a property/contract-attack predicate or attacked-function body evaluates ordinary `Int` arithmetic against must not be — coercing at the destructure site, not the generator, fixes it without disturbing shrinking

**Context.** Every `for all`-bound `Int` (bare or refined/opaque) draws its value
via `__bynkRng.int(lo, hi)`, which returns a JS `bigint` — correct for the
generator/shrink machinery itself (`__bynkShrinkInt(v: bigint, floor: bigint)`,
ADR 0397's own Context), but `emit_test_property_function`'s and
`emit_contract_attack_function`'s own `const [n] = __vals;` destructure bound
that raw `bigint` directly to the predicate's/attacked-function's own local.
Ordinary Bynk `Int` arithmetic lowers to plain TypeScript `+`/`-`/`*`, which
throws `TypeError: Cannot mix BigInt and other types` the instant one operand
is `bigint` and the other a `number` literal — reproduced directly via
`bynkc test` for the simplest possible scalar case (`for all n: Int { expect
n + 1 == n + 1 }`) and confirmed absent with the fix. Never previously hit:
every existing fixture's predicate either avoided arithmetic on a drawn `Int`
entirely, or routed through a refined type's own compile-time-only `as any`
cast (`refined_gen_ts`), which suppresses the *static* `tsc` error, not the
*runtime* one.

A second, latent instance of the identical gap was found while fixing the
first: `emit_contract_attack_function`'s own `requires`/`ensures` clauses
lower over the same raw, uncoerced `__vals` destructure. Its call site
already coerced arguments passed into the real attacked function
(`fname(Number(p), ...)`, v0.114) — masking the bug for a `requires` clause
that only *compares* a parameter (JS tolerates a bigint/number relational
comparison) — but a `requires` clause that does real arithmetic on a
parameter (`requires small: n + 1 < 1000`) still threw, confirmed via a
`fn bump(n: Int) -> Int requires small: n + 1 < 1000 { n + 1 }` repro against
`main` directly, unfixed and fixed.

**Decision.** Add `destructure_vals`, a shared helper both
`emit_test_property_function` and `emit_contract_attack_function` now call in
place of their own hand-formatted `const [{names}] = __vals;` line: for each
binding, wrap `__vals[i]` in `coerce_int_field`'s existing `Number(…)`-wrap
whenever the binding's own resolved type draws `bigint` (reusing the same
"does this type draw bigint" predicate #1398/ADR 0397 already established for
Sum/Record payload/field values, extracted into its own `ty_draws_bigint`
helper both call sites share), otherwise pass the value through unchanged.
Falls back to the original plain array-destructure text when *no* binding in
the set needs coercion — the common case (every existing fixture but one) —
so the fix's own diff footprint is limited to fixtures with a real top-level
`Int`-drawing binding, not a blanket reformat.

`emit_contract_attack_function`'s own pre-existing call-site `Number(…)`-wrap
(`numeric_or_scalar_base`-gated) is now redundant — every argument reaching
the call is already coerced at the bind site — so it's removed rather than
left to double-wrap; `numeric_or_scalar_base` itself is kept (still used to
gate contract attackability).

Review of this PR (#1432) raised two findings, both addressed:

1. **A real, genuine `tsc --strict` regression for a refined/opaque `Int`
   binding.** A bare `Number(…)` local's own inferred type is plain `number`
   — correct for a bare `Int` (which really does compile to `number`
   everywhere), but wrong for a refined/opaque `Int` (`Percent`, etc.),
   which compiles to a *branded* type (`number & { readonly __brand:
   "Percent" }`, `emit_refined_type`). Passing the now-concretely-`number`
   local to a real function expecting the named type is a new `TS2345`
   error this fix would otherwise introduce — confirmed to reproduce for a
   minimal `fn label(p: Percent) -> String` repro (compiled and ran fine
   pre-#1426, since the pre-fix local was `any`, not a concrete type) and
   confirmed closed by the fix below. Added `coerce_top_level_int_binding`:
   for a *named* refined/opaque `Int` specifically, wraps the coerced
   `Number(…)` in `(… as any)` — the same escape hatch `refined_gen_ts`
   already uses for exactly this shape, restoring the compile-time
   permissiveness a plain `any[]`-destructured element always had while
   still fixing the *runtime* value (`as any` is compile-time only; the
   `Number(…)` underneath still executes). `ts_any` rises 30 → 31 as a
   direct, expected consequence — a new, real `as any` escape-hatch site,
   the same class #1423's own floor-correction pass already argued is a
   legitimate, permanent kind of residual, not a regression to chase to 0.
2. **The behavioural test's own `"0 failed"` assertion was too weak** —
   satisfied vacuously by `"0 passed, 0 failed."`, so it would still pass
   if the contract-attack runner stopped being emitted entirely (its own
   golden fixture pins nothing about this, since `positive_fixtures`/
   `tsc_verify` compile without `contracts: true` at all). Strengthened to
   also assert each fixture's own expected case name actually appears in
   the run output, positively confirming the code path this fix touches
   ran, not merely that nothing failed.

**Consequences.** Two new fixtures pin the fix, each with a real predicate
that does arithmetic (not just comparison) on the coerced binding:
`1426_property_scalar_int_arithmetic` (the issue's own literal repro) and
`1426_contract_attack_int_arithmetic` (the `requires`-arithmetic instance
found during the fix). Golden-text/`tsc --strict` diffing alone can't prove a
runtime coercion fix (`__vals: any[]`, so the bug is invisible to `tsc`), so
both are additionally pinned by a new execution-based behavioural test
(`bynkc/tests/int_binding_arithmetic_behaviour.rs`, mirroring
`property_behaviour.rs`/`contract_behaviour.rs`'s own real-CLI-driving shape)
that drives the actual `bynkc test` CLI end to end and asserts a clean pass —
confirmed to fail with the exact reported `TypeError` against the pre-fix
code and pass against the fix, in both directions, for both fixtures.
`243_property_passes`'s own existing fixture (`for all a: Percent, b: Percent`)
is amended as a side effect (`a`/`b` now emit `Number(rng.int(...))` rather
than `(rng.int(...) as any)` at the bind site) — a real, correct extension of
the coercion to a previously-untouched refined-Int top-level binding,
re-blessed here.

Closes #1426.
