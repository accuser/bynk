# 0179 — Deploy plans before provisioning and is idempotent against its ledger

- **Status:** Accepted (v0.154)
- **Provenance:** #583, deploy track slice 0.
- **Relates:** ADR 0178 (provisioning state), ADR 0084 (driver pre-flight).

## Decision

Slice-0 `bynk deploy` follows this order: pre-flight tools, compile, derive and
print a plan, authenticate with `wrangler whoami`, confirm, provision missing
KV state, materialise generated configuration, then run `wrangler deploy`.

`--dry-run` (also `--plan`) exits after printing the plan. `--format short` is
line-oriented and `--format json` is machine-readable. Mutations require an
interactive confirmation or `--yes`; non-interactive callers must pass
`--yes`. Credentials remain Wrangler-owned (`wrangler login` or
`CLOUDFLARE_API_TOKEN`) and are checked before the first Cloudflare mutation.

An existing KV entry is reused, not created again. A newly created id is written
to the ledger before the Worker is pushed, so an interrupted run can be safely
retried without making another namespace.

## Consequences

The command is safe to rerun and makes every externally visible change clear
before it occurs. This slice deliberately supports exactly one context and KV;
dependency ordering, queues, Durable Object migrations, environments, and
secrets are later deploy-track slices.
