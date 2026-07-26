---
level: patch
changelog: The emitter now gives a same-block re-`let` of a name (`let x = 1; let x = x + 1`) its own emitted identifier instead of reusing the source name, so the second `const` no longer collides with the first and fails `tsc` (TS2451) — shadowing itself was already accepted by the checker and is a deliberate ML-family idiom (ADR 0064)
---
