---
level: patch
changelog: "Extract-variable no longer hoists an expression out of the scope of the bindings it reads (#1819). In an expression-bodied match arm (`Some(x) => x + 1`), a lambda body (`(v) => v * 2`), the right of an `is` test's `&&` or `implies` (`r is Ok(n) && n > 0`) or an observation's `with` predicate, the `let` it inserted above the statement read a name not bound there. Such a selection is now declined, as extract-function declines what it cannot lift soundly; one that reads none of those names, or sits in a block body where the `let` lands inside their scope, still extracts"
---
