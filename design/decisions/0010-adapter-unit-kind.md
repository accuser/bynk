# 0010 — Adapters are a distinct unit kind, containing contracts but no logic

- **Status:** Accepted (v0.17)
- **Spec:** §4.1.6, §4.1.13, §5.8

## Context
Host-backed capabilities (time, IO, npm libraries) need an implementation
escape hatch, but allowing host access in any context would pierce the
language's core property — user source is pure and safe by construction.

## Decision
A new unit kind, keyword **`adapter`**: capability contracts, boundary types,
inline pure helpers and `uses`, `exports`, and **external** providers — no
services, agents, or bodied providers. Pure helpers do not weaken containment
(they cannot touch the host); the host boundary is exactly the named binding.
Naming convention: library adapters by capability (`tokens`), platform adapters
by vendor (`cloudflare`), the reserved surface as `bynk`.

## Consequences
External providers are legal only inside adapters; ordinary contexts remain
provably host-free. The boundary is greppable: `adapter` plus its named
binding. The "external" marker dissolves — bodilessness inside an adapter *is*
the signal.
