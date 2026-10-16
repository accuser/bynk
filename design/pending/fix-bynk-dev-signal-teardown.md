---
level: patch
changelog: A SIGINT or SIGTERM sent to `bynk dev` itself now stops its `wrangler` and `workerd` processes before it exits, instead of leaving them running with their ports bound
---
