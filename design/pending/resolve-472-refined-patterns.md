---
level: minor
changelog: Refined patterns (`_ where <predicate>`) in `match` arms
---

## ADR: refined-patterns
title: Refined patterns in `match`: guard-only, `_`-inner, extends the ADR 0169 if-chain
summary: `_ where predicate` as a runtime guard over a literal-kind scrutinee

**Context.** `design/bynk-type-system.md` §2.3.4 has listed `p 'where'
refinement-predicate` in the pattern grammar since the literal-patterns
increment (#441, [ADR 0158](../decisions/0158-literal-patterns.md)), but ADR
0158 DECISION 6 explicitly deferred it: refined patterns interact with
refinement propagation (§2.5) and only make sense once narrowing-in-patterns
is designed, and they share nothing with literal-pattern parsing but the
grammar nonterminal. This closes #472.

Two adjacent decisions this one relies on without changing:

- [ADR 0169](../decisions/0169-nested-payload-patterns-and-match-arm-guards.md) already added the
  if/else-if match-lowering path (`match_needs_if_chain`, `pattern_match_tests`,
  `emit_pattern_bindings`, `emit_match_if_chain` in
  `bynk-emit/src/emitter/lower.rs`) for guarded and nested-payload arms, and
  `if`-guards already ship. This increment is an *extension* of that existing
  mechanism with one more pattern shape that also needs the if-chain, not a
  new conditional-arm mechanism.
- [ADR 0007](../decisions/0007-is-refinement-narrowing.md) already gives `is`
  its own refinement check over a *named* refined type (`value is
  RefinedType`), entirely at check time with no dedicated pattern AST node.
  This increment's `Pattern::Refined` is a distinct, new AST shape for an
  *inline* predicate in pattern position; it does not touch or duplicate ADR
  0007's mechanism.

**Decision.**

D1 — **Closed predicate vocabulary, reused verbatim.** `_ where predicate`
reuses the exact `Refinement`/`RefinementPred`/`PredKind` grammar and AST a
`type X = Base where P` declaration already uses (`Matches`, `InRange`,
`MinLength`, `MaxLength`, `Length`, `NonNegative`, `Positive`, `NonEmpty`).
Admitted only against a literal-kind scrutinee (`Int` or `String`); `Bool` has
no applicable predicate, and `Float` stays rejected like any other
literal-kind match (deferred, S1 below).

D2 — **Guard semantics, no narrowing.** A refined pattern is a runtime guard
only — matching it does not change the static type of anything in the arm's
body. Static narrowing waits on §2.5.4 (refinement propagation), which is
still the specification's largest open question. Like an `if` guard, a
refined arm alone never satisfies exhaustiveness (the predicate can fail at
runtime); a refined-only arm set still needs a wildcard `_` arm.

D3 — **`_`-inner only in v1.** The inner sub-pattern must be a wildcard;
`31 where P`, `Some(x) where P`, and a binding inner (`x where P`) are all
rejected (`bynk.parse.refined_pattern_inner`). This keeps the AST
(`Pattern::Refined { inner: Box<Pattern>, .. }`) forward-compatible with a
wider inner form later, without a rework.

D4 — **Grammar: admitted only at a top-level pattern position, never through a
nested payload.** `refinement`'s own `&&`-joined predicate-list repetition
collides with a surrounding expression grammar wherever a pattern is
expression-continuable (`&&`, `(`, `~>`, …) — a top-level `is_expr` pattern
being one such position, and a nested variant payload (reachable both from a
`match` arm and from `is`) another, since the whole enclosing pattern is
itself followed by more expression grammar once its `)` closes. The
tree-sitter grammar resolves this by admitting `refined_pattern` only as an
alternative on `match_arm`'s pattern field (`choice($._pattern,
$.refined_pattern)`) — never through the shared `_pattern` rule nested
payloads use, and never on `is_expr`'s pattern field, which keeps the
original 3-way `_pattern` choice unchanged.

The Rust hand-written parser mirrors this exactly, not just at the top level:
`parse_pattern_top` (which checks for a trailing `where`) is used only by
`parse_match_arm` and by `is`'s pattern in `parse_eq`; every *nested* pattern
position (`parse_pattern_binding`, reached from both a `match` arm's and
`is`'s variant payloads) calls the plain `parse_pattern`, which never checks
for `where` — so `Ok(_ where P)` and `r is Ok(_ where P)` are syntax errors in
both parsers, in agreement. (An earlier revision of this PR let the `where`
check leak into every recursive call via a shared function, so both
constructs silently parsed and compiled — a genuine grammar/parser
conformance break caught in review; the `parse_pattern` /
`parse_pattern_top` split is the fix, and both cases are now covered by a
tree-sitter corpus/conformance case and negative fixtures.)

D5 — **`match`-only; rejected after `is`, at parse time.** A refined pattern on
the right of `is` is invalid, mirroring ADR 0158's literal-pattern posture —
but unlike a literal pattern (which has no grammar-level reason to exclude
from `is_expr`, and so is checker-rejected), a refined pattern's `where` is
genuinely ambiguous inside `is_expr`'s expression-continuable position (D4).
Since the tree-sitter grammar therefore can never admit it there at all, the
Rust parser rejects it at the same point, in `parse_eq`, right after parsing
`is`'s top-level pattern — not deferred to the checker — so both parsers
agree on every input, not only the nested case. It still raises
`bynk.types.is_refined_pattern` (the code and message are unchanged; only the
layer that raises it moved), steering toward a named refined type
(`x is TypeName`, ADR 0007) or a `match` arm. `check_is` keeps a defensive
`Pattern::Refined` arm with the same diagnostic for exhaustiveness — it is
unreachable for any program that parses, since the parser has already
rejected the input by then.

D6 — **Composes with `if` guards for free.** `if`-guards already ship (ADR
0169); a `_ where P if guard => body` arm needs no new interaction handling —
`pattern_match_tests` appends the refinement's boolean test to the same `tests`
vector a guard's own `if` wraps around, so the two compose without any special
casing.

**Consequences.**

- Reuses `check_refinement` (bynk-check, bumped to `pub(crate)`) for
  predicate/base-compatibility typing — no new predicate-typing logic.
- Reuses `refined_check_as_bool` (bynk-emit, previously `lower_is`'s only
  caller) verbatim for the runtime boolean lowering.
- **S1 (deferred).** `InRange` over `Float` would make `Float` match
  dispatch usable, but requires lifting `match_non_sum_discriminant`'s
  existing `Float` rejection — out of scope here, symmetric with ADR 0158's
  Int/String-only literal set.
- **S2 (deferred).** No duplicate/subsumption detection between two refined
  arms (proving predicate-range overlap is a static-analysis rabbit hole) —
  same posture as ADR 0158 on refinement inhabitance; a redundant refined arm
  is harmless dead code, not a compile error.
- New diagnostics: `bynk.parse.refined_pattern_inner`,
  `bynk.types.is_refined_pattern` (now raised from the parser; see D5).
- New tree-sitter rule `refined_pattern`, reachable only from `match_arm`;
  `is_expr`'s pattern field is unaffected (still the original 3-way
  `_pattern` choice) and `_pattern` itself (nested payloads) is unaffected.
- Editor completion gains a `where`-position branch (offering the predicate
  vocabulary) shared by both a type declaration's `where` and a match arm's
  `_ where` — this vocabulary was not previously offered anywhere (only
  present as inert snippet placeholder text), so this is new completion
  surface, not an extension of an existing one. Excludes a `for all x: T, …
  where <cursor>` generative-test binder's clause, which takes an arbitrary
  `Bool` expression, not the predicate catalogue (caught in review — the
  purely textual `after_clause_keyword("where")` check doesn't distinguish
  grammar positions on its own).
