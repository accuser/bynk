---
level: patch
changelog: 'On the `workers` target, a context calling a service whose signature carries a `commons` type the caller does not `uses`, or a type the providing context does not export, now compiles: the caller imports the commons'' codecs or generates its own under the names the call uses. Both failed `tsc` before, though the checker and the `bundle` target accepted them (#1932)'
---
