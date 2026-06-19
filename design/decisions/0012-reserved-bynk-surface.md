# 0012 — The `bynk` surface is reserved, flat-named, and ambient-only

- **Status:** Accepted (v0.17; scope sealed v0.18)
- **Spec:** §5.8, §7.3.6

## Context
The toolchain ships first-party capabilities every platform guarantees. Their
namespace must be collision-proof, and their scope must not creep into
platform-shaped infrastructure.

## Decision
Any unit name whose first segment is `bynk` is **reserved**
(`bynk.namespace.reserved`). Bynk names are flat — `bynk.time` would be an
independent unit, not a child — so the surface ships as one `bynk` adapter,
splittable later. Its scope is **ambient primitives only**: `Clock`, `Random`,
`Logger`, `Fetch`, `Secrets`. No infrastructure capability ever joins it, and a
`bynk` capability may not depend on a platform-native one.

## Consequences
`consumes bynk { … }` is a reliable portability marker. Porting Bynk to a new
runtime means implementing this one adapter's interfaces (one binding per
platform).
