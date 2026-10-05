---
level: patch
changelog: Bynk's minimum Node.js version is now 22, the oldest release still supported (it was 18, which reached end of life in April 2025). `bynk doctor` warns below it, so its `ok` now means `bynkc test --inspect` works too. CI runs its test suites on Node 22 and now fails, rather than silently skipping, if the type-stripping check that `--inspect` depends on can't run (#1674, #1671)
---
