# The project model as a phase — migrating to the greenfield reference (phase 4)

- **Status:** **Settled — Slicing on merge.** §3's six questions were argued under a settling review on
  this branch. One of them (Q3) surfaced a finding the original draft didn't anticipate and changed shape
  substantially as a result — see the provenance note at the head of §3. Merging settles **direction**; it
  is not a build authorisation. Each slice is still an ordinary increment proposal, and `accepted` on that
  sub-issue is the approval to build.
- **Spine:** [#1107](https://github.com/accuser/bynk/issues/1107)
- **Theme:** **Phase 4** of [`../bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md) —
  discovery, the unit graph and the schema registry live below both `bynk-check` and `bynk-emit`, in
  their own crate; `bynk-ide` repoints at it and drops its `bynk-emit` edge via a new `bynk-check`
  analysis entry point. (The trajectory's own phase-4 section named contract hashes here too; §3.4
  below settles that as phase 8's, and the trajectory doc is corrected to match.) The trajectory's
  endpoint is the current compiler rebuilt on
  [`../bynk-greenfield-compiler.md`](../bynk-greenfield-compiler.md); this track walks phase 4 of it.
- **Phase boundaries are safe stopping points** (trajectory §2). Phase 3 is retired
  ([#1046](https://github.com/accuser/bynk/issues/1046)) and leaves a coherent compiler regardless of
  whether this phase lands.
- **Relates:** reference rules R3.7, R3.8, R3.9, R3.11, R10.2; [ADR 0200](../decisions/0200-cross-context-contract-hash.md)
  (the only `ContractHash`-shaped type in the tree — §3.4 finds it is phase 8's business, not this phase's);
  [ADR 0147](../decisions/0147-structural-test-ness-and-flat-paths.md) (the flat-`include` manifest schema
  `Roots` still partially models the pre-0147 shape of, per R3.9);
  [`../archive/retired-tracks.md`](../archive/retired-tracks.md)'s `identity-and-totality.md` closing
  entry (the passage that names this phase as what phase 3's retirement opens).
- **Precedent:** `identity-and-totality.md` (retired) is this track's direct predecessor on the same
  trajectory. Its own closing summary states the handoff explicitly: "What this track opens: phase 4
  (the project model as its own crate, `bynk-project`) … entry-gated on this track's own probe
  (`span_keyed_maps`) reading zero — met, modulo the stated `Ctx::pattern_binding_types` exclusion."
  `project-model.md` is the ninth track to run the ADR 0167 flow from the start, after
  `compiler-architecture.md` (sixth), `identity-and-totality.md` (seventh) and `content-ownership.md`
  (eighth) — the fourth in a row on the internal-architecture theme rather than language surface.

### ADR 0076 trigger check

| Trigger | Met? |
|---|---|
| Spans several increments | **Yes, if narrowly** — trajectory §5 rates this phase relative size 3 (medium confidence); §6 below names three slices, one (P4.1) larger than the original draft anticipated per Q3's finding |
| Surface not yet settled | **No** — the destination (reference R3.7, R3.8, R3.9, R3.11, R10.2) is specified; what was open was which parts of today's ~21.5k-line `bynk-emit/src/project.rs` module tree are "the project model" versus checking orchestration that only phase 5 can resolve, which §3 now closes |
| Security/safety boundary | **No** — this phase moves data-shape and crate-boundary code; it touches no capability gate the way phases 0–2's `ResolvedCommons` did |

**One of three, same count as `compiler-architecture.md` and `identity-and-totality.md` before it, same
argument.** The warrant is the tracks README's own: §3's questions governed three slices and constrain
what phase 5 inherits (§2 below — moving `validate.rs` before this phase settles means moving it twice),
and that governance needs a home that is neither a proposal nor the trajectory document itself.

---

## 1. Why now

**Phase 3's probe reads at its documented floor.** `span_keyed_maps` (`HashMap<Span` below the driver)
reads **3** — confirmed live (`cargo xtask greenfield-status`, 6 August 2026), all three
`Ctx::pattern_binding_types` in `bynk-check/src/checker.rs`/`linearity.rs`. `identity-and-totality.md`'s
own retirement recorded this exact residue as a **deliberate exclusion** (the `PatId`/`ExprId` split the
reference draws on purpose), not open work, and stated in the same breath that phase 4's entry condition
is "met, modulo" it.

**The reference's own appendix measures this phase's distance; §3.5 below finds one row of it stale.**
`bynk-greenfield-compiler.md`'s Appendix D:

| Rule | Reads | Distance |
|---|---|---|
| R3.7 | ~~no `bynk-project` crate~~ *(P4.0, #1113: closed — the crate exists)* | none |
| R3.8 | ~~`read_project_paths` still total~~ *(P4.0, #1113: closed — deleted, all callers on `try_read_project_paths`)* | none |
| R3.9 | ~~`Roots` still models the removed role split~~ *(P4.0, #1113: closed — `Roots::trees` walks every `include` entry)* | none |
| R3.11 | ~~schema registry read/written ambiently~~ *(§3.5: corrected — already closed by prior paydown, #1078)* | none |
| R10.2 | `bynk-ide` → `bynk-emit` edge present, for `analyse_project` alone | medium — extract `bynk-project`, repoint |

Confirmed live: `bynk-ide/Cargo.toml`'s own comment states the dependency's whole reason — `"analyse_project
(the non-bailing project analysis) lives in the project driver"` — and `bynk-project` does not exist as a
crate.

**The causal chain the trajectory names is visible in the code, not just asserted.** R3.5's rationale
(phase 5) reads: "context-level semantics were written in `project.rs` because that is where the project
model was, and the project model was in the emitter because emission needed it first. … Extracting
`bynk-project` as its own phase *below* both check and emit is what makes phase 5 the only place context
checking can live." `bynk-emit/src/project.rs`'s own top-of-file doc confirms the entanglement it names:
discovery, resolution, checking and emission are one two-pass pipeline by design, and the file imports
`crate::emitter` and glob-imports `validate::*` at its own top level. Phase 4 pulls the *model* out of that
pipeline without dragging the *checking* out with it — that second move is phase 5's, named already so it
isn't repeated. §3.3's finding sharpens exactly how tight that entanglement is: `run_checks`
(`bynk-emit/src/project.rs:3644-4206`, ~560 lines), the function both `compile_project` (line 584) and
`analyse_project_with` (line 979) call, does discovery, parsing, resolution *and* checking as one
sequence — there is no seam inside it that separates "project model" from "checking" at the granularity
this phase needs. (Size isn't the argument — the structural fact is: whatever `run_checks`'s length,
nothing inside it stops at project-model granularity.)

---

## 2. What this track is not

- **Not phase 5's semantics extraction.** `validate.rs` (5,019 lines, the context-level checks R3.5
  moves to `bynk-check`) is untouched here. Moving it before `bynk-project` exists would mean moving it
  twice — the trajectory's own "4 before 5" ordering argument (§4 of the trajectory doc). §3.3's new
  `bynk-check`-side analysis entry point (below) is a narrow, named exception to this line, not a reversal
  of it — see §3.3 for why it's necessary and why it doesn't grow into phase 5's move.
- **Not the fixture-format work `tests_emit.rs` (5,922 lines) implies.** Its residence and shape are
  phase 0/seams territory (broad fixture-kind adoption), not this phase's.
- **Not the full `R3.2` `ProjectGraph { units, files, edges, contract }` struct.** §3.1/§3.4 settle this:
  the typed struct — and the `contract` field specifically — is phase 8's, not this phase's. Phase 4
  extracts today's name-keyed logic; it does not build `UnitId`, `IndexVec`-keyed tables, or a
  `ContractHash`.
- **Not a manifest schema change.** R3.8/R3.9 ask for a typed, `Result`-returning reader and a `Roots`
  that matches ADR 0147's flat `include` list — not a new `bynk.toml` shape.
- **Not unifying the CLI's and LSP's file-discovery seams.** §3.3 relocates both as they are; the
  overlay-vs-disk-fallback asymmetry between them is named, accepted, and deferred, the same way
  content-ownership named three permanent `fs_below_driver` exceptions rather than closing them by force.
- **Not phases 6–8.** Carried as forward references in §7, not refusals, per the discipline
  `compiler-architecture.md` §7 and `identity-and-totality.md` §7 both used for phases not yet open.

---

## 3. Design questions — settled

> **Provenance: one finding changed the shape of this settling pass, and it wasn't the one the draft
> flagged as riskiest.**
>
> The draft's own §9 named Q4 (contract hashes) as "the one most likely to surprise." It settled cleanly
> and as anticipated (§3.4). The actual surprise was underneath Q3. The draft framed Q3 narrowly, around
> `bynk-driver/src/discovery.rs`'s own doc comment naming a file-*reading* seam split between the CLI and
> LSP paths. Tracing `analyse_project_with` fully (not just the fragment the draft's Q5 already looked at)
> found something bigger: it calls `run_checks` — the same function `compile_project` calls —
> which performs discovery, parsing, resolution *and* checking as one sequence. There is no existing
> boundary inside it at "project model" granularity. `bynk-ide`'s dependency on `bynk-emit` is not, as the
> draft assumed, a dependency on a *discovery* function this phase can cleanly relocate — it is a
> dependency on a function that also checks, which `bynk-project` (sitting below `bynk-check`) cannot
> absorb, and which `bynk-check` (this phase's stated non-target) is the only place that legally can. §3.3
> below is the resulting decision, and it is the one place this settling pass adds work the draft didn't
> foresee rather than only removing or confirming what it already posed.
>
> Q2's investigation surfaced a smaller version of the same shape, and it kept surfacing: `symbols.rs`'s
> `ProjectIndex`/`IndexBuilder` turned out to be `bynk-check` types already, not `bynk-emit`'s — removing an
> option the question posed rather than choosing among the options it posed. Four further review passes
> each found one more direction the same single literal-`bynk_check`-import test couldn't see:
> `schema_registry.rs` reaching `UnitTable` through an unqualified `use super::UnitTable`; `paths.rs`
> reaching a plain `bynk-emit`-internal utility (`json_string`) that isn't a checking output at all;
> `Mode`/`ErrorSink`/`ProjectFailure` passing the coupling test for the wrong reason and getting grouped
> with the one item (`AttributedError`) that genuinely needed to move; `schema_registry.rs`'s and
> `ParsedFile`'s private fields, reached into from the checking side once their owning structs moved;
> `ContextBoundaryInfo`, which passes every needs-based test yet stays behind purely on who builds it; and
> `ProjectAnalysis`, a composite of project facts and checker outputs that resolves upward to §3.3(a)'s new
> entry point rather than being homogeneous at all. §3.2 now states the full test this doc should have used
> from the start — five criteria plus a separate rule for composites — not "no literal `bynk_check`
> import."
>
> Q1 and Q4 turned out to be one decision, not two: once contract hashing is confirmed out of scope
> (Q4), the reference's `ProjectGraph` struct has no field this phase needs, so Q1's "extract today's
> shape" reading isn't a scope trade-off against the reference anymore — it's what's left once Q4 is
> subtracted. Q5 folded into Q3 as a straightforward finding, not a further decision. Q6 stands as drafted.

### 3.1 Q1 — Full `ProjectGraph` now, or extract today's shape and defer the typed-index upgrade? **Settled.**

**Decision: extract today's name-keyed logic; do not build the reference's typed `ProjectGraph` struct.**
None of this phase's five reference rules (R3.7, R3.8, R3.9, R3.11, R10.2 — trajectory §3, Phase 4) name
`ProjectGraph`, `UnitId`, or `IndexVec`; R3.2 itself, which defines the struct, is not in this phase's rule
list at all. The typed struct is what phase 8's query layer actually consumes (trajectory §3, Phase 8:
`Tokens(FileId)`, `UnitSignature(UnitId)`, `Body(DefId)`, `ProjectGraph`) — see §3.4 for where `contract`
specifically belongs. `bynk-project` holds `Roots`, `ProjectPaths`, `UnitKind`, `ParsedFile` and the
existing `HashMap`/`Vec`-based discovery, cycle-detection and manifest logic, relocated with minimal
reshaping. The `UnitId`/`IndexVec` upgrade is a named forward reference (§7), not refused, the same
discipline phase 3 used for `ExprId`-at-parse before committing to it.

### 3.2 Q2 — Module boundary: what actually moves, and what does `bynk-project` export? **Settled.**

Verified directly: `bynk-check/src/index.rs:302` defines `ProjectIndex`; `:430` defines `IndexBuilder`.
`bynk-emit/src/project/symbols.rs::assemble_index` imports both from `bynk_check::index` and exists only
to walk `ParsedFile`s and populate that checker-owned type — `bynk-ide/src/lib.rs:156` carries the result
directly (`pub index: index::ProjectIndex`). `symbols.rs` is a `bynk-check`-type builder that happens to
live in `bynk-emit`.

A first pass checked `discovery.rs`, `graph.rs`, `paths.rs`, `schema_registry.rs` and `consistency.rs` for
a literal `bynk_check` import and found none — but that grep is too coarse, and review caught it:
`schema_registry.rs:35` reads `use super::UnitTable;`, and `reconcile` (`:228-230`) takes
`unit_tables: &HashMap<String, UnitTable>`. `UnitTable` (`symbols.rs:284`) is itself `bynk_check`-coupled
— its `methods` field is `HashMap<String, ResolverMethodTable>`, an alias for `bynk_check::resolver::MethodTable`
(`project.rs:38`). The coupling arrives through an unqualified, glob-sourced name, which is exactly what a
literal-string grep for `bynk_check` misses. `schema_registry.rs`'s other two functions, `parse` (`:409`)
and `serialize` (`:446`) — `pub(crate)` today, like `reconcile`, so none of the three has a public surface
yet; P4.0 gives whichever half moves a real one — take no `UnitTable` and stay clean, which is the
module's own documented seam ("this module touches no disk … `parse`/`serialize` are pure") turning out to
also be the seam between what's project-model and what isn't. `graph.rs` and `consistency.rs` re-check
clean under the corrected test — their `use super::*` only resolves to
`ParsedFile`/`CompileError`/`Span`/`HashMap`, no `UnitTable`/`MethodTable`/`Types`/`Ty`/`ProjectIndex`/
`resolver::`/`checker::` anywhere. So do `discovery.rs` (`use super::*` plus `bynk_syntax::ast::TestTier`
only) and `paths.rs`, with one exception the checking-output test doesn't catch because it isn't a
checking output: `paths.rs:3` reads `use crate::json::json_string;`, and `json_string`
(`bynk-emit/src/json.rs:28`) is `pub(crate)` to `bynk-emit` — a `bynk-emit`-internal utility, not a checker
output, but still something `bynk-project` (sitting below `bynk-emit`) cannot depend on. P4.0 has to move,
duplicate, or inline it alongside `paths.rs`; it isn't covered by the enumerated carry-along list below,
which is types-only.

Splitting `schema_registry.rs` surfaces a second cost the "checking-output" framing doesn't price in,
because it runs in the opposite direction. `SchemaRegistry`'s fields (`version`, `events`) are private,
its component types `EventEntry`/`FieldShape` are private to the module, and `lock_version()` is a private
helper — and *both* halves of the split construct the struct through them: `parse` (`:411`) returns
`SchemaRegistry { version: lock_version(), events: BTreeMap::new() }` on the fresh-project path and reads
`reg.version` back; `reconcile` (`:233`) builds the same literal and populates `events` with `EventEntry`
values. That works today because all three functions share a module. Once `SchemaRegistry`/`parse`/
`serialize` move to `bynk-project` and `reconcile` moves to `bynk-check` (P4.1), one of them has to give:
either `bynk-project` publishes the lockfile's serde representation — `version`, `events`, `EventEntry`,
`FieldShape`, `lock_version` — as real crate API, which is exactly what "`parse`/`serialize` are pure" was
encapsulating, or `bynk-project` grows a constructor/mutator surface `reconcile` builds through instead of
touching the fields directly. This is a design commitment P4.0/P4.1 has to make, not an enumeration detail
— named here so the slice inherits it rather than discovering it at the crate boundary. §3.2's two-sided
test screens coupling reaching *up* into `bynk-check` (the direction `Mode` and `schema_registry.rs`'s
first pass both got wrong); it has nothing to say about this reverse direction — a `bynk-check`-side
function needing a `bynk-project` type's privates — which is the direction this cost comes from.

The same question needed asking of `diagnostics.rs`, which the first pass never assigned a home at all.
Of its seven top-level items, four have no `bynk_check` anywhere (`Mode`, `AttributedError`,
`ProjectFailure`, the crate-private `ErrorSink`) — but an earlier pass of this section grouped all four as
"plain bookkeeping" headed to `bynk-project` on that ground alone, which review caught as three-quarters
wrong: being `bynk_check`-free is necessary, not sufficient, and only one of the four actually needs to
move. `ContextSequenceInfo` carries `resolver::CrossContextInfo`, a clean checker output.
`ContextBoundaryInfo` is AST-typed but built during `run_checks`'s `Checked` arm from
`combined_types_for`/`unit_tables`, the same checking pass, not discovery — its own fields pass every one
of (a)–(d) below, since they're all parse-time AST types, but it stays on the checking side anyway,
because (a)–(d) ask what an item *needs*, not *who builds it*, and this item is entirely a question of
provenance: nothing about `ServiceDecl`/`AgentDecl`/`TypeDecl` in its fields requires checking, but the
values that populate them come from resolved, `uses`-merged tables that only exist mid-`run_checks`, not
from anything `discovery.rs` produces.

`ProjectAnalysis` is not the same shape as those two, and an earlier pass of this section overstated it as
one: it has twelve fields, not the six enumerated here on a first read (`snapshots`, `errors`, `index`,
`hints`, `expr_types`, `ty_intern`, `locals`, `requirements`, `unit_sources`, `sequence_info`,
`boundary_info`, `doc_scope`), and "every field a checker output" is false of it. `index` (`ProjectIndex`),
`hints`, `expr_types`, `ty_intern` (`Arc<bynk_check::checker::Types>`), `locals` and `requirements` are
checker outputs, cleanly. But `snapshots: Vec<(PathBuf, String)>` is, by its own doc comment,
"`(project-relative source path, analysed text)` for every file read, including clean files" — a
discovery-and-read output, produced whether or not checking ran, and the single most project-model-shaped
field on the struct. `errors: Vec<AttributedError>` holds the exact type this same section sends to
`bynk-project` two paragraphs up as plain bookkeeping. `unit_sources` is a qualified-unit-name → source-file
map "in discovery order" — R3.7's territory. `doc_scope` is assembled from the `uses`/`consumes` edges,
unit-graph facts. `ProjectAnalysis` is a *composite*: some fields are checker outputs, some are project-model
facts that happen to be populated in the same pass today. It stays on the checking side not because every
field demands it, but because only something sitting above both `bynk-project` and `bynk-check` — §3.3(a)'s
new entry point — can assemble a value with both kinds of field in it. That's a real shape constraint on
P4.1, not just a placement one: the entry point's return type has to surface `bynk-project`-shaped
discovery data (`snapshots`, `unit_sources`) alongside `bynk-check`-shaped checking data, not just the
latter, which "the analogue of `ProjectAnalysis`" (§3.3(a)) was already committing to without saying so
explicitly.

Being `bynk_check`-free isn't the whole test, though, and three of the four items above show the other way
it can fail — each traced individually rather than classified as a group, since the group classification
is exactly what let the other three ride along with the one that actually belongs there.

`Mode`'s use sites split across two functions, not one: `check_unit_files` (`project.rs:3210`), which
takes `mode: Mode` (`:3230`) and branches on it (`:3476,3495,3539,3556`) to decide whether a per-file error
path also records best-effort types for the editor; and `run_checks` (`:3644`) itself, which takes its own
`mode: Mode` (`:3654`) and branches on it once (`:3961`, whether to bail). `run_checks` calls
`check_unit_files`, passing `mode` through — so `Mode` doesn't just parameterise the top-level orchestrator,
it threads down into the per-unit check-and-emit loop underneath it. Its seven callers (`:592,665,724,795,987`,
plus two in tests) are all `run_checks`'s, and all inside `project.rs`.

`ErrorSink` (`diagnostics.rs`) is the collection-point sink `run_checks`/`check_unit_files` thread through
essentially every internal function that can raise a diagnostic — 15 `errors: &mut ErrorSink` parameters
in `project.rs` alone, another 7 in `validate.rs` — and it appears by name in five files, only three of them
imports: `diagnostics.rs` (its own definition), `project.rs`, `validate.rs`. The other two are prose, not
imports — `bynk-check/src/index.rs`'s own two "`ErrorSink` analogue" mentions are a doc comment pointing at
this type by name for a reader's context (`bynk-check` has its own, separate collection-point sink), and
`bynkc/tests/deterministic_diagnostic_order_behaviour.rs:6`'s own doc comment names it the same way
(`//! \`ErrorSink\` depended on \`HashMap\`'s randomised iteration order…`). Every one of `ErrorSink`'s
readers stays behind, exactly like `Mode`.

`ProjectFailure` is `compile_project`'s own failure-return type, and no consumer of it sits below
`bynk-emit`: `bynk-driver/src/lib.rs`'s `print_project_failure`/`print_project_failure_short`, `bynkc`'s own
re-export and flattener comment (`bynkc/src/lib.rs:37,50`), and the `bynkc`/`bynk` test suites (including
`bynk/tests/new.rs`, `bynk/tests/dev_inspect.rs`) all read it — every one of them a caller *above*
`bynk-emit`. Nothing under `run_checks` produces or reads a
`ProjectFailure`; it's assembled once, at the top, on the bail path. It fails the same test `Mode` and
`ErrorSink` do, for a related but distinct reason: not "belongs with its orchestrator" but "is consumed
only by callers above the crate it's already in," which never gave it a reason to move down in the first
place.

`AttributedError` is the one item of the four that really does move, and the reason isn't "plain
bookkeeping" — `bynk-render/src/lib.rs`'s own doc comment states `AttributedError`/`ProjectFailure` "live
in `bynk-emit`" and that `bynk-render` "must never see" them, which is further evidence `ProjectFailure`
belongs where it is, not where it was headed. `AttributedError` differs because §3.3(a)'s new `bynk-check`
entry point returns *"the analogue of `ProjectAnalysis`,"* whose `errors` field is `Vec<AttributedError>`
(§3.2 above) — so that entry point, living in `bynk-check`, has to construct `AttributedError` values
itself. `bynk-check` cannot depend on `bynk-emit` for the type, so `AttributedError` has to live somewhere
both crates can reach: `bynk-project`, below both. That's a layering requirement the new entry point's
return type creates, not a "no `bynk_check` coupling, so why not" default the way the original grouping
implied.

`Mode`, `ErrorSink` and `ProjectFailure` are a statement about how the checking pipeline is driven and
consumed, not a fact about the project — the checking-coupling test screens out items reaching *up* into
`bynk-check`, but says nothing about an item that belongs with its orchestrator, or with the crate whose
callers already consume it, regardless of which crate that is.

**Decision:** `bynk-project` receives `discovery.rs`, `graph.rs`, `paths.rs`, `consistency.rs`;
`schema_registry.rs`'s `SchemaRegistry` type plus `parse`/`serialize` only (not `reconcile`, and pending
the private-field question above); `paths.rs`'s dependency on `bynk-emit::json::json_string` moves,
duplicates, or inlines alongside it; and, from `diagnostics.rs`, only `AttributedError` — moved for a
layering reason (§3.3(a)'s new entry point must construct it without depending on `bynk-emit`), not because
it's bookkeeping. `Mode`, `ErrorSink` and `ProjectFailure` all stay in `bynk-emit`: `Mode` and `ErrorSink`
with `run_checks`/`check_unit_files`, their only consumers; `ProjectFailure` because nothing below
`bynk-emit` reads it at all — plus the project-model types declared directly in `project.rs` these depend
on (`UnitKind`, `Roots`, `ParsedFile`, `ProjectPaths`, the
`CompileOptions::schema_registry`/`SchemaLock` shape). `ParsedFile` (`discovery.rs:80`) carries the same
(d)-shaped cost `SchemaRegistry` does, at larger scale: its fields are `pub(crate)`, and both `symbols.rs`
(seven `.identity_path` reads, three through `pf`) and `validate.rs` (dozens of field accesses) stay on the checking
side and read them directly — once `discovery.rs` moves, those reads cross the crate boundary the same way
`reconcile`'s do, and P4.0 needs the same either/or: keep the fields `pub(crate)`-visible-enough for
`bynk-check` to reach (which `pub(crate)` alone won't do across a crate boundary — it needs `pub`, or an
accessor surface) or add real accessors. This is the same class of cost as `schema_registry.rs`'s, named
here rather than left for P4.0 to discover mid-slice, and it's the bigger of the two: one struct's fields,
reached from two files staying behind, not one function's.

Staying on the checking side, to become part of §3.3's new `bynk-check` entry point rather than
`bynk-project`: `symbols.rs` in full, `schema_registry.rs`'s `reconcile`, `Mode`/`ErrorSink`/`ProjectFailure`
as above, and `diagnostics.rs`'s `ProjectAnalysis`, `ContextSequenceInfo`, `ContextBoundaryInfo` — the last
three either genuinely checker-output, a checking-side composite (`ProjectAnalysis`), or checking-side by
provenance rather than by field type (`ContextBoundaryInfo`). `ProjectAnalysis` in particular is the return
type §3.3(a)'s entry point already commits to producing "the analogue of," so its new home was already
implied; this just makes it explicit rather than leaving it unassigned. `project.rs`'s orchestration
(`run_checks`, `compile_project`) does not move as a body; it becomes a caller of `bynk-project`'s
functions instead of owning that logic inline.

The rule this leaves, stated once so P4.0 doesn't have to rediscover it per file, has grown well past the
two parts it started with — each addition surfaced by tracing one specific item (`Mode`, `ErrorSink`,
`ProjectFailure`, `schema_registry.rs`, `ParsedFile`, `paths.rs`, `ContextBoundaryInfo`,
`ProjectAnalysis`) in a direction the prior parts didn't check. A type or function's home is settled only
once all of (a)–(d) hold, with (e) and the composite rule after them as separate questions for the items
they apply to:

(a) it needs nothing that exists solely as an output of resolution or checking (`UnitTable`, `Ty`/`TyId`,
`ResolverMethodTable`, anything under `bynk_check::`, however it's named at the use site — not "no literal
`bynk_check` import," which this section's own first pass shows is too weak on its own): coupling reaching
*up* into `bynk-check`.

(b) it needs nothing that is a plain `bynk-emit`-internal utility either, checking-related or not
(`paths.rs`'s `use crate::json::json_string`, `bynk-emit/src/json.rs:28`, `pub(crate)`): coupling reaching
*sideways* into `bynk-emit` for reasons unrelated to checking, which (a) doesn't screen for because
nothing about it involves `bynk_check`.

(c) it is a fact *about the project* — not merely "nothing above the checking pipeline needs it to stay," a
test `Mode` and `ErrorSink` both pass for the wrong reason (both `bynk_check`-free and `bynk-emit`-internal
otherwise clean, but both facts about how `run_checks`/`check_unit_files` are driven, with no reader
outside those two functions), and `ProjectFailure` fails a related way (consumed only by callers *above*
`bynk-emit` — `bynk-driver`, `bynkc`'s tests — never by anything below it, so nothing ever gave it a reason
to move down). Phrased around "a fact about the project" rather than "consumed below `run_checks`"
specifically, because `run_checks` is exactly what phases 4→5 dissolve — once P4.1's entry point exists
there are two orchestrators, and once phase 5 lands there may be neither, so a rule anchored to today's one
orchestrator wouldn't survive its own phase. Three of the four items an earlier pass grouped as "plain
bookkeeping" fail here; only `AttributedError` genuinely moves, and for a fourth reason entirely — not (a)
through (d), but that §3.3(a)'s new entry point must construct it without depending on `bynk-emit`, a
layering requirement the entry point's own return type creates.

(d) nothing staying behind needs to reach back into its privates once it moves. Two examples, one per
direction: `schema_registry.rs`'s `SchemaRegistry`/`EventEntry`/`FieldShape`/`lock_version` pass (a)–(c)
cleanly, and still cost something — `reconcile`, staying on the checking side, constructs `SchemaRegistry`
through fields that are private today and would have to become a real `bynk-project` API (or gain a
builder) once the struct moves without it. `ParsedFile` (`discovery.rs:80`) is the same shape at larger
scale: its fields are `pub(crate)`, and `symbols.rs`/`validate.rs` — both staying behind — read them
directly at dozens of sites between them. Once `discovery.rs` moves, those become cross-crate reads a
`pub(crate)` field can't serve; P4.0 needs the same either/or `schema_registry.rs` does, across two files
instead of one. (a)–(c) all screen what the moving item needs; (d) is the direction that bites when
something *staying behind* needs the moving item's insides.

(e) — a different question from (a)–(d), which all ask what an item *needs* — is who *produces* it.
`ContextBoundaryInfo` passes (a) through (d) cleanly: its three fields (`types`, `services`, `agents`) are
all `bynk_syntax::ast` types, not checker outputs, nothing `bynk-emit`-internal, and nothing reaches back
into it. By (a)–(d) alone it reads as a `bynk-project` candidate. It stays on the checking side anyway,
because the *values* that populate it (`project.rs:1104`, inside `run_checks`'s `Checked` arm, built from
`combined_types_for`/`unit_tables`) only exist after resolution — `discovery.rs` has nothing that could
construct one. (a)–(d) classify by what a type's definition needs; (e) classifies by what actually builds
a value of it, and the two can disagree, as they do here.

A composite item is a further, separate case: it doesn't split field-by-field and doesn't stay whole on
either side, and none of (a)–(e) resolve it. **It resolves upward**, to whatever sits above both
`bynk-project` and `bynk-check`, which for `ProjectAnalysis` — part project fact, part checker output — is
§3.3(a)'s new entry point.

Public surface: `bynk-project` exports only what its real consumers use (mirroring R10.4's existing
discipline), not a blanket glob. The exact enumerated list is P4.0's job, not a design question — reviewed
as part of that slice, the same way R10.4's own audit was a slice of its own rather than a design question.

### 3.3 Q3 — The project's real coupling to `bynk-emit`, and what closes R10.2 without doing phase 5 early. **Settled.**

Two distinct issues live under this question; both are now resolved, but differently.

**(a) The orchestration coupling — the one the draft didn't see.** `analyse_project_with`
(`bynk-emit/src/project.rs:970`) calls `run_checks` (`:3644`) with `Mode::Analyse`; `compile_project`
(`:573`) calls the same `run_checks` (`:584`) with `Mode::Build`. `run_checks` is not `pub` — it is private
to `bynk-emit::project`, reachable from nowhere outside the crate — and its body does discovery, parsing,
resolution and checking together (it destructures `RunChecks::Checked { unit_tables, unit_flattened,
kinds, parsed, .. }`, all checker outputs, in the same match arm that builds `ProjectAnalysis`). This is
the *real* reason `bynk-ide` depends on `bynk-emit`: not a discovery function this phase can relocate
whole, but an orchestration function that also checks — which `bynk-project` (below `bynk-check`) cannot
absorb, and which moving into `bynk-emit` and keeping it there indefinitely defeats R10.2 outright.

**Decision:** phase 4 does not move `run_checks`. It gains one narrow entry point in `bynk-check` —
`bynk-check`'s natural long-term home for it under R3.5 regardless — that performs the same
discovery(`bynk-project`)→parse→resolve→check(`bynk-check`, already local) sequence `run_checks`'s
`Mode::Analyse` arm does today, and returns what `bynk-ide` actually needs (the analogue of today's
`ProjectAnalysis`). `bynk-ide` calls this instead of `bynk-emit::analyse_project`. `run_checks` itself stays
in `bynk-emit`, unchanged, serving `compile_project`/emission alone.

This is a deliberate, temporary duplication — the new `bynk-check` entry point and `run_checks`'s
`Mode::Analyse` arm do overlapping work — accepted rather than avoided, because avoiding it means either
(a) moving checking into `bynk-project`, which breaks the "below both check and emit" invariant this whole
phase exists to establish, or (b) doing phase 5's centralisation now, which is explicitly out of scope
(§2) and would mean deciding `validate.rs`'s new home under this phase's much smaller review budget
(relative size 3, not phase 5's much larger one). The duplication is phase 5's to remove: once context
checking centralises in `bynk-check` (R3.5), `bynk-emit`'s CLI path can call the same entry point this
phase adds, and `run_checks`'s `Mode::Analyse` arm — and eventually all of `run_checks`'s checking half —
is deleted rather than ported. Naming the debt here is what keeps phase 5 from rediscovering it as a
surprise the way this phase just rediscovered `bynk-ide`'s real dependency shape.

**(b) The file-reading seam split — the draft's original framing, now the smaller of the two.**
`bynk-driver/src/discovery.rs`'s own doc comment names this directly: the CLI reads every file up front
(`CompileOptions.sources`, seamed above `bynk-emit` since #1077) while the LSP path depends on
`bynk-emit`'s own overlay-miss disk-read fallback — one of content-ownership's three permanent, documented
`fs_below_driver` exceptions. Relocating `bynk-project`'s discovery module (§3.2) carries this fallback
along unchanged; it does not fix it, because fixing it means changing what the LSP path is allowed to see
without a full overlay, a decision this phase doesn't need to make to close R10.2.

**Decision: relocate, don't unify.** The asymmetry is named and accepted, not silently carried — the same
discipline content-ownership used for its own three exceptions. Unification remains real, named future
work (§7), gated on an actual trigger (an observed CLI/LSP divergence, not appetite), consistent with how
every track on this trajectory opens.

### 3.4 Q4 — Where does `ProjectGraph.contract: IndexVec<UnitId, ContractHash>` actually come from? **Settled — deferred to phase 8.**

The only `ContractHash`-shaped type in the tree, `bynk-check/src/contract.rs` (ADR 0200's cross-context
wire-contract hash), is necessarily downstream of type-checking — it canonicalises resolved types via
`combined_types_for`/`CrossContextService`. That conflicts with "lives below both check and emit" if it
were this phase's `contract` field.

**Decision: `ProjectGraph.contract` is phase 8's `UnitSignature`-identity concept, misfiled onto phase 4's
struct sketch.** The trajectory's own words about phase 8 — "ADR 0200's contract hash is already
`UnitSignature`'s identity function pointed at a different problem" — are read here as a direct statement
of this, not a hint. Corroborating: phase 4's reference-rule list never cites R3.2, and neither does phase
5, 6 or 7's — only phase 8's R3.13–R3.15 (query granularity), which is exactly where a stable per-unit
identity/signature is consumed. Contract hashing, in any form, is out of this phase's scope. Since §3.1
already found nothing in §6 builds `ProjectGraph` at all, this doesn't even need a stub — there is no
`contract` field to defer, because there is no struct.

### 3.5 Q5 — Is R3.11's remaining distance the same defect as Q3? **Settled — folded in; R3.11 is already closed, appendix corrected.**

Verified: `bynkc/src/main.rs:64-100` shows the CLI's schema-lock round trip fully wired —
`bynk_driver::schema_lock::read` → `CompileOptions::schema_registry` (`SchemaLock::On { existing }`) →
`bynk_driver::schema_lock::write` on the output. `run_checks` (`bynk-emit/src/project.rs:3644`) has seven
call sites total: five in production code — **two** `Mode::Build` (`compile_project` `:584`,
`compile_in_memory` `:716` — not four non-build as an earlier pass of this section had it, since
`compile_in_memory` passes `Mode::Build` at `:724`) and three non-build (`check_project` `:657`,
`analyse_in_memory_with_types` `:787`, `analyse_project_with` `:979`) — plus two more inside the file's own
`#[cfg(test)]` module (`:5844`, `:5988`, both `Mode::Analyse`). Every one of the six *non-`compile_project`*
call sites passes an explicit `&SchemaLock::Off` — `:673`, `:729`, `:800`, `:995` in production, `:5857`
and `:6001` in tests — not an ambient read. `:3638`'s own comment attributes the `None`-when-`Off` shape to
#1078. But "not a build" only justifies five of those six: the LSP path (`analyse_project_with`) and the
other two non-build entry points genuinely aren't builds, so `Off` is correct-by-design for them — a build
with no business persisting a lockfile doesn't exist to persist one. `compile_in_memory` *is* a build
(`Mode::Build`), and its `Off` needs the other reason, which lives at `bynkc/src/main.rs:75-78`'s own
comment: "Every other `compile_project` caller (in-memory builds, `bynkc/tests/e2e.rs`'s in-place
fixtures, the LSP) leaves this off" — an in-memory build has no `bynk.schema.lock` on disk to reconcile
against in the first place, not "isn't a build." Both reasons land on the same conclusion (explicit, not
ambient), which is what R3.11 asks for, but they're different reasons and this section's original wording
collapsed them into one. (`:6330`'s comment — "rather than drift into a second, independently-maintained
`run_checks` call" — is itself a sign the test suite already treats a stray extra call site as a known
risk worth pinning against, which the two test-module calls above are not instances of: both delegate
through the same `run_checks`, not a copy of it.)

**Decision:** the Appendix D row ("schema registry read/written ambiently … thread two values") is closed
by paydown that postdates whenever that row was last verified — corrected directly in
`bynk-greenfield-compiler.md` as part of this settling pass rather than left flagged with no landing
place, since the fix is a one-line edit and this settling review is the review that would otherwise have
had to name a follow-up owner for it. §3.3's new `bynk-check` entry point carries the same explicit-`Off`
behaviour forward (nothing about relocation changes it). Because R3.11 is now closed rather than merely
"appears closed," P4.0 (§6) does not cite it as a rule it closes — relocating `schema_registry.rs`'s
`parse`/`serialize` doesn't close an open rule, since none was open by the time P4.0 lands.

Naming the method plainly, since the appendix's own preamble argues against it in general: this is a hand
edit to a table that preamble says "must be generated" by `cargo xtask greenfield-status`, and hand
maintenance is the exact failure mode it warns about. It's made here anyway because the fix is one line
and self-contained, not because the general objection doesn't apply. Other rows this same PR's own
evidence could update — the trajectory §7 phase-3 row already says `span_keyed_maps` "reads 3, down from
27" and R3.10/R4.1/R4.2/R4.3 closed, yet Appendix D still carries `R2.4 | … = 27 | large` and those four
rules at their pre-retirement distances — are deliberately left alone: this settling pass corrects the one
row its own investigation produced new evidence for, not every row a closer read would also update. If
that's the wrong line to draw, the fix is to extend the standard, not to have applied it nowhere.

### 3.6 Q6 — Freeze scope. **Settled.**

**Decision: freeze `project.rs` itself** (not the wider ~10.5k-line candidate surface, which — per §3.2 —
is mostly self-contained submodules with few contended edit sites) **for the duration of P4.0** (§6).
Mirrors phase 3's `checker.rs` freeze (`identity-and-totality.md` §3.5): one hub file, one freeze, lifted
when the slice touching it merges. No freeze is proposed for phase-5 work starting early — trajectory §2
permits abandoning a phase at a boundary, not interleaving mid-slice; if phase 5 work is proposed before
this track retires, that is a scheduling call for whoever proposes it, using the same reasoning
`identity-and-totality.md` §3.6 (its own Q6) applied to the state-migrations track.

---

## 4. Posture

Extends ADR 0059 as amended by ADR 0309 and by `identity-and-totality.md`'s pending LSP-surface-fixture
amendment (§3.4 there — directly relevant here, since §3.3(a)'s new `bynk-check` entry point is exactly
the kind of migration that amendment's LSP-fixture requirement was written for), the same standing
properties every track on this trajectory has run under: behaviour-preserving by default (a slice that
changes observable behaviour states so, ships a fixture, gets a CHANGELOG entry); short-lived branches, one
PR per slice; no per-increment ADRs beyond what §11 front-loads; every slice citing `Closes-Rule:`.

---

## 5. The completion criterion

Same principle as every prior track on this trajectory: a slice is complete when the old path is
**deleted**, not when the new crate merely exists alongside it. Here: `bynk-ide/Cargo.toml`'s `bynk-emit`
dependency line — and the comment justifying it — are gone, `bynk-project` is a real workspace member, and
the new `bynk-check` analysis entry point is what `bynk-ide` actually calls. The manifest-level check R10.2
asks for (a CI gate on the dependency graph, not just a probe reading zero once) is not something this
phase has to build — `ide_emit_edge` already is that gate (§8) and already runs continuously in CI; once
the dependency line above is gone it simply reads absent. One deliberate exception to "deleted": `run_checks`'s `Mode::Analyse` arm is not deleted by this track — §3.3(a)
keeps it, duplicated with the new entry point, as named debt for phase 5 to remove. This phase's own old
path (`bynk-ide` reaching `bynk-emit`) is fully deleted; the old *arm* inside `run_checks` is phase 5's
deletion, not this one's, and completion here doesn't wait on it.

---

## 6. Slice decomposition

§3 is settled; all three slices below are scoped and buildable as drafted, not forward references — each
still becomes its own increment proposal, and `accepted` on that sub-issue (not this doc settling) is the
actual approval to build it, per this doc's own Status block.

| Slice | What lands | Rules | Gated on |
|---|---|---|---|
| **P4.0** | `bynk-project` crate skeleton; `discovery.rs` (plus `ParsedFile`'s private-field question resolved, per §3.2's (d)), `graph.rs`, `paths.rs` (plus `json_string`, moved/duplicated/inlined), `consistency.rs` relocated with minimal reshaping, plus `schema_registry.rs`'s `SchemaRegistry`/`parse`/`serialize` (not `reconcile`; its own private-field question resolved too) and, from `diagnostics.rs`, only `AttributedError` (not `Mode`/`ErrorSink`/`ProjectFailure`, which all stay in `bynk-emit`; not `ProjectAnalysis`/`ContextSequenceInfo`/`ContextBoundaryInfo`), plus the project-model types they need (`UnitKind`, `Roots`, `ParsedFile`, `ProjectPaths`, the `SchemaLock` shape); public surface enumerated and reviewed (R10.4-style) as part of this slice, per §3.2 | R3.7, R3.8, R3.9 | §3 settled |
| **P4.1** | The new `bynk-check` analysis entry point (§3.3(a)): discovery via `bynk-project`, resolve/check via `bynk-check`'s existing resolver/checker; `symbols.rs` (all of it), `schema_registry.rs`'s `reconcile`, and `diagnostics.rs`'s `ProjectAnalysis`/`ContextSequenceInfo`/`ContextBoundaryInfo` relocate here, alongside the entry point itself, per §3.2's corrected boundary. `run_checks` in `bynk-emit` is untouched. The file-reading asymmetry (§3.3(b)) carries over unchanged, not fixed here. Per §9's drift risk, this slice should also carry a differential fixture comparing this entry point's diagnostics against `run_checks`'s `Mode::Analyse` arm, or state why one isn't feasible | R3.7, R10.2 (partial) | P4.0 |
| **P4.2** | `bynk-ide` repoints at the new `bynk-check` entry point instead of `bynk-emit::analyse_project`; `bynk-emit` dependency and its justifying Cargo.toml comment deleted, flipping the existing `ide_emit_edge` CI gate (§8) to absent — no new gate is built | R10.2 | P4.1 |

No slice cites `R3.11`: §3.5 found it already closed by prior paydown (#1078), corrected directly in
`bynk-greenfield-compiler.md` by this settling pass, so there is no open rule left for a slice to close.

**Completion probe:** `ide_emit_edge` = absent. Already built (T0.0's probe harness, `xtask
greenfield-status`) and CI-gated (`greenfield_status_table_is_current`); reads **present** as of this
settling pass.

---

## 7. Out of scope — forward references, not refusals

| Item | Phase | Entry condition |
|---|---|---|
| Semantics in the checker (`validate.rs` dissolves; `run_checks`'s checking half folds onto P4.1's `bynk-check` entry point, deleting the duplication §3.3(a) accepts) | 5 | this track's probe (`ide_emit_edge`) reads absent |
| The full IR (reference Part 6) | 6 | phase 5 complete |
| The TypeScript tree and printer (reference Part 7) | 7 | phase 6 complete |
| Incrementality — `ProjectGraph{units: IndexVec<UnitId,...>, files, edges, contract}`, `UnitSignature(UnitId)`, the query firewall (§3.1, §3.4) | 8 | phases 3 and 4 complete |
| Unifying the CLI's and LSP's file-discovery seams (§3.3(b)) | *unopened — no trigger yet* | a named trigger (an observed CLI/LSP divergence), not appetite alone — the same discipline every track on this trajectory opens under |
| Publishing the emit ABI: the `@bynk/*` package, the three ADR 0200 layers, the build-time enumeration guard | *packaging track* | ADR 0310 merged; not gated on this track's phases |

---

## 8. Keeping the reference true

No new probe infrastructure is needed for this phase's gate — `ide_emit_edge` already exists, already
runs in CI (`greenfield_status_table_is_current`), and already reads the value this track needs to flip.
§3.5's Appendix D correction (R3.11) is applied directly in this settling pass, not left as a follow-up.
One addition worth raising under review, not yet decided: a probe for `bynk-project` crate existence
itself.

---

## 9. Risks

**P4.1 is larger than the draft's original sizing assumed.** §3.3(a)'s finding means this phase adds a new
`bynk-check` entry point duplicating part of `run_checks`, not merely relocating a self-contained
discovery function. The trajectory's relative-size-3 rating may understate this phase now that its real
shape is known — the same kind of recalibration `identity-and-totality.md` §9 found in the July review's
"three consumer crates" undercount for its own phase, caught here before slicing rather than during it.

**The temporary duplication §3.3(a) accepts is itself a risk if phase 5 doesn't land promptly, and it is
currently unmitigated.** Two analysis paths (`bynk-emit::run_checks`'s `Mode::Analyse` arm, kept for now,
and the new `bynk-check` entry point) can drift if a fix lands in one and not the other before phase 5
deletes the first, and the drift is silent: the two paths diverge in diagnostics the LSP shows vs. what
`bynkc` reports, which no existing test compares. `:6330`'s own comment — "rather than drift into a second,
independently-maintained `run_checks` call" — already treats a milder version of exactly this failure as a
known risk worth pinning a test against; the direct analogue here is a differential fixture: run a project
through both `bynk-emit::analyse_project_with` and the new `bynk-check` entry point and assert identical
diagnostics, self-deleting when phase 5 removes `run_checks`'s `Mode::Analyse` arm alongside it. P4.1 (§6)
should carry this fixture, or state why it isn't feasible (for instance if the return types deliberately
differ, per §3.2's composite finding) — left as a question for that slice, not decided by this settling
pass. Named here so a reviewer watches for it either way — phase 5 landing promptly is the actual fix.

**P4.2's mechanical repoint is small; P4.1's relocation is what actually needs coverage, and the two were
conflated.** `bynk-driver/src/discovery.rs`'s "100+ call sites" names the `diagnose_project(&root,
&HashMap::new())` pattern, not `analyse_project_with` specifically — verified: `analyse_project_with` has
exactly one production caller outside `bynk-emit`, `bynk-ide/src/lib.rs:320`, inside `diagnose_project_with`,
which `diagnose_project` (`bynk-ide/src/lib.rs:287`) wraps. 85 `diagnose_project(` call sites exist across
the tree (87 raw text matches include the function's own two definitions — `bynk-ide/src/lib.rs:287` and
a same-named `:410` test-module helper that itself calls the public one), all reaching
`analyse_project_with` indirectly through that one wrapper — none of them is something P4.2 edits. P4.2's
actual edit surface is the wrapper's one call. What the 100+ figure is
genuine evidence for is different and stronger stated correctly: that many tests exercise this analysis
path *without naming it*, so they'll catch behavioural drift from P4.1's relocation without failing on the
repoint itself — which is exactly the gap §4's LSP-surface-fixture requirement exists to close, not a
statement about how many call sites change.

**A naive "no literal `bynk_check` import" test is not a reliable module-boundary check, and P4.0 is where
this bites again if it's forgotten.** §3.2's own history is the evidence, and it took four review passes,
not one, to close: `schema_registry.rs`'s and `diagnostics.rs`'s real coupling hidden behind unqualified,
glob-sourced names (`super::UnitTable`, a bare `Arc<bynk_check::checker::Types>` field); `paths.rs`'s plain
`bynk-emit`-internal `json_string` dependency, a boundary cost no direction of the coupling test catches at
all; `Mode`, `ErrorSink` and `ProjectFailure`, all three of which pass the coupling test for the wrong
reason and were then wrongly grouped with the one item (`AttributedError`) that actually needed to move;
`ParsedFile`'s and `schema_registry.rs`'s private-field costs, which a checking-side function needs to
reach back *into* once the struct holding them moves out from under it; and `ContextBoundaryInfo`, which
passes every needs-based test yet stays behind because of who produces it, not what it needs. §3.2 now
states all of (a)–(e) plus the composite rule explicitly; P4.0's implementer needs to apply the full test
per function and per field, not per file, and not stop at the first criterion an item happens to fail. The
count itself — one weak test, missed in six distinct ways across four review passes — is worth taking as a
signal that a further one exists, not as evidence the search is exhausted.

**The evidence ages.** Every fact, line number and quotation in this doc was measured against the tree on
6 August 2026, on branch `track/project-model-settling`. Re-check before a slice proposal cites one, per
every prior track's own §9.

---

## 10. What this phase causes

In the trajectory's own words: "Cheap, and it is the *cause* the next phase treats. Context semantics
live in the emitter because the project model does." §3.3(a)'s finding sharpens this further than the
draft anticipated: it's not only that phase 5 cannot start cleanly without phase 4 — it's that phase 4's
own probe cannot close cleanly without borrowing a small, explicitly-scoped piece of phase 5's destination
shape (a `bynk-check`-side analysis entry point) ahead of time. That borrowed piece is named, bounded to
`Mode::Analyse`'s needs, and explicitly phase 5's to absorb and delete the duplicate of — not a backdoor
into doing phase 5 now.

---

## 11. ADRs

Per ADR 0167 step 2, load-bearing, hard-to-reverse decisions land before slicing. Three do, with this
settling pass (`level: patch`, no code); their numbers are assigned at merge by the stamp (ADR 0206), so
this doc refers to them by letter until they exist — the pattern `compiler-architecture.md` §11 and
`identity-and-totality.md` §11 both used.

- **ADR-A — phase 4 extracts today's name-keyed project-model logic into `bynk-project`; it does not
  build the reference's typed `ProjectGraph`/`UnitId`/`ContractHash` struct, which is phase 8's.** §3.1
  (Q1), §3.4 (Q4).
- **ADR-B — `bynk-project`'s module boundary is a (a)–(e)-plus-composites test, not "no literal
  `bynk_check` import."** Excludes `symbols.rs` in full and `schema_registry.rs`'s `reconcile` (but not its
  `parse`/`serialize`) as `bynk-check`-owned concerns reached through unqualified, glob-sourced names a
  literal-string grep misses (a). Excludes `ContextSequenceInfo` the same way, and `ContextBoundaryInfo` on
  a different ground — its fields pass every test but it's built mid-`run_checks`, a provenance argument
  (e) rather than a needs-based one. Excludes `ProjectAnalysis` as a composite that resolves upward to
  §3.3(a)'s new entry point rather than splitting or moving whole. Of `diagnostics.rs`'s four
  `bynk_check`-free items, only `AttributedError` moves — for a layering reason (§3.3(a)'s entry point must
  construct it without depending on `bynk-emit`), not because it's bookkeeping; `Mode` and `ErrorSink` stay
  with `run_checks`/`check_unit_files`, their only consumers (c), and `ProjectFailure` stays because
  nothing below `bynk-emit` reads it at all (c) — three items an earlier pass of this ADR grouped with
  `AttributedError` on the weakest of the tests below and got wrong on all three. Also flags `ParsedFile`
  and `schema_registry.rs`'s private-field construction as (d)-shaped costs — something staying behind
  needs to reach into what moved — and `paths.rs`'s dependency on `bynk-emit`'s own `json_string` as a (b)
  cost. §3.2 (Q2).
- **ADR-C — a new, narrow `bynk-check` analysis entry point closes R10.2 without moving `run_checks` or
  doing phase 5's checking-centralisation early; the resulting duplication with `run_checks`'s
  `Mode::Analyse` arm is deliberate, temporary, and phase 5's to remove.** §3.3(a) (Q3). The most
  load-bearing and hardest-to-reverse of the three: it fixes the shape `bynk-ide`'s live analysis path
  takes for the phase-4-to-phase-5 window. Its direct edit surface is one call site
  (`bynk-ide/src/lib.rs:320`, behind the stable `diagnose_project`/`diagnose_project_with` wrapper); its
  coverage surface is wider — 85 `diagnose_project(` call sites across the tree exercise this path
  without naming it, catching behavioural drift without gating the repoint itself.

Lands as `design/pending/project-model-settling.md`.

---

## 12. Retirement

Mirrors every prior track on this trajectory: retires when §6's probe (`ide_emit_edge`) reads absent and
every slice named to reach it has landed. The retirement PR removes this doc, appends its closing summary
to `../archive/retired-tracks.md`, and closes the spine issue ([#1107](https://github.com/accuser/bynk/issues/1107)).
