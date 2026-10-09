---
level: patch
changelog: "Workers builds import every commons codec they call, at the top of the module (#1815, #1823, #1817). A commons record whose field has a type from another commons called that commons' codec without importing it (TS2552). A cross-context call whose argument or result type was a commons type appearing in none of the caller's own signatures did the same (TS2304). And a codec import was emitted after the agent wire table that used it, so a Worker whose agent handler took a commons type failed to load under a `system` test (`ReferenceError: Cannot access … before initialization`)"
---
