---
level: patch
changelog: Settle phase 4 of the compiler trajectory (`design/tracks/project-model.md`) — `bynk-project` extracts today's project-model logic below both `bynk-check` and `bynk-emit`; contract hashing and the typed `ProjectGraph` defer to phase 8; a new `bynk-check` analysis entry point closes R10.2 without moving `run_checks` early
---

## ADR: project-model-phase4-scope
title: Phase 4 extracts today's project-model shape; the typed `ProjectGraph` and contract hashing defer to phase 8
summary: `bynk-project` carries today's name-keyed discovery/graph/paths/schema-registry logic, not the reference's `IndexVec<UnitId,...>` struct or a `ContractHash` field

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

## ADR: project-model-symbols-boundary
title: `bynk-project`'s module boundary is "needs no output of resolution or checking," not "no literal `bynk_check` import"
summary: `symbols.rs`, `schema_registry.rs`'s `reconcile`, and three of `diagnostics.rs`'s seven items are `bynk-check`-owned concerns reached through unqualified names a naive import grep misses

**Context.** Phase 4 needs to decide which of `bynk-emit/src/project.rs`'s nine sibling modules
(`consistency.rs`, `diagnostics.rs`, `discovery.rs`, `graph.rs`, `paths.rs`, `schema_registry.rs`,
`symbols.rs`, plus `validate.rs` and `tests_emit.rs`, excluded on other grounds) move into the new
`bynk-project` crate.

`bynk-check/src/index.rs` already defines `ProjectIndex` (line 302) and `IndexBuilder` (line 430) — both
`bynk-check`-owned types, not `bynk-emit`'s. `bynk-emit/src/project/symbols.rs::assemble_index` imports
both from `bynk_check::index` and exists only to walk parsed files and populate that checker-owned type.
`bynk-ide` carries the result directly (`pub index: index::ProjectIndex`, `bynk-ide/src/lib.rs:156`).

A first pass tested the remaining modules for a literal `bynk_check` import and found none in
`discovery.rs`, `graph.rs`, `paths.rs`, `schema_registry.rs` or `consistency.rs`. That test is too weak:
`schema_registry.rs:35` reads `use super::UnitTable;`, and `reconcile` (`:228-230`) takes
`unit_tables: &HashMap<String, UnitTable>`. `UnitTable` (`symbols.rs:284`) is itself `bynk_check`-coupled
via `methods: HashMap<String, ResolverMethodTable>`, an alias (`project.rs:38`) for
`bynk_check::resolver::MethodTable` — the coupling arrives through an unqualified, glob-sourced name, which
a literal-string grep for `bynk_check` cannot see. `schema_registry.rs`'s other two functions, `parse`
(`:409`) and `serialize` (`:446`), take no `UnitTable` and stay clean. `graph.rs` and `consistency.rs`
check out clean on the same closer read.

`diagnostics.rs` was never assigned a home by the first pass at all. Of its seven top-level items,
`Mode`, `AttributedError`, `ErrorSink` and `ProjectFailure` are plain bookkeeping with no `bynk_check`
anywhere. `ProjectAnalysis` carries `pub ty_intern: Arc<bynk_check::checker::Types>` plus `ProjectIndex`,
`FileHints`, `FileExprTypes`, `FileLocals`, `FileRequirements` — every field a checker output.
`ContextSequenceInfo` carries `resolver::CrossContextInfo`. `ContextBoundaryInfo` is AST-typed but built
during `run_checks`'s `Checked` arm from `combined_types_for`/`unit_tables`, the same checking pass, not
discovery. Being `bynk_check`-free isn't the whole test, though: `Mode`'s every use site — `run_checks`'s
own parameter and match arms, and all of its callers — is inside `project.rs` itself; none of
`discovery.rs`/`graph.rs`/`paths.rs`/`consistency.rs` reference it, and the new `bynk-check` entry point
(below) is scoped to `Mode::Analyse`'s behaviour specifically, so it has no branch to take either. `Mode` is
a statement about how `run_checks` is driven, not a fact about the project.

**Decision.** `bynk-project` receives `discovery.rs`, `graph.rs`, `paths.rs`, `consistency.rs`;
`schema_registry.rs`'s `SchemaRegistry` type plus `parse`/`serialize` only; and `diagnostics.rs`'s
`AttributedError`, `ErrorSink`, `ProjectFailure` — plus the project-model types these depend on. `Mode`
stays with `run_checks` in `bynk-emit`, its only consumer. Staying on the checking side, to become part of
the companion `project-model-analysis-entry-point` ADR's new `bynk-check` entry point rather than
`bynk-project`: `symbols.rs` in full, `schema_registry.rs`'s `reconcile`, and `diagnostics.rs`'s
`ProjectAnalysis`/`ContextSequenceInfo`/`ContextBoundaryInfo`. The test going forward is two-sided: a type
or function moves to `bynk-project` only if (a) it needs nothing that exists solely as an output of
resolution or checking, however that output is named at the use site — not "no literal `bynk_check`
import," which misses coupling reached through an unqualified name — and (b) something below `run_checks`
actually consumes it, not merely "nothing above `run_checks` needs it to stay," which `Mode` would
otherwise pass for the wrong reason.

**Consequences.** `bynk-project` has no dependency on `bynk-check`, preserving the "below both check and
emit" invariant this phase exists to establish. Every checking-coupled item — `symbols.rs`,
`schema_registry.rs::reconcile`, and `diagnostics.rs`'s three checking-result types — relocates alongside
the rest of the checking pipeline the companion ADR moves into `bynk-check`, rather than needing a second,
separate migration later. `ProjectAnalysis` in particular is already the return type that ADR commits the
new entry point to producing "the analogue of," so this assignment was implied before it was explicit.

## ADR: project-model-analysis-entry-point
title: A new `bynk-check` analysis entry point closes R10.2 without moving `run_checks` or checking itself
summary: `bynk-ide` gets a narrow `bynk-project` + `bynk-check`-only analysis path; `run_checks` stays in `bynk-emit`, serving compilation/emission alone, with the resulting duplication named as phase 5's to remove

**Context.** `bynk-ide/Cargo.toml`'s own comment names the reason it depends on `bynk-emit`:
`analyse_project`, "the non-bailing project analysis," lives there. Tracing `analyse_project_with`
(`bynk-emit/src/project.rs:970`) found it calls `run_checks` (`:3644`, private to `bynk-emit::project`) —
the same function `compile_project` (`:573`) calls for the CLI/emission path (`:584`). `run_checks` is
~2,200 lines performing discovery, parsing, resolution *and* checking as one sequence; there is no
existing seam inside it separating "project model" from "checking" at the granularity phase 4 needs.
`bynk-ide`'s real dependency on `bynk-emit` is therefore not a dependency on a relocatable discovery
function — it is a dependency on a function that also checks, which the new `bynk-project` crate (sitting
below `bynk-check`) cannot absorb without breaking the layering phase 4 exists to establish, and which
moving to `bynk-check` in full is phase 5's job (R3.5), not phase 4's, and larger than phase 4's review
budget.

**Decision.** Phase 4 does not move `run_checks`. It adds one narrow entry point in `bynk-check` —
`bynk-check`'s natural long-term home under R3.5 regardless — performing the same
discovery(`bynk-project`)→parse→resolve→check(`bynk-check`, already local) sequence `run_checks`'s
`Mode::Analyse` arm performs today, returning what `bynk-ide` needs in place of today's `ProjectAnalysis`.
`bynk-ide` calls this instead of `bynk-emit::analyse_project`. `run_checks` stays in `bynk-emit`,
unchanged, serving `compile_project`/emission alone.

**Consequences.** This is a deliberate, temporary duplication: the new `bynk-check` entry point and
`run_checks`'s `Mode::Analyse` arm do overlapping work until phase 5 centralises checking in `bynk-check`,
at which point `bynk-emit`'s CLI path calls the same entry point and `run_checks`'s checking half is
deleted rather than ported. The alternative — doing phase 5's centralisation now to avoid the duplication —
is explicitly out of phase 4's scope and would decide `validate.rs`'s new home under a much smaller
review budget than that decision deserves. The duplication is named here specifically so phase 5 inherits
it as known, bounded debt rather than rediscovering it as a surprise. This is the most load-bearing and
hardest-to-reverse of this settling pass's three decisions: it fixes the shape `bynk-ide`'s live analysis
path takes, across 100+ existing call sites, for the phase-4-to-phase-5 window.
