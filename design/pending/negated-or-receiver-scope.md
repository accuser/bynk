---
level: patch
changelog: An `is` test with a call as its receiver on the right of a negated `||` now binds correctly in the `else` branch. `if !(f(a) is Some(x)) || !(f(b) is Some(y)) { 0 } else { x + y }` emitted TypeScript that read the right operand's value outside the scope it was declared in, which `tsc` rejected and which threw a `ReferenceError` at runtime (#1751)
---
