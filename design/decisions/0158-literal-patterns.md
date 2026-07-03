# 0158 — Literal patterns in `match`: scope, kind split, and exhaustiveness

- **Status:** Accepted (v0.130.0; 2026-07-03)
- **Spec:** `design/bynk-type-system.md` §2.3.4 (Pattern matching)
- **Closes:** [#441](https://github.com/accuser/bynk/issues/441)

## Context

§2.3.4's pattern grammar has always listed a literal pattern (`p ::= … | c`),
but the parser never implemented it: any `match` arm with a bare literal
(`31 => …`) failed with `expected identifier as a match-arm pattern`. The common
idiom it blocked — dispatching a raw primitive (an external reference code, a
flag) against a fixed set of values — had to be written as an `if`/`else if ==`
chain. Closing the gap is not a one-line parser fix: `match` was gated end-to-end
on a sum/`Result`/`Option` discriminant (`match_non_sum_discriminant`), so
matching a primitive is new surface across the AST, checker, and emitter.

## Decision

**D1 — The literal set is [ADR 0001](0001-literal-admission-set.md)'s closed set,
minus `()`.** Integer (including a leading unary minus), string, and boolean
literals — reused verbatim so the language has one notion of "a literal".
Excluded: **`Float`** (equality/`switch` on floating point is the comparison the
language deliberately avoids — ADR 0040/0044) and **`()`** (matching a
single-inhabitant type is vacuous). A `Float` scrutinee keeps the
`match_non_sum_discriminant` rejection.

**D2 — A `match` is *either* variant-kind *or* literal-kind; they don't mix.**
The discriminant classifies as variant-kind (sum/`Result`/`Option`) or
literal-kind (`Int`/`String`/`Bool`, or a refinement over one of those bases,
which widens to its base). A literal pattern is admitted only against a
literal-kind scrutinee and a variant pattern only against variant-kind; a mix is
`pattern_type_mismatch`. A refined scrutinee is a raw primitive at runtime, so
literal arms are checked against the **base** type; whether a literal actually
inhabits the refinement is **not** enforced (a dead arm is harmless, and
enforcing it would drag ADR 0001's static evaluator into pattern position).

**D3 — Exhaustiveness: `Int`/`String` require a wildcard; `Bool` is complete
with both literals.** `Int` and `String` are unbounded, so a literal-only match
can never be complete — require a trailing `_` (`non_exhaustive_match`). `Bool`
is two-valued, so it completes once both `true` and `false` appear *or* a
wildcard does; forcing `_` on a type a reader knows is finite is a papercut. A
repeated literal arm is `duplicate_literal_arm` (mirroring
`duplicate_variant_arm`).

**D4 — Lower to a value-`switch`.** A literal-kind match lowers to
`switch (<value>) { case 31: … default: … }`, mirroring the existing
tag-`switch` so the source-map arm-stepping contract (ADR 0103) is preserved.
JS `switch` compares with `===` — exactly the value equality wanted for the
three types.

**D5 — Literal patterns are `match`-only; reject them after `is`.** The grammar
shares one pattern nonterminal between match arms and `value is pattern`, but
`is` tests a value's type/refinement, not equality — `x is 31` would trap a
reader who means `x == 31`. The checker rejects a literal after `is`
(`is_literal_pattern`), steering to `==`.

**D6 — `where`-refined patterns and or-patterns stay deferred.** §2.3.4 also
lists `p where predicate` and `p | p`. Both are out of scope: refined patterns
interact with refinement propagation (§2.5) and only make sense once
narrowing-in-patterns is designed; they share nothing with literal parsing but
the grammar nonterminal.

## Consequences

- The literal set can widen later (ADR 0001's own consequence) without breaking
  programs; nothing depends on admitting `Float`/`()` or computed expressions.
- A binary `+`/`-` beginning a new line no longer continues the previous
  expression (so `10` ⏎ `-2 => …` reads as two arms, not `10 - 2`). No existing
  program continued an expression with a leading operator on the next line, so
  this is a clarification, not a break.
- Not enforcing refinement inhabitance (D2) means a literal arm outside a refined
  scrutinee's predicate is silently dead code; a future warning-channel lint
  (ADR 0117) could flag it without failing the build.
