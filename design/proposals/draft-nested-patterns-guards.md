<!--
LONG-FORM DRAFT of an increment proposal — transient, not a durable artefact.
Drafted here for line-anchored review before promotion (design/proposals/README.md
§"Drafting long-form proposals"). On acceptance this is promoted to a GitHub
issue from .github/ISSUE_TEMPLATE/increment-proposal.md (label `proposal`) and
this file is deleted; the issue is the sign-off artefact, `accepted` is the
approval to build. Do NOT pre-allocate the version or ADR number — both are
taken when the implementing PR lands.

Resolves the design-review finding in #541 (Bynk Language Design Review
2026-07-05, §8 Language P1 #2). Adjacent to #472 (refined `where` patterns) and
#474 (or-patterns) — the three deferred forms from ADR 0158 DECISION 6.
-->

# Nested payload patterns + match-arm guards

## Summary

- **Scope:** **grammar/AST** (`bynk-syntax`), **checker** (`bynk-check`),
  **emitter** (`bynk-emit`), and the four editor surfaces (`bynk-lsp`,
  `bynk-fmt`, `tree-sitter-bynk`). Runtime unchanged. Two additive surfaces: a
  payload binding may itself be a **pattern** (`Some(Ok(x))`, `Err(PollClosed)`),
  and a match arm may carry an **`if` guard** (`Ok(r) if r.status == 200 => …`).
- **Addresses:** patterns are flat — a variant payload binds only a *name*, never
  a nested pattern (`PatternBinding` at `bynk-syntax/src/ast.rs:2331` is
  `Positional { name }` / `Named { field, name }`, with no recursion), and a
  match arm is `pattern => body` with no guard slot (`MatchArm` at `:2247`).
  Two concrete costs the review names:
  - **`Err(PollClosed)` / `Err(UnknownChoice)` cannot discriminate an error
    cause.** Coverage is a flat set of *outer* variant names
    (`covered.insert(variant.name)` at
    `bynk-check/src/checker/expressions.rs:2381`), so a second `Err(…)` arm is
    rejected `bynk.types.duplicate_variant_arm` (`:2383`) even though the two
    payloads are different variants. No example anywhere discriminates error
    causes in a `match res` block.
  - **Double-`match` stacks** where one nested match would do — e.g.
    `Some(s) => match Json.decode[Status](s) { Ok(st) => … Err(_) => … }`
    (`examples/uptime-monitor/src/monitor.bynk:69`), and the `Some(user)` / `None`
    unwrap over a `whoami` result in `examples/sessions/src/sessions.bynk:58`.
    These are `Some(Ok(st))` / `Some(Err(_))` / `None` in one flat match.
- **Realises:** a match arm may destructure a variant's payload with a nested
  pattern and gate on an arbitrary `Bool` guard, so error causes discriminate in
  one `match` and the nested-`match` stacks collapse. `Err(PollClosed) => …` and
  `Err(UnknownChoice) => …` type-check as distinct arms; `Some(Ok(st)) => …`
  reads the inner success in place.

## Framing (why this is the language's to fix)

Errors-as-values is a headline feature — `Result`, `Ok`/`Err`, `Some`/`None`
are the language's answer to "make illegal states unrepresentable"
(`README.md`). But the pattern language that consumes them stops one level deep:
you can name the payload, you cannot look inside it, and you cannot say "this
arm, but only when …". The two everyday shapes that follow directly from
errors-as-values — *discriminate which error* and *unwrap an `Option` of a
`Result`* — are exactly the two the flat pattern language cannot express. The
author is pushed back to a nested `match` (extra indentation, a second
non-exhaustive `throw`, an intermediate binding) or, for error discrimination,
cannot express it at all without a downstream `if`/`is` ladder. This is
compiler-shaped work: the checker already walks a variant's payload types
(`bynk-check/src/checker/expressions.rs:2444`) and the emitter already reads
payload fields off the tag object (`bynk-emit/src/emitter/lower.rs:3468`) — the
missing piece is *recursion*, not a new capability.

**This closes an inconsistency, not just a convenience.** A payload *type* is an
ordinary type — often itself a sum (`Result[T, E]`, `Option[T]`) — yet the one
place the language refuses to match a sum is when it is a payload. Everywhere
else a sum is matchable; as a payload it is opaque-until-rebound. Nested patterns
make "a payload is just a value, matched the same way" true.

### Relationship to #472 and #474 (the deferred-pattern cluster)

ADR 0158 DECISION 6 deferred three pattern forms behind literal patterns
(shipped v0.130, #441): refined `where` patterns (#472), or-patterns (#474), and
the nested-payload + guard case this proposal covers. They are adjacent but
**distinct surfaces** — and, critically, they share **two pieces of new
machinery** that today's flat pattern language does not have:

1. **A recursive `Pattern` AST.** All three need a payload/alternative position
   to hold a `Pattern`, not a name.
2. **A conditional-arm lowering path.** Match arms lower to a JS `switch` on
   `.tag` (`emit_match_tail` at `bynk-emit/src/emitter/lower.rs:3381`,
   ADR 0158 D4). A `switch` structurally cannot express a per-arm guard or a
   nested test. #472's own resolution plan already calls this out and proposes
   "when any arm carries a `where`, the whole match lowers to an `if / else-if`
   chain" — the identical mechanism a guard or a nested payload needs.

#472's plan even names this exact increment on paper: its DECISION D6 reserves
`if` for "arbitrary `Bool` over payloads", complementary to `where`'s "closed
vocabulary over primitives". **So the `if` guard here is the complement #472
designed against, and the if-chain lowering is shared with #472 and #474.**

This raises **[DECISION A]** below: whether to unify the three into a feature
track or ship #541 standalone and have it *own* the shared machinery. The
recommendation is standalone-but-coordinated; see the decision for the reasoning.

## What exists today (grounded)

**AST** (`bynk-syntax/src/ast.rs`):

- `Pattern` is flat — `Wildcard(Span)` | `Literal { value, span }` |
  `Variant { type_name, variant, bindings, span }` (`:2274`). A variant's
  `bindings` are `Vec<PatternBinding>` (`:2290`).
- `PatternBinding` (`:2331`) is `Positional { name: Ident }` |
  `Named { field: Ident, name: Ident }` — a binding is a *name*, never a
  sub-pattern. There is no recursion point.
- `MatchArm` (`:2247`) is `{ pattern, body, span }` — no guard field.

**Parser** (`bynk-syntax/src/parser/expressions.rs`):

- `parse_pattern` (`:1019`) dispatches wildcard / literal / built-in-variant
  (`Ok`/`Err`/`Some`/`None`, `:1116`) / ident-led tag pattern; a binding list is
  `parse_pattern_binding` (`:1151`), which parses only `_`, `name`, or
  `field: name`. A match arm is parsed as `pattern => body` with no guard branch.

**Checker** (`bynk-check/src/checker/expressions.rs`, `check_match` at `:2239`):

- Coverage/duplicate detection is a **flat set of outer variant names**:
  `covered.insert(variant.name.clone())` (`:2381`); a repeat is
  `bynk.types.duplicate_variant_arm` (`:2383`). This is the direct cause of the
  `Err(PollClosed)` / `Err(UnknownChoice)` rejection.
- Each binding is resolved to a payload field's *type* and `ctx.bind`-ed as a
  leaf name (`:2450` positional, `:2421` named) — the payload type is never
  itself matched.
- Non-exhaustiveness is computed by diffing `covered` against the scrutinee's
  variant set (`:2484`); a wildcard sets `saw_wildcard` and short-circuits.

**Emitter** (`bynk-emit/src/emitter/lower.rs`):

- A `match` lowers to `switch (disc.tag) { case "Ok": { … } … }`
  (`emit_match_tail` `:3381`, `emit_match_case` `:3430`). A variant arm binds
  each payload field to a `const` off the tag object
  (`const {local} = {disc_var}.{field};`, `:3482`), then emits the body. There is
  **no** if/else-if lowering path and **no** guard emission anywhere.

**Fixtures** confirm the flat surface: `positive/40_match_with_bindings`,
`41_match_with_named_bindings`, `42_match_with_wildcard`, and
`131_assert_nested_match` (a *stacked* match, the shape this increment
flattens). Negatives `293_match_literal_duplicate_arm`,
`33_match_non_exhaustive`, `35_match_unreachable_arm`.

## The surface

Two additive forms. No existing program changes meaning.

**1 — Nested payload patterns.** A payload binding position may hold a pattern
(wildcard, literal, or variant) instead of only a name:

```bynk
match res {                       -- res: Result[Response, FetchError]
  Ok(r)               => r.status
  Err(PollClosed)     => 0        -- discriminate the error cause …
  Err(UnknownChoice)  => 400      -- … in the same match (was: duplicate_variant_arm)
}

match Json.decode[Status](stored) {   -- stored decode, once
  Some(Ok(st))   => Ok(st)
  Some(Err(_))   => ServerError("stored status is corrupt")
  None           => NotFound
}
```

Named nesting composes the same way: `Held(guest: Some(g)) => …`. The inner
pattern binds at its depth; a nested `_` discards; a nested literal matches by
value (reusing v0.130 literal patterns).

**2 — Match-arm `if` guard.** A trailing `if <Bool-expr>` gates the arm; the
guard sees the arm's bindings:

```bynk
match req {
  Get(path) if path.startsWith("/api")  => route(path)
  Get(path)                             => static(path)
  Post(body) if body.length() > maxSize => TooLarge
  _                                     => NotFound
}
```

A guarded arm never contributes to exhaustiveness (it may fail at runtime), so a
following arm over the same shape stays reachable — the standard guard treatment,
and the same rule #472 applies to a `where` arm.

## Decisions

**[DECISION A] Standalone increment vs. a "patterns" feature track (Recommended:
standalone, owning the shared machinery).**
The three deferred forms (#541 here, #472 refined, #474 or-patterns) share the
recursive `Pattern` AST and the if/else-if lowering path (see Framing). Option 1
— fold all three into a `patterns` feature track (a spine issue with three
slices, ADR 0167). Option 2 — ship #541 standalone; it *introduces and owns* the
recursive AST and the conditional-arm lowering, and #472/#474 build on top.
Recommend **Option 2**: #541 is already scoped as a single P1 increment, and
nested payloads + guards are the highest-yield of the three (they unblock error
discrimination and collapse the nested-`match` stacks the review cites). A track
adds spine-issue ceremony for a three-node dependency that a plain "blocked-by"
edge captures. **Consequence:** this increment's ADR must define the recursive
`Pattern` node and the if-chain lowering as *reusable* — #472 folds `where` into
the same guard slot, #474 adds an `Or` node the same coverage walk consumes — and
must state that whichever of the three lands first builds the if-chain path (if
#472 ships first, #541 reuses its path rather than rebuilding it). This is called
out explicitly so the overlap is coordinated, not rediscovered.

**[DECISION B] Nested recursion vs. binding rework (Recommended: make the payload
position a `Pattern`).**
To nest, a payload position must hold a `Pattern`. Option 1 — replace
`PatternBinding`'s leaf name with a sub-pattern, so positional/named bindings
carry `Pattern` (a bare `name` becomes sugar for a binding sub-pattern). Option 2
— add a parallel `Pattern::Variant { nested: Vec<Pattern> }` alongside the
existing name bindings. Recommend **Option 1**: one recursion point, and the
existing "bind a name" stays the base case (an identifier pattern binds and
matches anything, `_` discards). Concretely: a `PatternBinding` gains a
`pattern: Pattern` and the leaf identifier becomes `Pattern::Binding(Ident)` (or
the existing name is retained as the wildcard-binding base case). `Pattern::span`
(`ast.rs:2317`) and every `match &pattern` site extend to the new arm — the
compiler's exhaustive `match` makes the checker/emitter/fmt update sites
self-locating.

**[DECISION C] Guard keyword `if` vs. `when` (Recommended: `if`).**
Recommend **`if`** — it reads as English (`Get(path) if path.startsWith(…)`),
matches Rust/Swift/OCaml intuition, and is already a keyword (no lexer change).
It composes cleanly with #472's `where`: **`if` = arbitrary `Bool` over the arm's
bindings; `where` = closed refinement vocabulary over a primitive scrutinee** —
exactly #472 DECISION D6's split, neither subsumes the other. Parsing is
unambiguous: a match-arm `if` appears only between a pattern and `=>`, never at
statement head. **Consequence:** `MatchArm` gains `guard: Option<Expr>`.

**[DECISION D] Exhaustiveness with nested patterns (Recommended: bounded
structural coverage).**
Flat name-set coverage (`covered: HashSet<String>`) cannot express "`Err` is
covered *when* its payload is exhausted". Option 1 — bounded structural coverage:
track, per outer variant, the coverage of its payload (recursively, bounded by
the sum's arity); an outer variant is *covered* only when its payload is
exhausted (a wildcard/name binding covers it fully; nested variant arms cover it
when the inner variant set is exhausted). Option 2 — require a wildcard whenever
any arm nests (no true nested exhaustiveness). Recommend **Option 1**: it is what
makes `Some(Ok(_))` / `Some(Err(_))` / `None` provably exhaustive *without* a
`_`, which is the whole point of moving the discrimination into the type system.
Recursion is bounded by declared sum arity, so it terminates. A guarded arm
(DECISION C) and, later, a `where` arm never mark coverage. **Consequence:**
`covered` becomes a small coverage tree; `duplicate_variant_arm` fires only on a
*structurally* identical (unguarded) pattern, so `Err(PollClosed)` and
`Err(UnknownChoice)` are distinct. New/changed diagnostics:
`non_exhaustive_match` gains nested witnesses ("variant `Err(UnknownChoice)` is
not covered"); `duplicate_variant_arm` keys on pattern shape, not name.

**[DECISION E] Emitter lowering: decision tree vs. if/else-if chain (Recommended:
if/else-if chain when any arm nests or is guarded).**
A `switch` on `.tag` cannot test a nested tag or a guard. Option 1 — keep the
`switch` for flat/unguarded matches (unchanged, zero risk to existing output) and
lower to an **`if / else-if` chain** the moment any arm has a guard or a
non-binding nested sub-pattern: each arm becomes
`if (disc.tag === "Err" && disc.value.tag === "PollClosed") { … }`, a guard
appends `&& (<lowered guard>)`, bindings are `const`s hoisted into the arm block,
wildcard tail → `else`, retaining the non-exhaustive `throw`. Option 2 — always
if-chain (simpler, but churns every existing emitted match and its golden
fixtures). Recommend **Option 1**: it preserves ADR 0103's per-arm source-map
contract (each arm's lines anchor to that arm's span, as `emit_match_case`
already does) and touches no existing golden output. **Consequence:** factor
per-arm body/binding/span emission out of `emit_match_case` so the `switch` and
`if`-chain paths share it — the same refactor #472's plan describes, done once
here.

**[DECISION F] Record patterns — in or out (Recommended: out of v1, stated).**
The review lists "no record patterns" (`Status { name, ok }`) as an adjacent gap.
It is a *distinct destructuring surface* (field patterns on a record type), not a
payload nesting. Recommend **defer**: keep v1 to nested *variant/literal
payloads* + guards — the two forms that directly unblock the review's cited costs
— and leave record patterns to a follow-on once the recursive `Pattern` node
exists (this increment builds exactly the node a record pattern would reuse). The
ADR names record patterns as the natural next slice so the deferral is a plan,
not an omission.

## The deltas (concretely)

- **Grammar / AST (`bynk-syntax`).** `PatternBinding` gains a nested `Pattern`
  (DECISION B); `MatchArm` gains `guard: Option<Expr>` (DECISION C). `parse_pattern`
  recurses into a binding position; `parse_match_arm` parses an optional
  `if <expr>` before `=>`. `Pattern::span` and the built-in-variant path extend.
  EBNF productions `variant_pattern` / `pattern_binding` / `match_arm` in
  `bynk-grammar/src/lib.rs` (rendered into `spec/syntactic-grammar.md` §4.7,
  `spec/grammar-appendix.md`, and the generated grammar JSON) gain the recursion
  and the guard.
- **Checker (`bynk-check`).** `check_match` (`checker/expressions.rs:2239`):
  replace flat `covered` with the bounded coverage tree (DECISION D); recurse
  into nested payload patterns to bind at depth and to type each inner pattern
  against the payload field's type (reusing the existing per-arm binding logic at
  `:2444`); type the guard expression as `Bool` and bind the arm's names into its
  scope; a guarded arm skips coverage. New diagnostics:
  `bynk.types.guard_not_bool`; `duplicate_variant_arm` / `non_exhaustive_match`
  updated to structural keying/witnesses. `check_is` continues to reject the new
  richer forms after `is` (guards/nesting are `match`-only, mirroring the
  literal-pattern posture).
- **Emitter (`bynk-emit`).** Add the `if/else-if` lowering path (DECISION E) in
  both match positions (`emit_match_tail` and the IIFE path
  `lower_match_as_iife`/`build_match_iife`); factor shared per-arm emission out of
  `emit_match_case`; lower a nested pattern to conjoined `.tag`/value tests and a
  guard to a trailing `&& (<guard>)`. Flat unguarded matches keep the `switch`.
- **Runtime.** None — nested tags and guards are ordinary JS property reads and
  boolean expressions; no runtime library surface changes.

## Risks & mitigations

- **Scope creep into a full pattern-match compiler (decision trees, redundancy
  analysis).** → Bound it: coverage recursion is limited by declared sum arity;
  no redundancy/subsumption lint in v1 (a dead guarded arm is harmless — same
  posture #472 S2 takes). Record patterns and or-patterns stay out (DECISIONS F,
  A).
- **Duplicate/`covered` rework regresses existing exhaustiveness diagnostics.** →
  The coverage tree degenerates to the current name-set for flat matches; the
  existing negative fixtures (`33`, `293`, `295`, `227`, `35`) are the guardrail
  and must pass unchanged.
- **If-chain lowering diverges from `switch` semantics or breaks source maps.** →
  Only matches that *need* it take the new path; golden fixtures for existing
  flat matches are unchanged; new golden fixtures assert per-arm span anchoring
  (ADR 0103).
- **Overlap with #472/#474 causes rework.** → DECISION A makes this increment own
  the reusable AST node and if-chain path, and its ADR states the sequencing;
  #472/#474 become "blocked-by" and extend, not rebuild.

## Docs delta

- **Reference / Guide / Spec:** spec §4.7 (`syntactic-grammar.md`) — nested
  `variant_pattern` and the `match_arm` guard; `static-semantics.md` — nested
  coverage/exhaustiveness rules and the guard's non-exhaustiveness + `Bool`-type
  well-formedness; `emission.md` — the if-chain lowering. Book: the
  `guides/type-system/match.md` learning page gains a **nested patterns** section
  and a **guards** section (via `{{#include}}` from the new compiled fixtures so
  they cannot rot); `reference/diagnostics.md` + `diagnostics.rs` gain
  `guard_not_bool` and the reworded duplicate/non-exhaustive entries;
  `reference/grammar.md` regenerated. A genuinely new *concept* (guards) earns an
  "Understand" note, not just a recipe.
- **Changelog + version history:** advance the currency banner and
  `spec/appendix-version-history.md` to the version this ships as (assigned by
  `scripts/bump-version.sh` at implementation, not pre-allocated); add the
  `reference/changelog.md` entry.
- **Roadmap:** move nested payload patterns + guards from planned → shipped; keep
  refined patterns (#472), or-patterns (#474), and record patterns (DECISION F)
  as planned, named as intent rather than version-pinned.

## Tooling delta (ADR 0156 — silence is an oversight)

- **Hover:** unchanged — a nested pattern's bindings hover as their (now inner)
  payload types via the same binding-type recording; no new declaration surface.
- **Completion:** **changed** — after a variant pattern's `(`, offer the
  payload's variant names as nested-pattern completions (e.g. `Ok`/`Err` inside
  `Some(…)` when the payload is a `Result`); this reuses the variant-completion
  the top-level pattern already offers, applied at depth.
- **Semantic tokens:** **changed** — the match-arm `if` guard keyword tokenizes as
  a keyword; nested variant names tokenize as variants (reuse). tree-sitter
  `_pattern` gains the recursion and `match_arm` the optional guard; regenerate
  `parser.c`, add a corpus entry, bump `tree-sitter-bynk`/`vscode-bynk`.
- **Signature help:** unchanged — patterns introduce no invocation.
- **Formatter (`bynk-fmt`):** **changed** — canonicalise a nested pattern's
  spacing and the `pattern if guard => body` arm; add a golden fixture.

## Done when

- `Err(PollClosed) => …` and `Err(UnknownChoice) => …` type-check as distinct
  arms in one `match res` (no `duplicate_variant_arm`); the `uptime-monitor`
  (`monitor.bynk:69`) and `sessions` (`sessions.bynk:58`) nested-`match` stacks
  are expressible — and rewritten — as one flat match each.
- `Some(Ok(_)) | Some(Err(_)) | None` is proven exhaustive with **no** wildcard;
  a missing nested variant reports `non_exhaustive_match` with a nested witness.
- A guard gates its arm, sees the arm's bindings, must be `Bool`
  (`guard_not_bool`), and never satisfies exhaustiveness on its own.
- Existing flat matches emit the unchanged `switch`; nested/guarded matches emit a
  span-anchored if-chain that `tsc --strict` accepts.
- Fixtures (next free indices) cover: nested variant payload (`Some(Ok(x))`),
  nested error discrimination (`Err(A)`/`Err(B)`), nested literal
  (`Ok(200)`-style), guarded arm, guard-not-bool (negative), nested
  non-exhaustive (negative), and the two rewritten example programs.
- Docs current per the delta above; all four tooling surfaces stated.
- Version bump (`scripts/bump-version.sh`) — this is a language/tooling increment.
- A new ADR records DECISIONS A–F (number assigned when the implementing PR
  lands), defines the recursive `Pattern` node and if-chain lowering as reusable
  by #472/#474, and supersedes the nested-payload half of ADR 0158 DECISION 6.
  The implementing PR closes this issue (`Closes #541`).
