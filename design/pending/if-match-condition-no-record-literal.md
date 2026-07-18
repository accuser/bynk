---
level: minor
changelog: An `if`/`match` condition ending in a bare identifier no longer swallows a single-identifier brace body as a record construction
---

## ADR: if-match-condition-no-record-literal
title: An `if`/`match` condition does not parse a trailing brace body as a record literal
summary: A no-record-literal restriction on the condition spine, lifted inside delimiters

**Context.** `if cond { … }` and record construction `TypeName { … }` share the
surface shape `identifier {`. The parser disambiguated them with a context-free
two-token lookahead past the `{` (`looks_like_record_construction`), with no
parser state. That lookahead treats `Ident }` — a single shorthand field, `T {
field }` — as a record. But a block whose entire tail is a bare identifier has
the identical shape. So when an `if`/`match` condition **ended in a bare
identifier** and the branch/arm body **was exactly one bare identifier**, the
condition greedily consumed the body as a record construction:

```
if ready { result } else { fallback }
=> `ready { result }` parsed as a record; then the branch parse hit `else`
=> [bynk.parse.expected_token] expected `{` to open the `if` branch, found `else`
```

The failure was always an error, never a silent misparse — but the diagnostic
pointed several tokens right of the cause and named `else`, which was not the
problem. Operators offered no protection (`!ready`, `a && b` descend to the
trailing identifier), only a closing paren did. The empty case leaked too:
`match result {}` parsed `result {}` as an empty record, so `match`'s own
`expect(LBrace)` failed and the intended `bynk.parse.empty_match` diagnostic was
unreachable. Reported as #636.

**Decision.** Introduce a parser-state restriction — `no_record_literal`,
modelled on Rust's `NO_STRUCT_LITERAL` — that is set only while parsing the
*spine* of an `if` condition or a `match` discriminant (`parse_cond_expr`). While
set, a bare `ident {` on that spine is an identifier followed by an unrelated
block, never a record construction. The restriction is confined to the spine:
`parse_expr` clears it on entry, so it is lifted the moment parsing descends
through any delimiter — parentheses, call arguments, a list, a record field. A
record literal in condition head position is written parenthesised, exactly as
in Rust:

```
if (ready { result }) { … }        -- constructs a record
if check(Point { x: 1 }) { … }     -- record literal as a call argument
match (A { x: 1 }) { … }           -- record literal as a discriminant
```

The two-token lookahead stays a pure token-shape predicate; the ambiguity it
could not resolve alone (`Ident }` and the empty `}`) is now resolved by
context. This threads through exactly the two expr-then-block call sites
(`parse_if_expr`, `parse_match_expr`); there is no `while`/`for` in the surface.

**Consequences.** All six reported reproductions now parse, and the misdirecting
diagnostic is gone. `match result {}` now reaches `bynk.parse.empty_match`. No
restriction is placed on record type-name spelling — lowercase record type names
still parse everywhere they did (the earlier convention that type names lead with
an uppercase letter is not enforced here, and this fix does not begin to enforce
it). The one behavioural narrowing is that a record literal in the *head*
position of a condition must now be parenthesised; previously `match A { x: 1 } {
… }` parsed unparenthesised. That shape is vanishingly rare (matching a
freshly-constructed record is just binding its fields), appears nowhere in the
example corpus, and the parenthesised form is unambiguous and consistent with
the `if` treatment and with Rust. The restriction is spine-local state with an
explicit save/restore, not a pass over the whole expression.
