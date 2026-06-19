# 0051 — `Kv` write options as distinct camelCase ops (`putTtl`)

- **Status:** Accepted (v0.23)
- **Spec:** §7.3.6

## Context
Cloudflare KV writes accept `expirationTtl` / `expiration` / `metadata`.
Bynk has no optional parameters and no overloading, so the options have
to surface as either distinct methods or an options record.

## Decision
`fn putTtl(key: String, value: String, ttlSeconds: Int) -> Effect[()]` —
a **distinct method**, passing `{ expirationTtl }` through. TTL is the
single most-used KV write option; a distinct method beats an options
record until options proliferate (the record is the escape hatch *later*
if they cluster). **camelCase** (`putTtl`, not `put_ttl`) — the language
surface is camelCase throughout; a snake_case name would have been the
first.

Deferred: absolute `expiration` and per-write `metadata`.

## Consequences
`put` stays bare and byte-stable. If `expiration`/`metadata` later
cluster, a `putWith(options)` record supersedes this in a new decision
rather than a third and fourth sibling method appearing silently.
