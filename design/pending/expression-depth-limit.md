---
level: minor
changelog: A long operator or member chain (`1 + 1 + … + 1`, `a.b.c…`, `!!!…`) is rejected with `bynk.parse.nesting_too_deep` instead of overflowing the stack on a valid program (#714)
---

## ADR: operator-chain-depth-bound
title: Bound operator-chain depth on the shared front-end nesting budget
summary: Count iteratively-built operator-chain folds against the #713 depth budget so a long chain can't overflow a downstream recursive walk

**Context.** #713 (ADR for the parser recursion bound) added a single fixed
limit, `MAX_NESTING_DEPTH = 64`, and a live `Parser::depth` counter incremented
through `enter_recursion` at the self-recursive descent points (`parse_expr`,
`parse_type_ref`, patterns), so deeply *nested* source diagnoses
(`bynk.parse.nesting_too_deep`) instead of overflowing the parser's stack.

That guard does not cover #714. The parser builds associative operator chains
(`+`, `*`, `&&`, `||`) **iteratively** in the precedence ladder — `parse_or`,
`parse_and`, `parse_add`, `parse_mul` each loop rather than recurse — so a flat
`1 + 1 + … + 1` never re-enters `parse_expr` and never trips `enter_recursion`.
The parser survives it, but hands downstream an arbitrarily deep left-nested
`Expr` tree, and every *recursive* consumer of that tree then overflows a frame
per node: the checker's `type_of`/`check_binop` (the issue's crash site), the
formatter, the emitter, and the AST's own compiler-generated recursive `Drop`.
A 20 000-term chain aborted `bynkc check` and `bynkc fmt` (exit 134) on a
**valid** program. Guarding only the checker's recursion is insufficient — the
deep tree still exists, so its `Drop` and the formatter still overflow; the
tree must never be built.

**Decision.** Reuse #713's mechanism rather than add a parallel one. Every
expression builder that grows a tree *iteratively* — and so escapes
`enter_recursion` — is counted against the same `depth` budget:

- The four associative operator loops (`+`, `*`, `&&`, `||`) call a new
  `enter_chain_fold` once per fold, unwinding their fold count from `depth`
  before returning so the live count behaves like a recursive descent.
- The postfix receiver spine (`a.b.c…`, `f()?.g()…`) calls `deepen_spine` per
  member/`?` fold, with `parse_postfix` restoring `depth` wholesale on the way
  out (its several error-return paths make a save/restore wrapper cleaner).
- The two constructs that *do* recurse but bypass `parse_expr` — the
  right-associative `implies` chain and the `-`/`!` unary run — are routed
  through `enter_recursion` directly.

Because it is the *same* budget, a chain composes with the ambient nesting depth:
`((…parens…))`-plus-a-chain is bounded by the total along a root-to-leaf path,
not by each independently. The bound stays at #713's `MAX_NESTING_DEPTH = 64` so
there is one limit and one diagnostic code (`bynk.parse.nesting_too_deep`); the
flat-chain / spine cases carry a chain-appropriate message ("this expression is
more than 64 levels deep" pointing at `let` bindings and `.sum()`/`.fold(...)`)
rather than the "nests … deep" wording, which fits parentheses but not a flat
chain.

**Consequences.** A chain, receiver spine, or unary run past the bound is now a
clean `bynk.parse.nesting_too_deep` diagnostic on the CLI and in the LSP,
matching the nested-source case from #713. No checker-side guard is added:
bounding every iterative/recursive expression builder at the parser means the
checker, formatter, emitter, and the AST's `Drop` are never handed a tree deeper
than the bound — the same guarantee #713 already relies on for nested source.

The 64 bound is a deliberate, conservative reuse. It is generous for genuine
nesting but stingy for a *flat* chain: a 65-term `&&` guard or `+` reduction is
not pathological, and compiled before this change, yet is now rejected. Keeping
one shared bound is the tradeoff — it is what makes the composition guarantee
hold and keeps a single diagnostic — and the message steers such code to the
idiomatic `.sum()`/`.fold(...)`/`let`-splitting. Raising the flat-chain limit
above the nesting limit (two budgets) is possible later if real programs hit it.
Surfacing a front-end panic as a diagnostic rather than an opaque WASM
`RuntimeError` on the ~1 MiB playground stack remains #717, as does the fact that
64 levels of *parenthesis* nesting can still overflow a small (≈2 MiB) stack
before the logical bound is reached — the ladder-per-paren descent cost is a
separate calibration question for #713/#717, not changed here.
