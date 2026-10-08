---
level: patch
changelog: On Windows, stopping `bynk dev` now stops each context's whole wrangler process tree, `workerd` included, by running it in a job object, so the next `bynk dev` no longer fails with `Address already in use` (#1762)
---
