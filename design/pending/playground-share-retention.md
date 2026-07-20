---
level: patch
changelog: The playground's share service expires stored snippets 30 days after creation, via `Kv.putTtl`, instead of retaining them indefinitely.
---

## ADR: playground-share-retention
title: Playground share links expire 30 days after creation
summary: Bound the share service's KV storage with a fixed-TTL retention policy instead of indefinite retention

**Context.** The playground's share service (`playground/share/`, in-browser track slice 5c) stores each shared snippet in Cloudflare KV under a random id, with no expiry — `Kv.put`, not `Kv.putTtl`. Issue #398 deferred a richer gist-style upgrade (stable shortlinks, listing, retention, edit history) but flagged retention/abuse bounds specifically as worth addressing on its own: an unbounded namespace is unbounded storage cost and an unbounded abuse surface (anyone can POST arbitrary — if size-bounded — text forever).

**Decision.** Store snippets with a fixed 30-day TTL from creation (`Kv.putTtl(keyOf(id), body.source, snippetTtlSeconds())`), not refreshed on read. `Kv.putTtl` already existed in the `bynk.cloudflare` adapter (v0.23, decision 0051) and lowers to Cloudflare's native `expirationTtl`, so this is config, not new capability surface. A **sliding** window (bumping the TTL on every `GET`) was considered and rejected: it would mean a frequently-viewed link never expires, which defeats the bound the policy exists to provide. A fixed window from creation is simpler and matches the share link's purpose — showing someone a program now, not permanent hosting.

**Consequences.** A share link stops resolving (`404`) 30 days after it was created, regardless of how often it was viewed. The stable-shortlink, listing, and edit-history parts of #398 remain open/deferred; this closes only the retention/abuse-bound note. No schema or endpoint change — same KV shape, same two handlers.
