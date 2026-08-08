# Semantics in the checker — migrating to the greenfield reference (phase 5)

- **Status:** **Draft — Settling.** Spine open. This file exists locally as the track doc but is not
  yet committed via a settling draft PR (tracks/README.md step 2). Nothing here is decided — every
  question in §3 is open, each with the investigation it needs, not yet argued under review. Opening
  a draft PR that adds this file (referencing the spine, "Part of #1126") is the next step; marking it
  ready for review asserts §3's five questions are closed.
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
  trajectory §3) — **but see Q1: three of the four non-R3.5 rules already read ✅ landed in the
  reference's own Appendix D**, closed by paydown that predates this track. [ADR 0200](../decisions/0200-cross-context-contract-hash.md)
  is not this phase's (phase 8's, per `project-model.md` §3.4); the schema-registry lockfile contract
  R3.11 names (already closed) is directly adjacent to one of this phase's candidate slices (§6, P5.1).
- **Precedent:** `project-model.md` (retired) is this track's direct predecessor on the same trajectory.
  Its own closing summary names the handoff explicitly: "Opens phase 5 (semantics centralisation —
  `validate.rs` dissolves into `bynk-check`)," and its §3.3(a) named a specific debt for this phase to
  collect — a temporary duplication between a new `bynk-check` analysis entry point and
  `bynk-emit::run_checks`'s `Mode::Analyse` arm, "phase 5's to remove." §1 below finds that debt already
  paid, by the implementing slice rather than by this track — worth reading in full because it changes
  this phase's real shape. `semantics-in-the-checker.md` would be the tenth track to run the ADR 0167
  flow from the start, after `compiler-architecture.md` (sixth), `identity-and-totality.md` (seventh),
  `content-ownership.md` (eighth) and `project-model.md` (ninth).

### ADR 0076 trigger check

| Trigger | Met? |
|---|---|
| Spans several increments | **Yes, if narrowly** — trajectory §5 rates this phase relative size 12, confidence *low*, "200 diagnostic codes and a reverse edge dragging `icu.rs`." §1 below finds the real remaining surface is far smaller than that sizing assumed: `icu.rs` and `websocket::analyse_open_shape` are already in `bynk-check`, and the live probe reads 49 (not 200). The size question itself is one of §3's open items (Q1, Q2), not yet answered downward with confidence |
| Surface not yet settled | **No** — the destination (R3.5: "no crate but `bynk-check` checks or diagnoses") is specified; what's open is which of the ~10 remaining genuine diagnostic sites are "checking" under that rule versus emission-time invariants R3.5 doesn't reach, a line the phase-4 implementation already drew once, informally, without a settling review behind it (§1, §3 Q2) |
| Security/safety boundary | **No** — the three gates R4.6 exists to protect (`.raw`, `T.unsafe(…)`, owner-only event emission) are already closed, per Appendix D, by phase 1's `ResolvedCommons` constructor. This phase's own risk in that direction is regression, not a new gate (§9) |

**One of three, same count as every internal-architecture track on this trajectory before it.** The
warrant is the tracks README's own: §3's questions govern which of a handful of candidate slices this
phase actually needs, and where the crate boundary really sits once the two false-large assumptions in
the trigger table above are corrected — governance that needs a home neither a proposal nor the
trajectory document itself provides.

---

## 1. Why now

**Phase 4's own closing note opens this phase by name.** `project-model.md`'s retirement summary reads:
"the resulting duplication named as phase 5's to remove" — the debt was `bynk-emit::run_checks`'s
`Mode::Analyse` arm sitting alongside a new, narrower `bynk-check` analysis entry point doing
overlapping work, accepted deliberately in §3.3(a) rather than avoided, with phase 5 named as the
crate that inherits it.

**That debt is already paid — by the implementing slice, ahead of the design doc's own commitment.**
`project-model.md` §3.3(a)'s decision text is explicit: *"phase 4 does not move `run_checks`… `run_checks`
itself stays in `bynk-emit`, unchanged."* The shipped code disagrees. `bynk-check/src/project_model.rs`'s
own module doc (1,941 lines, verified live):

> "P4.1 (#1115), second scope finding on the tracking issue: this pipeline — `phase_discovery` through
> `assemble_unit_info`, plus the per-unit symbol composition… — used to live only in
> `bynk-emit/src/project.rs`, inline in `run_checks`. …rather than write a second, independently-maintained
> copy… it moved here. `bynk-emit`'s `run_checks` becomes a caller of these functions instead of owning
> the logic."

`bynk-emit/src/project.rs` carries the same story at the call site (lines 926–939, verified live): two
`P4.1 (#1115)` comments record that `record_analyse_types` and the whole discovery→parse→group→resolve
pipeline relocated to `bynk-check::check_pipeline`/`bynk-check::project_model`, "the same way P4.0 turned
`project.rs` into a caller of `bynk-project`." The implementing PR went further than the settled decision
it shipped under, closed the named debt early, and neither `project-model.md` (deleted at retirement, so
uncorrectable in place) nor the trajectory doc's phase-5 sizing (§5: relative size 12, "very large") was
updated to reflect it. This is the same failure mode trajectory §9 names — "the evidence ages" — now
caught before a slice proposal cites the stale figure rather than after.

**The live probe confirms the shape.** `design/greenfield-status.md`, regenerated at commit `7d2c382c`
(P4.2, the same commit series that closed phase 4), gated and CI-checked
(`greenfield_status_table_is_current`):

```
emit_diagnostics: bynk-emit=49/53, bynk-check=346/353 (true/naive)
```

49 registered `bynk.*` codes still originate in `bynk-emit` — down from the trajectory's own §3.0
baseline of 200 (30 July 2026) and the review's 190. R3.5 was the one row the trajectory's own Appendix D
flagged as moving the *wrong* direction between review and baseline (§3.0: "phase 5's distance has
**grown**"). It has since reversed hard, and by more than ordinary churn alone plausibly accounts for —
worth settling in Q1 whether the reversal is fully attributable to P4.1 or partly to an untracked Wave-5
pass (`git log` shows a commit titled "Wave 5: layering for the compiler pipeline review (#956)" that
independently relocated `icu.rs` and `websocket::analyse_open_shape` into `bynk-check` and gave
`Ctx` its `pub(crate)` visibility — both already true today, verified live, and both named in this
phase's own invariant).

**What's actually left, verified directly (naive-string grep, cross-checked against the probe's 49/53):**

| File | Real diagnostic sites | What they check |
|---|---|---|
| `bynk-emit/src/project/validate.rs` (1,160 lines, down from the 5,019-line figure `project-model.md` cited for the *whole* `project.rs` module tree — not an apples-to-apples comparison, flagged for correction) | ~15: `bynk.target.*` (platform lock), `bynk.messages.*`/`bynk.locale.*` (message/locale completeness), `bynk.event.*` (subscription/pattern checks) | Whole-project, cross-unit semantic checks |
| `bynk-emit/src/project/schema_registry.rs` | 2: `bynk.event.schema_version_mismatch`, `bynk.event.non_additive_schema_change` | R3.11's lockfile-contract enforcement |
| `bynk-emit/src/emitter/secrets.rs` | 1: `bynk.secrets.computed_name` | A single named-secret validation |
| `bynk-emit/src/emitter/emit.rs` | 1: `bynk.emit.unresolved_cross_context_signature` | Reads as an emission-time internal-consistency assertion, not a user-diagnosable program error — flagged in Q2 |
| `bynk-emit/src/project.rs` | 1: `bynk.project.schema_registry_corrupt` | The remaining 15 naive hits in this file are `bynk.toml`/`bynk.locale`/`bynk.map` string literals and test assertions, not diagnostic emission |

**`bynk-check/src/project_model.rs`'s own doc comment already draws the line this phase must ratify or
revise, without a settling review behind it:** "What stayed in `bynk-emit` (not shared, because only the
`Mode::Build` path needs it, or because it's genuinely emission-shaped): the whole-project
`messages`/locale-ambiguity/event-subscription checks, schema-registry reconciliation, platform-lock, the
`Mode::Build` bail gate… `check_function_type_boundaries` also stays in `bynk-emit` (`validate.rs`) —
[`phase_group`] reaches it only through an optional hook, never a direct call… a documented residual gap."
That is a real, working boundary — but it was drawn by an implementer closing a different slice's finding,
not settled under this phase's own review, and R3.5's text carries no "only used during `Build`" carve-out.
Q2 and Q3 below exist because of this passage specifically.

---

## 2. What this track is not

- **Not phase 6 (the IR).** `bynk-emit` naming no AST type and `CommitShape` becoming data are untouched
  here — this phase only removes checking, per the trajectory's own "5 before 6" ordering argument.
- **Not re-opening R4.6, R4.11 or R10.4.** Appendix D already marks all three ✅ landed, closed by phase
  1's `ResolvedCommons` constructor and by prior `pub`-surface paydown, not by this phase. This track's
  business with them is narrower: verifying the remaining relocations (§6) don't regress them by
  hand-rolling a new `ResolvedCommons`-shaped struct at a new call site — a real risk named in §9, not new
  construction work.
- **Not a `bynk-emit` crate rename.** R10.1 asks for an accurate one-line input/output description, not
  new crate boundaries — the reference's own target crate list (§10, `bynk-greenfield-compiler.md` ~line
  1610) does not add an orchestration crate distinct from `bynk-driver`. Whether `bynk-emit`'s remaining
  job needs only a doc correction or a real split is Q5, not a foregone restructuring.
- **Not R3.2** (`bynk check` still runs the bailing path) — a different rule, untouched by this phase's
  scope.
- **Not phases 6–8.** Carried as forward references in §7, not refusals, per the discipline every prior
  track on this trajectory has used for phases not yet open.

---

## 3. Design questions — open

None of the five below has been argued under a settling review yet. Each states the investigation
already done and the tension it leaves.

### 3.1 Q1 — Does this track's rule list still include R4.6, R4.11 and R10.4?

Appendix D marks all three ✅ landed, attributed to work outside this phase (phase 1's `ResolvedCommons`
constructor for R4.6/R4.11; prior facade-deletion paydown for R10.4, `#1048`). The trajectory doc's own
phase-5 section (§3) still lists all five rules together, written before that paydown landed. Keeping
them in this track's `Relates:` list as "verify-closed, not build" items gives phase 5 a reason to check
that the remaining relocations (§6) don't quietly reintroduce a hand-rolled `ResolvedCommons` at a new
`bynk-check` call site — a real regression shape, since every relocation in §6 needs *some* commons view
of resolved types at its new home. Dropping them risks losing that verification step entirely.

**Needs settling:** keep R4.6/R4.11/R10.4 as an explicit, narrow verification item in this track (a
"stays closed" check alongside the "closes" checks the other rules need), or drop them from this track's
scope and rely on Appendix D's own regeneration to catch a regression after the fact?

### 3.2 Q2 — Does phase 5 ratify `project_model.rs`'s own emission/checking boundary, or move more of it?

R3.5's text is unconditional: "`bynk-emit` performs no checks and emits no diagnostics." The line P4.1
actually drew — messages/locale/event checks and schema-registry reconciliation stay because "only the
`Mode::Build` path needs it" or because they're "genuinely emission-shaped" — is a judgment call made
inside an unrelated slice, not argued under this phase's own review. Two sub-cases:

- **The whole-project checks** (`validate.rs`'s `bynk.messages.*`/`bynk.locale.*`/`bynk.event.*`,
  `schema_registry.rs`'s two codes) read as ordinary semantic checking under R3.5's own rationale — the
  causal chain the trajectory names ("context-level semantics were written in `project.rs` because that
  is where the project model was") applies to them exactly as it did to everything P4.1 already moved.
  "Only `Mode::Build` needs it" describes *when* a check runs, not *what kind* of check it is, and R3.5
  draws no such distinction.
- **`emit.rs`'s `bynk.emit.unresolved_cross_context_signature`** reads differently in kind — its own
  message text ("no signature for…") is phrased as an internal-consistency assertion about the emitter's
  own state, not a diagnosable error in the *program* being compiled. If that reading holds, it may not
  be R3.5's business at all — it could be an emission-time invariant that belongs behind an `assert!` or
  a genuine internal-error type, a question closer to R4.3's `Ty::Error`/phase-6 IR territory than to
  checking.

**Needs settling:** does R3.5 apply uniformly to every `bynk.*`-coded site in `bynk-emit`, or does the
compiler need (and does the reference specify) a distinct category for emission-time internal invariants
that are exempt? If the latter, which of the five files in §1's table fall into it?

### 3.3 Q3 — `check_function_type_boundaries`'s optional-hook seam: close it, or is it a legitimate permanent seam?

`project_model.rs`'s own doc names this "a documented residual gap": `bynk-check`'s new entry point
reaches `bynk-emit`'s `check_function_type_boundaries` (`validate.rs:941`, `pub(crate)`) only through an
optional hook, never a direct call, so the entry point "can genuinely omit it… rather than duplicate it
with a different diagnostic order." That is `bynk-check` reaching *forward* into `bynk-emit` for a check
— arguably the reverse-boundary problem R3.5's invariant ("no crate reaches back across a boundary to
drive the checker") exists to remove, just pointed the opposite direction from every other case this
trajectory has fixed so far.

**Needs settling:** does the function move into `bynk-check` (closing the hook, matching every other
check's home), or is there a structural reason it has to stay caller-supplied that the other relocations
in §6 don't share? content-ownership's precedent (three *permanent*, named `fs_below_driver` exceptions)
shows this trajectory does sometimes accept a residual seam rather than force it closed — Q3 is where
that same judgment gets made for this one, not assumed either way.

### 3.4 Q4 — Freeze scope: `validate.rs`, or `project_model.rs`?

Phase 3 froze `checker.rs` for one slice; phase 4 froze `project.rs`. `validate.rs` is the obvious
candidate here (it holds most of §1's remaining real diagnostic sites), but `project_model.rs` — the
1,941-line shared hub `bynk-emit::run_checks` and `bynk-check::analyse_project` both now call into,
per §1 — is arguably the more contended file for whichever slice lands the relocation, since it gains
new callers and new functions in the same pass `validate.rs` loses them.

**Needs settling:** freeze one file, both, or neither (mirroring `identity-and-totality.md`'s own answer
to the same question when the touched surface turned out to be mostly self-contained)?

### 3.5 Q5 — R10.1: a doc correction, or a real orchestration/emission split?

R10.1's text: "a crate is named for what it produces, and if its input type and output type cannot be
stated in one line, it is not a crate yet." Once §6's relocations land, `bynk-emit`'s remaining job is
close to pure TS emission — but `project.rs`'s `compile_project`/`run_checks` orchestration (discovery →
parse → resolve → check → emit, sequenced) still lives there too, and the reference's own target crate
diagram doesn't show a distinct orchestration crate separate from `bynk-driver`.

**Needs settling:** does R10.1 close with an accurate crate-doc/`Cargo.toml` description once checking
leaves (orchestration is legitimately emission's neighbour, not a separate concern), or does
`compile_project`/`run_checks` itself need to move toward `bynk-driver`, which is a materially bigger
slice this track hasn't sized?

---

## 4. Posture

Extends ADR 0059 as amended by ADR 0309, the same standing properties every track on this trajectory has
run under: behaviour-preserving by default (a slice that changes observable behaviour states so, ships a
fixture, gets a CHANGELOG entry); short-lived branches, one PR per slice; no per-increment ADRs beyond
what §11 front-loads; every slice citing `Closes-Rule:`.

---

## 5. The completion criterion

Same principle as every prior track on this trajectory: a slice is complete when the old path is
**deleted**, not when the new home merely exists alongside it. Here: `emit_diagnostics` reads 0/0
(true/naive) for `bynk-emit`, `validate.rs` either no longer exists or contains no diagnostic-emitting
code, and R10.1's crate-level doc states an accurate one-line input/output for whatever `bynk-emit`'s job
turns out to be once §3.5 settles.

---

## 6. Slice decomposition — candidate, pending §3

None of the below is accepted yet; §3 governs both which of these actually ship and their order.

| Slice (candidate) | What it would move | Rules | Gated on |
|---|---|---|---|
| **P5.0** | `validate.rs`'s platform-lock, messages, locale and event-pattern checks relocate to `bynk-check` (destination TBD by Q3/Q4 — extends `project_model.rs`, or a new module) | R3.5 | Q2 (confirms these are in-scope), Q4 (freeze) |
| **P5.1** | `schema_registry.rs`'s two event-schema codes relocate, paired with R3.11's already-threaded `schema_lock` plumbing | R3.5 | Q2 |
| **P5.2** | `secrets.rs`'s `bynk.secrets.computed_name` relocates | R3.5 | Q2 |
| **P5.3** | `check_function_type_boundaries` moves into `bynk-check`, closing the optional-hook seam, or is confirmed a permanent exception | R3.5 | Q3 |
| **P5.4** | `emit.rs`'s `bynk.emit.unresolved_cross_context_signature` reclassified (moved, or converted to an internal invariant, per Q2) | R3.5 (or none, if Q2 finds it out of scope) | Q2 |
| **P5.5** | `bynk-emit`'s crate-level doc/`Cargo.toml` description corrected; `bynk-project.rs`'s `bynk.project.schema_registry_corrupt` relocated; `validate.rs` deleted once empty | R10.1 | P5.0–P5.4, Q5 |
| **P5.6** | Verification pass: confirm no relocation hand-rolled a new commons-shaped struct (R4.6/R4.11 stay closed); confirm R10.4's facade discipline holds at the new call sites | R4.6, R4.11, R10.4 (verify-only) | Q1; rides with whichever of P5.0–P5.3 lands last |

**Completion probe:** `emit_diagnostics` = 0/0. Already built and CI-gated
(`greenfield_status_table_is_current`); reads **49/53** as of this draft.

---

## 7. Out of scope — forward references, not refusals

| Item | Phase | Entry condition |
|---|---|---|
| The full IR (reference Part 6) | 6 | this track's probe (`emit_diagnostics`) reads 0/0 |
| The TypeScript tree and printer (reference Part 7) | 7 | phase 6 complete |
| Incrementality — query granularity, `UnitSignature`, the firewall | 8 | phases 3 and 4 complete (already true; phase 8 itself waits on phase 7 landing first per the trajectory's stated order) |
| A real orchestration/emission crate split, if Q5 decides one's needed | *unopened — no trigger yet unless Q5 settles that way* | Q5 |
| Unifying the CLI's and LSP's file-discovery seams | *unopened — no trigger yet* | named by `project-model.md` §3.3(b); still no observed divergence |

---

## 8. Keeping the reference true

No new probe infrastructure is needed for this phase's gate — `emit_diagnostics` already exists, already
runs in CI (`greenfield_status_table_is_current`), and already reads the value this track needs to drive
to zero. One correction worth raising under review: the trajectory doc's own §3.0/§5 (baseline 200,
relative size 12) and phase-5 rule list (still citing R4.6/R4.11/R10.4 as open) are both stale against
Appendix D and the live probe — a routine update this track's settling pass should carry, the same way
`project-model.md` §3.5 corrected a stale R3.11 appendix row ahead of the generator.

---

## 9. Risks

**The "genuinely emission-shaped" boundary (§1, Q2) was drawn without a settling review.** It may be
right — but it was a judgment call inside a slice scoped for something else, and this is the first time
it gets argued rather than assumed. If Q2 finds part of it wrong, the candidate slice list in §6 changes
shape, not just size.

**Relocating checks risks a quiet R4.6/R4.11 regression.** Every relocation candidate in §6 needs some
resolved-type view at its new call site; building that view by hand rather than reusing
`ResolvedCommons`'s real constructor would reopen exactly the defect phase 1 closed, in a new location
Appendix D wouldn't catch until its next `bynk-emit`-side sweep. P5.6 exists to make this an explicit
check, not an assumption.

**The trajectory's own sizing for this phase (relative size 12, confidence low) may now be
overstated in the other direction — or understated, depending on how Q5 settles.** §1's findings cut the
diagnostic-code surface from ~200 to ~20 real sites; but if Q5 finds `compile_project`/`run_checks`
orchestration needs to move toward `bynk-driver`, that's a materially different, unsized piece of work
this draft hasn't scoped. Both directions need settling before this track's own size estimate is worth
trusting.

**The evidence ages.** Every fact, line number and quotation in this doc was measured against `main` at
commit `7d2c382c` on 8 August 2026. Re-check before a slice proposal cites one, per every prior track's
own §9 — this draft's own §1 is a direct demonstration of why that discipline exists.

---

## 10. What this phase causes

Per the trajectory's own "5 before 6" argument: "An IR built while hundreds of diagnostic codes still
live in the emitter would have to model the emitter's checking needs. Move the checks first and the IR
only has to carry what emission needs." §1's finding sharpens the stakes without changing the argument —
phase 6 was always going to inherit whatever boundary phase 5 leaves between "checking" and "emission
detail," and Q2's disposition of `emit.rs`'s one remaining code is a preview of exactly that boundary
question at IR scale, not a one-off.

---

## 11. ADRs

Per ADR 0167 step 2, load-bearing, hard-to-reverse decisions land before slicing. Candidates, pending §3
actually closing under review (numbers assigned at merge by the stamp, per every prior track's own
convention of referring to them by letter until then):

- **ADR-A** — whether R4.6/R4.11/R10.4 stay in this track's scope as verify-only items or are dropped
  entirely (§3.1, Q1).
- **ADR-B** — where the emission/checking boundary actually sits: which of `validate.rs`'s and
  `schema_registry.rs`'s remaining checks are R3.5's business, and how `emit.rs`'s one remaining code is
  classified (§3.2, Q2). Likely the most load-bearing of the set — it decides §6's real slice list.
  Likely the hardest to reverse: once P4.1 has already drawn this line once, informally, a second
  informal redraw compounds the same problem project-model.md's own Q3 named for the previous phase.
- **ADR-C** — `check_function_type_boundaries`'s home (§3.3, Q3).

---

## 12. Retirement

Mirrors every prior track on this trajectory: retires when §6's probe (`emit_diagnostics`) reads 0/0 and
every slice named to reach it has landed. The retirement PR removes this doc, appends its closing summary
to `../archive/retired-tracks.md`, and closes the spine issue once one exists.
