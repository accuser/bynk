---
level: patch
changelog: A binding from an `is` test on the right of `&&` (or of `implies`, or a negated `||`) now type-checks wherever it is read. `if o is Some(v) && p is Some(w) { v + w }` emitted `p.value` outside the arrow that held `p`'s tag test, so TypeScript could not narrow it and `tsc --strict` failed with TS2339. The read now casts to the variant the checker proved (#1752)
---
