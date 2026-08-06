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
| R3.7 | no `bynk-project` crate | medium |
| R3.8 | `read_project_paths` still total | small |
| R3.9 | `Roots` still models the removed role split | small |
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
(`bynk-emit/src/project.rs:3644`), the ~2,200-line function both `compile_project` (line 584) and
`analyse_project_with` (line 979) call, does discovery, parsing, resolution *and* checking as one
sequence — there is no seam inside it that separates "project model" from "checking" at the granularity
this phase needs.

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
> found something bigger: it calls `run_checks` — the same ~2,200-line function `compile_project` calls —
> which performs discovery, parsing, resolution *and* checking as one sequence. There is no existing
> boundary inside it at "project model" granularity. `bynk-ide`'s dependency on `bynk-emit` is not, as the
> draft assumed, a dependency on a *discovery* function this phase can cleanly relocate — it is a
> dependency on a function that also checks, which `bynk-project` (sitting below `bynk-check`) cannot
> absorb, and which `bynk-check` (this phase's stated non-target) is the only place that legally can. §3.3
> below is the resulting decision, and it is the one place this settling pass adds work the draft didn't
> foresee rather than only removing or confirming what it already posed.
>
> Q2's investigation surfaced a smaller version of the same shape: `symbols.rs`'s `ProjectIndex` and
> `IndexBuilder` are not `bynk-emit` types at all — they're defined in `bynk-check/src/index.rs`. This
> resolved Q2 by removing an option the question posed (`symbols.rs` moving into `bynk-project`), rather
> than by choosing among the options it posed. A second review pass found the same shape had been missed
> twice more, both times because a literal `grep bynk_check` is the wrong test: `schema_registry.rs`
> reaches `UnitTable` (itself `bynk_check`-coupled) via an unqualified `use super::UnitTable`, and
> `diagnostics.rs` — never assigned a home at all in the first pass — is split three checking-result types
> (`ProjectAnalysis`, `ContextSequenceInfo`, `ContextBoundaryInfo`) from four plain bookkeeping ones. §3.2
> now states the real test this doc should have used from the start: needs nothing that is an *output* of
> resolution or checking, not "no literal `bynk_check` import."
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
literal-string grep for `bynk_check` misses. `schema_registry.rs`'s other two public functions, `parse`
(`:409`) and `serialize` (`:446`), take no `UnitTable` and stay clean — this is the module's own documented
seam ("this module touches no disk … `parse`/`serialize` are pure"), and it turns out to also be the seam
between what's project-model and what isn't. `graph.rs` and `consistency.rs` check out clean on re-read —
their `use super::*` only resolves to `ParsedFile`/`CompileError`/`Span`/`HashMap`.

The same question needed asking of `diagnostics.rs`, which the first pass never assigned a home at all.
Of its seven top-level items, four are pure bookkeeping (`Mode`, `AttributedError`, `ProjectFailure`, plus
the crate-private `ErrorSink`) — no `bynk_check` anywhere. The other three are checking results in
substance: `ProjectAnalysis` carries `pub ty_intern: Arc<bynk_check::checker::Types>` plus `ProjectIndex`,
`FileHints`, `FileExprTypes`, `FileLocals`, `FileRequirements` — every field a checker output;
`ContextSequenceInfo` carries `resolver::CrossContextInfo`; `ContextBoundaryInfo` is AST-typed but built
during `run_checks`'s `Checked` arm from `combined_types_for`/`unit_tables`, the same checking pass, not
during discovery.

Being `bynk_check`-free isn't the whole test, though, and `Mode` shows the other way it can fail: every use
site — `run_checks`'s own parameter and match arms (`project.rs:3230,3476,3495,3539,3556,3654,3961`), and
its six callers (`:592,665,724,795,987`, plus two in tests) — is inside `project.rs` itself. None of
`discovery.rs`/`graph.rs`/`paths.rs`/`consistency.rs` reference it, and §3.3(a)'s new `bynk-check` entry
point is scoped to `Mode::Analyse`'s behaviour specifically, so it has no branch to take and no reason to
carry the enum either. `Mode` is a statement about how `run_checks` is driven, not a fact about the
project — the checking-coupling test screens out items reaching *up* into `bynk-check`, but says nothing
about an item that belongs with its one orchestrator regardless of which crate that orchestrator is in.

**Decision:** `bynk-project` receives `discovery.rs`, `graph.rs`, `paths.rs`, `consistency.rs`;
`schema_registry.rs`'s `SchemaRegistry` type plus `parse`/`serialize` only (not `reconcile`); and
`diagnostics.rs`'s `AttributedError`, `ErrorSink`, `ProjectFailure` (not `Mode`, which stays with
`run_checks` in `bynk-emit` — its only consumer) — plus the project-model types declared directly in
`project.rs` these depend on (`UnitKind`, `Roots`, `ProjectPaths`, the
`CompileOptions::schema_registry`/`SchemaLock` shape). Staying on the checking side, to become part of
§3.3's new `bynk-check` entry point rather than `bynk-project`: `symbols.rs` in full, `schema_registry.rs`'s
`reconcile`, and `diagnostics.rs`'s `ProjectAnalysis`, `ContextSequenceInfo`, `ContextBoundaryInfo` — every
one of them either builds or is a checker-output type. `ProjectAnalysis` in particular is the return type
§3.3(a)'s entry point already commits to producing "the analogue of," so its new home was already implied;
this just makes it explicit rather than leaving it unassigned. `project.rs`'s orchestration (`run_checks`,
`compile_project`) does not move as a body; it becomes a caller of `bynk-project`'s functions instead of
owning that logic inline.

The rule this leaves, stated once so P4.0 doesn't have to rediscover it per file, is two-sided — one
direction catches `schema_registry.rs`/`diagnostics.rs`'s checking coupling, the other catches `Mode`'s: a
type or function moves to `bynk-project` only if (a) it needs nothing that exists solely as an output of
resolution or checking (`UnitTable`, `Ty`/`TyId`, `ResolverMethodTable`, anything under `bynk_check::`,
however it's named at the use site — not "no literal `bynk_check` import," which this section's own first
pass shows is too weak on its own), **and** (b) something below `run_checks` actually consumes it — not
merely "nothing above `run_checks` needs it to stay," which is a test `Mode` passes for the wrong reason:
it's `bynk_check`-free, but it's also a fact about how `run_checks` is driven, not a fact about the
project, and its only readers are `run_checks` itself.

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
call sites total: five in production code — one `Mode::Build` (`compile_project`, `:584`) and four
non-build (`check_project` `:657`, `compile_in_memory` `:716`, `analyse_in_memory_with_types` `:787`,
`analyse_project_with` `:979`) — plus two more inside the file's own `#[cfg(test)]` module (`:5844`,
`:5988`). Every one of the six non-build call sites passes an explicit `&SchemaLock::Off` — `:673`, `:729`,
`:800`, `:995` in production, `:5857` and `:6001` in tests — not an ambient read, six independent explicit
values at six independent call sites, all agreeing. `:3638`'s own comment attributes the `None`-when-`Off`
shape to #1078. The LSP path (`analyse_project_with`) does non-bailing live analysis, not a build; it has
no business persisting a build lockfile, so `Off` is correct-by-design there, not a defect — and the same
holds for the other five non-build call sites on the same evidence. (`:6330`'s comment — "rather than drift
into a second, independently-maintained `run_checks` call" — is itself a sign the test suite already
treats a stray extra call site as a known risk worth pinning against, which the two test-module calls
above are not instances of: both delegate through the same `run_checks`, not a copy of it.)

**Decision:** the Appendix D row ("schema registry read/written ambiently … thread two values") is closed
by paydown that postdates whenever that row was last verified — corrected directly in
`bynk-greenfield-compiler.md` as part of this settling pass rather than left flagged with no landing
place, since the fix is a one-line edit and this settling review is the review that would otherwise have
had to name a follow-up owner for it. §3.3's new `bynk-check` entry point carries the same explicit-`Off`
behaviour forward (nothing about relocation changes it). Because R3.11 is now closed rather than merely
"appears closed," P4.0 (§6) does not cite it as a rule it closes — relocating `schema_registry.rs`'s
`parse`/`serialize` doesn't close an open rule, since none was open by the time P4.0 lands.

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
dependency line — and the comment justifying it — are gone, `bynk-project` is a real workspace member, the
new `bynk-check` analysis entry point is what `bynk-ide` actually calls, and the manifest-level check
R10.2 asks for (a CI gate on the dependency graph, not just a probe reading zero once) exists.

---

## 6. Slice decomposition

§3 is settled; all three slices below are scoped and buildable as drafted, not forward references — each
still becomes its own increment proposal, and `accepted` on that sub-issue (not this doc settling) is the
actual approval to build it, per this doc's own Status block.

| Slice | What lands | Rules | Gated on |
|---|---|---|---|
| **P4.0** | `bynk-project` crate skeleton; `discovery.rs`, `graph.rs`, `paths.rs`, `consistency.rs` relocated with minimal reshaping, plus `schema_registry.rs`'s `SchemaRegistry`/`parse`/`serialize` (not `reconcile`) and `diagnostics.rs`'s `AttributedError`/`ErrorSink`/`ProjectFailure` (not `Mode` — stays with `run_checks` in `bynk-emit`; not `ProjectAnalysis`/`ContextSequenceInfo`/`ContextBoundaryInfo`), plus the project-model types they need (`UnitKind`, `Roots`, `ProjectPaths`, the `SchemaLock` shape); public surface enumerated and reviewed (R10.4-style) as part of this slice, per §3.2 | R3.7, R3.8, R3.9 | §3 settled |
| **P4.1** | The new `bynk-check` analysis entry point (§3.3(a)): discovery via `bynk-project`, resolve/check via `bynk-check`'s existing resolver/checker; `symbols.rs` (all of it), `schema_registry.rs`'s `reconcile`, and `diagnostics.rs`'s `ProjectAnalysis`/`ContextSequenceInfo`/`ContextBoundaryInfo` relocate here, alongside the entry point itself, per §3.2's corrected boundary. `run_checks` in `bynk-emit` is untouched. The file-reading asymmetry (§3.3(b)) carries over unchanged, not fixed here | R3.7, R10.2 (partial) | P4.0 |
| **P4.2** | `bynk-ide` repoints at the new `bynk-check` entry point instead of `bynk-emit::analyse_project`; `bynk-emit` dependency and its justifying Cargo.toml comment deleted; CI dependency-graph gate added | R10.2 | P4.1 |

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

**The temporary duplication §3.3(a) accepts is itself a risk if phase 5 doesn't land promptly.** Two
analysis paths (`bynk-emit::run_checks`'s `Mode::Analyse` arm, kept for now, and the new `bynk-check` entry
point) can drift if a fix lands in one and not the other before phase 5 deletes the first. Named here so a
reviewer watches for it, not because this phase can prevent it — phase 5 is the actual fix.

**Relocating `analyse_project`'s pipeline (P4.1) still touches real call-site surface.** `bynk-driver/src/discovery.rs`'s
own count puts `analyse_project_with`'s callers at 100+ across `bynk-ide`'s inline tests, `bynk-lsp/tests`
and `bynkc/tests`. Even with behaviour unchanged, a relocation this wide needs the LSP-surface-fixture
coverage §4 commits to, not just a byte-identical-output check.

**A naive "no literal `bynk_check` import" test is not a reliable module-boundary check, and P4.0 is where
this bites again if it's forgotten.** §3.2's own history is the evidence: it took two review passes to find
`schema_registry.rs`'s and `diagnostics.rs`'s real coupling, both hidden behind unqualified,
glob-sourced names (`super::UnitTable`, a bare `Arc<bynk_check::checker::Types>` field with no re-export
trail to grep for). P4.0's implementer needs the test §3.2 states explicitly — no dependency on anything
that is an *output* of resolution or checking — applied per function and per field, not per file, or the
same class of mistake ships as code instead of being caught in review.

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
- **ADR-B — `bynk-project`'s module boundary is "needs nothing that is an output of resolution or
  checking," not "no literal `bynk_check` import."** Excludes `symbols.rs` in full, `schema_registry.rs`'s
  `reconcile` (but not its `parse`/`serialize`), and `diagnostics.rs`'s `ProjectAnalysis`/
  `ContextSequenceInfo`/`ContextBoundaryInfo` (but not its `Mode`/`AttributedError`/`ErrorSink`/
  `ProjectFailure`) — all `bynk-check`-owned concerns reached through unqualified, glob-sourced names a
  literal-string grep misses. §3.2 (Q2).
- **ADR-C — a new, narrow `bynk-check` analysis entry point closes R10.2 without moving `run_checks` or
  doing phase 5's checking-centralisation early; the resulting duplication with `run_checks`'s
  `Mode::Analyse` arm is deliberate, temporary, and phase 5's to remove.** §3.3(a) (Q3). The most
  load-bearing and hardest-to-reverse of the three: it fixes the shape `bynk-ide`'s live analysis path
  takes for the phase-4-to-phase-5 window, across 100+ existing call sites.

Lands as `design/pending/project-model-settling.md`.

---

## 12. Retirement

Mirrors every prior track on this trajectory: retires when §6's probe (`ide_emit_edge`) reads absent and
every slice named to reach it has landed. The retirement PR removes this doc, appends its closing summary
to `../archive/retired-tracks.md`, and closes the spine issue ([#1107](https://github.com/accuser/bynk/issues/1107)).
