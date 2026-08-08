# Semantics in the checker — migrating to the greenfield reference (phase 5)

- **Status:** **Settled — Slicing on merge.** §3's five questions were argued under a settling review on
  this branch. Q2 surfaced a finding the original draft didn't anticipate — the emission/checking
  boundary P4.1 drew informally is already a documented, fixture-pinned, CHANGELOG-named live regression
  in the editor, not just an architectural judgment call — see the provenance note at the head of §3.
  Merging settles **direction**; it is not a build authorisation. Each slice is still an ordinary
  increment proposal, and `accepted` on that sub-issue is the approval to build.
- **Spine:** [#1126](https://github.com/accuser/bynk/issues/1126).
- **Theme:** **Phase 5** of [`../bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md) —
  everything that makes Bynk *Bynk* is checked in one crate, `bynk-check`; `bynk-emit` emits no
  diagnostic and no crate reaches back across a boundary to drive the checker. The trajectory's
  endpoint is the current compiler rebuilt on
  [`../bynk-greenfield-compiler.md`](../bynk-greenfield-compiler.md); this track walks phase 5 of it.
- **Phase boundaries are safe stopping points** (trajectory §2). Phase 4 is retired
  ([#1107](https://github.com/accuser/bynk/issues/1107), settled 6 August 2026, retired 8 August 2026)
  and leaves a coherent compiler regardless of whether this phase lands.
- **Relates:** reference rules R3.5, R4.6, R4.11, R10.1, R10.4 (the trajectory's own phase-5 list,
  trajectory §3) — **Q1 settles that R4.6/R4.11/R10.4 stay in scope narrowly, as verify-only items**;
  Appendix D already marks all three ✅ landed elsewhere. [ADR 0200](../decisions/0200-cross-context-contract-hash.md)
  is not this phase's (phase 8's, per `project-model.md` §3.4). R3.11's lockfile contract (already
  closed) is directly adjacent to candidate slice P5.2 below.
- **Precedent:** `project-model.md` (retired) is this track's direct predecessor on the same trajectory.
  Its own closing summary names the handoff explicitly: "Opens phase 5 (semantics centralisation —
  `validate.rs` dissolves into `bynk-check`)," and its §3.3(a) named a specific debt for this phase to
  collect. §1 finds that debt already substantially paid, by the implementing slice rather than by this
  track. `semantics-in-the-checker.md` is the tenth track to run the ADR 0167 flow from the start, after
  `compiler-architecture.md` (sixth), `identity-and-totality.md` (seventh), `content-ownership.md`
  (eighth) and `project-model.md` (ninth).

### ADR 0076 trigger check

| Trigger | Met? |
|---|---|
| Spans several increments | **Yes, if narrowly** — trajectory §5 rates this phase relative size 12, confidence *low*. §1/§6 replace that estimate with a concrete, named seven-category decomposition, most of it small; only category 7 (test/integration-suite processing, P5.4) is genuinely large |
| Surface not yet settled | **No** — the destination (R3.5) is specified; §3 closes the question of which of `bynk-emit`'s remaining diagnostic sites are R3.5's business |
| Security/safety boundary | **No** — the three gates R4.6 protects are already closed (phase 1). This phase's own risk in that direction is regression, not a new gate (§9) |

**One of three, same count as every internal-architecture track on this trajectory before it.**

---

## 1. Why now

**Phase 4's own closing note opens this phase by name**, and names a specific debt: a temporary
duplication between a new `bynk-check` analysis entry point and `bynk-emit::run_checks`'s
`Mode::Analyse` arm, accepted deliberately in `project-model.md` §3.3(a), "phase 5's to remove."

**That debt is already substantially paid — by the implementing slice, ahead of the design doc's own
commitment.** `project-model.md` §3.3(a)'s decision text was explicit: *"`run_checks` itself stays in
`bynk-emit`, unchanged."* The shipped code disagrees. `bynk-check/src/project_model.rs`'s own module doc
(1,941 lines, verified live) records that P4.1 (#1115) relocated the whole discovery→parse→group→resolve
pipeline into `bynk-check::project_model`, with `bynk-emit`'s `run_checks` now a caller rather than an
owner. Neither `project-model.md` (deleted at retirement) nor the trajectory doc's phase-5 sizing (§5:
relative size 12) was updated to reflect it — trajectory §9's own "the evidence ages" warning, caught
here before a slice proposal cites the stale figure.

**The live probe confirms the shape.** `design/greenfield-status.md`, regenerated at commit `7d2c382c`
(P4.2, the same commit series that closed phase 4), gated and CI-checked
(`greenfield_status_table_is_current`):

```
emit_diagnostics: bynk-emit=49/53, bynk-check=346/353 (true/naive)
```

49 registered `bynk.*` codes still originate in `bynk-emit` — down from the trajectory's own §3.0
baseline of 200. Two independent moves account for the drop: P4.1 (#1115, above) and an earlier,
untracked pass — commit `ac3daca5`, "Wave 5: layering for the compiler pipeline review (#956)" — which
relocated `icu.rs` and `websocket::analyse_open_shape` into `bynk-check` and gave `Ctx` its `pub(crate)`
visibility, both verified true today and both named in this phase's own invariant.

**`bynk-check/src/analysis.rs`'s own module doc is the authoritative accounting of what's left, and it
is far more precise than anything a fresh grep sweep would produce.** It enumerates exactly seven
categories of whole-project checking that `run_checks`'s `Mode::Analyse` arm performs and the new
`bynk-check::analyse_project` entry point (P4.1/P4.2) does not port:

| # | Category | Owning function | Status |
|---|---|---|---|
| 1 | Schema-registry reconciliation | `schema_registry::reconcile` | Gap in name only — unreachable on the analyse path anyway (`analyse_project_with` always passes `SchemaLock::Off`) |
| 2 | `messages` bundle validation | `check_messages_bundles` | **Live gap** |
| 3 | Locale bundle ambiguity | `check_locale_bundle_ambiguity` | **Live gap** |
| 4 | Event-subscription validation | `check_event_subscriptions` | **Live gap** |
| 5 | Platform-lock enforcement | `check_platform_lock` | Gap in name only — `analyse_project_with` hardcodes `Platform::default()`/`BuildTarget::Bundle`, so `lock_violation` can never fire on this path for any project |
| 6 | Function-type-boundary checks | `check_function_type_boundaries` | **Live gap** — reached only through an optional hook the new entry point passes as `None` |
| 7 | Test/integration-suite processing | `process_tests`/`process_integration_tests` | **Live gap**, plus a second consequence: go-to-definition/find-references/completion for a test-only binding also regress, since both functions also populate `RefSink` |

**This is not a hypothetical architectural question — it is an already-shipped, already-diagnosed, already
CI-pinned regression**, confirmed directly:

- `CHANGELOG.md` (the v0.247.something entry covering #1122) names the five live gaps explicitly under
  "Language server," states they are "accepted, tracked debt (`design/tracks/project-model.md` §3.3(a)),
  closed when phase 5 of that track ports these checks into `bynk-check`'s analysis entry point," and
  tells users to run `bynkc build`/`bynkc test` to see the diagnostics meanwhile.
- `bynk-lsp/tests/analysis_residual_gap.rs` pins each live-gap category as a direct assertion that
  today's `bynk_ide::diagnose_project` output lacks it, sourced from real `bynkc/tests/fixtures/negative`
  cases — "pin the gap as an assertion, not an absence." Its own header records a correction made while
  grounding the fixture set: `check_platform_lock` was initially counted as a sixth live regression, then
  found to be gap-in-name-only for the same reason as schema-registry reconciliation.
- `bynk-check/tests/differential_analysis.rs`'s `new_entry_point_omits_test_body_diagnostics` pins
  category 7's divergence directly against `analyse_project_with`.

This phase's job, in concrete terms, is closing that named debt: porting categories 2, 3, 4, 6 and 7 so
the editor sees them again, and categories 1 and 5 for R3.5 compliance even though nothing currently
observes the improvement.

**Two sites from a first-pass file grep turned out not to belong to any of the seven categories, and
needed direct verification rather than being taken at naive-grep face value:**

- `bynk-emit/src/emitter/emit.rs`'s `bynk.emit.unresolved_cross_context_signature` is **not a registered
  diagnostic at all** — verified directly: both occurrences are inside a `panic!`/`assert_eq!` message
  string, never a `CompileError::new(...)` construction. Its own comment explains why: "the checker
  resolved this call before the emitter ran, so an absent signature is the emitter disagreeing with the
  checker — a compiler bug. Fail instead [of silently degrading]." It is an internal-consistency
  assertion, correctly outside R3.5's scope, and outside `emit_diagnostics`'s true/naive gap.
- `bynk-emit/src/project/tests_emit.rs` was mischaracterised in an earlier pass over this doc as
  fixture/test noise. It is not — despite the filename, it holds `process_tests`/`process_integration_tests`
  (category 7 above), real production code with roughly 19 genuine `bynk.*` codes (`bynk.suite.*`,
  `bynk.stub.*`, `bynk.tier.*`, `bynk.test.*`, `bynk.contract.*`, `bynk.history.*`, `bynk.val.*`,
  `bynk.property.*`). Corrected here rather than left silently wrong.

---

## 2. What this track is not

- **Not phase 6 (the IR).** `bynk-emit` naming no AST type and `CommitShape` becoming data are untouched
  here — this phase only removes checking, per the trajectory's own "5 before 6" ordering argument.
- **Not new construction against R4.6, R4.11 or R10.4.** Appendix D already marks all three ✅ landed.
  This track's business with them (P5.5, §6) is verification only: confirming the relocations in §6
  don't regress them by hand-rolling a new `ResolvedCommons`-shaped struct at a new call site.
- **Not a `bynk-emit` crate rename.** §3.5 (Q5) settles this: R10.1 closes with an accurate crate-level
  doc/`Cargo.toml` description once checking leaves, not a restructuring toward `bynk-driver`.
- **Not R3.2** (`bynk check` still runs the bailing path) — a different rule, untouched by this phase's
  scope.
- **Not `emit.rs`'s internal-consistency assertion.** §1 found it isn't a registered diagnostic at all;
  it needs no relocation and carries no rule.
- **Not phases 6–8.** Carried as forward references in §7, not refusals.

---

## 3. Design questions — settled

> **Provenance: Q2 changed shape during this settling pass, in the direction of more evidence, not
> more doubt.**
>
> The draft framed Q2 as an open judgment call — does R3.5 apply uniformly, or does `bynk-emit` get a
> carve-out for checks "only the `Mode::Build` path needs"? Reading `bynk-check/src/analysis.rs`'s own
> module doc and its neighbours (`bynk-lsp/tests/analysis_residual_gap.rs`, `CHANGELOG.md`) closed the
> question with more force than the draft anticipated: the boundary isn't merely an informal judgment
> call worth re-examining — it's an already-shipped regression the codebase's own authors already named,
> fixture-pinned, and scheduled onto this exact track by name (`CHANGELOG.md`: "closed when phase 5 of
> that track ports these checks"). Five of seven categories are confirmed live gaps in the editor today;
> the other two are confirmed-unreachable regardless of where they live, which changes their priority but
> not their in-scope status. Q3 folded into this finding directly — `check_function_type_boundaries` is
> category 6, one of the five live gaps, which settles "close the hook" without a separate argument.
>
> Q1, Q4 and Q5 resolved close to the draft's own framing, each for the reason stated below.

### 3.1 Q1 — Does this track's rule list still include R4.6, R4.11 and R10.4? **Settled.**

**Decision: keep all three, narrowed to a verify-only item (P5.5).** Every category in §6 needs some
resolved-type view at its new `bynk-check` call site; building that by hand rather than reusing
`ResolvedCommons`'s real constructor would reopen the exact defect phase 1 closed, in a location
Appendix D's next `bynk-emit`-side sweep wouldn't catch until after the fact. Dropping the rules from
this track's scope removes the reason to check for that regression at review time. P5.5 is the explicit,
narrow slice this verification rides on — not new construction, not reopened decisions.

### 3.2 Q2 — Does phase 5 ratify `project_model.rs`'s own emission/checking boundary, or move more of it? **Settled — and the finding is bigger than the question.**

**Decision: all seven categories in §1's table are this phase's scope**, split by priority rather than
by in/out:

- **Categories 2, 3, 4, 6, 7 (live gaps) are the priority order** — each closes a named, fixture-pinned,
  CHANGELOG-documented editor regression the moment it ships. `check_function_type_boundaries` (category
  6) settles Q3 (§3.3) as part of this same decision: its home is `bynk-check`, called directly, not
  through an optional hook.
- **Categories 1 and 5 (gap in name only) move too, for R3.5 compliance**, but ship after the live-gap
  categories since nothing currently observes the improvement — `analyse_project_with`'s own hardcoded
  `SchemaLock::Off`/`Platform::default()` means neither can fire on the analyse path regardless of where
  the checking code lives. (A related, smaller question the relocation should resolve in passing: whether
  the *new* `bynk-check` entry point should stop hardcoding those values once it owns the checks — a
  slice-level design call for P5.2/P5.3, not this track's.)

**`emit.rs`'s one flagged site is out of scope entirely** — confirmed not a registered diagnostic (§1).
**`project.rs`'s own `bynk.project.schema_registry_corrupt`** is real and outside all seven categories
too, but unambiguously in scope — an eighth site the category accounting doesn't cover, relocated by
P5.5 alongside the crate-doc correction (§6). **`secrets.rs`'s `bynk.secrets.computed_name`** is also real
(`CompileError::new`-constructed) and, per its own surrounding comment, reachable from `bynk check`/the
LSP already — but unlike `project.rs`'s site, it is not yet placed in a specific slice: whether it
already reaches the editor via some other already-ported path, or is a ninth, unnamed gap, isn't resolved
by this settling pass. Carried into P5.5 as a verification item rather than assumed either way — see §9.

### 3.3 Q3 — `check_function_type_boundaries`'s optional-hook seam: close it, or a legitimate permanent seam? **Settled, folded into Q2.**

**Decision: close it.** It is category 6 of §1's table — a confirmed live gap, not a defensible permanent
exception. `bynk-check`'s new entry point currently reaches into `bynk-emit` for it through an optional
hook it passes as `None`; the function moves into `bynk-check`, called directly like every other check,
removing both the reverse-reach and the gap in one move. content-ownership's precedent for permanent,
named exceptions (three `fs_below_driver` cases) doesn't apply here — those exceptions have no user-facing
regression attached; this one does.

### 3.4 Q4 — Freeze scope: `validate.rs`, or `project_model.rs`? **Settled.**

**Decision: freeze per-slice, not track-wide, and scoped to whichever file that slice's category moves
out of or into.** §1's seven-category breakdown means this phase ships as six small slices (§6) rather
than one large one; a single freeze spanning the whole track would block unrelated `bynk-emit`/`bynk-check`
work far longer than any prior phase's freeze did. Each slice freezes its own source file for its own
duration only, mirroring phase 4's `project.rs` freeze for P4.0 specifically rather than for phase 4 as a
whole: `validate.rs` for P5.0, P5.1, P5.2 and P5.3 (four of six slices move code out of it, not two —
platform-lock/messages/locale/event-pattern checks all live there), `tests_emit.rs` for P5.4.
`project_model.rs` (the shared destination, gaining callers across several slices) is not frozen — it's
the landing site, not contended source, the same distinction phase 4 drew for `bynk-project`.

### 3.5 Q5 — R10.1: a doc correction, or a real orchestration/emission split? **Settled.**

**Decision: a doc correction, not a split — deferred, not refused.** Once §6 lands, what remains in
`bynk-emit` is TypeScript emission plus `compile_project`/`run_checks`'s own per-unit build sequencing.
That sequencing is not generic command-dispatch orchestration of the kind `bynk-driver` already owns for
other commands (R10.5) — it's interleaved with emission's own per-context, per-unit iteration in the
two-pass structure the trajectory's own R3.5 rationale already names ("discovery, resolution, checking
and emission are one two-pass pipeline by design"). Forcing it into `bynk-driver` now would split a
tightly coupled sequence for a rule (R10.1) that only asks for an accurate one-line description, not a
specific crate boundary — the reference's own target crate diagram doesn't show a distinct orchestration
crate either. `bynk-emit`'s crate-level doc and `Cargo.toml` description get corrected once §6 ships
(P5.5); a real split stays a named forward reference (§7), gated on an actual second orchestration
consumer appearing — the same "named trigger, not appetite" discipline this trajectory has used for
every other deferred item.

---

## 4. Posture

Extends ADR 0059 as amended by ADR 0309, the same standing properties every track on this trajectory has
run under: behaviour-preserving by default (a slice that changes observable behaviour states so, ships a
fixture, gets a CHANGELOG entry); short-lived branches, one PR per slice; no per-increment ADRs beyond
what §11 front-loads; every slice citing `Closes-Rule:`.

---

## 5. The completion criterion

Same principle as every prior track on this trajectory: a slice is complete when the old path is
**deleted**, not when the new home merely exists alongside it. Here: `emit_diagnostics` reads **4/4**
(true/naive), not 0/0 — a named floor, not open work (found under review; see below) —
`validate.rs` either no longer exists or contains no diagnostic-emitting code,
`bynk-lsp/tests/analysis_residual_gap.rs`'s pinned gaps are deleted or flipped to positive-coverage
assertions as each category closes, and R10.1's crate-level doc states an accurate one-line
input/output for `bynk-emit`'s remaining job.

**Why 4/4, not 0/0.** `emit_diagnostics` (`xtask/src/greenfield_status.rs`) has no test-scope filter —
unlike `has_production_std_fs` in the same file, it counts `bynk.*` literals inside `#[cfg(test)]`
modules. `bynk-emit/src/project.rs`'s own trailing `#[cfg(test)] mod tests` (from line 3413 to the file's
end) asserts against four registered codes — `bynk.exports.undeclared_capability`,
`bynk.types.let_annotation_mismatch`, `bynk.project.inconsistent_commons_name`,
`bynk.types.uninferable_element_type` — that are genuinely `bynk-check`-owned diagnostics (type/export
checks), referenced here only as expected strings in integration-test assertions over
`compile_project`'s/`run_checks`'s end-to-end output. Since §3.5 settles that `run_checks` itself stays in
`bynk-emit`, these assertions have no reason to move with the checks they're asserting on, and the probe
counts them regardless. Once every §6 slice lands, `emit_diagnostics` reaches this floor, not zero.

A real 0/0 read is available as an optional, non-blocking follow-on: add a `#[cfg(test)]`-exclusion to
`emit_diagnostics` mirroring `has_production_std_fs`'s existing technique in the same file — a
probe-precision fix, not a code relocation. Named here, filed separately, and not gating this track's
retirement, the same treatment `content-ownership.md` gave R2.3's `fs_below_driver` named floor
(filed as [#1104](https://github.com/accuser/bynk/issues/1104) rather than blocking retirement).

---

## 6. Slice decomposition

§3 is settled; all slices below are accepted, buildable, not forward references.

| Slice | What lands | Rules | Gated on |
|---|---|---|---|
| **P5.0** ✅ | `check_messages_bundles` + `check_locale_bundle_ambiguity` relocated to `bynk-check::project_model` as `phase_messages_bundles`/`phase_locale_bundle_ambiguity`, wired into `analyse_project`; closes categories 2–3. `emit_diagnostics` dropped `bynk-emit` from 49/53 to 37/41 (true/naive) — exactly the 12 codes these two checks own. `bynk-lsp/tests/analysis_residual_gap.rs`'s two corresponding pins flipped to positive coverage (#1128) | R3.5 (partial) | §3 settled |
| **P5.1** ✅ | `check_event_subscriptions` (+ `check_event_pattern`/`check_schema_dispatch`/`check_event_pattern_value`/`resolve_type_decl` helpers) relocated to `bynk-check::project_model` as `phase_event_subscriptions`, wired into `analyse_project`; closes category 4. `emit_diagnostics` dropped `bynk-emit` from 37/41 to 30/34 (true/naive) — exactly the 7 `bynk.event.*` codes this check owns. `bynk-lsp/tests/analysis_residual_gap.rs`'s pin flipped to positive coverage (#1130) | R3.5 (partial) | P5.0 (colocation — same `validate.rs` region) |
| **P5.2** ✅ | `check_function_type_boundaries` (+ helpers `collect_type_decls`/`check_function_type_boundary_items`) relocated to `bynk-check::project_model` as `phase_function_type_boundaries`; `phase_group`'s optional `function_type_boundary_check` hook parameter is deleted and `phase_group` now calls the relocated function directly at the exact point the hook used to fire, preserving diagnostic order for both `run_checks` and `analyse_project` with no hook to drift on; closes category 6. No `emit_diagnostics` movement — every diagnostic this check emits was already constructed inside `bynk-check::context_checks` (`reject_fn_types`/`validate_store_field_value_types`), so the probe stays **30/34**. `bynk-lsp/tests/analysis_residual_gap.rs`'s corresponding pin flipped to positive coverage; `bynk-check/tests/differential_analysis.rs` gained a dedicated parity test for a boundary-violating fixture | R3.5 | §3.3 |
| **P5.3** | `schema_registry::reconcile` and `check_platform_lock` relocate; closes categories 1 and 5 (no editor-visible change — see §3.2) | R3.5, adjacent to R3.11 | P5.0–P5.2 landed (lower priority, per §3.2) |
| **P5.4** | `process_tests`/`process_integration_tests` relocate from `tests_emit.rs`; closes category 7, including the go-to-definition/`RefSink` regression. Largest slice — emission-coupled types (`CompiledFile`, `RunnableTest`, `ImportExt`, `contracts`) need their own resolution, per `analysis.rs`'s own note that porting this "out of proportion" as a single move | R3.5 | P5.0–P5.3 (last, by design — the one category `analysis.rs` itself flagged as needing more care) |
| **P5.5** | `bynk-emit`'s crate-level doc/`Cargo.toml` description corrected; `project.rs`'s `bynk.project.schema_registry_corrupt` relocated; `validate.rs` deleted once empty; verification pass confirming R4.6/R4.11 stay closed and R10.4's facade discipline holds at every new call site; `secrets.rs`'s `bynk.secrets.computed_name` resolved as either already-covered or an eighth relocation | R10.1, R4.6/R4.11/R10.4 (verify-only) | P5.0–P5.4 |

Each slice deletes its corresponding pinned gap in `bynk-lsp/tests/analysis_residual_gap.rs`, converting
the assertion from "category X is absent" to a positive coverage check, per §5.

**Completion probe:** `emit_diagnostics` = **4/4** (true/naive), not 0/0 — see §5 for the named floor and
why it's a probe-precision gap, not open relocation work. Already built and CI-gated
(`greenfield_status_table_is_current`); read **49/53** at this settling pass, **37/41** after P5.0,
**30/34** after P5.1, **still 30/34** after P5.2 (its diagnostics were already `bynk-check`-owned
before the relocation — see the P5.2 row above).

---

## 7. Out of scope — forward references, not refusals

| Item | Phase | Entry condition |
|---|---|---|
| The full IR (reference Part 6) | 6 | this track's probe (`emit_diagnostics`) reads its named floor, 4/4 (§5) |
| The TypeScript tree and printer (reference Part 7) | 7 | phase 6 complete |
| Incrementality — query granularity, `UnitSignature`, the firewall | 8 | phases 3 and 4 complete (already true; phase 8 itself waits on phase 7 per the trajectory's stated order) |
| A real orchestration/emission crate split (§3.5, Q5) | *unopened — no trigger yet* | a second orchestration consumer actually appearing, not appetite alone |
| Unifying the CLI's and LSP's file-discovery seams | *unopened — no trigger yet* | named by `project-model.md` §3.3(b); still no observed divergence |

---

## 8. Keeping the reference true

No new probe infrastructure is needed for this phase's gate — `emit_diagnostics` already exists, already
runs in CI (`greenfield_status_table_is_current`), and already reads the value this track needs to drive
to zero. Two corrections worth raising under review, ahead of the generator: the trajectory doc's own
§3.0/§5 (baseline 200, relative size 12) and phase-5 rule list (still citing R4.6/R4.11/R10.4 as fully
open) are both stale against Appendix D and the live probe — the same kind of routine update
`project-model.md` §3.5 made ahead of the generator for a stale R3.11 row.

---

## 9. Risks

**`secrets.rs`'s `bynk.secrets.computed_name` is not fully accounted for.** §3.2 leaves it a P5.5
verification item rather than a scoped relocation, because it doesn't fit `analysis.rs`'s own seven-category
accounting (unlike `project.rs`'s own eighth site, which is scoped) and this settling pass didn't trace its
exact reachability from the new entry point far enough to be certain. If P5.5 finds it's a genuine ninth
gap, that's new, unsized scope discovered late — named here so a reviewer watches for it rather than
assuming §6's six slices are exhaustive.

**Relocating checks risks a quiet R4.6/R4.11 regression.** Every slice in §6 needs some resolved-type view
at its new call site; building that view by hand rather than reusing `ResolvedCommons`'s real constructor
would reopen exactly the defect phase 1 closed, in a new location. P5.5 makes this an explicit check, not
an assumption — but it's the last slice, so a regression introduced by P5.0–P5.4 lives undetected until
then unless each slice's own review catches it first.

**P5.4 (category 7) is the one category `analysis.rs`'s own author flagged as disproportionate to port
as originally scoped.** Its emission-coupled types (`CompiledFile`, `RunnableTest`, `ImportExt`,
`contracts`) may force a design choice this settling pass hasn't made — a shared type moving to
`bynk-check`, a duplicate lighter-weight representation for the analyse path, or something else. Sequenced
last deliberately (§6), so the smaller slices land — and the four smaller live-gap regressions close — even
if P5.4 needs its own mini-settling pass.

**The evidence ages.** Every fact, line number and quotation in this doc was measured against `main` at
commit `7d2c382c` on 8 August 2026. Re-check before a slice proposal cites one, per every prior track's
own §9 — this doc's own §1 is a direct demonstration of why that discipline exists.

---

## 10. What this phase causes

Per the trajectory's own "5 before 6" argument: "An IR built while hundreds of diagnostic codes still
live in the emitter would have to model the emitter's checking needs. Move the checks first and the IR
only has to carry what emission needs." §3.2's settled decision sharpens this: the boundary phase 6
inherits is not a fuzzy architectural preference but the same live-gap/gap-in-name-only distinction this
phase already had to draw precisely, category by category — phase 6 gets a `bynk-emit` that emits and
nothing else, with the boundary question already answered rather than reopened.

---

## 11. ADRs

Per ADR 0167 step 2, load-bearing, hard-to-reverse decisions land before slicing. Three do, with this
settling pass (numbers assigned at merge by the stamp; referred to by letter until then):

- **ADR-A** — R4.6, R4.11 and R10.4 stay in this track's scope as verify-only items (P5.5), not reopened
  decisions. §3.1 (Q1).
- **ADR-B** — all seven categories `bynk-check/src/analysis.rs` names are this phase's scope, sequenced
  by whether they close a live editor regression (categories 2, 3, 4, 6, 7) or only architectural
  compliance (categories 1, 5); `emit.rs`'s flagged site is confirmed out of scope entirely. §3.2 (Q2).
  The most load-bearing of the three — it fixes §6's whole slice list — and the one this settling pass
  found the most direct, contemporaneous evidence for (CHANGELOG, a dedicated regression-fixture file,
  and the module doc all naming this exact phase by name already).
- **ADR-C** — `check_function_type_boundaries` moves into `bynk-check`, closing the optional-hook seam.
  §3.3 (Q3), folded from the same evidence as ADR-B.

---

## 12. Retirement

Mirrors every prior track on this trajectory: retires when §6's probe (`emit_diagnostics`) reads its
named floor (**4/4**, true/naive — §5) and every slice named to reach it has landed. The retirement PR
removes this doc, appends its closing summary to `../archive/retired-tracks.md`, and closes the spine
issue ([#1126](https://github.com/accuser/bynk/issues/1126)).
