# 0161 — Contextual keywords carry hover and a mechanical coverage floor of their own, beside the reserved-keyword floor

- **Status:** Accepted (v0.137.0; 2026-07-04)
- **Provenance:** the v0.137.0 agent-state-hover increment — an LSP-only change addressing #476. Hovering `key` or `store` in an `agent` body produced no hover: both are *contextual* keywords, lexed as identifiers, so they are absent from `bynk-syntax::keywords::KEYWORDS` and never reach `describe_keyword_at`; and the fields they declare are neither top-level declarations nor `let`/param locals, so `describe_symbol` and the locals path miss them too. Every hover path fell through.
- **Realises:** a hover for the `key`/`store` agent-state surface — the words a reader reaches for first when learning agents — rendered from the parsed field, and a test that fails when a future contextual keyword lands without one.
- **Relates:** ADR 0156 (the editor surface tracks the language, with a mechanical floor over hover and completion — this extends that floor from reserved keywords to contextual ones); ADR 0159 (the `cors` contextual keyword — a sibling contextual keyword this mechanism will also want to cover); ADR 0111 (`@`-annotations — the `store`-field annotations this hover renders through the formatter's own renderer).

## Context

ADR 0156 gave the editor surface a mechanical floor: every lowercase-initial
`KEYWORDS` entry must have a completion doc and a hover path, so a new keyword
cannot land with a silent hover gap. But that floor iterates `KEYWORDS`, which
is drift-guarded to equal exactly the lexer's reserved `#[token]`s. **Contextual
keywords** — words that read as keywords in one position but stay usable as
ordinary identifiers elsewhere, so they are lexed as `Ident` (`key`, `store`, and
`cors` per ADR 0159) — are deliberately absent from that table. They fell outside
the floor entirely: #476 is the gap made visible.

A contextual keyword's hover cannot come from the reserved-keyword path
(`describe_keyword_at` matches source text against `KEYWORDS`, which by
construction excludes them), and the state fields they introduce are not the kind
of binding the top-level (`describe_symbol`) or locals (`describe_local_at`) paths
know. So both the keyword and the field it declares need a dedicated path.

## Decisions

**D1 — Contextual keywords have their own registry, `CONTEXTUAL_KEYWORDS`,
beside `KEYWORDS` — not entries in it.** Adding `key`/`store` to `KEYWORDS` would
be false: they are not reserved, and the `keywords_reference.rs` drift guard —
which asserts `KEYWORDS` equals the lexer's reserved tokens — would fail, as would
the generated keyword reference page that tells readers "reserved words cannot be
used as identifiers." A second, parallel `KeywordInfo` table names the contextual
words and their one-line docs without misrepresenting them as reserved. It is the
single source of truth the hover path and the coverage test both read.

**D2 — The hover renders the field the keyword introduces, from the AST, on the
keyword *or* the field name alike.** Hovering `store` and hovering the `items` it
declares answer the same question — "what is this field?" — so both render the
same content: the field signature (`store items: Map[String, Int]`) with its
`@indexed`/`@bounded`/… annotations (through the formatter's own
`annotation_to_string`, not a copy that could drift), followed by the contextual
keyword's doc line. The `key`/`store` keyword spans are recovered from spans the
AST already carries — the key field's name (its keyword is the token before it)
and each store field's span (the parser starts it at the `store` token) — so no
new span plumbing is added to the parser. Matching is by span, not by name, so an
identifier that merely *reads* `id`/`items` elsewhere (an annotation argument, a
handler local) is never mistaken for the declaration.

**D3 — The mechanical floor extends to `CONTEXTUAL_KEYWORDS`.** A second coverage
test asserts every entry resolves to a hover in the construct that makes it a
keyword (an `agent` body). This is the same tooth ADR 0156 established for
reserved keywords, now closing the gap that let #476 exist: a future contextual
keyword — `cors`, or the next one — that lands without a hover fails a test, not a
user's expectation.

## Consequences

- `bynk-fmt::annotation_to_string` is now public, joining `expr_to_string` and
  `refinement_to_string` as a surface renderer the editor tier shares with the
  formatter (ADR 0156 D1 — one renderer, no drift).
- `cors` (ADR 0159) is a contextual keyword with a bespoke hover already; folding
  it into `CONTEXTUAL_KEYWORDS` so it too sits under D3's floor is a small,
  obvious follow-on, not done here.
- The reserved-keyword floor and this one are deliberately separate tests over
  separate registries; neither subsumes the other, and a word belongs to exactly
  one.
