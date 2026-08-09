# 0332 — The IR and its lowering pass land inside `bynk-emit`, not as new `bynk-ir`/`bynk-lower` crates

- **Status:** Accepted (v0.247.33)

**Context.** `bynk-greenfield-compiler.md`'s Part 10 (crate graph) names `bynk-ir` ("the checker's
output; the only thing lower consumes") and `bynk-lower` (`CheckedProgram → Ir`) as crates distinct
from a narrower, `Ir`-consuming `bynk-emit` that in the target graph "depends on `bynk-ts`, NOT on
`bynk-check`." Neither crate is mentioned anywhere in `bynk-compiler-trajectory.md`'s own phase-6
narrative — its stated invariant is that `bynk-emit` names no AST type, not that `bynk-emit` stops
being the lowering site — and the trajectory's own phase-6 reference-rule list omits R10.1/R10.2
entirely, meaning severing `bynk-emit`'s `bynk-check` dependency was never one of this phase's
commitments. R10.3 states plainly that a crate is carved "when a dependency arrives that only one
consumer needs — prospectively, at the moment the dependency appears," citing `bynk-strip` as "the
control case for this entire document" precisely because it was carved for a real, present dependency
(`oxc`), not a speculative future one. Today exactly one consumer of the IR exists: `bynk-emit` itself.
The reference's own second consumer, `bynk-ts`, is trajectory phase 7 and does not exist yet.

**Decision.** The IR's core types (`IrExpr`, `IrStmt`, `IrItem`, `Callee`, `IrPat`, …) and the
`CheckedProgram → Ir` lowering pass land as new modules inside the existing `bynk-emit` crate, not as
new `bynk-ir`/`bynk-lower` crates. `bynk-emit` continues depending on `bynk-check` exactly as it does
today. Carving `bynk-ir`/`bynk-lower` out — and, at the same or a later moment, reconsidering whether
`bynk-emit` still needs `bynk-check` once the IR is fully typed and resolved — is named as phase 7's
own entry condition (`design/tracks/the-ir.md` §7), not refused.

**Consequences.** This phase's own crate-graph footprint stays minimal: no new crate, no new manifest
edges, no CI dependency-graph gate to add. The reference's own target graph is not contradicted, only
deferred — a reviewer reading Part 10 cold could otherwise reasonably expect two new crates from this
phase and be surprised by their absence; this ADR is the record that the absence is deliberate. If a
second real consumer of the IR appears before phase 7 opens (unlikely, but not impossible — an
alternative backend, a standalone IR-level linter), that is new evidence for revisiting this ADR's
scope under its own review, not grounds to quietly build the split without one.
