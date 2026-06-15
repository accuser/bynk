# 0074 — `toString` on the numeric kernel

- **Status:** Accepted (v0.42)
- **Spec:** `docs/src/spec/static-semantics.md` (numeric kernel), `docs/src/spec/emission.md`
- **Relates to:** ADR 0046 (the string kernel; the UTF-16 normative precedent this follows)

## Context
The numeric kernel (v0.21/v0.22a) renders nothing as text — `Int.parse` covers
the *other* direction, but there is no way to put an `Int` or `Float` into a
`String`. Any program that displays a counter, timestamp, id, or measurement
hits this immediately (it shaped `examples/hello-world`, #44).

## Decision
Add **`toString() -> String`** to the numeric kernel — total on **both `Int` and
`Float`** — via the established kernel-method extension points: a checker
dispatch arm (`check_numeric_kernel_method`), a registry entry (`INT_METHODS` /
`FLOAT_METHODS`, so completion + the drift test cover it), and an emitter lowering.

- **Emission: `n.toString()` → `String(n)`.** For `Int`, the result is the
  canonical decimal integer. For `Float`, the contract is **the host's
  number→string** — ECMAScript `Number::toString` (the shortest decimal that
  round-trips; `1e21`, `Infinity`, `NaN`, `-0` render as the platform renders
  them). This is stated **normatively**, the same way ADR 0046 pins the string
  kernel's UTF-16 semantics to the host: the runtime is JS/TS, and reproducing
  a bespoke float formatter would be both surprising and a maintenance burden.
- **Bare base types only.** Like the rest of the numeric kernel, `toString`
  dispatches on `Ty::Base(Int|Float)`; a refined numeric reaches it once it
  widens to its base (the #48 note guides that case).
- **Total.** No failure mode — every `Int`/`Float` has a string form — so it
  returns `String`, not `Result`.

## Consequences
The most common first wall after hello-world is gone, with a one-line-per-site
addition and no new runtime helper. The Float contract is the host's, so output
is exactly what a TS developer expects (and varies only as the host's number
formatting does — acceptable and documented). A locale-/precision-controlled
formatter (`format(…)`) is a larger, separate feature if ever wanted.
