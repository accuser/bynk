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
title: `bynk-project`'s module boundary is a five-part test plus a composite rule, not "no literal `bynk_check` import"
summary: `symbols.rs` and `schema_registry.rs`'s `reconcile` fail on checking coupling; `Mode`/`ErrorSink`/`ProjectFailure` and `paths.rs`'s `json_string` pass that test but fail differently; `ContextBoundaryInfo` fails on provenance despite passing every needs-based test; `ProjectAnalysis` is a composite that resolves upward instead

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
check out clean on the same closer read; so does `discovery.rs`.

`diagnostics.rs` was never assigned a home by the first pass at all. Of its seven top-level items, four
have no `bynk_check` anywhere (`Mode`, `AttributedError`, `ErrorSink`, `ProjectFailure`) — a second pass
grouped all four as "plain bookkeeping" on that ground alone, which a third pass found three-quarters
wrong: `bynk_check`-free is necessary but not sufficient, and only `AttributedError` actually needs to
move. `ErrorSink` is the collection-point sink `run_checks`/`check_unit_files` thread through nearly every
internal function that can raise a diagnostic (15 `errors: &mut ErrorSink` parameters in `project.rs`
alone, 7 more in `validate.rs`) and appears in exactly three files: its own definition, `project.rs`,
`validate.rs` — every reader stays behind, the same as `Mode`. `ProjectFailure` is `compile_project`'s own
failure-return type, consumed only by callers *above* `bynk-emit` (`bynk-driver`'s
`print_project_failure`/`print_project_failure_short`, `bynkc`'s tests) — `bynk-render/src/lib.rs`'s own
doc comment states `AttributedError`/`ProjectFailure` "live in `bynk-emit`" and that `bynk-render` "must
never see" them, further evidence `ProjectFailure` belongs exactly where it is. `AttributedError` is the
one that moves, and not because it's bookkeeping: the companion `project-model-analysis-entry-point` ADR's
new entry point returns "the analogue of `ProjectAnalysis`," whose `errors` field is
`Vec<AttributedError>`, so that entry point — living in `bynk-check`, which cannot depend on `bynk-emit` —
has to construct `AttributedError` itself. It moves for a layering reason its return type creates, not
because nothing needs it to stay.

`ContextSequenceInfo` carries `resolver::CrossContextInfo`, a clean checker output.
`ContextBoundaryInfo` is a different case again: its three fields (`types`, `services`, `agents`) are
`bynk_syntax::ast` types, not checker outputs — by field-content alone it reads as a `bynk-project`
candidate. It stays on the checking side because of who builds it, not what it needs: the values populating
it are assembled during `run_checks`'s `Checked` arm (`project.rs:1104`) from `combined_types_for`/
`unit_tables`, tables that exist only after resolution. `discovery.rs` has nothing that could construct
one.

`ProjectAnalysis` is not the same shape as those two. It has twelve fields, not the six an earlier pass of
this doc enumerated before generalising "every field a checker output" — false as stated. `index`, `hints`,
`expr_types`, `ty_intern` (`Arc<bynk_check::checker::Types>`), `locals` and `requirements` are checker
outputs. But `snapshots: Vec<(PathBuf, String)>` is, by its own doc comment, "for every file read,
including clean files" — a discovery output, not a checking one. `errors: Vec<AttributedError>` holds the
exact type this ADR sends to `bynk-project` as bookkeeping. `unit_sources` is a unit→file map "in discovery
order" (R3.7's territory); `doc_scope` is built from the `uses`/`consumes` edges. `ProjectAnalysis` is a
composite of project facts and checker outputs, not a homogeneous checker-output struct — it stays on the
checking side because only something above both `bynk-project` and `bynk-check` (the companion
`project-model-analysis-entry-point` ADR's new entry point) can assemble a value with both kinds of field,
not because every field independently demands it. That composite shape is itself a constraint on the new
entry point's return type, not just on where `ProjectAnalysis` the type lives: it has to surface
`bynk-project`-shaped data (`snapshots`, `unit_sources`) alongside `bynk-check`-shaped data, which
"the analogue of `ProjectAnalysis`" was already committing to without saying so.

`Mode`'s use sites split across `check_unit_files` (`project.rs:3210`, param `:3230`, branches
`:3476,3495,3539,3556`) and `run_checks` (`:3644`, param `:3654`, branch `:3961`) — `run_checks` calls
`check_unit_files`, threading `mode` down into the per-unit check-and-emit loop, not just the top-level
orchestrator. None of `discovery.rs`/`graph.rs`/`paths.rs`/`consistency.rs` reference it, and the new
`bynk-check` entry point is scoped to `Mode::Analyse`'s behaviour specifically, so it has no branch to take
either.

Three more findings, each in a direction the coupling test above didn't check. `paths.rs:3` reads
`use crate::json::json_string;`; `json_string` (`bynk-emit/src/json.rs:28`) is `pub(crate)` to `bynk-emit`
— not a checking output, so it passes the `bynk_check`-coupling test, but it's still something
`bynk-project` (below `bynk-emit`) cannot depend on. Splitting `schema_registry.rs` costs something in the
reverse direction: `SchemaRegistry`'s fields (`version`, `events`), `EventEntry`, `FieldShape` and
`lock_version()` are all private, and both `parse` (`:411`) and `reconcile` (`:233`) construct
`SchemaRegistry` directly through them — once `reconcile` stays on the checking side and
`SchemaRegistry`/`parse`/`serialize` move, either those privates become real `bynk-project` API or
`bynk-project` grows a builder surface for `reconcile` to use instead. `ParsedFile` (`discovery.rs:80`) has
the same cost at larger scale: `pub(crate)` fields, read directly by `symbols.rs` (eight
`pf.identity_path` sites alone) and `validate.rs` (dozens of field accesses), both staying behind — the
same either/or `schema_registry.rs` needs, across two files instead of one function.

**Decision.** `bynk-project` receives `discovery.rs` (plus `ParsedFile`'s private-field question resolved
the same way), `graph.rs`, `paths.rs` (plus `json_string`, relocated/duplicated/inlined), `consistency.rs`;
`schema_registry.rs`'s `SchemaRegistry` type plus `parse`/`serialize` only, with its own private-field
question resolved too; and, from `diagnostics.rs`, only `AttributedError` — plus the project-model types
these depend on. `Mode`, `ErrorSink` and `ProjectFailure` all stay in `bynk-emit`: the first two with
`run_checks`/`check_unit_files`, their only consumers; `ProjectFailure` because nothing below `bynk-emit`
reads it. Staying on the checking side, to become part of the companion `project-model-analysis-entry-point` ADR's
new `bynk-check` entry point rather than `bynk-project`: `symbols.rs` in full, `schema_registry.rs`'s
`reconcile`, `Mode`/`ErrorSink`/`ProjectFailure`, and `diagnostics.rs`'s `ProjectAnalysis`/
`ContextSequenceInfo`/`ContextBoundaryInfo`. The test going forward: a type or function moves to
`bynk-project` only if (a) it needs nothing that exists solely as an output of resolution or checking,
however that output is named at the use site — not "no literal `bynk_check` import," which misses coupling
reached through an unqualified name; (b) it needs nothing from `bynk-emit` for non-checking reasons either
(`json_string`); (c) it is a fact about the project, not merely "nothing above the checking pipeline needs
it to stay" (`Mode`, `ErrorSink`) or "consumed only by callers above this crate" (`ProjectFailure`); (d)
nothing staying behind needs to reach back into its now-private internals (`schema_registry.rs`,
`ParsedFile`); and (e) it is what actually builds the value, not merely what the value's fields need
(`ContextBoundaryInfo` passes (a)–(d) and still stays, because only `run_checks` can construct one). A
composite item — part project fact, part checker output, like `ProjectAnalysis` — resolves upward to
whatever sits above both crates rather than being tested against (a)–(e) at all.

**Consequences.** `bynk-project` has no dependency on `bynk-check`, preserving the "below both check and
emit" invariant this phase exists to establish. Every checking-coupled item — `symbols.rs`,
`schema_registry.rs::reconcile`, `Mode`/`ErrorSink`/`ProjectFailure`, and `diagnostics.rs`'s three
checking-result types — relocates alongside the rest of the checking pipeline the companion ADR moves into
`bynk-check`, rather than needing a second, separate migration later. `ProjectAnalysis` in particular is
already the return type that ADR commits the
new entry point to producing "the analogue of," so this assignment was implied before it was explicit.

## ADR: project-model-analysis-entry-point
title: A new `bynk-check` analysis entry point closes R10.2 without moving `run_checks` or checking itself
summary: `bynk-ide` gets a narrow `bynk-project` + `bynk-check`-only analysis path; `run_checks` stays in `bynk-emit`, serving compilation/emission alone, with the resulting duplication named as phase 5's to remove

**Context.** `bynk-ide/Cargo.toml`'s own comment names the reason it depends on `bynk-emit`:
`analyse_project`, "the non-bailing project analysis," lives there. Tracing `analyse_project_with`
(`bynk-emit/src/project.rs:970`) found it calls `run_checks` (`:3644`, private to `bynk-emit::project`) —
the same function `compile_project` (`:573`) calls for the CLI/emission path (`:584`). `run_checks`
(`:3644-4206`, ~560 lines) performs discovery, parsing, resolution *and* checking as one sequence; there is
no existing seam inside it separating "project model" from "checking" at the granularity phase 4 needs —
a structural fact independent of the function's size.
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
unchanged, serving `compile_project`/emission alone. `ProjectAnalysis` itself is a composite of discovery
outputs (`snapshots`, `unit_sources`, `doc_scope`) and checker outputs (`index`, `hints`, `expr_types`,
`ty_intern`, `locals`, `requirements`, `sequence_info`, `boundary_info`) — see the companion
`project-model-symbols-boundary` ADR — so this entry point's return type has to surface both kinds, not
just checker output; "the analogue of `ProjectAnalysis`" means composing `bynk-project`-sourced data with
`bynk-check`-sourced data, not producing a purely `bynk-check`-shaped value.

**Consequences.** This is a deliberate, temporary duplication: the new `bynk-check` entry point and
`run_checks`'s `Mode::Analyse` arm do overlapping work until phase 5 centralises checking in `bynk-check`,
at which point `bynk-emit`'s CLI path calls the same entry point and `run_checks`'s checking half is
deleted rather than ported. The alternative — doing phase 5's centralisation now to avoid the duplication —
is explicitly out of phase 4's scope and would decide `validate.rs`'s new home under a much smaller
review budget than that decision deserves. The duplication is named here specifically so phase 5 inherits
it as known, bounded debt rather than rediscovering it as a surprise. This is the most load-bearing and
hardest-to-reverse of this settling pass's three decisions: it fixes the shape `bynk-ide`'s live analysis
path takes for the phase-4-to-phase-5 window. The direct edit is one call site
(`bynk-ide/src/lib.rs:320`, behind the stable `diagnose_project`/`diagnose_project_with` wrapper); 85
`diagnose_project(` call sites across the tree (87 raw matches include the function's own two definitions)
exercise this path without naming it, which is a coverage argument for the relocation, not a statement
that 85+ sites need editing.
