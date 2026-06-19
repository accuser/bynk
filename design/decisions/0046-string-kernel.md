# 0046 — The string kernel: built-in methods, UTF-16 code units, pinned footguns

- **Status:** Accepted (v0.22a)
- **Spec:** §4.6.8, §5.2, §7.3.8, §8

## Context
`String` is opaque in Bynk — no character access — so string operations
must come from the compiler. Every host string API hides at least one
surprise (first-occurrence `replace`, surrogate-splitting `split("")`,
negative-index wrap-around); each had to be pinned or inherited.

## Decision
String operations are **compiler built-in value methods** lowering to TS
string methods (the 0034/0037 hybrid posture): `length()`, `split(sep)`,
`trim()`, `toUpper()`, `toLower()`, `concat(t)`, `contains(sub)`,
`startsWith(sub)`, `endsWith(sub)`, `replace(a, b)`, `slice(lo, hi)`,
`indexOf(sub) -> Option[Int]`, `chars() -> List[String]`.

**Semantics are UTF-16 code units, normatively**, with pinned exceptions:

- `replace` replaces **every** occurrence (`replaceAll` — not TS's
  first-only string form).
- `chars()` splits by **code points** (`[...s]`), so
  `s.length() != s.chars().length()` for astral characters.
- `slice` **clamps negative indices to 0** — no wrap-around, no throw.
- `indexOf` returns `None`, never the `-1` sentinel.

`concat` is a method, **not** an extension of `+` (no operator change —
`+` stays numeric). Derived helpers are Bynk-written in the injected
`bynk.string` commons (currently `join(parts, sep)`, folding to
`Option[String]` so empty-string elements join faithfully).

## Consequences
String code is deterministic across hosts; the four footguns are
decisions, not accidents. Grapheme-cluster semantics remain out of scope
(code units/points only).
