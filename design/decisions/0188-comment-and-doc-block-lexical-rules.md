# 0188 — A `--` comment must be whitespace-preceded; a `---` marker always opens a doc-block

- **Status:** Accepted (v0.162)
- **Provenance:** the keyword-hygiene batch (Bynk Language Design Review
  2026-07-05, §8 Language P1 #9, issue #548). The review flagged two
  **underspecified** lexical rules: `a--b` (is it a comment or a subtraction?) and
  the `---`-divider-vs-doc-block boundary. Fourth item of the batch (after ADR
  0185 `stub`, 0186 `&&`, 0187 `websocket`).
- **Realises:** the comment and doc-block rules are pinned normatively. `a--b`
  lexes as `a - -b`; a lone `---` is a doc-block marker, never a horizontal rule.
- **Relates:** none load-bearing beyond the lexer; the doc-block external token
  and `bynk.lex.unclosed_doc_block` pre-date this ADR.

## Context

**`a--b`.** Bynk writes line comments with `--` (never `//`) and subtraction /
negation with `-`. So a bare `--` adjacent to a term is ambiguous to a reader:
`a--b` could be `a` followed by a comment `--b`, or the arithmetic `a - -b`. The
lexer resolved it silently as *comment* — and, worse, a `count--` typed by someone
expecting a decrement silently became `count` plus a comment that **ate the rest
of the line**. The rule was never written down.

**`---`.** A line of three-or-more hyphens opens (and closes) a doc-block. A reader
coming from Markdown might expect a lone `---` to be a horizontal-rule *divider*.
It is not — an unmatched marker is an error. The "three or more hyphens" detail
lived only in a code comment and the external scanner, not the spec.

## Decision

**D1 — A `--` opens a comment only at start-of-input or when the preceding
character is whitespace.** Adjacent to a preceding token, `--` is not a comment:
`a--b` lexes as `a`, `-`, `-`, `b` (`a - -b`), and `x--` as `x` followed by two `-`
operators. This picks **subtraction** over comment for the ambiguous adjacency and
removes the silent line-swallowing footgun — a stray `x--` now surfaces loudly as
a dangling operator, not a swallowed line. A trailing comment is written ` -- …`
(leading space), which every existing comment in the corpus already does.

**D2 — Breaking, but empty in practice.** No `.bynk` in the repo (fixtures,
examples, book snippets) writes a `token--comment` adjacency, so the migration is a
no-op here; a pre-1.0 author who wrote one adds a space.

**D3 — The tree-sitter grammar keeps its context-free `line_comment` and is
documented as an approximation.** D1 is a lexer rule with one byte of *look-behind*
("was the previous character whitespace?"), which a context-free token cannot
express. Rather than move line comments into the doc-block external scanner —
position-sensitive C that risks the well-tested comment/doc-block highlighting for
a case no real program contains — the tree-sitter rule stays
`line_comment ::= "--" /[^\n]*/`. The **compiler lexer is normative**; the grammar
may over-highlight the `--` in the pathological `a--b`, and the spec says so
(§3.3.1). This is a *documented, highlighting-only* approximation, deliberately
distinct from a silent parse divergence.

**D4 — A `---` marker line always opens or closes a doc-block; there is no
standalone divider.** The marker is **three or more** consecutive hyphens alone on
a line (modulo horizontal whitespace). A lone `---` with no matching close is
`bynk.lex.unclosed_doc_block`, not a horizontal rule. This is unchanged behaviour,
now written into the spec (§3.3.2) rather than only the scanner.

## Consequences

- The `a--b` ambiguity has one normative answer (subtraction), and the
  line-swallowing footgun is gone. A `--` comment is unambiguously a
  whitespace-preceded construct.
- The grammar/compiler agree on every input a real program contains; the only
  difference is editor highlighting of `a--b`, and it is documented, not silent.
- The doc-block marker rule ("≥3 hyphens, no standalone divider") is specified,
  closing the second gap the review named.
- The last keyword-hygiene item (#548) — the docs-only enum-spelling convention —
  and the separate `where`-tier documentation land on their own.
