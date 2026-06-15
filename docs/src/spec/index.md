# The Karn Language Specification

The normative definition of Karn: the language as accepted and compiled by
`karnc` at the **current version, v0.41**. It states what a conforming
implementation must accept, what it must reject, and what a program means. Where
the [grammar reference](../reference/grammar.md) is a friendly, per-construct
lookup for people writing Karn, this is the complete, citable definition for
implementers and for precise reference. Each language increment updates this
document in place ([Scope §1.1](scope.md)); the decisions behind increments are
recorded in `design/decisions/`.

The two coexist by register, not by contradiction. They draw on the **same
generated ingredients** — the grammar productions, the static-semantics-to-
diagnostics weave, the diagnostic index, the grammar appendix — so the hard
facts cannot diverge; only the prose differs (explanatory there, normative here).

> [!NOTE]
> This section is built phase by phase. The chapters listed below as plain text
> are not yet written; they are the planned structure, shown so the shape of the
> whole is visible. Only written chapters appear in the navigation. This is
> informative.

## How meaning is defined

Karn is **translation-defined**. Its three layers of definition are:

- **Syntax** — the grammar, generated from `tree-sitter-karn`, so the
  productions in this spec cannot drift from the parser.
- **Static semantics** — well-formedness rules. A program is well-formed exactly
  when it provokes no `karn.*` diagnostic; each rule is tied to its diagnostic
  code(s), so the rule catalogue and the compiler cannot drift.
- **Dynamic meaning** — defined **by translation**: each construct's behaviour is
  the TypeScript it emits, together with the runtime-library contract. There is
  no separate operational semantics.

[Conventions §2](conventions.md) makes this model precise and fixes the
notation; [Scope §1](scope.md) fixes what is normative and what conformance
means.

## Chapters

- [§1 Scope & conformance](scope.md) — what the spec covers; what a conforming
  implementation must accept and reject; RFC 2119 keywords.
- [§2 Notation & conventions](conventions.md) — grammar notation; how rules are
  written and linked to diagnostics; the translation-defined model; normative vs
  informative; citation.
- [§3 Lexical grammar](lexical-grammar.md) — tokens, identifiers, literals,
  comments, doc-blocks, trivia.
- [§4 Syntactic grammar](syntactic-grammar.md) — the productions, organised by
  construct.
- [§5 Static semantics](static-semantics.md) — well-formedness per construct,
  woven to diagnostics.
- [§6 The type system](type-system.md) — base, refined, opaque, sum, record, and
  enum types; `Result`, `Option`, `Effect`; refinement and admission.
- [§7 Meaning by translation](emission.md) — what each construct emits, and the
  [runtime-library contract](runtime-library.md).
- [§8 Compilation model](compilation-model.md) — the `karn.toml` manifest, project
  layout, and the build contract.
- [§9 Diagnostics](diagnostics.md) — the normative catalogue; the codes are the
  identifiers of the §5 rules.
- [§10 Conformance & test corpus](conformance.md) — the `karnc` fixture corpus as
  the conformance suite.
- [§11 Complete grammar](grammar-appendix.md) — the complete generated grammar.
- [Appendix A — Planned features](appendix-planned.md) (post-MVP, non-normative).
- [Appendix B — Version history](appendix-version-history.md).
