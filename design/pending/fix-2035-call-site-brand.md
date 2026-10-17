---
level: patch
changelog: On `workers`, a cross-context call argument of a named type is cast to the callee's type, so a caller's own commons values built into it, such as `Map.empty()` in a field of a consumed record, pass `tsc`
---
