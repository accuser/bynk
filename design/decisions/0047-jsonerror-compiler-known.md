# 0047 — `JsonError`: a compiler-known, Bynk-inspectable record

- **Status:** Accepted (v0.22b)
- **Spec:** §5.2, §6.2, §7.3.9

## Context
`BoundaryError` is TS-runtime-only — Bynk programs never see it (workers
turn it into a 400 response). `Json.decode` returning
`Result[T, JsonError]` puts a boundary failure **in the program's hands**
for the first time, so the error must be a type the checker and runtime
both know — the `ValidationError` precedent — not a Bynk-declared sum in
a commons.

## Decision
`JsonError` is a compiler-known **record**: `kind`, `path`, `message`,
all `String`, inspectable by ordinary field access. The decode lowering
maps into it uniformly:

- a `JSON.parse` failure → `kind: "Malformed"`, `path: "$"`;
- a `BoundaryError` → its `kind` (`"StructuralMismatch"`,
  `"RefinementViolation"`, …), the **tracked field path**
  (`$.items[2].qty`), and a rendered message (`expected integer, got
  2.5`; a refinement violation surfaces its `ValidationError` message).

A uniform record was chosen over exposing `BoundaryError`'s sum shape:
Bynk-side inspection needs only field access (no built-in-sum match
machinery), and the heterogeneous `BoundaryError` variants flatten
losslessly enough for programs that branch on `kind`. Decode failures
are runtime values, never compile diagnostics. `JsonError` itself is not
codable (`bynk.types.json_uncodable`) and not declarable.

## Consequences
`match Json.decode[Order](s) { Err(e) => log(e.kind, e.path) … }` works
with no new pattern machinery. If a future increment wants typed cases,
a compiler-known sum can supersede this record in a new decision.
