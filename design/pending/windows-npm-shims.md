---
level: patch
changelog: On Windows, `bynkc test` and `bynk test` run an npm-installed `tsc`, `tsx` or `npx`, and `bynk dev`/`bynk deploy` run an npx-provisioned wrangler. These tools are `.cmd` shims, which were found on `PATH` but then spawned by bare name, which Windows resolves to `.exe` only. A test runner that is found but fails to start is now reported as such, rather than as missing with advice to install it (#1758)
---
