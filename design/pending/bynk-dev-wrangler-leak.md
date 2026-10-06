---
level: patch
changelog: When `bynk dev` stops, it no longer leaves `wrangler` and `workerd` processes running and holding their ports, which made the next `bynk dev` fail with `bind(): Address already in use`. Before, the leak happened when one context's wrangler exited and the others were stopped: with wrangler resolved via npx, every context's processes survived, and with any wrangler, the exited context's `workerd`s did. `bynk dev` now also stops whatever is still running in its worker directories (#1742)
---
