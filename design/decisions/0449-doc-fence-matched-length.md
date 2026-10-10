# 0449 — A doc-block closes on a marker of its opener's length, and may not contain code

- **Status:** Accepted (v0.321)

**Context.** A doc-block is fenced by lines of three or more dashes, and ADR 0188
D4 pinned that any such line opens or closes one. That left three faults, found
reviewing bynk-lang/compat-board (#1885):

- A pair of dash dividers around code, the most natural divider in a `--`
  language, turned the code into the next declaration's documentation with no
  diagnostic at all (or, with a blank line after, only the
  `bynk.parse.orphan_doc_block` warning). The code was silently not compiled.
- A doc could not contain a `---` line, so it could not hold a Markdown rule,
  front matter, or a Bynk example with its own doc-block; the inner marker closed
  the block and the rest was a parse error.
- VS Code's `blockComment` was `["---", "---"]`, so *Toggle Block Comment* on a
  selection of code produced exactly that swallowing doc-block. Bynk has no
  non-doc block comment.

**Decision.**

- **A. Matched-length markers.** The closing marker must have exactly as many
  dashes as the opening one, like a Markdown code fence; a marker line of another
  length inside the block is content. A `----` block can therefore hold `---`
  lines. This supersedes ADR 0188 D4 in part: a marker line still always opens or
  closes a doc-block (there is still no standalone divider), but it closes only
  a block of its own length. Migration is empty: every doc-block in the repository
  was `---`/`---`. The alternative, markers of exactly `---`, fixes nothing about
  nesting.
- **B. `bynk.parse.doc_block_contains_code`.** A doc-block whose content, with its
  Markdown code fences (```` ``` ```` or `~~~`) blanked out first, parses as one
  or more declarations is an error, pointing at the block. The probe parses the
  content as an `adapter` body in fragment form (an adapter accepts every item a
  context or commons does, plus `binding`); success means the whole content
  parsed and declared at least one item, `uses`, `consumes`, `exports` or
  `binding`. A content holding only another doc-block (front matter, say) is
  prose. It runs only when the content lexes and contains a declaration keyword,
  so ordinary prose costs one tokenize at most, and prose that starts like a
  declaration ("type of the thing", "fn is used here") fails the parse and is
  not flagged. Because A makes nesting legal, code can hide one level down: a
  doc-block inside the content lexes to one opaque token, so the probe recurses
  into each inner block's content (up to four levels) and reports any finding on
  the outer block. Content that fails to lex because it holds a lone `---` (a
  Markdown rule) is probed again with its marker lines blanked out. The
  probe's own parser reports nothing on inner blocks. The note suggests `-- Helpers --` for a divider and a code fence for an
  example. This catches the divider pair whether or not a declaration follows it,
  which upgrading `orphan_doc_block` to an error would not (the attached shape is
  the worse one), and avoids a first-line keyword heuristic's false positives.
  `bynk.lex.unclosed_doc_block`'s note now names the length that would close the
  block and says a line of dashes is a marker, not a divider.
- **Where B is reported.** The parser reads a doc-block at the top of every item
  loop, before its recovery point, so the error is collected on the side and
  each parse entry point turns it into a parse error after the parse: the strict
  parse fails, and the recovering (IDE) parse lists it with the rest without
  abandoning the body.
- **C. No editor block comment.** `blockComment` is removed from
  `vscode-bynk/language-configuration.json`: there is nothing safe to map it to,
  so toggling a block comment does nothing rather than create a doc.
- **Tooling follows A.** `bynk fmt` prints the shortest fence (at least three
  dashes) longer than every marker-shaped line in the doc, so its output re-lexes
  to the same block and a doc with none keeps `---`. The tree-sitter external
  scanner, the TextMate grammar (`end` is a back-reference to the opener) and the
  extension's inline doc rendering all pair markers by length.

**Consequences.** A doc can carry any Markdown, and code between dividers is no
longer lost silently. The probe's ceilings:

- It does not see a suite's members (`case`, `property`, `stub`) or a whole unit
  header inside a doc; neither is a declaration-swallowing shape in practice.
- It flags content only when all of it parses as declarations, so prose followed
  by code is documentation, as the rule says.
- It stops at four nested levels.
- Content that fails to lex for any reason other than a stray marker line is not
  probed. Module docs (one per context, hover on
unit names, the documentation page merge) are the second half of #1885, in a
later increment.
