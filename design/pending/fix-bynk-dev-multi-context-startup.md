---
level: patch
changelog: `bynk dev` with two or more contexts now starts without `--inspect`: each context gets its own inspector port instead of all racing for 9229, and a wrangler resolved via `npx` is installed once before the contexts start
---
