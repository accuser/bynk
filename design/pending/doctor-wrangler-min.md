---
level: patch
changelog: `bynk doctor` warns when your `wrangler` is too old to serve the compatibility date Bynk pins (currently it needs 4.107.0 or newer), and `bynk dev` says so before serving, rather than leaving wrangler to fail with "This Worker requires compatibility date …". Deploying still works with an older wrangler, so `doctor --only deploy` doesn't fail on it (#1732)
---
