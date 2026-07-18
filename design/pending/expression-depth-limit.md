---
level: minor
changelog: A long binary-operator chain (`1 + 1 + … + 1`) is rejected with `bynk.parse.nesting_too_deep` instead of overflowing the stack on a valid program (#714)
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

**Decision.** Reuse #713's mechanism rather than add a parallel one. A new
`Parser::enter_chain_fold` counts each operator-chain fold against the same
`depth` budget and reports the same `bynk.parse.nesting_too_deep` diagnostic at
`MAX_NESTING_DEPTH`. The four associative loops call it once per fold and unwind
their fold count from `depth` before returning, so the live count behaves like a
recursive descent. Because it is the *same* budget, a chain composes with the
ambient nesting depth — `((… 64 parens …))`-plus-a-chain is bounded by the total,
not by each independently. The right-associative `implies` chain, which recurses
rather than loops, is folded into `enter_recursion` for the same reason.

**Consequences.** A binary-operator chain past the bound is now a clean
`bynk.parse.nesting_too_deep` diagnostic on the CLI and in the LSP, matching the
nested-source case from #713 — one limit, one diagnostic, one budget. The bound
is a fixed language constraint (64 total nesting/chain depth); a program that
genuinely needed a deeper single expression must split it across `let` bindings
or a reducer (`.sum()`/`.fold(...)`). No checker-side guard is added: bounding at
the parser keeps every downstream walk safe by construction, exactly as #713
already relies on. Surfacing a front-end panic as a diagnostic rather than an
opaque WASM `RuntimeError` on the ~1 MiB playground stack remains #717.
