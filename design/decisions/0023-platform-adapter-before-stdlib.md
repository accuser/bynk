# 0023 — The Cloudflare adapter lands before the standard library

- **Status:** Accepted (post-v0.18 re-plan)
- **Spec:** roadmap (versioning & roadmap; changelog)

## Context
Both the first platform adapter (`cloudflare.Kv`/`Queue`) and the functional
core (`List`/`Map`, stdlib) are next. Original sequencing had the platform
slice immediately follow v0.18.

## Decision
**Platform adapter first**: a minimal `Kv` (get/put/delete) is collection-free
— it needs only `String`/`Option`/`Effect`, which exist. Each increment stays
single-purpose: language/stdlib work and adapter work never share an
increment. Sequence: v0.19 Kv + lock enforcement; v0.20 collections + the
language-generality call; v0.21 wider stdlib; v0.22 extend cloudflare
(`Kv.list`, JSON values, `Queue`).

## Consequences
Lock enforcement becomes exercisable (cloudflare vs node) without waiting on
collections; Fetch's header compromise (0022) is retired by v0.20; the adapter
extension increment is isolated from stdlib churn.
