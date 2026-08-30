---
level: patch
changelog: Internal — a typed `ProjectGraph` (P8.3) lands in `bynk-check`, populated from resolved discovery/uses/consumes facts; data-model half only, not yet wired into cycle detection or the compose-root generator.
---

## ADR: p8-3-project-graph-shape-and-placement
title: P8.3's `ProjectGraph` — deferred `contract`, hand-rolled maps, dual `Provides` edges, and why it lives in `bynk-check`
summary: Five decisions settling ADR 0326's phase-8 deferral for the typed project graph

**Context.** ADR 0326 (phase 4) deferred the reference's typed `ProjectGraph` (`design/bynk-greenfield-compiler.md`
§3.2 — `units: IndexVec<UnitId, Unit>`, `files: IndexVec<FileId, UnitId>`, `edges: Vec<(UnitId, UnitId,
EdgeKind)>`, `contract: IndexVec<UnitId, ContractHash>`) to phase 8, noting the shape "conflicts with a
project-model layer meant to sit below both `bynk-check` and `bynk-emit`" if it ever needed resolved,
post-type-checking facts. Phase 8 opened and settled (#1507–#1509), and P8.1 (#1512, PR #1517) landed
`UnitId` as a `String`-wrapping newtype and `UnitSignature` in `bynk-check::unit_signature` — not
`bynk-project`, since P8.1's own builder needs `UnitTable`, a `bynk-check`-only concept. P8.3 (#1514) is
the slice that builds `ProjectGraph` itself, keyed by that same `UnitId`.

**Decision.**

1. **No `contract` field.** No `ContractHash` type exists anywhere in the tree — `contract.rs` (ADR 0200)
   produces a canonical `String` rendering, never a hashed, `UnitId`-keyed value — and nothing in this
   phase's own settled scope (ADR 0412/0413/0414) names a consumer for one. Building `ContractHash` here
   would be new, unscoped work. The field is omitted entirely, a forward reference rather than a fake
   placeholder.

2. **No `IndexVec`; `units`/`files` are `HashMap`s, not integer-indexed `Vec`s.** No `index_vec` crate (or
   equivalent) exists in this workspace — the same posture P8.1's own Decision A already took, extended
   here. This also resolves the fork P8.1's Decision A explicitly left open ("whether `ProjectGraph`
   adapts to a string-keyed `UnitId` instead" of widening it to a dense integer): it does. `UnitId` is a
   `String`, so a `Vec` indexed by its own bytes was never available regardless of `IndexVec` — a
   `HashMap<UnitId, Unit>` (and the reverse `HashMap<FileId, UnitId>`) is the direct, no-new-infrastructure
   translation of the reference's intent.

3. **All three `EdgeKind` variants are built (`Uses`, `Consumes`, `Provides`), and `Provides` is the
   structural dual of `Consumes`.** `uses`/`consumes` are real, already-resolved facts
   (`project_model.rs`'s `phase_resolve_uses`/`phase_resolve_consumes`). No separately-resolved
   inter-unit "provides" fact exists anywhere in the tree — `phase_validate_providers` diagnoses per-unit
   provider/capability signature consistency, not an edge between two units, and
   `graph.rs::detect_provider_dependency_cycles` operates on intra-context capability-name edges, not
   `UnitId`-to-`UnitId` ones. Rather than inventing new resolution logic no other consumer needs yet,
   `Provides` is derived directly from `Consumes`: whenever unit A consumes from unit B, B provides to A.
   This is a real correction to this issue's own initial framing, which asserted "provider resolution" as
   an already-resolved fact without naming a concrete function that produces inter-unit edges — verified
   fresh against the live tree before implementing, not assumed from the issue text.

4. **Cycle detection (`graph.rs`) and the compose-root generator (R8.16, `bynk-emit`) are left untouched.**
   Data-model half only, per `design/tracks/incrementality.md` §6. Migrating either is real, separate work
   with its own regression surface, not named as this track's job in §3/§6.

5. **`ProjectGraph` and its builder live in `bynk-check::project_graph`, not `bynk-project` as this issue's
   own "deltas" section first proposed.** Two real blockers, both confirmed against the live `Cargo.toml`s
   and source before implementing (the issue's own grounding never checked either): `bynk-project` cannot
   depend on `bynk-check` — the crate graph runs the other way (`bynk-check/Cargo.toml` depends on
   `bynk-project`, not the reverse) — so `bynk-project` could never reach P8.1's `UnitId`. And the
   `uses`/`consumes` edges this builder needs are resolved facts computed by `bynk-check::project_model`'s
   own phases; `bynk-project::discovery` only parses files, it never resolves a cross-unit reference.
   Moving either `UnitId` or the resolution phases down to `bynk-project` would be a much larger, riskier
   refactor than this slice's own scope. Keeping `ProjectGraph` beside `UnitTable`/`UnitSignature` (which
   already reuse `bynk-project`'s own `UnitKind`/`ParsedFile`) is the minimal, honest fix — and directly
   realises the tension ADR 0326 itself flagged three years earlier ("a project-model layer meant to sit
   below both `bynk-check` and `bynk-emit`" conflicting with resolved, post-parse facts).

**Consequences.** `cargo xtask greenfield-status`'s `incremental_query_types` probe originally searched
only `bynk-project/src` for `struct ProjectGraph` (mirroring the issue's own assumed location); fixed
alongside this slice to search `bynk-check/src` too, the same "search both crates" shape already used for
P8.5's `body`/`type_of` query functions, with a regression test pinning the real landed location. A
fidelity fixture (`bynk-check/tests/project_graph_fidelity.rs`) runs the real discovery→parse→group→resolve
pipeline against a three-unit project and asserts `ProjectGraph`'s edges agree exactly with the resolved
`unit_uses`/`unit_consumes` maps — not a hand-built map that could paper over a translation bug. If a
future slice needs `ContractHash` or migrates `graph.rs`/R8.16 onto `ProjectGraph`, that is new work with
its own trigger and its own proposal, not retrofitted here on spec.
