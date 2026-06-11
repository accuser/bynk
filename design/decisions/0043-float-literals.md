# 0043 — Float literals: fraction/exponent, digit-both-sides, reject overflow, store the lexeme

- **Status:** Accepted (v0.21)
- **Spec:** §3.4a, §4.6

## Context
A float literal syntax must coexist with method calls on numeric
literals (`1.toFloat()`, 0041) and with the existing `IntLit`.

## Decision
`FloatLit` is digits `.` digits (`1.0`, `0.5`), an exponent (`1e10`,
`1.5e-3`), or both. A **digit is required on both sides** of the `.` —
`1.` and `.5` are `karn.parse.malformed_float_literal`. `1` stays
`Int`; `1.0` or an exponent makes it `Float`.

- **Strictly additive:** `1e10` previously lexed as `IntLit(1)` +
  `Ident(e10)` — never valid downstream — so claiming exponents takes
  no working syntax.
- **Reject overflow:** a literal that parses to a non-finite double
  (`1e999`) is `karn.lex.float_literal_overflow`, mirroring `IntLit`'s
  out-of-range reject. (The proposal sketched this code under
  `karn.parse.*`; it lands in `karn.lex.*` because the check runs in
  `tokenize()` exactly where `karn.lex.integer_overflow` does.)
- **Store the lexeme:** the AST keeps the source text alongside the
  parsed `f64` (`1e10` must not normalise to `10000000000`), so emission
  and formatting are byte-stable. Float refinement bounds keep their
  (signed) lexemes for the same reason.

## Consequences
`2.5.round()` lexes as FloatLit `2.5` then `.round` under maximal
munch; `1.toFloat()` works because `1.` alone is not a FloatLit prefix.
The malformed-literal cases are parser-level (the lexer cannot reject
`1.` without breaking method calls on integer literals).
