---
level: patch
changelog: `bynk deploy --dry-run --env staging` names the Workers, Service Binding targets and queues a staging deploy creates (`deploy shop-orders-staging`, `order shop-payment-staging → shop-orders-staging`, `queue create job-intake-staging`), in text and in `--format json` (`order`, each context's `worker`, `binds_to` and `queues`, and orphaned queues and Workers), where it printed the bare names. The deploy itself already pushed `-<env>` Workers; it now states the Worker's `name` in the generated `[env.<env>]` block, from the same function the plan uses, rather than leaving it to Wrangler's default, which is the same name (#2028).
---
