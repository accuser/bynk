# Incrementality — query granularity and the firewall (phase 8)

- **Status:** **Draft — Settling.** Spine open. This file is drafted for a settling draft PR
  (`tracks/README.md` step 2). Nothing here is decided — every question in §3 is open, each with
  the investigation it needs, not yet argued under review. Marking that PR ready for review
  asserts §3's questions are closed.
- **Spine:** [#1507](https://github.com/accuser/bynk/issues/1507).
- **Theme:** **Phase 8** of [`../bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md) —
  the last phase. Every compiler output is decomposed to the granularity at which it is
  *invalidated* — `Tokens(FileId)`, `Ast(FileId)` at file level; `UnitSignature(UnitId)` at unit
  level, declarations only, no bodies; `Body(DefId)`/`TypeOf(DefId)` at definition level;
  `ProjectGraph` at project level — with `UnitSignature` proved stable under any edit to a body
  inside that unit (the firewall). The scheduler that would actually memoise these queries — salsa,
  a hand-rolled table, or nothing at all — is a separable decision this phase commits the
  *granularity* for and explicitly defers. The trajectory's endpoint is the current compiler
  rebuilt on [`../bynk-greenfield-compiler.md`](../bynk-greenfield-compiler.md); this track walks
  the last phase of it, and its retirement is the trajectory's own.
- **Phase boundaries are safe stopping points** (trajectory §2). Phase 7 is retired
  ([#1293](https://github.com/accuser/bynk/issues/1293), retired 29 August 2026) and leaves a
  coherent compiler regardless of whether this phase lands.
- **Relates:** reference rules R3.13, R3.14, R3.15 in full (`bynk-greenfield-compiler.md` Part 3,
  lines 454–495), and the precondition table immediately above them (lines 440–449) — six
  preconditions for demand-driven recomputation, each already claimed closed by a named prior
  phase (P1 purity — phase 2; P3/R2.3 no ambient state — phase 0; R2.4 stable interned identity —
  phase 3; R2.5 total side tables — phase 3; R3.11 explicit cross-build state — phase 4; R3.3
  serialisable/comparable outputs — assumed, not separately tracked by any phase). Part 15.1's
  "demand-driven query framework" entry (lines 2195–2200) is this phase's own standing frame:
  "the **architecture is adopted**... the **framework is deferred**," triggered separately by "a
  hand-rolled memo table... measurably the bottleneck."
- **Precedent:** `the-typescript-tree.md` (retired) is this track's direct predecessor on the same
  trajectory. Its own §10 ("What this phase causes") named this phase's entry condition explicitly
  — "needs phases 3 and 4 **together with** this one, since a query firewall over an emitter that
  still writes strings ad hoc has no stable unit to memoize against" — and flagged, without
  deciding, that its own `Artefacts` type (R7.8) is "plausibly phase 8's `UnitSignature` target for
  the emit side." Its closing summary (`../archive/retired-tracks.md`) repeats the same open note
  and adds a second one: R8.16's own PARTIAL finding ([ADR 0388](../decisions/0388-typescript-tree-r8-scope.md))
  left "R8.16's data-model half" — a typed `ProjectGraph` — as phase 8's, matching
  [ADR 0326](../decisions/0326-project-model-phase4-scope.md)'s own deferral from phase 4: "extract
  today's name-keyed shape; the typed `ProjectGraph`/`UnitId`/`ContractHash` defer to phase 8."
  Neither note is settled here — both become §3's Q1. `incrementality.md` would be the thirteenth
  track to run the ADR 0167 flow from the start, after `compiler-architecture.md` (sixth),
  `identity-and-totality.md` (seventh), `content-ownership.md` (eighth), `project-model.md`
  (ninth), `semantics-in-the-checker.md` (tenth), `the-ir.md` (eleventh) and `the-typescript-tree.md`
  (twelfth).

### ADR 0076 trigger check

| Trigger | Met? |
|---|---|
| Spans several increments | **Yes, but the smallest margin on this trajectory.** Trajectory §5 rates this phase relative size 5, medium confidence — smaller than every phase from 3 onward, on the strength of "the preconditions are all paid by then." §1 below finds that claim substantially true, which is unusual: every other track on this trajectory found its row understated (phase 5, phase 6, phase 7 all revised upward). This is the first phase whose real surface may be close to its own row |
| Surface not yet settled | **No** — R3.13–R3.15 fully specify the destination: the four query levels, the firewall property, and the scheduler-deferred posture. What's open is the same "migration method" question every internal-architecture track on this trajectory has had: whether `bynk-project`'s existing untyped, name-keyed shape becomes `ProjectGraph`/`UnitSignature` by extension or by parallel construction, whether a memo table is this track's own deliverable or a later one's, and where the four query levels actually cut against the current tree |
| Security/safety boundary | **No** — this phase's invariant (`UnitSignature` stability, no stale re-parse) is a performance and architecture property. A defect here produces wrong or slow analysis, not an authorization or capability failure |

**One of three**, the same count as every internal-architecture track on this trajectory before it.

---

## 1. Why now

**Phase 7's retirement is the procedural trigger, and its own closing note is the substantive
one.** The trajectory's own rule — "a phase's track opens when the previous phase's probe reads
zero" — is satisfied: all four of phase 7's gated probes (`ts_writes` 809, `ts_any` 26,
`verbatim_origins` 1, `verbatim_sites` 2) retired at argued floors on 29 August 2026. But
`the-typescript-tree.md`'s own §10 went further, naming this phase's real precondition as phases 3
**and** 4 *together with* phase 7 — not the trajectory's own §4 diagram, which shows only "3 and 4
before 8" and no edge from 7. That gap is real, not decorative: `Artefacts` (R7.8, phase 7's typed
document set) is the only candidate today for what an incremental **emit** side would key on, and
nothing about it is settled here — see Q1.

**The rationale bugs R3.13 cites are half-closed, and the surviving half is the one the probe is
literally named for.** R3.13's own rationale cites two bugs: #65 ("every IDE query re-lexes and
re-parses from `&str`") and #62 ("`for_each_unit` never filters the cursor's own path out of
`files`, so the current file is parsed twice per keystroke"). Both are closed for **completion**
— issue [#733](https://github.com/accuser/bynk/issues/733) ("lsp: interactive requests trigger
full-project re-analysis and re-parse-from-disk, defeating the debounce") is closed, and
`bynk-ide/src/completion.rs`'s `for_each_unit` (`:1578`) now takes a caller-built `files` overlay
rather than re-sweeping disk, backed by a content-keyed cache, `PROJECT_UNIT_CACHE` (cap 4096,
`:1530`), through `cached_project_unit` (`:1537–1561`). **But the diagnostics path is untouched.**
`bynk-ide::diagnose_project_with` (`bynk-ide/src/lib.rs:299`) calls
`bynk_check::analysis::analyse_project` (`bynk-check/src/analysis.rs:262`) unconditionally on
every invocation — full discovery, parse, resolve and check over the whole project, with no cache
of any kind below the file-overlay level. "Keystroke-to-diagnostic latency," the probe's own name,
names exactly this path — the half of R3.13's own cited defect that ordinary paydown has not
touched. This is not the "phases 0–1 were fixed by accident" story the trajectory's own §3.0
tells about itself; it is the same story, half-told: one call site paid down, the sibling call
site — the one this phase's probe is named after — still live.

**None of R3.13's four query types exist as real identifiers anywhere in the workspace.** A
grep for `UnitSignature`, `ProjectGraph`, `TypeOf(`, `Body(`, `Tokens(FileId` as code, not doc
prose, across every crate returns zero hits. `bynk-project` (2,458 lines across nine files —
`discovery.rs` 616, `paths.rs` 623, `graph.rs` 331, `roots.rs` 319, `schema_registry.rs` 220,
`consistency.rs` 183, `json.rs` 94, `lib.rs` 52, `diagnostics.rs` 20) is the crate this phase's
`ProjectGraph` would sit in, per phase 4's own placement — but its closest types, `graph.rs`'s and
`roots.rs`'s own, are untyped and name-keyed, exactly as ADR 0326 described them at phase 4's
retirement: "extract today's name-keyed shape; the typed `ProjectGraph`/`UnitId`/`ContractHash`
defer to phase 8."

**One piece of the firewall already exists, and it is smaller and narrower than R3.14 needs.**
ADR 0200's cross-context contract hash — the piece R3.14's own rationale cites as "the query
already exists in substance" — is real: `combined_types_for` (`bynk-check/src/symbols.rs:1147`,
22 lines) folds a unit's own declared types with its direct `uses` targets' types into one
`HashMap<String, Arc<TypeDecl>>`, called from 24 sites across `bynk-check`, `bynk-emit`,
`bynk-lower`, `bynk-ide` and `bynkc/tests/contract_hash.rs`. It is genuinely signature-shaped
(declared types only, no bodies) and already keyed per-unit. But it computes **types only** — not
the full surface design notes §15's own annotation policy requires at "visible boundaries":
function and handler declarations, agent storage declarations, cross-context type references, and
capability sets via `given` (`design/bynk-design-notes.md:921–936`, "**Visible boundaries,
invisible internals**"). "Already substantially exists" is accurate; "already is
`UnitSignature`" is not — see Q1.

**No memoisation infrastructure of any kind exists below the file-overlay layer.** Zero
occurrences of `salsa` in any `Cargo.toml` or `Cargo.lock` in the repository. Zero occurrences of
`Database`, `MemoTable` or `memo_table` as identifiers. This is exactly R3.15's own stated
posture — "the architecture is adopted, the framework is deferred" — confirmed true today, not
merely claimed: there is nothing to migrate away from, no accreted hand-rolled cache to reconcile
with a real query decomposition, no framework dependency to argue about removing.

**The probe as literally named cannot be measured — not "not yet measured," structurally cannot
be, until the query levels it attributes latency to exist.** §3.0's baseline records "not
measured"; it still reads that way, because no `criterion` harness, no `Instant`/`Duration`
instrumentation and no benchmark of any kind touches the diagnostic path anywhere in the repo
today. This is not an oversight to fix with a stopwatch — "attributed by query level" presupposes
levels this phase has not built yet. Every other phase on this trajectory found its own gated
probe needed correcting once real work started (`ast_importers`'s floor of 5, four separate
argued floors at phase 7's retirement); this is the first phase where the correction is visible
*before* any slice lands rather than discovered mid-track — see Q5.

**One correction to the trajectory document itself, the same "evidence ages" housekeeping every
phase's opening has carried.** §9 describes `cargo xtask greenfield-status` as "specified, not
yet built (track slice T0.0)." It has existed since phase 0–2's own track (`xtask/src/greenfield_status.rs`,
3,419 lines, `design/greenfield-status.md` the committed, CI-gated output) and now runs thirteen
gated probes plus four trend-only probes. A phase 8 probe, once §3.5 settles its shape, would be
the fourteenth gated probe — following `ts_writes`/`ts_any`'s own precedent of landing as an
early, dedicated slice (P7.0/[#1296](https://github.com/accuser/bynk/issues/1296)), not a
retirement-week afterthought.

---

## 2. What this track is not

- **Not adopting salsa or any query framework.** R3.15 defers the scheduler explicitly, and its
  own trigger — "a hand-rolled memo table... measurably the bottleneck" — cannot have fired, since
  no memo table exists yet to be a bottleneck. This track's business is the granularity (R3.13)
  and the firewall (R3.14); the scheduler stays a separate, later decision with its own evidence
  requirement (keystroke-to-diagnostic latency on a multi-context project, attributed by level —
  data this track's own work is a precondition for collecting, not a deliverable of it).
- **Not rebuilding `bynk-project`'s discovery machinery wholesale.** `discovery.rs` and `paths.rs`
  (616 and 623 lines) stay; this track's business is giving their output a typed, query-shaped
  identity (`ProjectGraph`), not re-deriving what they discover.
- **Not a further crate re-graph.** R10.5 (`bynk-driver` consolidation) is named in the reference
  but has no trigger; this phase's crate surface is confined to `bynk-project` and `bynk-check`,
  the two crates R3.13's four query levels actually live in.
- **Not a new LSP-facing feature.** `editor-currency.md`'s surface — hover, completion, scaffolds,
  codelens — is untouched; this track's business is what those features call underneath, not what
  they show.
- **Not re-opening R8.16's per-consumer surface generation.** ADR 0388 split R8.16 down the
  middle: the generation logic itself (already correct, per `the-typescript-tree.md`'s own
  verification) stays phase 7's closed business; only its data-model half — a typed `ProjectGraph`
  to build it from, instead of an untyped `HashMap` — is this track's.

---

## 3. Design questions — open

None of the five below has been argued under a settling review yet. Each states the investigation
already done and the tension it leaves.

### 3.1 Q1 — Is `UnitSignature` built by extending ADR 0200's `combined_types_for` in place, or as a new, parallel type?

`combined_types_for` is real, per-unit, signature-shaped, and has 24 call sites depending on its
current, narrower shape (types only). R3.14 needs a wider surface — function/handler signatures,
storage declarations, cross-context type references, capability sets — the same list design notes
§15 already names as "required" annotation sites. Widening the existing function in place risks
disturbing 24 call sites that depend on its current contract (notably `bynkc/tests/contract_hash.rs`,
the fixture pinning ADR 0200's own correctness guarantee); building a new, parallel
`UnitSignature` type risks the "two facts, one hand-synced" failure phase 1's own invariant exists
to prevent (`design/bynk-compiler-trajectory.md` §3, Phase 1: "no fact exists in two hand-synced
copies where one can be derived"). This question also folds in `the-typescript-tree.md` §10's own
open note — whether `Artefacts` (R7.8) is a second, emit-side signature target, or whether one
`UnitSignature` on the check side is sufficient and the emit side reads through it.

**Needs settling:** extend `combined_types_for` into `UnitSignature`'s real identity function
(widening its return type and threading the additional annotation-policy fields through its 24
existing call sites), or introduce `UnitSignature` as a superset type constructed from
`combined_types_for`'s output plus the additional fields, leaving the existing function's contract
untouched for its current callers? And: does `Artefacts` need its own signature concept for the
emit side, or is that question out of this track's scope until a query on the emit side is
actually proposed?

### 3.2 Q2 — Does the file level (`Tokens(FileId)`, `Ast(FileId)`) need any work at all, or is the diagnostics path's missing cache this track's whole file-level business?

`bynk-ide/src/completion.rs`'s `PROJECT_UNIT_CACHE`/`cached_project_unit` already closes #733 for
completion requests — a real, working, content-keyed cache at file granularity. But it is
`bynk-ide`-local, ad hoc (a `HashMap` behind a cache constant, not a typed `Tokens(FileId)` query),
and — critically — `analyse_project`'s own diagnostics path does not use it: `diagnose_project_with`
calls `analyse_project` directly, with no cache anywhere in between. R3.13 names `Tokens(FileId)`/
`Ast(FileId)` as real query types with an invalidation contract, not an incidental
`bynk-ide`-local optimisation.

**Needs settling:** does this track give the diagnostics path the same cache completion already
has (the cheap fix, closing the probe's own namesake bug directly), generalise
`PROJECT_UNIT_CACHE`'s existing shape into a real `Tokens(FileId)`/`Ast(FileId)` query type both
paths share (closing the duplication as well as the bug), or treat file-level caching as
sufficiently addressed by #733's precedent and scope this track to Unit/Definition/Project levels
only, with the diagnostics-path gap filed as separate, smaller paydown?

### 3.3 Q3 — Is a hand-rolled memo table this track's own deliverable, or does the track stop once the query keys and the firewall exist?

R3.15's own rationale: "the hard part is R3.13/R3.14. Once queries have the right keys and the
firewall holds, a scheduler is a few hundred lines." That reads as license to stop before building
any scheduler at all — but a query decomposition with no caching behind it produces no measurable
change in `analyse_project`'s own behaviour, which makes the probe (§3.5) unmeasurable in
practice even after the types exist.

**Needs settling:** does this track's own completion criterion require a working, if minimal,
hand-rolled memo table wired into `analyse_project`'s call path (so the probe becomes measurable
at all), or does "the granularity is committed" mean the types and the firewall proof are
sufficient, with wiring a cache in as a phase-8-adjacent follow-on named but not built here?

### 3.4 Q4 — What does `UnitSignature` actually need to contain, and what proves R3.14 holds?

Design notes §15's required list (function/handler declarations, storage declarations,
cross-context type references, capability sets via `given`) is a starting point, not a verified
final field list — no prior phase has audited it against R3.14's own literal text ("stable under
an edit to any `Body(DefId)` within that unit"). Phase 4's own P4.1 slice built a differential
fixture (`bynk-check/tests/differential_analysis.rs`) to pin a new entry point's output against an
existing one; the same technique is a candidate for proving `UnitSignature` stability
mechanically — edit a body, assert the signature's hash is unchanged — rather than by inspection.

**Needs settling:** the field list, rule by rule against design notes §15 and the current
`AgentDecl`/service/handler declaration shapes; and whether a differential/property fixture
(edit-a-body-assert-signature-unchanged) is this track's own required proof or an optional
strengthening.

### 3.5 Q5 — What does the gated probe actually measure, given the literal one can't be?

"Keystroke-to-diagnostic latency by query level" needs query levels and a scheduler wired in
before it means anything (Q3). Every other phase on this trajectory corrected an unmeasurable-
or-wrong-shaped probe at retirement, once real work exposed the gap (`ast_importers`'s floor of 5,
`ts_writes`/`ts_any`/`verbatim_sites`/`verbatim_origins`'s four argued floors); this phase can name
the correction now, before any slice lands, rather than discover it mid-track.

**Needs settling:** a structural gated probe — candidates include "the four query types exist as
real Rust types" (a one-time existence check, not a trend) and "a differential fixture proves
`UnitSignature` stability under body edits" (Q4) — with the literal latency number staying a
trend-only probe (following `test_density`'s own precedent in `greenfield_status.rs`) reported
once instrumentation exists, not gated on, since nothing in this track's own scope requires
building a benchmark harness to satisfy R3.13/R3.14's own text.

---

## 4. Posture

Extends ADR 0059 as amended by ADR 0309, the same standing properties every track on this
trajectory has run under: behaviour-preserving by default (a slice that changes observable
behaviour states so, ships a fixture, gets a CHANGELOG entry — the diagnostics-path cache in §3.2
is the one slice here with real behavioural stakes, since a wrong cache invalidation would produce
stale diagnostics silently); short-lived branches, one PR per slice; no per-increment ADRs beyond
what §11 front-loads; every slice citing `Closes-Rule:`.

---

## 5. The completion criterion

Pending Q5, not asserted here: the working candidate is two probes, one gated and structural, one
trend-only:

- **`query_types_exist`** (candidate name) — do `UnitSignature`, `ProjectGraph`, `Body`/`TypeOf`
  keyed types exist as real Rust types in `bynk-project`/`bynk-check`, with `UnitSignature`
  proved stable under a body edit by a differential or property fixture (Q4)? A one-time
  existence-and-proof check, not a count that trends toward zero the way every prior phase's
  probe has — the first phase on this trajectory whose gate is a proof rather than a shrinking
  number, since R3.13/R3.14 describe a property to construct, not a defect to exhaust.
- **`keystroke_latency`** (trend-only, not gated) — reported once a scheduler exists to measure
  (Q3), following `test_density`'s own precedent for trend-reported-not-gated probes in
  `xtask/src/greenfield_status.rs`.

Both need adding to `xtask/src/greenfield_status.rs` (14th/15th probes) — a candidate first slice,
mirroring `the-ir.md`'s P6.0 and `the-typescript-tree.md`'s P7.0 both being real instrumentation
work, not ceremony.

---

## 6. Slice decomposition — candidate, pending §3

None of the below is accepted yet; §3 governs both which of these actually ship and their real
order. Provisional, in rough dependency order:

| Slice (candidate) | What it does | Rules | Gated on |
|---|---|---|---|
| **P8.0** | Structural probe(s) added to `xtask/src/greenfield_status.rs`, per §5 | instrumentation | Q5 |
| **P8.1** | `UnitSignature` type built — extension or parallel, per Q1's answer — carrying design notes §15's required-annotation fields | R3.14 | Q1, Q4 |
| **P8.2** | Differential/property fixture proving `UnitSignature` stability under a body edit | R3.14 | P8.1, Q4 |
| **P8.3** | Typed `ProjectGraph` in `bynk-project`, replacing `graph.rs`/`roots.rs`'s untyped shape (ADR 0326/ADR 0388's own deferral) | R3.13, R8.16 (data-model half) | — |
| **P8.4** | File-level fix for the diagnostics path (§3.2's answer): either share `PROJECT_UNIT_CACHE`'s existing cache or a real `Tokens(FileId)`/`Ast(FileId)` query type | R3.13 | Q2 |
| **P8.5** | Definition-level queries (`Body(DefId)`, `TypeOf(DefId)`) decomposed out of `analyse_project`'s current monolithic pass | R3.13 | P8.1 |
| **P8.6** | Hand-rolled memo table wired into `analyse_project`'s call path, if Q3 settles that this track builds one | R3.15 | Q3, P8.1–P8.5 |

**Working slice-count estimate: 6–9.** Close to the trajectory's own row (relative size 5, medium
confidence) — the first phase on this trajectory where the opening measurement does not obviously
contradict the trajectory's own sizing, though Q3's answer (whether a memo table ships here) is
the single biggest swing factor.

---

## 7. Out of scope — forward references, not refusals

| Item | Phase | Entry condition |
|---|---|---|
| A demand-driven query framework (salsa or equivalent) | *unopened — no trigger yet* | R3.15's own trigger: a hand-rolled memo table (this phase's own P8.6, if built) measurably becomes the bottleneck |
| R10.5's `bynk-driver` consolidation | *unopened — no trigger yet* | named in the reference (Part 10) but not this phase's own invariant |
| A lossless CST (rowan) | *unopened — no trigger yet* | Part 15.1's own trigger: per-file reparse timings on the largest real file measured as costly — this phase's own P8.4 is the first place such a timing could be collected, but collecting it is not this phase's job |

---

## 8. Keeping the reference true

This phase's own probe needs building from nothing — `xtask/src/greenfield_status.rs` has
thirteen gated and four trend probes today, none phase-8-shaped. P8.0 is this track's own first
slice for exactly that reason, the same "instrumentation first" precedent `the-ir.md`'s P6.0 and
`the-typescript-tree.md`'s P7.0 both set.

One correction this track's settling pass carries, the same "evidence ages" housekeeping every
prior phase's opening has done: §9's claim that `cargo xtask greenfield-status` is "specified, not
yet built" is stale — it has existed since the phase 0–2 track and gates thirteen probes today.

---

## 9. Risks

**Q1's answer touches 24 existing call sites if extension is chosen.** `combined_types_for` is
depended on by `bynk-check`, `bynk-emit`, `bynk-lower`, `bynk-ide` and `bynkc/tests/contract_hash.rs`
— the fixture pinning ADR 0200's own correctness guarantee. Widening its signature without
breaking any of the 24 needs care; this is the reason Q1 is a front-loaded ADR candidate rather
than a slice-time decision.

**The probe's own shape is unresolved going into this doc's own settling review**, more than any
prior phase's opening. Every other phase's settling review closed its open questions against a
probe whose *target* was already fixed (0, or an argued floor of an existing count); this phase's
own Q5 is deciding what the gate even measures. If §3's review cannot converge on a structural
probe that is both meaningful and buildable without a scheduler, this phase may need its
invariant restated before any slice is proposed — worth surfacing to the settling PR's reviewer
explicitly rather than assumed resolvable.

**The evidence ages.** Every fact, line number and count in this doc was measured against `main`
on 29 August 2026. Re-check before a slice proposal cites one, per every prior track's own §9.

---

## 10. What this phase causes

**This is the trajectory's last phase.** Its retirement is not a forward reference to a ninth
phase — trajectory §1's endpoint ("the compiler Bynk ships today, feature for feature, rebuilt on
the architecture in `bynk-greenfield-compiler.md`") is reached when this phase's probe (§5) is
satisfied, not before. What this phase does *not* close, named so a future reader does not mistake
silence for completeness: R3.15's scheduler decision (§7, its own separate trigger), R10.5's
`bynk-driver` consolidation (§7, no trigger), and rowan's lossless-CST question (§7, its own
trigger against real reparse timings this phase's own P8.4 could — but is not required to —
start collecting).

---

## 11. ADRs

Per ADR 0167 step 2, load-bearing, hard-to-reverse decisions land before slicing. Candidates,
pending §3 actually closing under review (numbers assigned at merge by the stamp, per every prior
track's own convention of referring to them by letter until then):

- **ADR-A** — whether `UnitSignature` extends `combined_types_for` in place or is built as a
  parallel type (§3.1, Q1). The most load-bearing of the set — it decides whether 24 existing call
  sites change shape or a new type is introduced beside them, and whether `Artefacts` (phase 7)
  gets its own signature concept or none.
- **ADR-B** — whether this track builds a hand-rolled memo table or stops at the query
  decomposition and firewall proof (§3.3, Q3). Directly determines the completion criterion's own
  shape (§5) and the slice count (§6).
- **ADR-C** — the gated probe's final shape (§3.5, Q5) — likely written once P8.0's instrumentation
  work clarifies what is actually measurable, the same way `the-ir.md`'s own floor-of-5 argument
  was P6.58, not a day-one ADR.

## Threat model

None, because this phase's invariant — `UnitSignature` stability, cache correctness — is a
performance and architecture property. A defect here produces wrong or stale analysis results
(an availability/correctness failure inside the editor), not an authorization, capability or data-
exposure failure; no new trust boundary is introduced or crossed.

---

## 12. Retirement

Mirrors every prior track on this trajectory: retires when §5's structural probe reads satisfied,
with the trend probe reporting whatever number instrumentation allows at that point, not gated on
it. The retirement PR removes this doc, appends its closing summary to
[`../archive/retired-tracks.md`](../archive/retired-tracks.md), and closes the spine issue — and,
uniquely among this trajectory's tracks, closes `../bynk-compiler-trajectory.md` itself, per §1's
own endpoint.
