# 0179 — Deploy provisioning state lives in `bynk.deploy.lock`

- **Status:** Accepted (v0.154)
- **Provenance:** #583, deploy track slice 0.
- **Realises:** Cloudflare resource identity is persistent driver state, never a
  hand-edit to generated Worker configuration.

## Decision

`bynk deploy` owns a project-root, committed, secret-free
`bynk.deploy.lock`. Its TOML schema is versioned and environment-keyed from the
first slice. Slice 0 records only KV namespace ids:

```toml
version = 1

[environments.default.kv.api]
id = "cloudflare-namespace-id"
```

The key is the context's logical worker identity. The generated
`wrangler.toml` remains a template containing `<KV_NAMESPACE_ID>`; immediately
before a remote Wrangler invocation, the driver materialises that placeholder
from the ledger. `deploy` writes the ledger after each successful provision;
`dev -- --remote` reads it but never provisions.

The ledger contains no credentials or secret values. Its initial author is a
human: CI may deploy against recorded state but refuses an unrecorded KV
namespace with a "provision locally first" remedy, preventing orphaned
resources when it cannot commit the resulting state.

## Consequences

Recompiling can safely replace `.bynk/deploy/workers/*/wrangler.toml` without
losing Cloudflare identity. A second deploy reuses recorded namespaces. Future
queue, secret, and migration entries extend their own reconciliation shapes;
they are not forced into a misleading universal id map.
