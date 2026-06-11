# §3 Lexical grammar

The lexical grammar defines Karn's terminals: the tokens a source text is divided
into, and the trivia discarded between them. Each production below is generated
from the grammar ([§2.1](conventions.md)); this chapter states only the
**syntactic** facts. Constraints beyond lexing (for example, the admissible range
of a literal) are well-formedness rules and are deferred to §5.

## §3.1 Identifiers and names

### §3.1.1 identifier

{{#grammar identifier}}

A letter followed by letters, digits, or underscores. Identifiers name
declarations, parameters, fields, and bindings. A source word that matches a
[keyword](../reference/keywords.md) is lexed as that keyword, not as an
identifier.

### §3.1.2 constant_name

{{#grammar constant_name}}

An upper-case-initial name. Constant names denote sum-type variants and enum
constants.

## §3.2 Literals

### §3.2.1 number_literal

{{#grammar number_literal}}

A run of decimal digits. A number literal is unsigned; a leading `-` is the unary
negation operator ([§4.6](syntactic-grammar.md)), not part of the token. An
integer literal that does not fit a 64-bit signed integer is
`karn.lex.integer_overflow`.

### §3.2.1a float_literal

{{#grammar float_literal}}

A `Float` literal (v0.21): a fraction with a **digit required on both sides**
of the `.` (`1.0`, `0.5`), an exponent (`1e10`, `1.5e-3`), or both. `1.` and
`.5` are rejected as `karn.parse.malformed_float_literal`. Like
`number_literal` the token is unsigned. A literal that does not fit a finite
IEEE 754 double (`1e999`) is `karn.lex.float_literal_overflow` — there is no
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

A comment from `--` to the end of the line. Karn uses `--`, never `//`. Line
comments are trivia ([§3.4](#34-trivia)).

### §3.3.2 doc-blocks

A **doc-block** is a `--- … ---` documentation block. It is an *external token*:
it is not a grammar rule but a terminal recognised by the lexer and attached to
the declaration that follows it. Like comments and whitespace, a doc-block is
trivia ([§3.4](#34-trivia)).

## §3.4 Trivia

Between tokens the lexer discards **trivia**: whitespace (`/\s+/`), line comments
([§3.3.1](#331-line_comment)), and doc-blocks ([§3.3.2](#332-doc-blocks)). Trivia
is insignificant to the syntactic grammar; it does not appear in the productions
of §4. The complete token-and-trivia summary is part of the grammar appendix
([§11](grammar-appendix.md)).
