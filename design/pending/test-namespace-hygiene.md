---
level: patch
changelog: "`bynk test` works for a context or commons that declares a function with its own name (`context greet` with `fn greet`). The generated suite imported the unit as a namespace named after it and destructured the function from it, shadowing the import, so `tsc` rejected the suite. A test module's namespaces now carry a reserved `__ns_` prefix (#1759)"
---
