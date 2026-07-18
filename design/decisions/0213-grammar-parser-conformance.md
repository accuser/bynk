# 0213 — The normative grammar is tied to the compiler parser by a conformance test

- **Status:** Accepted (v0.192)

**Context.** Bynk has two parsers. The compiler parses with the hand-written
recursive-descent parser in `bynk-syntax`; the editor tooling — and the
normative grammar appendix rendered from it — parses with the independent
`tree-sitter-bynk` grammar. The specification claimed a production "cannot drift
from the parser" on the strength of being generated from tree-sitter. That is a
non-sequitur: generation ties the appendix to *tree-sitter*, not to the parser
that actually compiles Bynk. Every existing grammar guard is intra-side
(appendix vs `grammar.json`, `grammar.json` vs `{{#grammar}}` directives,
`keywords.rs` vs `lexer.rs`); none consulted `bynk-syntax`. Three drifts lived
in that gap, in both directions:

- **`Bytes`** — a base type the parser accepts (`type Blob = Bytes` compiles),
  but which the grammar's `base_type` omitted, so tree-sitter could not parse it
  at all. The spec *under*-specified.
- **Sum/enum variant capitalisation** — the grammar's `constant_name`
  (`/[A-Z][A-Za-z0-9_]*/`) declares variant names uppercase, and tree-sitter
  rejects a lowercase one, but the parser accepted `| active` end-to-end through
  the checker. The spec *over*-specified a rule the toolchain did not enforce.
- **Built-in generic arity** — the grammar's `generic_type_ref` over-generated
  with `sep1` (one-or-more arguments for every built-in), while the parser
  rejects the wrong arity (`Option[Int, String]`, `Result[Int]`, `Map[String]`)
  at parse time with a `bynk.parse.*` diagnostic. The grammar over-generated *as
  a production*, so §2.2's "a production states what parses" defence did not
  apply.

**Decision.** Close the seam and each drift so the two parsers describe the same
language.

- Add a **cross-parser conformance test** (`tree-sitter-bynk/tests/conformance.rs`,
  backed by a new unpublished Rust binding for the grammar) that parses the same
  sources with both parsers and asserts they accept and reject the same
  programs. This is what makes the "cannot drift" claim honest; the false claim
  is narrowed to "cannot drift from the tree-sitter grammar" wherever it appears,
  with the conformance test cited for the tie to the compiler.
- **`Bytes`** joins `base_type` in the grammar, with tree-sitter corpus
  coverage. The parser already accepted it.
- **Variant capitalisation** is enforced by the compiler parser: a lowercase
  sum-variant or enum-tag name is a parse error, `bynk.parse.variant_name_case`.
  Enforcing at parse time (not as a later well-formedness pass) is what keeps the
  parser in agreement with tree-sitter, which rejects the lowercase name during
  lexing of `constant_name`.
- **Generic arity** is expressed in the grammar: `generic_type_ref` splits into a
  unary form (`Name[T]` — `Option`, `Effect`, `HttpResult`, `List`, `Stream`,
  `Query`, `Connection`, `History`) and a binary form (`Name[K, V]` — `Result`,
  `Map`), so the production states the arity the parser enforces rather than
  deferring it.

The stale end-of-input message in `parse_base_type` (which listed only four of
the seven base types) is corrected in passing.

**Consequences.** The generated grammar now describes what the compiler accepts,
and a regression on either side — the grammar admitting what the compiler
rejects, or the reverse — fails the conformance test rather than shipping as
silent drift. Rejecting a lowercase variant name is a new refusal, but no
first-party source used one, so nothing observable regresses. The conformance
test's scope is the type surface where the drifts lived (base types, sum/enum
variants, built-in generics); it is the place to extend whenever a change could
move the two parsers apart. Provenance: third-party language review, filed as
issue #635.
