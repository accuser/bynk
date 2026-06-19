# 0048 — `Option`/`Result` combinators and numeric helpers are kernel methods

- **Status:** Accepted (v0.22a)
- **Spec:** §4.6.8, §5.2, §7.3.8

## Context
The original v0.22 draft proposed Bynk-written free-function commons
(`bynk.option`, `bynk.int`, …) per the 0037 pattern. Review killed that
twice over: free functions imported by bare name collide
(`bynk.resolve.duplicate_fn` — `bynk.list` already exports `map`, so
`uses bynk.list` + `uses bynk.option` could never resolve), and a single
generic `abs[A]` is impossible under 0028's no-bounds rule (a generic
body cannot compare `A` to zero). 0037's stated reason for free functions
(no generic *user* methods) never forbade *built-in* methods.

## Decision
- **`Option[T]`**: `o.map(f)`, `o.andThen(f)` (must return `Option`),
  `o.getOrElse(x)`, `o.isSome()`, `o.okOr(e) -> Result[T, E]`.
- **`Result[T, E]`**: `r.map(f)`, `r.andThen(f)` (must return a `Result`
  with the receiver's error type), `r.mapErr(f)`, `r.getOrElse(x)`,
  `r.isOk()`.
- **Numerics** (extending the v0.21 kernel): `x.abs()`, `a.min(b)`,
  `a.max(b)`, `x.clamp(lo, hi)` on **both** numeric types (`clamp` on
  `Int` too — the proposal's `f.clamp` was an example, not a
  restriction); `f.isNaN()`, `f.isFinite()` on `Float`.
- **`Int.parse(s)` / `Float.parse(s)` -> `Option[T]`** as statics (0041:
  ways to obtain a value). Full-string parse; empty/whitespace → `None`;
  beyond the safe-integer range (`Int` — `Number.isInteger(1e21)` is
  `true`, so `isSafeInteger` is the honest bound) or non-finite (`Float`)
  → `None`.

Function arguments type their parameters contextually from the receiver;
the return is read from the actual (the v0.20a pass-2 flexible-var rule).
A lambda body that itself needs an expected type (bare `Ok`/`Err`)
annotates a `let` — the same rule as generic-call lambdas, machinery
shared deliberately.

Deliberate gaps: no `isNone`/`isErr` (negate), no `unwrap` (no panics in
Bynk), no effectful combinator variants (the `foldEff` precedent says add
them when a fixture needs them).

## Consequences
Zero collision surface, and **method chaining works from day one**
(`o.map(f).getOrElse(x)`) — built-in methods chain without the deferred
generic-user-methods feature. The cost: these combinators are compiler
code, not dogfooded Bynk; `bynk.string`'s `join` keeps a foot in the
Bynk-written camp.
