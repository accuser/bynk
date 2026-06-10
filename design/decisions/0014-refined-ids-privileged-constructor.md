# 0014 — Boundary IDs are refined types; bindings construct them through `.of`

- **Status:** Accepted (v0.17)
- **Spec:** §6.5

## Context
A binding generates values (e.g. `Random.uuid()`) the rest of the program
trusts as validated. TypeScript checks the brand but never the refinement
predicate, so a raw cast would mint unvalidated "validated" values invisibly.

## Decision
ID-like results are **refined types** (`Uuid`), and a binding constructs them
**only through the emitted validating `.of`**, handling the `Result` and
treating an unreachable `Err` as a bug — even when the binding is a trusted
generator. Raw casts and `.unsafe` are disallowed by convention; the same
emitted-constructors rule covers sums (`T.Variant`) and `Result`/`Option`
(`Ok`/`Err`/`Some`/`None`).

## Consequences
The predicate runs as defence-in-depth at the host boundary. Bindings never
couple to the emitter's internal ADT lowering.
