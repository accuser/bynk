# 0041 — No implicit `Int`↔`Float` coercion; conversions are value methods

- **Status:** Accepted (v0.21)
- **Spec:** §5.6, §6.1

## Context
Most languages silently widen `Int` to `Float` in mixed arithmetic;
silent widening is exactly the surprise Karn's strict-by-default posture
avoids. And once conversions are explicit, their call surface sets the
precedent the v0.22 numeric stdlib copies.

## Decision
Mixing `Int` and `Float` in any operation is a static error,
`karn.types.no_numeric_coercion`. Conversion is explicit, via the
**numeric kernel** — built-in **value methods on the bare base types**,
the same dispatch as the collection kernel (0036/0037):

- `i.toFloat() -> Float` — total.
- `f.round()`, `f.floor()`, `f.ceil()`, `f.truncate()` — each
  `-> Int`, **named and lossy**; there is deliberately no ambiguous
  `toInt`.

**Statics are reserved for constructors and parsers** (`T.of`,
`List.empty()`, and v0.22's `Float.parse`) — the rule: operations on a
value are methods, ways to obtain a value are statics. A refined numeric
value reaches the kernel by widening (refined types already widen to
their base in operator positions); kernel dispatch itself is on the bare
base type.

## Consequences
`1 + 2.0` is an error with a fix-it note instead of a silent `3.0`.
Digit-both-sides literals (0043) make `1.toFloat()` and `2.5.round()`
lex unambiguously, so the method surface costs no lexical contortions.
