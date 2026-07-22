# 0215 — The parser and interpolation lexer bound recursion depth

- **Status:** Accepted (v0.194)

**Context.** The recursive-descent parser and the string-interpolation scanner
had no depth guard: each nesting level costs one stack frame, so deeply nested
source overflows the stack and the process aborts with `SIGABRT`. Four
self-recursive descents reach it — parenthesised expressions (`parse_primary`
→ `parse_expr`), generic type arguments (`parse_type_ref` → `parse_type_atom`
→ `parse_type_ref`), nested variant patterns (`parse_pattern` →
`parse_pattern_binding` → `parse_pattern`), and mutually recursive interpolation
(`scan_str` ↔ `scan_hole`). The overflow is
reachable on the 8 MB main thread (~880 parenthesised levels) and, because the
LSP and the in-browser playground run on ~1 MB stacks, in the low hundreds
there. A compiler/LSP must never abort on malformed source: a panic kills an LSP
request and crashes the playground, and the `parse` fuzz target documents this
front-end as shared by all three surfaces (#713).

**Decision.** Introduce a single fixed nesting bound,
`MAX_NESTING_DEPTH = 64`, shared by the parser and the interpolation lexer. The
parser tracks live recursion depth across its three self-recursive entry points
(`parse_expr`, `parse_type_ref`, `parse_pattern`) — every nested subexpression,
type, and pattern routes through one of them, and nested blocks nest only via
the `if`/`match`/lambda expressions that pass through `parse_expr` — and the
lexer threads a depth count through `scan_str`/`scan_hole`. Exceeding the bound
yields a diagnostic
(`bynk.parse.nesting_too_deep` or `bynk.lex.interpolation_too_deep`) instead of
another recursion. The value sits well below the ~110 levels a 1 MB stack holds
in release (~9 KB/level), leaving comfortable headroom, and far above any
realistic hand-written or generated source, where expression, type, and
interpolation nesting past a handful is already exceptional.

**Consequences.** Malformed or adversarial deeply nested source now fails
cleanly with a spanned diagnostic on every surface, closing a
denial-of-service/crash class rather than aborting the process. The bound is a
deliberate, revisable engineering limit, not a language semantic: source nested
deeper than 64 levels — vanishingly rare outside a fuzzer — is rejected where it
previously (barely) parsed, hence a language increment. The limit is a single
named constant so it can be retuned if a surface's stack budget changes.
