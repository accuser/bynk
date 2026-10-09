---
level: patch
changelog: "`bynkc compile`, and the build behind `bynk dev` and `bynk deploy`, strip every `suite`, as the project-layout guide and ADR 0147 say (#1821). They used to type-check and emit the project's suites into `out/tests/`, so the deployable carried test code, a broken suite failed the build, and a bundle build of a project with a `system` suite failed `tsc` (its test module imports the workers layout). `bynkc test` still compiles and runs them, and `bynk check` still checks them"
---
