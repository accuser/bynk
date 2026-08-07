# 0326 — Phase 4 extracts today's project-model shape; the typed `ProjectGraph` and contract hashing defer to phase 8

- **Status:** Accepted (v0.247.23)

**Context.** The reference (`design/bynk-greenfield-compiler.md` §3.2) specifies a typed
`ProjectGraph { units: IndexVec<UnitId, Unit>, files: IndexVec<FileId, UnitId>, edges: Vec<(UnitId,
UnitId, EdgeKind)>, contract: IndexVec<UnitId, ContractHash> }` as part of "the project model." Phase 4
of the compiler trajectory (`design/bynk-compiler-trajectory.md`) needs to decide how much of that shape
it commits to.

No `UnitId`, `ProjectGraph`, or `IndexVec`-keyed graph exists anywhere in the tree today. What exists is
`bynk-emit/src/project/graph.rs`'s cycle detection, keyed by plain unit-name strings in a `HashMap`. The
only `ContractHash`-shaped type in the tree, `bynk-check/src/contract.rs` (ADR 0200's cross-context
wire-contract hash), is necessarily downstream of type-checking — it canonicalises resolved types — which
conflicts with a project-model layer meant to sit below both `bynk-check` and `bynk-emit`.

Phase 4's own reference-rule list (R3.7, R3.8, R3.9, R3.11, R10.2) never cites R3.2, the rule that defines
`ProjectGraph`. Neither does phase 5's, 6's, or 7's rule list. Only phase 8's rules (R3.13–R3.15, query
granularity) name the kind of stable per-unit identity `ProjectGraph`/`UnitId` would provide, and the
trajectory's own phase-8 description names `UnitSignature(UnitId)` and `ProjectGraph` together as that
phase's machinery. The trajectory's own aside about phase 8 — "ADR 0200's contract hash is already
`UnitSignature`'s identity function pointed at a different problem" — reads as a direct statement that the
`contract` field belongs to phase 8's `UnitSignature` concept, not phase 4's.

**Decision.** Phase 4 extracts today's name-keyed discovery, unit-graph, manifest-parsing and
schema-registry logic into a new `bynk-project` crate, relocated with minimal reshaping. It does not build
the reference's typed `ProjectGraph` struct, `UnitId`, or any `ContractHash`-bearing field. That full
shape is phase 8's, gated on phase 8 opening in turn.

**Consequences.** Phase 4's own completion probe (`bynk-ide` → `bynk-emit` edge absent) is achievable
without designing a pre-resolution contract hash or committing to an `IndexVec`-keyed graph now. Phase 8,
when it opens, inherits the full `ProjectGraph`/`UnitId`/`ContractHash` design work in one place rather
than finding half of it already built to a shape phase 8's own query-granularity needs might not have
chosen. If a later phase finds it needs typed unit identity before phase 8 opens, that is grounds to
revisit this decision under its own review, not to have silently pre-built it here.
