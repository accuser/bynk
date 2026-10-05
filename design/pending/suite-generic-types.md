---
level: patch
changelog: A test suite whose target declares a generic type, or `uses` one, now type-checks and runs; the generated test module aliases each generic type with its type parameters, where before every such suite failed `tsc` with `TS2314` even if no case mentioned the type (#1703)
---
