# 0009 — Integration tests run a simulated wire in Node, not miniflare

- **Status:** Accepted (v0.16)
- **Spec:** §7.3.5, §10

## Context
Multi-Worker integration tests must exercise the cross-Worker boundary
(serialise/deserialise, projection, boundary errors). Options ranged from
in-process bundle composition (no wire) to the real workerd runtime.

## Decision
**Simulated wire (M2)**: compile participants in workers mode, stand each up as
an in-process object behind a generated env graph — Service Bindings stubbed to
call the target's real `fetch`, Durable Objects backed by in-memory storage.
Entry and inter-participant calls travel the real serialise → JSON → deserialise
path. Plain Node + `tsc`, no new dependency.

## Consequences
The exact emission-only code paths get tested; `karnc test` stays
dependency-free. Cloudflare-runtime quirks are explicitly out of scope —
a deploy-time concern (M3 remains the post-MVP fidelity upgrade).
