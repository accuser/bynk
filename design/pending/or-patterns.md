---
level: minor
changelog: Or-patterns (`p₁ | p₂`) in `match` arms and after `is`
---

## ADR: or-patterns
title: Or-patterns (`p₁ | p₂`)
summary: Pattern alternation in `match` and `is`, and its emitter split between a flat switch and an if-chain

**Status:** Accepted

**Provenance:** [#474](https://github.com/accuser/bynk/issues/474), deferred out
of the v0.130 literal-patterns increment as
[ADR 0158](0158-literal-patterns.md) DECISION 6, and named by
[ADR 0169](0169-nested-payload-patterns-and-match-arm-guards.md) DECISION A as
one of the two forms expected to extend (not rebuild) its recursive `Pattern`
AST and if-chain lowering.

**Realises:** `design/bynk-type-system.md` §2.3.4's or-pattern grammar
(`p₁ | p₂ | … | pₙ`, left-associative) and §2.3.6's "or-patterns with `is`" —
both previously specified but unimplemented.

**Relates:** ADR 0158 (literal patterns — the sibling form that shipped
v0.130), ADR 0169 (nested payload patterns and match-arm guards — the shared
`Pattern` AST and if-chain machinery this increment extends), ADR 0156 (the
editor surface tracks the language). The other deferred sibling, #472
(refined `where` patterns), has not landed.

## Context

Two everyday shapes fell out of scope when ADR 0169 shipped nested payload
patterns and guards: several variants (or literals) sharing one arm body
(`Held(_) | Confirmed(_) => …`), and picking out a *subset* of a shared shape
across variants with different field layouts (`Held`'s room is its 2nd field,
`Confirmed`'s is its 2nd field too, but the reservation id sits at different
offsets). Without or-patterns, both require either duplicating the arm body
per variant or an early return keyed on a manual `match … { A | B => true, _
=> false }` predicate — exactly the boilerplate the spec's or-pattern grammar
was written to avoid. ADR 0169 pre-committed the AST/emitter shape (DECISION
A): a recursive `Pattern` node and a conditional-arm (if-chain) lowering path
for whatever a `switch` on `.tag` can't express. This increment adds the `Or`
node and teaches both lowering paths, the checker's binding-consistency
rules, and (per §2.3.6) the `is` operator about it.

## Decisions

**A — `Pattern::Or(Vec<Pattern>, Span)`, flattened by the parser, never
nested.** The parser builds it with an iterative chain-fold over `|`
(mirroring the existing fold over boolean `||`), same shape as `parse_or`,
so a long `1 | 2 | … | N` chain costs one nesting level against the shared
recursion budget (#714) rather than N. A parenthesized sub-pattern (see
DECISION E) splices its alternatives into the enclosing fold instead of
nesting, so every `Pattern::Or` a checker/emitter pass sees has leaf
alternatives (`Wildcard`/`Binding`/`Literal`/`Variant`) — no consumer needs
to handle a nested `Or`.

**B — Three well-typedness rules, checked by re-running the existing
per-alternative check and reading the result back.** `check_pattern`
recurses into each alternative with the *same* per-kind logic every other
pattern already uses (so Rule 3, "same value type", is enforced for free —
mismatches raise the existing `pattern_type_mismatch`), then verifies Rule 1
(same bound names) and Rule 2 (same type, *including refinement* — exact
`Ty` equality, not `join_ty`'s refined-to-base widening, since Rule 2 exists
specifically to reject that widening) by reading each alternative's binding
types back out of `ctx.expr_types`, which the per-kind recursion just
populated. New diagnostics: `bynk.types.or_pattern_binding_mismatch`,
`bynk.types.or_pattern_type_mismatch`.

**C — Coverage and duplicate-detection flatten an `Or` into its
alternatives.** `missing_patterns` flattens its pattern list one level at
entry (covering both the top-level call and its own recursive calls over a
matching variant's single-field payload, so `Some(1 | 2)` gets or-aware
coverage too) — an or-pattern's alternatives each independently contribute
to coverage, exactly as separate arms would. Duplicate-arm detection
likewise compares alternatives rather than whole arm patterns, so `1 | 2 =>
…` followed by a later `2 => …` is caught.

**D — Emitter: a flat switch when bindingless, an if-chain with per-alternative
`let`-dispatch when bound.** `match_needs_if_chain` only routes an or-pattern
arm to the if-chain when an alternative has bindings or its own nested
refutable test; a purely literal or nullary-variant or-pattern
(`1 | 2 | 3`, `Pending | Cancelled(_, _)`) stays on the cheap flat switch,
lowering to N fall-through `case` labels sharing one body — the natural
extension of how a literal pattern already lowers a single label. When an
alternative binds names, the if-chain can't emit one `const` per name (the
alternatives can put the same name at different structural paths — `Held`'s
2nd field vs `Confirmed`'s 4th), so it declares each shared name once with
`let` and dispatches per alternative to assign it, keeping the (potentially
large) guard/body code un-duplicated — only the small per-alternative field
lookup repeats.

**E — Or-patterns are legal after `is` (§2.3.6); parentheses around them are
transparent grammar, not just prose convention.** The spec's own worked
example (`is (Held(...) | Confirmed(...))`) requires the parens to actually
parse — `is` calls into the same pattern grammar, which had no parenthesized
form — so the parser gained a `(` pattern `)` production that returns its
inner pattern unchanged (spliced into an enclosing `|`-fold per DECISION A).
`is`'s existing literal-pattern rejection (ADR 0158 D5) applies
per-alternative for free through the same recursion as DECISION B; Rules 1/2
are enforced the same way, reusing the existing `gather_pattern_bindings`
helper (which already computes exactly the `(name, Ty)` set a variant
pattern contributes after `is`) once per alternative.

**Two scoping decisions, both consistent with limitations the spec itself
already states:**

- **No synthetic "union of variants" type.** The spec's example narrates
  `state` as narrowed "to `(Held | Confirmed)`" but immediately caveats that
  "direct field access...is not generally admissible" — because
  TypeScript's *own* control-flow narrowing on a `.tag === "Held" || .tag
  === "Confirmed"` disjunctive test against a discriminated-union type
  already produces exactly that narrowing in the emitted output, with no
  new `Ty` construct needed on the bynk side. The checker only needs to type
  the shared bindings (Rule 2), which is what it does.
- **`is`-position bindings stay flat depth-1**, matching the *pre-existing*
  limit for an ordinary (non-Or) variant pattern after `is` (nesting/guards
  are match-only per ADR 0169). An or-pattern's refined-type-name narrowing
  special case (`x is Email | Username`) is not extended — no shared
  binding to type, and the scrutinee itself would need the deferred union
  type above.

## Consequences

- `1 | 2 | 3 => …`, `Login(u, _) | Register(u, _) if u.isAdmin => …`, and
  `Some(1 | 2)` are all now expressible, matching the spec's worked
  examples exactly (verified: emitted output passes `tsc --strict`).
- Two new diagnostics for the binding-consistency rules; every other
  or-pattern misuse (wrong value type, wrong arity) surfaces through
  existing pattern diagnostics via the per-alternative recursion.
- No churn to any existing emitted output — a bindingless, unguarded
  or-pattern is the *only* new arm shape on the flat-switch path, and every
  pre-existing single-pattern arm is unaffected.
- The formatter renders an or-pattern by joining alternatives with `" | "`.

## Tooling (ADR 0156)

- **Hover:** unchanged — an or-pattern's bindings hover as their (uniform,
  Rule-2-checked) types via the same `ctx.expr_types` recording every other
  pattern binding uses.
- **Completion:** unchanged — no new completion surface.
- **Semantic tokens:** unchanged — `|` is a new anonymous token in the
  tree-sitter grammar (`or_pattern`, `paren_pattern`), already covered by
  the grammar's default tokenisation; no new highlight query rule needed.
- **Signature help:** unchanged — patterns introduce no invocation.
- **Formatter:** renders `p₁ | p₂` joined by `" | "` (`pattern_to_string`).

## Alternatives considered

- **A right-nested `Or(Box<Pattern>, Box<Pattern>)` pair instead of a flat
  `Vec`.** Matches the grammar's binary `p '|' p` production more literally,
  but every consumer (binding-consistency check, coverage flattening,
  case-label emission) wants "the list of alternatives," not a binary tree —
  the flat form removes a recursive unwrap at every one of those sites, and
  nothing needs the parenthesisation captured to invert `p₁ | p₂` back to a
  single associated pair.
- **Always lower through the if-chain, forgoing the flat-switch
  optimization for bindingless or-patterns (DECISION D).** Simpler — one
  fewer emitter code path — and still spec-compliant (the issue accepted
  either shape). Rejected because the switch/`case`-label form is the
  issue's own worked example and a near-zero-risk extension of the literal
  pattern's existing single-label case.
- **Skip the `paren_pattern` grammar addition, treating the spec's example
  parens as illustrative-only.** Considered, but the spec's grammar block
  says "syntactically optional," implying they parse; and the worked
  example is written with them. Adding a transparent grouping production
  was a few lines against the shared pattern recursion, versus leaving a
  spec-documented example unable to compile.
