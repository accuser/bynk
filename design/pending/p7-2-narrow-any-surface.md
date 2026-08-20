---
level: patch
changelog: "P7.2: narrowed 24 of the 55 `ts_any`-counted sites (`as any`/bare `: any`) in `bynk-emit` to `unknown`, real declared/qualified types, or generated structural types (#1300) -- `ts_any` drops from 55 to 31. The other 31 (2 residual, needing a runtime type R7.7 hasn't exported yet, and 29 deferred -- more than the proposal's 3, several found only by the full `tsc --strict` fixture pass) are named at their own site with a comment recording why, not silently dropped."
---
