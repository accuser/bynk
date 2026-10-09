---
level: patch
changelog: "Extract-variable no longer hoists an expression out of the scope of the bindings it reads (#1819). In an expression-bodied match arm (`Some(x) => x + 1`) or a lambda body (`(v) => v * 2`), the `let` it inserts above the statement read `x` or `v` where neither is bound. Such a selection is now declined, as extract-function declines what it cannot lift soundly. A selection there that reads none of them still extracts"
---
