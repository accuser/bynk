# 0027 — Lambda syntax is `(params) => expr`, the shared value arrow

- **Status:** Accepted (v0.20a)
- **Spec:** §4.6.9a, §5.2

## Context
First-class functions need a value-level syntax that cannot collide with the
type arrow or read ambiguously next to `match`.

## Decision
`(params) => expr` (block body `(params) => { … }`), always parenthesised —
the parens read as "this is a function". `=>` is the **value** arrow, shared
with `match` arms; `->` stays the **type** arrow. No `fn(...)` expression
form, no `_` placeholder. Param annotations are optional where an expected
function type supplies them, required otherwise. Parsing uses a
depth-counting scan to the matching `)` plus a one-token peek for `=>`.

## Consequences
One arrow per register; the parser needs bounded lookahead but no
backtracking; tree-sitter carries a small GLR conflict until the `=>`
disambiguates.
