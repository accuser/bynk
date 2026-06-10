# 0021 — Secrets reads an optional injected env, falling back to a globalThis probe

- **Status:** Accepted (v0.18)
- **Spec:** §7.3.6

## Context
`karn.Secrets` needs Worker env vars on Cloudflare and `process.env` on Node;
bundle-target compose threads no env, and the tsc gate must stay free of
`@types/node`.

## Decision
First-party metadata flags env-taking providers (only `SecretsProvider`). Both
platforms' class takes `constructor(private env?: unknown)`; lookup order is
explicit env, then `(globalThis as any).process?.env`. The workers compose
passes `env`; the bundle compose passes nothing. `unknown` rather than a record
type — the emitted `Env` interface has no index signature.

## Consequences
Clock/Random/Logger stay no-arg (prior fixtures byte-stable). The metadata hook
is what platform-adapter resource derivation extends.
