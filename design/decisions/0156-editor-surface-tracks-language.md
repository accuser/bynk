# 0156 — The editor surface tracks the language, with a mechanical floor over hover and completion

- **Status:** Accepted (doc-ADR; 2026-07-03)
- **Spec:** `design/bynk-lsp-spec.md` §3.3 (hover), §3.14 (semantic tokens), §3.15 (completion), §3.16 (signature help)
- **Realises:** the editor-currency track (retired), ADR 1 of 2 — front-loaded ahead of every slice.

## Context

The editor surface (`bynk-lsp` + `vscode-bynk`) is a *projection of the
language*: what the checker understands should be legible in hover, offerable
in completion, and reachable through the UI. That projection has fallen
behind. Hover renders only top-level declarations from a lossy re-parse
(`bynk-lsp/src/symbols.rs::describe_symbol`) and knows nothing of the testing
track's `requires`/`ensures`/`transition`/`invariant`/`expect`/`suite`/`case`/
`property` (v0.115–v0.119) — `describe_symbol` and `find_declaration_span`
both hardcode `SourceUnit::Suite(_) => &[]`. Completion is structurally
complete (ADR 0093's matrix holds) but ships a scaffold, `test "…" { }`, for a
keyword the grammar retired.

This is not a series of oversights; it is a gap in the increment recipe. The
tooling roadmap already named the obligation — "each language increment's
tooling delta must explicitly enumerate LSP (completion, hover, semantic
tokens for the new constructs)" (`design/bynk-tooling-roadmap.md`) — but the
proposal template's "tooling deltas" line (`design/proposals/README.md`) names
nothing and nothing fails when it is skipped. The testing track landed five
increments of grammar, checker, fmt and tree-sitter delta with no hover or
completion delta, and no test caught it.

## Decision

**The editor surface tracks the language, with two teeth:**

1. **Single source of truth.** Hover, completion, and signature help render
   from the checker's captured analysis tables (`bynk-ide::diagnose_project`'s
   `locals`, `expr_types`, `hints`; the `bynkc` keyword/type/kernel-method
   registries) rather than each re-parsing and re-guessing independently — so
   they cannot diverge from each other. This generalises the discipline the
   three responses already share for type rendering (`symbols::type_ref_str`).
2. **A mechanical coverage test over the enumerable construct sets, backstopped
   by a proposal-template checklist for the residue that can't be enumerated:**
   - **Coverage test, clause A:** every lowercase-initial
     `bynk-syntax::keywords::KEYWORDS` entry must have a completion doc *and* a
     hover path. This is the tooth that would have caught the testing-track
     gap — a new keyword landing with no hover coverage becomes a failing
     test, not a silent omission.
   - **Coverage test, clause B:** every construct that carries a dedicated
     semantic-token type/modifier must have a legend mapping. A construct with
     no dedicated token type (most clause keywords, matching `if`/`else`/
     `match`) is out of this clause's scope by construction — it does not
     require every keyword to be tokenised, only that tokenisation, where it
     exists, is declared.
   - **Proposal-template checklist:** every language-slice proposal names
     hover, completion, semantic tokens, and signature help explicitly and
     states what changes (or that nothing does, and why) — mirroring the
     existing docs-delta rule where silence is treated as an oversight, not a
     no-op.

## Consequences

- The coverage test is the load-bearing claim for everything countable;
  the checklist is honour-system for genuinely new *shapes* of content (e.g.
  "should a contract clause render specially in hover?") that no enumeration
  can demand.
- Signature help is named in the checklist but is not part of the mechanical
  coverage test itself — there is no enumerable set of "constructs needing
  dedicated signature-help wiring" the way keywords and token types are
  enumerable (call-site resolution in `bynk-lsp/src/signature_help.rs` is
  lexical and callee-driven, not keyword-driven). Its currency is judged
  per-slice via the checklist.
- This ADR settles the mechanism; it closes no arrears itself. The track's
  first proposal (slice 0) implements both tests, deletes the `test` snippet
  they catch, strengthens the proposal-template line, and folds in whatever
  hover/token/signature-help shortfall its own audit finds — so the gate does
  not demand of future slices what this track leaves unverified.
