# 0263 — In-editor doc-comment rendering is style-in-place client decorations over a client-side Markdown tokenizer

- **Status:** Accepted (v0.223.1)

**Context.** Bynk doc comments (`--- … ---`, `bynk-syntax/src/lexer.rs`) are
Markdown, but in the source buffer they render as flat, comment-coloured text: a
reader gets no visual structure while reading the code itself, only when they
open hover or a separate panel. This increment gives the `--- … ---` blocks
light Markdown affordances **in place** — heading colour, `**bold**`, `*italic*`
— and lets the `[Name]`/`[Owner.member]` intra-doc links (already resolved
server-side via `textDocument/documentLink`, ADR for #848) read as clickable
without leaving the buffer. It is the extension's first editor-decoration
surface; it introduces no grammar, checker, emitter, or runtime change, and no
new LSP request.

**Decision.**

- **[A] Style-in-place, not conceal-and-reveal.** The `#`/`**`/`*` markers stay
  visible; decorations apply colour/weight/style to the span they mark
  (`# Heading` colours the line, `**bold**` renders bold with the asterisks
  still shown). Concealing markers to look fully rendered needs a "conceal when
  the cursor leaves the line, reveal when editing" dance that fights selection,
  copy, and multi-cursor — a well-known VS Code pain point. Style-in-place never
  fights the editor and is consistent with the deliberate no-font-size rule
  (line height stays stable). Conceal-and-reveal is a later opt-in follow-up.

- **[B] Direct decorations, not semantic tokens.** Bynk could add
  `docHeading`/`docStrong`/`docEmphasis` semantic-token types, but semantic
  tokens cannot *guarantee* bold/italic — the theme decides, and most would not
  render "strong" as bold. Since the goal is "bold looks bold", the affordances
  map to `TextEditorDecorationType` `fontWeight`/`fontStyle` directly, accepting
  that they are less themeable. Heading colour alone would be a colour-only
  signal, so heading decorations pair the themeable `bynk.docHeadingForeground`
  colour with `fontWeight: 'bold'` — a non-colour cue for colour-vision-deficient
  readers, matching the weight/style cue bold and italic already carry.

- **[C] Client-parse, with a small self-contained tokenizer.** The Markdown →
  range mapping is a pure client-side function (`src/inlineDoc.ts`), not a new
  `bynk/docDecorations` LSP verb — inline rendering is a pure rendering concern
  and needs no server round-trip per keystroke. The #849 proposal framed this as
  "reuse #847's vendored Markdown parser", but #847 (the documentation-view
  proposal that would vendor one) is still unbuilt, so this increment ships a
  minimal, dependency-free tokenizer covering exactly the affordances it renders
  (headings, `**`/`__` strong, `*`/`_` emphasis) rather than pulling in a parser
  for a surface that does not exist yet. If #847 later vendors a full parser and
  the two surfaces need identical tokenization, promoting to a shared pass (or
  the server request) is the follow-up; until then the tokenizer is the single
  source for in-editor rendering.

- **Block ranges are found by client-side line scanning, in UTF-16 units.** The
  tokenizer pairs `--- … ---` marker lines directly (mirroring the lexer's
  sequential `doc_block_close` pairing) rather than consuming the server's Rust
  **byte** spans. Working in JS string indices — which are UTF-16 code units,
  exactly what `vscode.Position.character` counts — means a doc block with
  multi-byte content (emoji, non-Latin text) maps to correct columns with no
  byte↔UTF-16 conversion, sidestepping a classic off-by-N decoration bug.

**Consequences.** The affordances layer on top of the existing semantic-token
comment colouring; the token stream is unchanged. Recompute is debounced (the
500 ms pattern the Test Explorer already uses) and applied only to visible
`.bynk` editors, on edit, editor-switch, and config change. Hover, completion,
signature help, and semantic tokens are unchanged (ADR 0156 tooling checklist).
Because VS Code exposes no API to read applied decorations, the range logic is
verified by unit tests over the pure tokenizer, not by asserting rendered
decorations. Conceal-and-reveal and a shared Rust-side Markdown pass are noted
follow-ups, not in this increment.
