---
level: patch
changelog: Bynk's emitted TypeScript is now verified under both TypeScript 5 and TypeScript 7, the current major (CI previously checked only 5). `bynk doctor` checks your `tsc` version, warning below 5 and calling anything newer than 7 untested, and its install advice, like `bynkc test`'s, now names `npm install -g typescript@7`. The `npx` fallbacks provision TypeScript 7 (#1672)
---
