---
level: patch
changelog: Resolve #1049 — decline the Positive/NonNegative → InRange fold; neither base has a writable bound that could stand for infinity
---

## ADR: positive-nonnegative-inrange-fold-declined
title: The `Positive`/`NonNegative` → `InRange` fold stays undone
summary: Neither `Int` nor `Float` has a writable literal an unbounded `InRange` could fold into, so the fold has no target and only a cost

**Context.** `bynk-greenfield-compiler.md`'s R12.2 names `NonNegative ≡ InRange(0, ∞)` and
`Positive ≡ InRange(1, ∞)` as the Interval domain's canonical form, alongside the
`NonEmpty ≡ MinLength(1)` fold T1.8 shipped
(#1021/#1027, v0.247.0). That PR deliberately narrowed to `NonEmpty` alone and filed the remainder
as #1049, naming the blocker as `Positive`/`NonNegative` on `Float` having "a genuine
exclusive-vs-inclusive-bound ambiguity with no clean answer" — the checker already reads `Positive`
as the closed interval `[1, ∞)` on `Int` (`refinements.rs`'s `check_int_refinement_consistency`) and
the open interval `(0, ∞)` on `Float` (`check_float_refinement_consistency`), and `InRangeF` has no
exclusivity flag to spell the second reading.

Settling that ambiguity first, as #1049 asked, surfaces a prior question: what would the fold
actually change? `canon_predicate` is consumed by exactly two things — `service_contract_hash` and
`refinements_match`'s exact string-equality — so folding `Positive`/`NonNegative` into an `InRange`
spelling only matters if some *other*, independently-writable spelling collapses onto the same
string. That is what made the `NonEmpty` fold real: `MinLength(1)` is a spelling a caller already
writes, and after the fold `String where NonEmpty` and `String where MinLength(1)` hash and match
identically (pinned by `1021_refined_sugar_equivalence_cross_context`).

`Positive`/`NonNegative` have no such counterpart, on either base:

- **`Float`.** `InRangeF`'s bounds are parsed `f64` literals, and the lexer rejects any literal that
  would parse to infinity outright — `bynk.lex.float_literal_overflow` fires whenever
  `slice.parse::<f64>()` is not finite (`lexer.rs:614-625`). There is no source spelling of
  `InRangeF(0, ∞)` or `InRangeF(0.0…1, ∞)` for the fold to collapse onto; `∞` is not a value this
  grammar can produce.
- **`Int`.** `IntBound` stores `i64`, and `i64::MAX` (`9223372036854775807`) *is* a writable literal
  — `bynk.lex.integer_overflow` only rejects what does not fit in 64 bits (`lexer.rs:601-612`). But
  treating `i64::MAX` as a stand-in for "unbounded" would be inventing an equivalence the language
  does not state: `Int` erases to a JS `number` (`emitter.rs:3979`), whose safe integer range
  (`Number.MAX_SAFE_INTEGER`, `2^53-1`) is many orders below `i64::MAX`, so a literal
  `InRange(1, 9223372036854775807)` is not "no upper bound" to a reader or to the runtime — it is a
  specific, oddly-chosen finite bound, indistinguishable in the grammar from any other. Folding
  `Positive` onto that string would make `Int where Positive` and `Int where
  InRange(1, 9223372036854775807)` the same boundary type by coincidence of `i64`'s width, not by
  anything the two spellings actually assert in common.

So neither base has a real second spelling for the fold to land on. Implementing it anyway means
`canon_predicate` inventing a synthetic string (`"InRange(1, inf)"` or similar) that nothing else can
ever produce — observably identical to leaving `Positive` as its own literal, just with a different
label. The one measurable effect is cost: every boundary type carrying a transparent
`Positive`/`NonNegative` gets a new `X-Bynk-Contract` hash, with the coordinated-redeploy skew window
#1027 already paid once for `NonEmpty`, moving fixture goldens for no new match gained.

**Decision.** Leave `Positive`/`NonNegative` as their own canonical literals in `canon_predicate`; do
not thread a `base: BaseType` parameter through it, `canon_refinement`, or `refinements_match` for
this alone. R12.2's Interval-domain table is not wrong as a *destination* — it is what the same
document's R12.3 (entailment) would want to consume, so `InRange(5, ∞)` could be shown to entail
`NonNegative` by interval containment instead of by name. But R12.3 has no implementation anywhere in
the tree today (#1021's research), so there is no consumer for the normalised form yet, and picking
the `∞` representation and the `Float` bound reading in isolation — without the entailment code that
would exercise them — is exactly the kind of speculative infrastructure this codebase's own review
culture flags elsewhere. The right time to settle both is together, when R12.3 is actually built: the
entailment algorithm's own needs (does it want a sentinel bound, an `Option<Bound>`, or something
else?) should drive the representation, not a guess made a phase early.

This also answers the Float ambiguity #1049 asked to settle first: it does not need settling
independently. The checker's existing split reading — `Int` `Positive` closed at `[1, ∞)`, `Float`
`Positive` open at `(0, ∞)` — is internally consistent runtime behaviour, unaffected by this
decision, and stays exactly as shipped.

**Consequences.** #1049 closes with no code change. Appendix D's R12.2 row is updated to record the
fold as declined-not-open, with the trigger for revisiting named as R12.3 landing. A future PR
building entailment settles the `∞` representation as part of that work, not before it; if it turns
out interval-arithmetic entailment does not need `Positive`/`NonNegative` folded into `InRange` at
all (e.g. it dispatches per-predicate-name instead), this decision needs no reversal — it already
does nothing today.
