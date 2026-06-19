# 0040 — `Float` is a distinct base type, erased to `number`; finite at the boundary

- **Status:** Accepted (v0.21)
- **Spec:** §3, §6.1, §7.2, §7.6

## Context
Bynk could not express decimal data — prices, measurements, ratios —
beyond `Int`-as-cents. The v0.22 typed JSON codec needs a numeric type
that can represent non-integer numbers, or it ships visibly crippled.

## Decision
`Float` joins `Int`/`String`/`Bool` as a **fourth base type**, not a
refinement of `Int`. Both `Int` and `Float` lower to TS `number` — the
distinction is **Bynk-side only**, erased at runtime (like generics); the
checker is the only thing keeping them apart.

**The boundary is finite.** Arithmetic follows the host (0042 records
non-finite results as host-defined), but validated and wire-crossing
`Float` values are finite:

- `deserialise_` for a `Float` field requires `Number.isFinite` —
  `JSON.parse("1e999")` yields `Infinity`, which must not be admitted
  from the wire.
- Serialising a non-finite `Float` **throws** (a contract violation) —
  `JSON.stringify(NaN)` would otherwise silently produce `null` and break
  the round-trip.
- The `.of` validating constructor of a refined `Float` checks
  `Number.isFinite` (where refined `Int` checks `Number.isInteger`), so
  validated values are finite by construction — `.of` and the codec agree.

## Consequences
Decimal data round-trips exactly (IEEE 754 doubles both sides). A `Float`
that went `NaN`/`Infinity` through in-language arithmetic is caught at
the first boundary, not silently corrupted on the wire.
