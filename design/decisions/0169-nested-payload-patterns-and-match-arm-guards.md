# 0169 — Nested payload patterns and match-arm guards

- **Status:** Accepted (v0.144)
- **Provenance:** proposed in #541 (promoted from the reviewed long-form draft),
  resolving the design-review finding #541 (Bynk Language Design Review
  2026-07-05, §8 Language P1 #2). Supersedes the nested-payload half of
  [ADR 0158](0158-literal-patterns.md) DECISION 6.
- **Realises:** a `match` arm may destructure a variant's payload with a nested
  pattern (`Some(Ok(x))`, `Err(PollClosed)`) and gate on an arbitrary `Bool`
  guard (`Ok(r) if r.status == 200`), so error causes discriminate in one
  `match` and the double-`match` stacks collapse.
- **Relates:** ADR 0158 (literal patterns, the sibling form that shipped v0.130),
  ADR 0159 (the closed literal set), ADR 0103 (the per-arm source-map contract),
  ADR 0156 (the editor surface tracks the language). Adjacent deferred forms:
  #472 (refined `where` patterns), #474 (or-patterns).

## Context

Errors-as-values is a headline feature — `Result`, `Ok`/`Err`, `Some`/`None` are
the language's answer to "make illegal states unrepresentable". But the pattern
language that consumed them stopped one level deep: a variant payload bound only
a *name*, never a nested pattern, and a match arm had no guard. The two everyday
shapes that follow directly from errors-as-values — *discriminate which error*
and *unwrap an `Option` of a `Result`* — were exactly the two the flat pattern
language could not express:

- Coverage was a flat set of *outer* variant names, so `Err(PollClosed)` and
  `Err(UnknownChoice)` collided with `bynk.types.duplicate_variant_arm` — no
  example anywhere discriminated an error cause in a `match res` block.
- `Some(s) => match Json.decode[Status](s) { Ok(st) => … Err(_) => … }` — a
  double-`match` stack (extra indentation, a second non-exhaustive `throw`, an
  intermediate binding) where `Some(Ok(st))` / `Some(Err(_))` / `None` in one
  flat match would do.

This is compiler-shaped work: the checker already walked a variant's payload
types and the emitter already read payload fields off the tag object — the
missing piece was *recursion*, not a new capability. A payload type is an
ordinary type, often itself a sum; the one place the language refused to match a
sum was when it was a payload. Nested patterns make "a payload is just a value,
matched the same way" true.

## Decisions

**A — Standalone increment owning the shared machinery, not a feature track.**
The three deferred pattern forms (#541 here, #472 refined `where`, #474
or-patterns) share two pieces of new machinery: a recursive `Pattern` AST and a
conditional-arm lowering path (a JS `switch` on `.tag` expresses neither a guard
nor a nested test). This increment *introduces and owns* both; #472 folds its
`where` predicate into the same guard slot, and #474 adds an `Or` node the same
coverage walk consumes. Whichever lands first builds the if-chain path; the
others extend rather than rebuild it. A track's spine-issue ceremony was not
warranted for a three-node dependency a plain "blocked-by" edge captures.

**B — The payload position is a `Pattern`; a bare name is a `Pattern::Binding`.**
Each `PatternBinding` matches its payload field against a full sub-`Pattern`
(one recursion point), so a plain `name` is a binding pattern, `_` a wildcard,
and `Ok(x)` a nested variant. A lowercase-led identifier in pattern position is a
binding; an uppercase-led one is a variant constructor — the universal
capitalisation convention, recovered in the parser (the built-in
`Ok`/`Err`/`Some`/`None` are keyword tokens). This means an existing
`Variant(UppercaseName)` that previously bound the payload to an oddly-cased name
now *discriminates* the nested nullary variant — a deliberate, desirable
tightening (the two re-blessed fixtures had written `Err(Declined)` intending
exactly that discrimination).

**C — The guard keyword is `if`, complementary to `where`.** A trailing
`if <Bool-expr>` between the pattern and `=>` gates the arm and sees the arm's
bindings. `if` is an arbitrary `Bool` over the bindings; #472's `where` is the
closed refinement vocabulary over a primitive scrutinee — neither subsumes the
other (the split #472 anticipated). `MatchArm` gains `guard: Option<Expr>`.

**D — Exhaustiveness is bounded structural coverage.** A flat name-set cannot
express "`Err` is covered *when* its payload is exhausted". Coverage recurses,
bounded by declared sum arity: an outer variant is covered when a matching arm
binds its payload irrefutably, or (single-field payload) the arms' sub-patterns
exhaust the field's type. `Some(Ok(_))` / `Some(Err(_))` / `None` is therefore
provably exhaustive with **no** wildcard; a missing nested variant reports
`non_exhaustive_match` with a nested witness (`Err(UnknownChoice)`). A guarded
arm never marks coverage (a guard may fail at runtime). `duplicate_variant_arm`
keys on pattern *shape*, so `Err(A)` and `Err(B)` are distinct while `Ok(_)`
twice is still a duplicate. Multi-field refutable nesting is conservatively
reported uncovered unless a full arm exists (a future slice can widen this).

**E — Nested/guarded matches lower to an `if / else-if` chain; flat/unguarded
matches keep the `switch`.** A `switch` on `.tag` cannot test a nested tag or a
guard, so the moment any arm has a guard or a refutable nested sub-pattern the
whole match lowers to a sequence of independent `if` blocks: each tests its
structural pattern, binds names, then (if guarded) tests the guard, then runs a
body whose tail `return` short-circuits the remaining arms — a guard failing
falls through to the next arm. Flat, unguarded matches emit the unchanged
`switch`, so no existing golden output churns. Per-arm span anchoring (ADR 0103)
is preserved. The shared per-arm emission is the same refactor #472's plan
describes, done once here.

**F — Record patterns are out of v1 (stated deferral).** Field patterns on a
record type (`Status { name, ok }`) are a distinct destructuring surface, not a
payload nesting. They are the natural next slice and reuse exactly the recursive
`Pattern` node this increment builds; deferring them keeps v1 to the two forms
that unblock the review's cited costs.

## Consequences

- Nested error discrimination and `Option[Result[…]]` unwrapping are expressible
  in one flat match (`Ok(n)` / `Err(PollClosed)` / `Err(UnknownChoice)`;
  `Some(Ok(n))` / `Some(Err(_))` / `None`), demonstrated by the new fixtures. (A
  double-`match` whose inner scrutinee is a *computed* value rather than the
  outer payload — e.g. `Some(s) => match Json.decode(s) { … }` in
  `uptime-monitor` — is **not** a payload nesting and is intentionally left
  unchanged.)
- `Variant(UppercaseName)` changes meaning from "bind the payload" to "match the
  nested nullary variant" — a breaking change in principle, mitigated by the
  universal lowercase-binding convention (no example relied on the old meaning;
  two used the new meaning already).
- New diagnostic `bynk.types.guard_not_bool`; `duplicate_variant_arm` and
  `non_exhaustive_match` now key on / witness pattern shape.
- `is` stays flat: nesting and guards are `match`-only (a bare binding after
  `is` matches like `_`).

## Tooling (ADR 0156)

- **Hover:** unchanged — nested bindings hover as their inner payload types via
  the same binding-type recording.
- **Completion:** offering payload variant names as nested-pattern completions is
  a follow-up; the top-level variant completion is unchanged.
- **Semantic tokens:** unchanged — the match-arm `if` is the same keyword token
  as an `if` expression, already highlighted; the tree-sitter grammar's
  `_pattern` recursion + `match_arm` guard and its `parser.c` regeneration are a
  follow-up (the CST already tolerates the new shapes as nested `variant_pattern`
  nodes for highlighting).
- **Signature help:** unchanged — patterns introduce no invocation.
- **Formatter:** renders nested sub-patterns and the `pattern if guard => body`
  arm.

## Alternatives considered

- **Binding rework instead of a recursive payload (B).** A parallel
  `nested: Vec<Pattern>` beside the name bindings duplicates the recursion point;
  making the payload a `Pattern` keeps one.
- **Require a wildcard whenever any arm nests (D).** Simpler, but forfeits the
  whole point — wildcard-free nested exhaustiveness is what moves the
  discrimination into the type system. Kept as the documented fallback if
  closed-sum visibility at depth proves harder than expected.
- **Always lower to an if-chain (E).** Simpler, but churns every existing emitted
  match and its golden fixtures; gating on need keeps the switch for flat matches.
