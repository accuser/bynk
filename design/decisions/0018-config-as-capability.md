# 0018 — Configuration and IO reach bindings as capabilities; no `needs` clause

- **Status:** Accepted (v0.17/v0.18, superseding an interim `needs` draft)
- **Spec:** §7.3.6

## Context
Bindings need secrets, URLs, and HTTP access. An interim `needs <kind> "NAME"`
clause conflated configuration with vendor wrangler bindings and would grow
with every platform resource type.

## Decision
**No `needs` clause.** A binding's dependencies are all `given` capabilities —
config and IO arrive as `karn.Secrets` / `karn.Fetch` through the ordinary
deps object. `env` is read **only** inside first-party platform bindings,
explicitly, never injected into application adapters.

## Consequences
One dependency mechanism end to end. The v0.17 exemplars' secret/URL operation
parameters were removed in v0.18 once Secrets/Fetch existed.
