---
level: patch
changelog: `bynk deploy` re-runs the cross-environment KV title check when it re-creates a namespace whose recorded id has gone from the account, and stops with the same message before `wrangler kv namespace create`, where the plan-time check could not see the re-create and wrangler failed with code 10014. A recorded namespace in another environment whose id has gone no longer counts as holding its title, unless the source still declares it there (#2059).
---
