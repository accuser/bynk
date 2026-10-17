---
level: patch
changelog: `bynk deploy` refuses, before the plan and under `--dry-run` too, a KV namespace it would create whose title another environment in `bynk.deploy.lock` already holds or would create (Worker `shop`'s bare namespace under `--env staging` and its `capability Staging` in the default environment are both `shop-staging`), naming both namespaces and both environments, where it compared titles within one environment only and left the clash to fail at `wrangler kv namespace create` (#2048).
---
