# 0012 — The `karn` surface is reserved, flat-named, and ambient-only

- **Status:** Accepted (v0.17; scope sealed v0.18)
- **Spec:** §5.8, §7.3.6

## Context
The toolchain ships first-party capabilities every platform guarantees. Their
namespace must be collision-proof, and their scope must not creep into
platform-shaped infrastructure.

## Decision
Any unit name whose first segment is `karn` is **reserved**
(`karn.namespace.reserved`). Karn names are flat — `karn.time` would be an
independent unit, not a child — so the surface ships as one `karn` adapter,
splittable later. Its scope is **ambient primitives only**: `Clock`, `Random`,
`Logger`, `Fetch`, `Secrets`. No infrastructure capability ever joins it, and a
`karn` capability may not depend on a platform-native one.

## Consequences
`consumes karn { … }` is a reliable portability marker. Porting Karn to a new
runtime means implementing this one adapter's interfaces (one binding per
platform).
