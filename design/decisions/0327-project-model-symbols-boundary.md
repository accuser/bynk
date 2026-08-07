# 0327 — `bynk-project`'s module boundary is a five-part test plus a composite rule, not "no literal `bynk_check` import"

- **Status:** Accepted (v0.247.23)

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
alone, 7 more in `validate.rs`) and appears by name in five files, only three of them imports: its own
definition, `project.rs`, `validate.rs` — the other two (`bynk-check/src/index.rs`, `bynkc/tests/
deterministic_diagnostic_order_behaviour.rs`) are prose mentions, not imports — every reader stays behind,
the same as `Mode`. `ProjectFailure` is `compile_project`'s own failure-return type, consumed only by
callers *above* `bynk-emit` (`bynk-driver`'s `print_project_failure`/`print_project_failure_short`,
`bynkc`'s own re-export and flattener comment, and the `bynkc`/`bynk` test suites) — `bynk-render/src/lib.rs`'s
own doc comment states `AttributedError`/`ProjectFailure` "live in `bynk-emit`" and that `bynk-render` "must
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
the same cost at larger scale: `pub(crate)` fields, read directly by `symbols.rs` (seven `.identity_path`
reads, three through `pf`) and `validate.rs` (dozens of field accesses), both staying behind — the
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
