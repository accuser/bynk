---
level: patch
changelog: 'A value of a type a consumed context does not export can now be passed back to that context: `let b <- shop.quote()` then `shop.take(b)` was rejected as a `bynk.boundary.structural_mismatch` against its own declaration. A service or capability return that the caller''s own type of the same name cannot hold, because the shapes differ, is now rejected at the call with `bynk.boundary.structural_mismatch`, where before the caller''s type silently typed the value and its fields (#1941)'
---
