# 0070 — Folding & selection ranges from one recovered-AST visitor

- **Status:** Accepted (v0.37)
- **Spec:** `design/karn-lsp-spec.md` §3.20
- **Relates to:** the document-symbols provider (same parse path)

## Context
`textDocument/foldingRange` and `textDocument/selectionRange` are both structural
— they read the per-file parsed AST, not the binding index or the analysis round.
The server already parses each document for document symbols
(`parse_unit_with_recovery`), every AST node carries a `span`, and
`position::span_to_range` converts spans. So both features are one AST walk plus a
thin handler.

## Decision
Serve both from a single `structure.rs` module built on **one span visitor**:
`collect(source)` parses the recovered AST and walks it, pushing every node's
`(span, foldable)` pair. The two providers consume the same list.

- **AST-driven, not tree-sitter.** `karn-lsp` has no tree-sitter dependency;
  document-symbols / locals-nav / semantic-tokens all walk the hand-written
  `karnc` AST. Folding/selection follow suit — no second grammar to sync, and the
  recovered parse means they work mid-edit.
- **Folding** keeps the `foldable` spans — the multi-line block-like constructs:
  the `commons`/`context`/`adapter`/`test` container, type record/sum bodies,
  service/agent handler lists and their block bodies, provider/op and fn block
  bodies, `match` (and its arms), `if`, block expressions, and record/spread/list
  literals. A `FoldingRange` is emitted only when `endLine > startLine` (LSP folds
  ≥2 lines); duplicate `(start, end)` line pairs (a decl and its body sharing both
  lines) collapse via a seen-set. Structural ranges carry no `kind`.
- **Comment-run folding** is a separate scan of the lexer's `Comment` tokens
  (the trivia table keeps only bodies, not spans) — consecutive comments on
  adjacent lines group into one `FoldingRangeKind::Comment` range.
- **Selection** filters the same node list to spans **containing** the offset,
  de-duplicates, sorts by size, and links them into the `SelectionRange { range,
  parent }` chain (innermost first, widening to the file). A well-nested AST
  guarantees each parent contains its child. Falls back to an empty range at the
  cursor when nothing contains it or the file doesn't parse.
- **Pure module, thin handlers.** `structure::{folding_ranges, selection_ranges}`
  are pure; the `main.rs` handlers only fetch the document text and call them —
  the document-symbols shape.

## Consequences
Two structural features from one visitor, no analysis dependency, correct even
when the project doesn't check. The fold set lives in one `matches!`; new foldable
kinds are a one-line addition. Deferred: clause-list (`given`/`exports`/`consumes`)
folding and per-statement folding within blocks (low value, easy to add later).
