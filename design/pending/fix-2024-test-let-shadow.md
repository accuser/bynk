---
level: patch
changelog: A test case's `let` named like a service, agent or asset of the unit under test (`let level <- Counter("a").bump()` beside `service level`), or re-binding a property's `for all` binder, now shadows it in the emitted test instead of redeclaring the runner's own binding and failing `tsc` (TS2451) (#2024)
---
