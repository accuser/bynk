# 0186 — One conjunction spelling: refinements join with `&&`, and the `and` keyword is retired

- **Status:** Accepted (v0.160)
- **Provenance:** the keyword-hygiene batch (Bynk Language Design Review
  2026-07-05, §8 Language P1 #9, issue #548). The review found two conjunction
  spellings across the predicate surface — refinements used the keyword `and`,
  while contracts / `expect` / agent invariants / actor-claim predicates used the
  `&&` operator — and asked that one be picked. Second item of the batch (the
  first was ADR 0185, the `stub` keyword).
- **Realises:** every conjunction in Bynk is now spelled `&&`. A refinement is
  written `Int where MinLength(3) && MaxLength(20)`; the `and` keyword no longer
  exists and `and` is a free identifier again.
- **Relates:** ADR 0144 (the one-predicate surface — D1 already blessed "the
  operators" as the predicate vocabulary, of which `&&` is the conjunction; this
  ADR makes refinements conform to it), ADR 0007 (`is`-narrowing over refined
  types), ADR 0185 (the sibling keyword-hygiene increment).

## Context

The refinement catalogue is a **closed** list of built-in predicates
(`MinLength`, `Matches`, `InRange`, …) joined into a conjunction. It used the
keyword `and` as a hard grammar separator: `sep1(_refinement_pred, "and")`. The
separator was never a general boolean operator — there is no `||` or `!` in a
refinement, and `and` appeared in no other production.

Everywhere else a conjunction is written, it is the `&&` operator inside a full
boolean expression: a function `requires`/`ensures`, an agent `invariant` /
`transition`, a test `expect`, and the actor-claim predicate
(`hasClaim("a") && hasClaim("b")`). ADR 0144 D1 already names "the operators" as
*the* predicate vocabulary, so `&&` is the sanctioned spelling; refinements were
the lone outlier.

Two spellings for the same idea is exactly the punning the review asked us to
stop. Keeping `and` *only* for refinements also over-promised — it reads like a
boolean operator in a place that admits no other boolean operators.

## Decision

**D1 — Refinements join with `&&`.** The `refinement` production becomes
`sep1(_refinement_pred, "&&")`. The catalogue stays **conjunction-only**: `&&` is
the only combiner, and `||` / `!` remain absent (a refinement is a closed
catalogue, not a general predicate). This is the one place `&&` acts as a
list separator rather than a boolean operator, and the spec says so.

**D2 — The `and` keyword is retired.** With refinements moved to `&&`, `and` is
used by no production. Rather than keep a dead reserved word, the `#[token("and")]`
is removed from the lexer, its `KEYWORDS` entry and `is_reserved_keyword` arm are
dropped, and `and` is now an ordinary identifier. Retiring it (vs. leaving it
reserved-but-unused) is the honest outcome: a reserved word with no grammar is
its own small wart.

**D3 — Nothing else in the predicate surface changes.** Contracts, `expect`,
invariants, and actor-claim predicates already used `&&`; they are untouched. The
`BinOp::And` AST node and the `&&` operator token are the same as before — a
refinement's `&&` reuses the existing token at the lexer, disambiguated by grammar
position (a `where` clause parses a `refinement`, not an `_expression`).

**D4 — Breaking, and taken now.** A refinement written `where A and B` no longer
parses (`and` lexes as an identifier, so the second predicate is unreachable).
Pre-1.0 this is a mechanical `and` → `&&` in refinement clauses; the in-repo
fixtures, examples, playground, book, and the formatter's rendering all migrate
in this increment.

## Consequences

- The predicate surface has one conjunction spelling, `&&`, across production code
  and refinements alike — the visual unification ADR 0144 implied but had not
  enforced on the refinement catalogue.
- The refinement catalogue's closedness is unchanged and re-stated explicitly:
  `&&` is a separator there, and `||`/`!` are still not part of the surface, so
  `&&` does not open the door to arbitrary boolean refinements.
- `and` is freed as an identifier. The formatter renders refinements with `&&`;
  hover/completion are unaffected beyond the keyword list losing `and` (ADR 0156 —
  no new construct, a removed keyword).
- The remaining keyword-hygiene items (#548) — protocol-source casing, lexical
  tightening, the `where`-tier documentation, the enum-spelling convention — land
  separately, each with its own record where it makes a call.
