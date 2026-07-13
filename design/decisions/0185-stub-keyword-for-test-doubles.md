# 0185 — The test double gets its own keyword: `stub`, not a third pun on `provides`

- **Status:** Accepted (v0.159)
- **Provenance:** the keyword-hygiene batch (Bynk Language Design Review
  2026-07-05, §8 Language P1 #9, tracked as issue #548). The review found
  `provides` "punned three ways" — a provider declaration, an external provider
  (distinguished by *absence of a block*), and a test double (distinguished by an
  interior `.op(` shape) — and recommended giving the test double its own keyword
  (`stub Cap.op(_) returns v`). This ADR records the first item of that batch; the
  other hygiene items (one conjunction spelling, protocol-source casing, lexical
  tightening, the `where`-tier documentation, the enum-spelling convention) land
  as their own increments, each with its own ADR where it makes a call.
- **Realises:** a test double is now written `stub Cap.method(<pattern>) returns
  <value> | fails`. `provides` heads only a provider declaration / external
  provider. The two are disambiguated by *keyword*, not by looking past it for a
  `.op(` versus `=` shape.
- **Relates:** ADR 0154 (test doubles are provider overrides at a seam — the
  construct this ADR renames the keyword of; semantics unchanged), ADR 0153 (the
  tier dial the double sits under), ADR 0144 (the one-predicate surface the stub's
  argument pattern draws from — unchanged), ADR 0156 (the editor surface tracks
  the language — a new keyword owes hover/completion/semantic tokens).

## Context

`provides` carried three unrelated meanings:

1. a **provider declaration** — `provides Cap = Impl { … }`;
2. an **external provider** — the same head with no block, legal only in an
   adapter;
3. a **test double** — `provides Cap.op(_) returns v`, legal only in a
   `suite`/`case` body.

The three never competed in the same body (the test double is a test-body item;
the declarations are context/commons/adapter items), so the grammar disambiguated
them by parent and by the token after the capability name (`.` for the double, `=`
for the declaration). But a *reader* — and the design review — has to hold all
three readings and pick by squinting at what follows. That is precisely the pun
the review called out: a keyword should name one thing.

The test double is the odd one out. A provider declaration and its bodiless
external form are two shapes of the same idea (supply an implementation of a
capability at a seam). The test double *reuses that idea* — it substitutes a
provision at the same seam — but it is a distinct construct with its own scope
(test-only), its own right-hand side (a value or `fails`, never a body), and its
own match semantics (an argument pattern, first-match-wins). It had the weakest
claim to the shared word.

## Decision

**D1 — The test double is written `stub`.** `stub Cap.method(<args>) returns
<value> | fails` (and the sequenced `returns each [<outcome>, …]`) replaces the
`provides`-punned form. `stub` is a **reserved keyword**, legal only as a leading
`suite`/`case` body item — the same position the punned form occupied.
`provides` now heads only a provider declaration or external provider.

**D2 — Nothing but the keyword changes.** Scope (suite and case), precedence
(case > suite > tier default), the argument-pattern surface (`_`, literals, `is`;
first match wins), the value-or-`fails` right-hand side, and the sequenced
`returns each` form with last-outcome-repeat are all as ADR 0154 defined them.
The emitted TypeScript is byte-identical apart from one diagnostic string
("no stub clause matched …"). The AST node, grammar rule, and parser helpers are
renamed `StubClause` / `stub_clause` / `parse_stub_clause` to match.

**D3 — The diagnostics move with the keyword.** The four codes rename
`bynk.provides.*` → `bynk.stub.*` (`not_a_seam`, `unknown_op`, `rhs_type`,
`bad_sequence`). A code that reads `bynk.provides.bad_sequence` for a construct no
longer spelled `provides` would be its own small pun; the family follows the
keyword. `provides`'s own provider-declaration diagnostics (`bynk.provider.*`) are
untouched.

**D4 — Breaking, and taken now.** A test double written `provides Cap.op(…)` no
longer parses. Pre-1.0 this is a mechanical rename in the author's test files; the
in-repo fixtures, examples, book, and VS Code snippet migrate in the same
increment. The alternative — keeping the pun — is the cost the review is asking us
to stop paying, and it only grows with the corpus.

## Consequences

- The three meanings of `provides` drop to two shapes of one meaning (a provider,
  block or bodiless). A reader disambiguates `provides` from `stub` by the keyword
  alone.
- The editor gains `stub` as a reserved keyword (hover + completion + semantic
  tokens) and a `stub` snippet; signature help is unchanged (a stub head is not a
  call site). This satisfies ADR 0156's "editor surface tracks the language".
- ADR 0154 remains the semantic record for the construct; this ADR is the keyword
  provenance. A future reader tracing `stub` finds 0154 for *what it does* and
  0185 for *why it is spelled `stub`*.
- The remaining keyword-hygiene items (#548) are independent and land separately;
  this ADR deliberately scopes to the test-double rename so each breaking call
  carries its own decision record and version.
