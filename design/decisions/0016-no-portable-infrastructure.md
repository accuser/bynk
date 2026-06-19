# 0016 — No portable infrastructure tier; platforms are embraced as they are

- **Status:** Accepted (v0.17, superseding an interim three-tier draft)
- **Spec:** §7.3.6 (the surface's scope); informs the platform-adapter roadmap

## Context
An earlier draft proposed a portable `bynk.Kv` with selectable platform
providers — a lowest-common-denominator port over Cloudflare KV, DynamoDB, etc.

## Decision
**Dropped.** Capabilities split two ways: ambient primitives (identical
everywhere — the `bynk` surface) and infrastructure (semantics differ enough
that a portable abstraction lies about what's underneath — honest,
platform-shaped capabilities in platform adapters). Portability, where a
project genuinely needs it, is a **user-authored** abstraction adapter over the
platform adapters, choosing *that project's* lowest common denominator.

## Consequences
No selectable-provider mechanism, no foreign-capability provision rule, no
dishonest abstraction. A project's platform commitment is one greppable
`consumes` line.
