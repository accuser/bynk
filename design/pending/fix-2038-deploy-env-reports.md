---
level: patch
changelog: Under `bynk deploy --env staging`, the `--context` refusal for a binding target that has never been deployed (now naming `shop-orders-staging` and `shop-payment-staging`) and a failed run's `N further contexts were not deployed` list name each Worker as the plan does, `-staging`, where they printed the bare names; the deploy guide now says that `--format json`'s `orphans.workers` and `orphans.queues` hold account names while `orphans.kv` holds ledger keys (#2038).
---
