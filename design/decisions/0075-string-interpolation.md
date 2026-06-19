# 0075 — String interpolation

- **Status:** Accepted (v0.43)
- **Spec:** `docs/src/spec/lexical-grammar.md` (string literals), `docs/src/spec/static-semantics.md` (the hole rule), `docs/src/spec/emission.md` (template-literal lowering)
- **Relates to:** ADR 0046 (the string kernel — *not* overridden; `+` stays numeric, `concat` stays a method), ADR 0074 (`Int`/`Float.toString` — the display contract a numeric hole rests on)
- **Issue:** #45

## Context
`+` is `Int`/`Float`-only (ADR 0046 deliberately keeps `+` numeric and `concat`
a method), so building a string means a `.concat()` chain — the least elegant
line in `examples/hello-world` (`"Hello, ".concat(subject).concat("!")`), and
string-building is exactly what first programs do. Two cheaper options were
weighed and set aside: admitting `+` for `String` (would reopen ADR 0046), and
leaning on the already-shipped `bynk.string.join` (no better for the inline
case).

## Decision
Add **interpolation holes** to string literals: `"… \(expr) …"`.

- **Syntax: `\(expr)`.** Swift-style. Backward-compatible — `\(` was an
  *invalid* escape in the string grammar, so no existing valid literal contains
  it; `\\(` escapes a literal `\(`. `${…}` was rejected (it silently re-means
  existing literals, since `$`/`{` are ordinary string characters today).
- **AST: `ExprKind::InterpStr(Vec<InterpPart>)`** where `InterpPart` is
  `Chunk(String)` or `Hole(Box<Expr>)`. A plain `"…"` with **no** holes stays
  `ExprKind::StrLit`, so existing code and the emitter/formatter fast-path are
  untouched. A hole is not a new expression kind — only a new *container* — so
  the resolver, checker, emitter, and LSP reuse the existing expression
  machinery.
- **Lexing.** A hand-scanner in `tokenize` (logos cannot balance parens) emits
  one `InterpStr` token when a string contains a `\(` hole; plain strings keep
  the logos `StrLit` path verbatim. The scanner balances the hole's parens and
  skips nested strings (so `"\(label(")"))"` takes `label(")")` as the hole).
  The parser re-lexes each hole's source with **rebased absolute spans** and
  parses it as a full expression, so diagnostics (and the later LSP) point at
  the real location.
- **The hole rule — the central call: auto-convert the base scalars.** A hole
  of `String` interpolates as-is; `Int`/`Float` render via the ADR 0074
  `toString` contract; `Bool` renders as `true`/`false`; a **refinement of** any
  base scalar widens to its base (so `Subject` — `String where …` — displays as
  its `String`, the hello-world case). **Every other type is rejected**
  (`record`, `sum`, `Option`, `Result`, opaque, …) with
  `bynk.types.interpolation_non_scalar` — *"type T has no string form here"*.
  Rationale: interpolation's whole point is *display*, so requiring
  `.toString()` on every number would gut the ergonomic win; bounding the
  implicit to base scalars (and their refinements) forecloses JS's
  `[object Object]` footgun; and a display conversion *in a display context* is
  a far narrower, more defensible implicit than arithmetic coercion. Opaque
  types are excluded deliberately — their base is hidden, so a value must be
  `.raw`-ed out first.
  - *Considered and rejected:* holes must already be `String` (strict, no
    implicit). More consistent with Bynk's no-coercion stance, but the
    ergonomic cost (`"count: \(n.toString())"` everywhere) defeats the feature.
- **Emission: a TS template literal.** Chunks become escaped literal text
  (backslash, backtick, and `$` escaped so a chunk can neither close the literal
  nor start a substitution); each hole becomes `${String(<lowered expr>)}`.
  `String(…)` is identity for a `String` hole and the display form for the
  other scalars — type-agnostic, so the emitter needs no per-hole type lookup,
  and the checker's bound guarantees nothing else reaches it. Byte-stable.

## Consequences
The headline line of `examples/hello-world` becomes `"Hello, \(subject)!"`. The
change is additive: a new AST node, one lexer scanner, one checker rule, one
emitter arm, and a recursion arm in each expression walker; the `StrLit`
fast-path is untouched, so non-interpolated code emits and formats exactly as
before. ADR 0046 is **not** disturbed — `+` stays numeric, `concat` stays a
method; interpolation is a third, orthogonal way to build a string. Format
specifiers / padding inside holes (`\(n, width: 4)`) are out of scope — a
separate, larger feature. Delivered in three slices: **core** (this slice —
lexer/AST/parser/checker/emitter), **surface tooling** (formatter round-trip,
tree-sitter, TextMate), and **LSP polish** (hover/completion/semantic tokens
inside holes).
