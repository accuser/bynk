# 0017 — Platform lock is per deployment unit, propagating along given edges

- **Status:** Accepted (v0.17 design; enforcement live since v0.19)
- **Spec:** §5.8 (`karn.target.vendor_required`/`vendor_conflict`)

## Context
Consuming a platform-native capability (KV, Durable Objects) commits code to
that platform. The commitment's scope had to be precise.

## Decision
Lock is local to a **deployment unit** — the context under `--target workers`
(cross-context `consumes` is RPC and does not propagate lock), the whole
program under `bundle` (co-location locks the shared bundle). Lock propagates
along `given`/capability edges, which are in-process. Platform-native runtime
bindings lock; remote-API library adapters (S3 over HTTPS) do not.

## Consequences
A context's lock under workers is exactly its own `consumes` lines. The
transitive given-closure walk built for compose imports is the
lock-propagation primitive.
