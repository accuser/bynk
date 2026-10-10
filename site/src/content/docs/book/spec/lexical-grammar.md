---
title: "§3 Lexical grammar"
---
The lexical grammar defines Bynk's terminals: the tokens a source text is divided
into, and the trivia discarded between them. Each production below is generated
from the grammar ([§2.1](/book/spec/conventions/)); this chapter states only the
**syntactic** facts. Constraints beyond lexing (for example, the admissible range
of a literal) are well-formedness rules and are deferred to §5.

## §3.1 Identifiers and names

### §3.1.1 identifier

{{#grammar identifier}}

A letter followed by letters, digits, or underscores. Identifiers name
declarations, parameters, fields, and bindings. A source word that matches a
[keyword](/book/reference/keywords/) is lexed as that keyword, not as an
identifier.

### §3.1.2 constant_name

{{#grammar constant_name}}

An upper-case-initial name. Constant names denote sum-type variants and enum
constants.

## §3.2 Literals

### §3.2.1 number_literal

{{#grammar number_literal}}

A run of decimal digits. A number literal is unsigned; a leading `-` is the unary
negation operator ([§4.6](/book/spec/syntactic-grammar/)), not part of the token. An
integer literal whose magnitude exceeds 2^53 − 1 (9007199254740991), the largest
safe integer and so the `Int` range (#1657), is `bynk.lex.integer_overflow`.

### §3.2.1a float_literal

{{#grammar float_literal}}

A `Float` literal (v0.21): a fraction with a **digit required on both sides**
of the `.` (`1.0`, `0.5`), an exponent (`1e10`, `1.5e-3`), or both. `1.` and
`.5` are rejected as `bynk.parse.malformed_float_literal`. Like
`number_literal` the token is unsigned. A literal that does not fit a finite
IEEE 754 double (`1e999`) is `bynk.lex.float_literal_overflow` — there is no
way to write a non-finite `Float` literal.

`1` is an `Int`; `1.0` (or any exponent form) is a `Float`. The
digit-both-sides rule keeps method calls on numeric literals unambiguous
under maximal munch: `2.5.round()` lexes as `2.5` `.` `round`, and
`1.toFloat()` as `1` `.` `toFloat`. The compiler preserves the literal's
**lexeme** through emission and formatting — `1e10` does not normalise to
`10000000000`.

### §3.2.2 string_literal

{{#grammar string_literal}}

A double-quoted string. The escape sequences `\n`, `\t`, `\"`, and `\\` are
recognised; an unescaped newline does not appear within the token.

A string may also contain **interpolation holes** of the form `\(expr)`
(v0.43): the text `\(` opens a hole whose body runs to its matching `)`
(parentheses balance, and a nested `"…"` inside a hole is skipped so its
parens do not close the hole), and the body is an ordinary
[expression](/book/spec/syntactic-grammar/). `\\(` is the escape for a literal `\(`,
so existing literals are unaffected (a bare `\(` was previously an invalid
escape). A string containing one or more holes is an *interpolated string*; one
with none is the plain string literal above. The hole rule and emission are
specified in [§5.2 well-typedness](/book/spec/static-semantics/#52-well-typedness) and
[§7 emission](/book/spec/emission/).

### §3.2.2a string_interpolation

{{#grammar string_interpolation}}

One interpolation hole, `\(expr)`, inside a string literal (v0.43).

### §3.2.3 boolean_literal

{{#grammar boolean_literal}}

The two `Bool` values, `true` and `false`.

### §3.2.4 unit_literal

{{#grammar unit_literal}}

The unit value `()` — the single value of the unit type. It is lexically the
empty parenthesis pair.

## §3.3 Comments and doc-blocks

### §3.3.1 line_comment

{{#grammar line_comment}}

A comment runs from `--` to the end of the line. Bynk uses `--`, never `//`. Line
comments are trivia ([§3.4](#34-trivia)).

A `--` opens a comment **only at the start of input or when the preceding
character is whitespace** (a space, tab, carriage return, or newline). Adjacent to
a preceding token, `--` is **not** a comment: `a--b` lexes as `a - -b` (a
subtraction of a negation), and `x--` as `x` followed by two `-` operators, not a
comment that silently swallows the rest of the line. This resolves the `a--b`
"comment or subtraction?" ambiguity in favour of subtraction; write ` -- ` (with a
leading space) for a trailing comment. *(The tree-sitter grammar's
`line_comment ::= "--" /[^\n]*/` is a context-free approximation — a token cannot
express the preceding-whitespace condition — so an editor may over-highlight the
`--` in the rare `a--b`; the compiler's rule above is normative.)*

### §3.3.2 doc-blocks

A **doc-block** is a `--- … ---` documentation block. It is an *external token*:
not a grammar rule but a terminal recognised by the lexer and attached to the
declaration that follows it. Like comments and whitespace, a doc-block is trivia
([§3.4](#34-trivia)).

The **opening and closing markers** are each a line whose only content is **three
or more consecutive hyphens** (`---`, `----`, …), preceded only by optional
horizontal whitespace and followed only by optional horizontal whitespace to the
end of the line. The **closing marker has exactly as many hyphens as the opening
one**, as with a Markdown code fence; a marker line of any other length inside the
block is content. So a block opened with `----` can hold `---` lines: a Markdown
horizontal rule, front matter, or a Bynk example with its own doc-block.

```bynk
----
The answer, with a Markdown rule in its doc:

---

Below the rule.
----
fn answer() -> Int { 42 }
```

There is **no standalone `---` divider**: a marker line always *opens* (or
*closes*) a doc-block, so a marker with no closing marker of its length is the
error `bynk.lex.unclosed_doc_block`, not a horizontal rule. The content between
the markers is arbitrary text and may contain `--` fragments without closing the
block.

A doc-block's content must not be code. If the content, with its Markdown code
fences (```` ``` ```` or `~~~`) removed, parses as one or more declarations, the
block is the error `bynk.parse.doc_block_contains_code`. This catches a pair of
dash dividers around code, which would otherwise turn that code into
documentation silently, whether or not a declaration follows. For a divider,
write a line comment with text (`-- Helpers --`); to show code in a doc, put it in
a code fence.

```bynk,fail=bynk.parse.doc_block_contains_code
--------------------------------
fn helper() -> Int { 1 }
--------------------------------
fn f() -> Int { 2 }
```

### §3.3.3 doc_line

{{#grammar doc_line}}

A **doc line** is the one-line documentation form (#1888): `--|`
followed by text to the end of the line. A `--|` is a doc line only when it is
the **first thing on its line**, after optional horizontal whitespace. Anywhere
else, such as after a field on the same line (`repo: Repo, --| text`), it is an
ordinary line comment ([§3.3.1](#331-line_comment)) whose text starts with `|`.
There is no end-of-line doc.

Doc lines on **consecutive lines** form one doc. Its text is each line's text
after the `--|` marker and one separating space, joined with line breaks, so a
bare `--|` line separates Markdown paragraphs. A blank line or a `--` comment
line ends the run. A run of doc lines attaches to what follows it exactly as a
doc-block does, and is trivia ([§3.4](#34-trivia)) in the same way. Unlike a
doc-block, its content is not probed for code
(`bynk.parse.doc_block_contains_code`): every line is marked as a doc on
purpose, so no pair of dividers can turn code into one by accident.

```bynk
--| A full commit SHA.
type Commit = String

--| One canary result.
--|
--| A bare `--|` line starts a new paragraph.
type Run = {
	--| The repository, as `owner/name`.
	repo: String,
	-- A comment, not a doc.
	passed: Int,
}
```

**Where a doc may go.** A doc of either form documents the declaration that
follows it and, since #1888, each of these members: a record field, a sum
variant of either form (`| V` or an `enum { V }` tag), a variant's payload
field, and a `messages` entry. A function's or handler's parameters are
documented in the function's own doc. A doc separated from what follows by a
blank line, or with nothing after it in its body, attaches to nothing
(`bynk.parse.orphan_doc_block`, a warning).

**One form per target.** A doc of one form directly followed by a doc of the
other, with no blank line between, is the error `bynk.parse.doc_forms_mixed`:
a declaration or member is documented in one form. `bynk fmt` prints each doc in
the form it was written in and never converts between them.

```bynk,fail=bynk.parse.doc_forms_mixed
--| The answer.
---
The answer, again.
---
fn answer() -> Int { 42 }
```

## §3.4 Trivia

Between tokens the lexer discards **trivia**: whitespace (`/\s+/`), line comments
([§3.3.1](#331-line_comment)), doc-blocks ([§3.3.2](#332-doc-blocks)), and doc
lines ([§3.3.3](#333-doc_line)). Trivia
does not appear in the productions of §4 — no production has a newline
terminal — but it is not wholly insignificant: the parser consults whether a
newline separates two tokens at three sites, each a narrow, documented
carve-out rather than a general rule. A `+`/`-` beginning a new line does not
continue an additive chain ([§4.6.6](/book/spec/syntactic-grammar/#467-binary_expr));
a `[` opening a new line is a list literal rather than explicit type
application on the preceding name, both for a call
([§4.6.21a](/book/spec/syntactic-grammar/#4621a-list_literal)) and for a
method call ([§4.6.8](/book/spec/syntactic-grammar/#468-method_call)). No
other construct is newline-sensitive — in particular, `match`-arm separation
([§4.7.1](/book/spec/syntactic-grammar/#471-match_arm)) is not: arms are
terminated by their own greedy parse and an optional trailing comma, with no
newline check. The complete token-and-trivia summary is part of the grammar
appendix ([§11](/book/spec/grammar-appendix/)).
