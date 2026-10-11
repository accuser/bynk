# 0454 — A one-line doc form, `--|`, and docs on record fields and variants

- **Status:** Accepted (v0.326)

**Context.** Before #1888 a doc was a `---` fence, three lines for one sentence
(25 of the repository's 71 docs were a single line), and only declarations could
carry one. A doc above a record field or an enum variant was a parse error, so
field hover could only say "A field of `T`.", although records are Bynk's wire
contracts and a `--` comment above a field (kept since #1788) never reaches
hover, completion or the documentation page.

**Decision.**

- **A. The one-line form is `--|`.** A line comment that begins `--|` and is the
  first thing on its line (indentation aside) lexes as a `DocLine` token,
  distinct from a `Comment`. A `--|` after code on the same line is still a `--`
  comment whose text starts with `|`; this is how ADR 0188 D2's
  comment-eligibility rule applies, narrowed to line starts, so
  `repo: Repo, --| text` keeps meaning what it meant (DECISION E below). `--|`
  can't arise by accident; `--- text` was rejected because it would turn
  `--- Helpers ---` dividers into docs, the hazard #1885 closed. A `DocLine`
  span runs through its newline, as a `DocBlock`'s does, so the orphan
  blank-line check and the trailing-comment binning treat both forms alike.
- **B. Consecutive lines join.** Doc lines on consecutive lines form one doc:
  each line's text after the marker and one separating space, joined with line
  breaks, so a bare `--|` separates Markdown paragraphs. A blank line or a `--`
  line ends the run. The book says to reach for the fence once a doc needs
  structure; that is guidance, not a rule.
- **C. Both forms on one target is an error.** A doc of one form directly
  followed by one of the other (no blank line between), in either order, is
  `bynk.parse.doc_forms_mixed`, reported beside the parse like #1885's
  `bynk.parse.doc_block_contains_code` (collected on the side, so the
  recovering parse keeps the body). Two docs of the same form in a row are
  unchanged: the second is the existing "expected a declaration, found
  documentation" error. The rule is syntactic, so the spec states it in §3.3.3,
  not §5, whose remit is post-syntactic well-formedness.
- **D. Members take docs.** `documentation` is added to `RecordField`, `Variant`,
  `VariantField` and `MessageEntry`; both forms are accepted on each, in every
  position a comment above the member is (a pipe-form variant's doc sits above
  its `|`, the first one's after the `=`). A member doc follows the declaration
  rules: a blank line before the member, or the body's end after the doc,
  orphans it (`bynk.parse.orphan_doc_block`, kept in place). A doc after a
  pipe-form sum's last variant, with no `|` after it, is the next declaration's.
  Parameters stay documented in their function's doc. An `event`'s body is a
  record body, so its fields take docs too.
- **E. No end-of-line doc in this slice.** It would add a second attachment rule
  (backwards, to the member on the same line) and compete with the end-of-line
  comments #1788 stabilised. It can be added later without breaking anything; it
  is listed as a follow-up.
- **F. The formatter keeps each form.** Every `documentation` field now holds an
  `ast::Doc`, its text plus the `DocForm` it was written in (`Block` or `Lines`);
  it derefs to its text, so readers that want only the Markdown are unchanged,
  and `From<String>` builds a `Block`. `bynk fmt` prints a `Lines` doc as
  `--| ` lines (a bare `--|` for an empty line) and a `Block` doc as a fence,
  never converting; the marker's separating space is canonical (`--|text`
  prints as `--| text`). Its doc-loss guard now compares docs of both forms,
  keyed by form and text, so a printer path that dropped or converted a doc is
  refused. The alternative, recovering the form from the source tokens in the
  formatter by content, was rejected as fragile.
- **The code probe is for fences only.** `bynk.parse.doc_block_contains_code`
  does not probe a run of `--|` lines: every line is marked as a doc on
  purpose, so no pair of dividers can turn code into one by accident.
- **Tooling.** Hover on a documented field shows its doc in place of "A field of
  `T`." wherever field hover already resolves (declaration, access, record
  literal label). Variants gain hover: a `Sum.Variant` callee, and a bare
  variant name that exactly one sum in the file (or, failing that, another
  project file) declares, after every declaration rung so a variant never
  shadows a type. Field and variant completion items carry the doc as
  Markdown documentation. The documentation page lists documented members under
  their type (fields and variants at depth 1, payload fields at depth 2, message
  entries under their bundle); undocumented members get no entry, so the
  coverage view does not flag every field of every record. Doc-comment links
  resolve in doc lines. Doc-line runs fold as comments. The language server
  emits each doc line as a `comment` token with the `documentation` modifier
  (legend index 12 and modifier bit 4, appended); `---` blocks stay with the
  editor grammars. Signature help is unchanged.
- **Editors.** tree-sitter gains a `doc_line` extra that outranks `line_comment`
  by lexical precedence, highlighted `@comment.documentation`. An extras token
  cannot see the line-start condition, so a `--|` after code highlights as a doc
  there though it compiles as a comment, the same kind of approximation
  `line_comment` already makes for `a--b`. The TextMate grammar scopes a
  line-leading `--|` as `comment.line.documentation.bynk`, and VS Code continues
  a `--| ` line on Enter.

**Consequences.** compat-board can document `Run`'s fields and replace its
three-line, one-sentence docs. The emitter is unchanged: member docs are not yet
emitted as JSDoc on interface properties or union arms (a possible follow-up),
and type and function docs emit as before, whichever form they were written in.
A payload field and a message entry still keep no comments, so a `--` comment
above one is refused by `bynk fmt`'s comment-loss guard, as before; an orphaned
doc before a `messages` body's `}` is warned about and refused by the doc-loss
guard rather than kept.
