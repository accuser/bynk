---
level: patch
changelog: `bynk deploy` re-runs the cross-environment KV title check when it re-creates a namespace whose recorded id has gone from the account, and stops with the same message before `wrangler kv namespace create`, where the plan-time check could not see the re-create and wrangler failed with code 10014. At that deploy-time re-check only, another environment's recorded namespace whose id has gone no longer holds its title unless the source still declares it there. The plan-time check still counts every recorded namespace: clear a stale orphan with `bynk deploy --prune` in its own environment, or by removing it from `bynk.deploy.lock` (#2059).
---
