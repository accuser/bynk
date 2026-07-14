# 0189 — Three `where` predicate tiers, taught explicitly; the `actor … where` grammar matches the compiler

- **Status:** Accepted (v0.163)
- **Provenance:** the keyword-hygiene batch (Bynk Language Design Review
  2026-07-05, §8 Language P1 #9, issue #548). The review found that "one `where`"
  actually hosts **three** predicate sub-grammars, and that ADR 0144's "one
  predicate surface" claim *oversells* — it describes only one of the three. It
  asked that the tiering be taught explicitly if the tiers stay. Last non-docs-only
  item of the batch (after ADR 0185 `stub`, 0186 `&&`, 0187 `websocket`, 0188 the
  lexical rules).
- **Realises:** the three predicate tiers are named in the reference, and the
  tree-sitter grammar for `actor … where` matches what the compiler parses (a full
  expression), instead of mis-modelling it as the type-refinement catalogue.
- **Relates:** ADR 0144 (one predicate surface — this ADR *scopes* its claim, does
  not contradict it), ADR 0091 (authorisation invariants — the actor-claim
  catalogue), ADR 0186 (the `&&` conjunction, shared across the tiers).

## Context

`where`, and the predicate positions generally, look like one uniform surface but
are three:

1. **Type refinement** — `type T = Base where <catalogue>`. A **closed grammar**:
   the built-in `predicate_name`s (`Matches`, `InRange`, `MinLength`, …) joined by
   `&&`. Not user expressions.
2. **Actor claim** — `actor A = Base where <predicate>`. The compiler parses a
   **full expression** and a static-semantics rule restricts it to the closed
   **actor-claim catalogue** (`hasClaim` / `claimEquals` composed with `&&` / `||`
   / `!`) over a `Bearer` base.
3. **Boolean contracts & tests** — `requires` / `ensures` / `invariant` /
   `transition` / `expect`. The **open** tier: any pure `Bool` expression.

ADR 0144's "one predicate surface" describes **tier 3** — production and test
assertions share one grammar. It was never a claim about tiers 1 and 2, which are
deliberately *narrower* closed catalogues. Read as covering all three, it oversells;
the fix the review asked for is to *teach the tiering*, not to collapse the tiers.

Separately, the **tree-sitter grammar was wrong about tier 2**: it modelled
`actor … where <predicate>` as `refinement` (the tier-1 catalogue), so real code
like `actor Admin = User where hasClaim("admin")` produced an **ERROR node** in an
editor — while the compiler parsed it fine (a full expression) and the checker
accepted it. A genuine grammar↔compiler divergence, on the very surface the review
was auditing.

## Decision

**D1 — The tiering is taught, not collapsed (the tiers stay).** The reference
([`refinement`](grammar reference)) now names all three tiers and states which is
closed-in-grammar (1), closed-by-static-semantics (2), and open (3), and that ADR
0144's "one predicate surface" names tier 3. The two closed catalogues are
intentional and remain: a type refinement must stay statically checkable and
schema-serialisable (ADR-0044-era vocabulary discipline), and an actor claim must
stay a closed authorisation vocabulary.

**D2 — The `actor … where` grammar parses a full expression, matching the
compiler.** The tree-sitter `actor_decl` predicate changes from `refinement` to
`_expression`. This is the **grammar moving to the compiler**, not the reverse: the
compiler already parses a general expression and restricts it to the claim
catalogue in a static-semantics pass — exactly as function contracts do (parse
broad, check narrow). The grammar should reflect what *parses*; the claim-catalogue
restriction stays a §5 rule (`bynk.actor.refinement_predicate_unsupported` /
`…refinement_base_unsupported`), unchanged. No compiler, checker, or emitter change
— only the grammar and its regenerated artefacts.

**D3 — The stale "parsed and rejected" prose is corrected.** The grammar reference
and the tree-sitter comment said the actor refinement form was "rejected in
Foundations (`bynk.actor.refinement_unsupported`)" — doubly wrong: the form is
*accepted* for `Bearer` bases (ADR 0091 retired the blanket rejection), and that
diagnostic code no longer exists. Corrected to describe the claim narrowing and the
real diagnostics.

## Consequences

- An editor no longer flags `actor Admin = User where hasClaim("admin")` as an
  error; the grammar and compiler agree on every actor-refinement input. A new
  corpus case pins it.
- A reader learns that `where` is three tiers, and that "one predicate surface"
  (ADR 0144) is the contract/test tier — not a promise that a type refinement
  admits arbitrary boolean expressions.
- ADR 0144 stands; this ADR scopes its wording. No behaviour changes — the closed
  catalogues and the open contract tier are exactly as before.
- The batch's remaining item is docs-only: the canonical enum spelling (#548).
