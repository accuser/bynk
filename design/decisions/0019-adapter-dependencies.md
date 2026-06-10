# 0019 — Adapters depend on adapters via braced `consumes` and external `given`

- **Status:** Accepted (v0.18)
- **Spec:** §4.1.13, §5.5, §5.8, §7.3.6

## Context
Library adapters need the ambient surface (a JWKS fetch, a signing secret).
Adapters had no dependency mechanism; external providers were constructed
no-arg.

## Decision
Adapters gain `consumes` restricted to the **capability-selection form**
(adapters have no services to call) and to **adapter targets** (never a
context). An external provider's `given` is wired through the same by-name
deps object as bodied providers (0005), recursively — compose imports the
transitive closure of adapter bindings. Cycles fall to the existing
consumes-cycle check; no adapter-specific mechanism.

## Consequences
The binding author's contract is `constructor(private deps: { Cap: Cap })`,
checked by `tsc`. The closure walk doubles as the future lock-propagation walk
(0017).
