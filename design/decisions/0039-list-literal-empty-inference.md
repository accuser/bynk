# 0039 — List literal syntax; empty-literal inference; the line rule for `[`

- **Status:** Accepted (v0.20b)
- **Spec:** §4.6.21a, §5.10

## Context
v0.20a reserved `[` in expression position. A list needs a literal; `Map`
does not (`{ }` is records/blocks, and there is no pair literal to put in
one anyway).

## Decision
`[a, b, c]` is the `List` literal — a **leading** `[` in expression
position, with an optional trailing comma. No collision with type
application: `name[T](…)` is a *postfix* form on a callee identifier.
**No `Map` brace literal** — maps build via `Map.empty()` + `insert`
(0036). Elements check against the expected element type when one is
supplied (so refined literals admit, v0.9.4); a mismatched element is
`karn.types.list_element_mismatch`. An **empty `[]` requires an expected
type** (`karn.types.uninferable_element_type`); `List.empty()` /
`Map.empty()` share exactly that rule — explicit type application on a
qualified static does not parse in v0.20b, so an expected type is their
only source of type arguments.

One disambiguation rule ships with the literal: the `[` of type application
must sit on the **same line** as its callee. A `[` opening a new line
starts a list literal — without this, `let a = f` followed by `[1, 2]` on
the next line would greedily misparse as `f[…]`.

## Consequences
`expr[index]` stays unreserved-but-unused (no indexing; `get(i)` returns
`Option[T]`). The reserved-syntax diagnostic for `[` (`karn.parse.
reserved_syntax`) retired with its last use site.
