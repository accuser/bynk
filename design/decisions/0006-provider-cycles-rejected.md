# 0006 — Provider dependency cycles are rejected, not lazily wired

- **Status:** Accepted (v0.12)
- **Spec:** §5.5 (`karn.provider.dependency_cycle`)

## Context
Providers' `given` clauses induce a directed graph; the composition root needs
an instantiation order. Lazy wiring (resolve at call time) would tolerate
cycles.

## Decision
**Reject cycles**, including trivial self-provision. A capability that depends
on itself is almost always a design error, and the graph must be built anyway
to order composition.

## Consequences
Cleanly-typed eager wiring; a real design smell surfaces as a diagnostic. The
check extends unchanged to external (adapter) providers.
