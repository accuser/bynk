# The IR — the emitter consumes a typed intermediate representation (phase 6)

- **Status:** Draft. Settling under this PR.
- **Spine:** [#1137](https://github.com/accuser/bynk/issues/1137).
- **Theme:** **Phase 6** of [`../bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md) —
  `bynk-emit` consumes `CheckedProgram → Ir → TsProgram`, names no AST type, and every dispatch
  decision is a match on a resolved enum rather than a re-derivation. The trajectory's endpoint is the
  current compiler rebuilt on
  [`../bynk-greenfield-compiler.md`](../bynk-greenfield-compiler.md) Part 6 (the IR) and Part 5
  (patterns); this track walks phase 6 of it.
- **Phase boundaries are safe stopping points** (trajectory §2). Phase 5 is retired
  ([#1126](https://github.com/accuser/bynk/issues/1126), settled 8 August 2026, retired 9 August 2026)
  and leaves a coherent compiler regardless of whether this phase lands.
- **Relates:** reference rules R5.1–R5.11, R6.1, R6.5, R6.7–R6.16 (the trajectory's own phase-6 list,
  trajectory §3) — **Q4 settles that R6.10's "resolved in phase 5" names the reference's own Check
  phase (`bynk-check`), not the trajectory's retired phase-5 *track*, and commissions a slice for it
  here.** R6.2/R6.3/R6.4/R6.6 are the trajectory's own **phase 2** rules (already retired) and are not
  this track's business, despite living in the same reference Part — see §1's correction of the spine
  issue's own imprecision on this point. R4.9 (`bynk-greenfield-compiler.md` Part 4) is directly
  relevant without being in scope — Q2 settles why.
- **Precedent:** `semantics-in-the-checker.md` (retired) is this track's direct predecessor on the same
  trajectory. Its own closing summary names the handoff explicitly: "Opens phase 6 (the IR, per the
  trajectory)." `the-ir.md` is the eleventh track to run the ADR 0167 flow from the start, after
  `compiler-architecture.md` (sixth), `identity-and-totality.md` (seventh), `content-ownership.md`
  (eighth), `project-model.md` (ninth) and `semantics-in-the-checker.md` (tenth, now retired).

### ADR 0076 trigger check

| Trigger | Met? |
|---|---|
| Spans several increments | **Yes, and more than any prior phase on this trajectory** — trajectory §5 rates this phase relative size 20, the largest named, confidence low. §1/§6 give a ten-slice candidate decomposition, but unlike phase 5's own settling review (which found its phase materially *smaller* than estimated), this one confirms the phase is genuinely large: of its own 23-rule list (trajectory §3: R6.1, R6.5, R6.7–R6.16, R5.1–R5.11), only R6.5 and R6.11 show real partial progress from ordinary churn — §1 corrects the spine issue's own conflation of these with R6.2/R6.3/R6.4/R6.6, which were never on this phase's list at all (they are phase 2's, already retired) |
| Surface not yet settled | **Yes** — the reference's own target crate graph (Part 10) names `bynk-ir`/`bynk-lower` as separate crates the reference's phase-6/7 narrative text never mentions building; Q3 settles that this track does not build them yet, a genuinely open call the reference alone does not resolve |
| Security/safety boundary | **Partial** — R6.5's rationale is "the strongest single argument in the document" for its own rule: a store write reached through anything but a bare `Ident` receiver is silently dropped, a live data-loss defect the `expr_children` fix already narrowed but did not close (name-matching persists). Not a new gate like R4.6's three; a data-integrity property this phase makes structurally impossible rather than reachable-and-patched |

**Two of three — the same count as every internal-architecture track on this trajectory before it, and
for the first time on genuinely stronger multi-increment grounds than the estimate alone.**

---

## 1. Why now

**Phase 5's own closing note opens this phase by name.** `semantics-in-the-checker.md`'s retirement
(`design/archive/retired-tracks.md`) states directly: "Opens phase 6 (the IR, per the trajectory)."
Phase 5's probe (`emit_diagnostics`) reached its own named floor (4/6 true/naive, true=4) before
retiring — confirmed live (`cargo xtask greenfield-status`), not assumed.

**This phase's own probe, `ast_importers`, already reads below the trajectory's §3.0 baseline** — 9
files today, down from 13 at the 30 July measurement. Some of that drop is directly attributable, not
inferred: P5.5 (phase 5's own last slice) dropped it from 10 to 9 by relocating
`emitter/secrets.rs`'s AST-walking half into `bynk-check::secrets` as a side effect of closing a
different rule (R3.5), the same "a neighbouring phase's track pays down this phase's distance by
accident" shape phase 4 showed for phase 5 and phase 5's own settling review found for itself. The
nine remaining files: `project.rs`, `project/tests_emit.rs`, `emitter.rs`, `emitter/lower.rs`,
`emitter/serialisation.rs`, `emitter/workers.rs`, `emitter/workers_entry.rs`, `emitter/wrangler.rs`,
`emitter/runtime_use.rs`. `emitter/lower.rs` is the load-bearing one — 5,970 lines, 39 commits in the
last 30 days; `emitter.rs` is 4,653 lines, 54 commits in the same window. Both numbers matter directly
for §3's freeze-scope question below.

**Correction to the spine issue's own framing, found while grounding this doc — the same discipline
that finding itself asks for.** Issue #1137's "why now" cited R6.2 (statement sink, ✅ landed at T2.1,
#1017), R6.3 (short-circuit, ✅ landed pre-dating Appendix D plus T2.3, #1019), R6.5 (partial) and
R6.11 (partial) together as evidence that "ordinary churn already paid down part of the distance" —
true of R6.5/R6.11, **not true of R6.2/R6.3, which were never this phase's rules.** The trajectory's
own phase 2 (Typed lowering) names R6.2, R6.3, R6.4 and R6.6 as *its* reference rules
(trajectory §3, "### Phase 2"), and phase 2 retired properly, on its own schedule, as part of the
`0, 1, 2` combined track (retired 3 August 2026) — closing them was phase 2 doing its job, not an
accident this phase inherits. Only **R6.5** (the `expr_children` fix, name-matching intact) and
**R6.11** (the `joinOn`/`groupBy` gap, the bidirectional test and second copy intact) are genuinely
this phase's own rules with real partial progress from ordinary churn. The correction narrows the
"free" surface, not widens it — worth stating plainly so a slice proposal does not claim credit phase
2 already banked.

**Appendix D's remaining phase-6/5 rows, unclaimed by any prior phase:** R5.1–R5.11 (patterns —
`bynk-greenfield-compiler.md` Part 5), R6.1 (the fallible checker→emitter channel, R4.9-adjacent —
see Q2), R6.7–R6.9 (the desugaring table), R6.10/R6.12 (call dispatch — see Q4), R6.13 (declarations
as AST reads), R6.14 (store-field shapes derived at emission), R6.15/R6.16 (commit shape and handler
origin-independence). Twenty-one rules with no progress at all, against a 23-rule list — the genuine
remaining surface, and it is exactly as large as the trajectory's own §5 estimate implies once
R6.2–R6.4/R6.6 are correctly attributed away from it (they were never on this list to begin with).

---

## 2. What this track is not

- **Not phase 7 (the TypeScript tree and printer).** `bynk-ts` and the printer are untouched here —
  this phase only replaces what feeds emission, per the trajectory's own "5 before 6, 6 before 7"
  ordering (trajectory §4: "An IR built while hundreds of diagnostic codes still live in the emitter
  would have to model the emitter's checking needs. Move the checks first and the IR only has to carry
  what emission needs" — phase 5 did that; this phase is the "only has to carry" part).
- **Not a `bynk-ir`/`bynk-lower` crate split.** Q3 settles this: the reference's own target crate graph
  (Part 10) names both, but R10.3's "carve at the moment a dependency appears, not before" discipline
  argues against building them now, with only one consumer (`bynk-emit` itself) in existence. Carving
  is phase 7's entry condition, named as a forward reference (§7), not refused.
- **Not R4.9 structurally.** `expr_types` stays `HashMap<ExprId, TypedExpr>`, not becoming
  `IndexVec<ExprId, TypedExpr>` — Q2 settles why this phase's own totality guarantee (R6.1) does not
  need that container-type change to hold.
- **Not R10.1/R10.2 for `bynk-emit`.** The reference's target crate graph shows `bynk-emit` eventually
  depending on `bynk-ts`, not `bynk-check` — cutting that edge is not one of this phase's own reference
  rules (trajectory §3's phase-6 list omits R10.1/R10.2 entirely) and is not attempted here.
- **Not phases 7–8.** Carried as forward references in §7, not refusals.

---

## 3. Design questions — settled

> **Provenance.** All six questions below originate in the spine issue (#1137); none was closed there
> — opening a spine issue states the questions, it does not argue them (`design/tracks/README.md`'s
> own lifecycle step 1 vs. step 2). This settling pass is where each is actually argued, under this
> draft PR.

### 3.1 Q1 — Freeze scope: `emitter/lower.rs`/`emitter.rs` track-wide, or per-slice? **Settled.**

**Decision: freeze per-slice, scoped to whichever file that slice's node kind touches, never
track-wide.** `semantics-in-the-checker.md`'s own Q4 set this precedent for a phase whose real
decomposition (six small slices) turned out smaller than its freeze risk implied; this phase is the
opposite case — genuinely large (§1), and its two central files are far hotter than `validate.rs` ever
was: `emitter/lower.rs` (5,970 lines, 39 commits/30 days) and `emitter.rs` (4,653 lines, 54
commits/30 days), against `validate.rs`'s own peak of roughly 5,000 lines and a fraction of that churn.
A track-wide freeze on either file for a ten-slice, low-confidence phase would block most of
`bynk-emit`'s unrelated bug-fix traffic for the phase's entire — unknown — duration, the exact failure
mode ADR 0059's freeze discipline exists to bound, not invite. Each slice below freezes only the
specific `lower_*`/`emit_*` functions it relocates, for its own duration, the same discipline phase 5
used for `validate.rs`'s four-slice `messages`/`locale`/`event`/`platform-lock` overlap.

### 3.2 Q2 — Does R6.1 (`IrExpr` carries its type, no fallible lookup) require R4.9 (`expr_types` as `IndexVec`) closed first? **Settled.**

**Decision: no, for lowering driven from a certified `CheckedProgram` — the lowering pass enforces
totality there, by construction, independent of `expr_types`'s own container type, and a
checker/lowerer disagreement on *that* path panics rather than falls back.** `TypedCommons.expr_types`
is already `HashMap<ExprId, TypedExpr>` (T3.4, phase 3), not `HashMap<Span, Ty>` as R4.9's own
rationale text describes — phase 3 already replaced the position-keyed map with an identity-keyed one;
only the container's totality (`HashMap` vs. `IndexVec`, i.e. "can a lookup miss" vs. "cannot by
construction") remains, exactly what phase 3's own retirement named as open ("functionally but not
structurally"). A `CheckedProgram` is constructible only by `certify`, which rejects on any
error-severity diagnostic (R3.10) — so for a certified program, every reachable `ExprId` a real
(non-synthetic-post-check) expression owns should already have an `expr_types` entry by the checker's
own construction discipline, not by luck. Lowering's own walk over `TypedCommons.commons`, minting each
`IrExpr.ty` from `expr_ty(id)` as it goes, is therefore the total, single-pass consumer R4.9 was
written to make safe on *that* path — and this codebase already has the right precedent for how to
treat a "should never happen" miss inside it: `bynk-emit/src/emitter/emit.rs`'s
`lower_workers_cross_context_call` panics on its own `bynk.emit.unresolved_cross_context_signature`
rather than silently degrading, on exactly the same "the checker resolved this before I run; an absent
value here is the checker and I disagreeing, which is a compiler bug" reasoning.

**The panic discipline is scoped to that path deliberately, not applied uniformly — a real
counterexample rules out "always."** `TypedCommons` has a second, non-certified producer today:
`bynk-emit/src/project/tests_emit.rs`'s `synthetic_typed_commons_for_target` builds one with
`expr_types: HashMap::new()`, filled in by `bynk-check/src/test_suites.rs`'s
`let _ = checker::check_body(…)` — errors discarded by design, no `certify`, no R3.10 gate — and
test-suite emission lowers case/property bodies through the same `lower.rs` functions this track
replaces. A blanket `.expect()` there would turn a partially-typed test body (an ordinary, expected
state on that path) into a compiler panic on user source, not a caught compiler bug. `.expect()`
applies only to lowering reached from a real `CheckedProgram`; the test-suite emission path keeps its
existing `Some(..) => …, _ => …` fallback shape unless and until a slice routes it through `certify`
first — a call this settling pass does not make, since that path's own errors-discarded design looks
deliberate (case bodies partially type despite errors, so completion/hover-adjacent tooling can still
say something), not an oversight this track should reach into uninvited.

Every `Lower` function driven from a `CheckedProgram` `.expect()`s its `expr_ty` lookup — closing R6.1
on that path without touching `expr_types`'s own representation. R4.9 (the `IndexVec` conversion
itself) is filed as an optional, non-blocking residue once nothing performance-sensitive still queries
the `HashMap` — the same treatment `content-ownership.md` gave `fs_below_driver`'s carve-outs and
`semantics-in-the-checker.md` gave `emit_diagnostics`'s 4/4-vs-0/0 gap: named, filed separately, not
gating this track's slices.

### 3.3 Q3 — Where does the IR live: a new `bynk-ir`/`bynk-lower` crate pair, or inside `bynk-emit`? **Settled.**

**Decision: inside `bynk-emit`, as new modules — not a new crate, yet.** The reference's own target
crate graph (`bynk-greenfield-compiler.md` Part 10) names `bynk-ir` ("the checker's output; the only
thing lower consumes") and `bynk-lower` (`CheckedProgram → Ir`) as separate crates, distinct from a
narrower, `Ir`-consuming `bynk-emit` that in the target graph "depends on `bynk-ts`, NOT on
`bynk-check`." The trajectory's own phase-6 narrative text never mentions either crate — its stated
invariant is `bynk-emit` names no AST type, not that `bynk-emit` stops existing as the lowering site —
and phase 6's own reference-rule list (trajectory §3) omits R10.1/R10.2 entirely, meaning cutting
`bynk-emit`'s `bynk-check` edge is not one of this phase's commitments. R10.3's own discipline is
explicit: a crate is carved "when a dependency arrives that only one consumer needs — prospectively,
at the moment the dependency appears" — not before, and `bynk-strip` is named as "the control case for
this entire document" precisely because it was carved for a real, present dependency (`oxc`), not a
future one. Today there is exactly one consumer of the IR: `bynk-emit` itself. `bynk-ts` — the
reference's own second consumer, per Part 10 — does not exist until trajectory phase 7. Carving
`bynk-ir`/`bynk-lower` now would be prospective without the dependency R10.3 requires, the anti-pattern
its own rationale warns against. The IR type (`bynk-emit::ir`) and the lowering pass
(`bynk-emit::lower_ir` or similar — named precisely at P6.1) land as `bynk-emit` modules, consuming
`bynk-check` exactly as `bynk-emit` already does today. Carving `bynk-ir`/`bynk-lower` (and, at the
same moment, reconsidering whether `bynk-emit` still needs `bynk-check` at all, since a
fully-typed-and-resolved IR — R6.1, R6.5, R6.10 — genuinely might not) is named as phase 7's own entry
condition in §7, not refused.

### 3.4 Q4 — R6.10 says `Callee` dispatch is "resolved in phase 5." Does that mean this track's own retired predecessor missed scope, or does it name new work? **Settled.**

**Decision: it names new work, correctly commissioned here as an early slice (P6.0) — nothing was
missed.** `bynk-greenfield-compiler.md` Part 3's own phase table (§3, "The spine") defines the
reference document's *own* compiler-phase numbering: phase 5 is **Check** (`bynk-check`,
`Resolved → TypedProgram`), independent of and pre-dating the trajectory's own phase numbering, which
merely happens to track the reference's numbers by convention. R3.5's own text confirms the same usage
— "All semantic checking … happens in phase 5" is the reference's Check phase, the same rule
`semantics-in-the-checker.md` closed by relocating existing `bynk-emit` diagnostics into `bynk-check`.
R6.10's "resolved in phase 5" by the identical convention means: `Callee` classification is checking
work, and belongs in `bynk-check`, not in `bynk-emit`'s lowering pass. It is **not** a claim that the
trajectory's phase-5 *track* should already have built it — that track's own settled Q2
(`design/archive/retired-tracks.md`) scoped itself explicitly to the seven `bynk-check/src/analysis.rs`-
named diagnostic categories plus two sites its own accounting missed; a new `Callee` enum was never
named, argued, or implied as part of that scope by any of its own five design questions. Building it
is new work this phase needs and must commission itself: **P6.0**, adding a resolved `Callee`
classification to `bynk-check`'s own checked output (extending `TypedCommons`/`CheckedProgram`, not a
new phase-5-track reopening), which every later lowering slice (P6.2 onward) then consumes rather than
re-deriving from the AST the way `lower_method_call`'s "the order is load-bearing" dispatcher does
today.

### 3.5 Q5 — Is the trajectory's "relative size 20, low confidence" estimate still the right sizing? **Settled.**

**Decision: yes, materially unchanged — this settling pass found the phase's real surface, and it
really is this large.** Unlike every predecessor phase on this trajectory (4 found its phase smaller
via P4.1's side effect; 5 found its phase smaller via the same P4.1 side effect, twice), this one does
not shrink under scrutiny. §1's correction removes R6.2–R6.4/R6.6 from the "already paid down" column
entirely — they were never this phase's rules to begin with, only adjacent ones in the same reference
Part — leaving R6.5 and R6.11 as the only two of the phase's own 23 rules with real partial progress,
against twenty-one with none. The ten-slice candidate decomposition in §6 is offered as the concrete
replacement for the trajectory's abstract "20" figure, the same role `semantics-in-the-checker.md`
§1/§6's seven-category accounting played for its own row — but where that accounting *shrank* the
estimate, this one largely confirms it.

### 3.6 Q6 — Part 13.4's idempotency "partial failure" names an unbuilt extension point (E7). Does phase 6 build it? **Settled.**

**Decision: no — named as a forward reference (§7), not built.** Part 13.4 (`bynk-greenfield-compiler.md`)
records its own worked-exercise finding honestly: a durable `Idempotency` provider needs a
"transactional-participation contract" (E7, Part 14) that does not yet exist, and calls the case a
"partial failure" for exactly that reason. No durable `Idempotency` provider exists in the shipped
compiler or its fixtures today — confirmed by search, not assumed. Building E7 speculatively, against
no real consumer, is precisely what R10.3's "carve when a dependency arrives, not on appetite"
discipline argues against, the same "named trigger, not appetite" treatment
`semantics-in-the-checker.md` §3.5 gave its own R10.1 crate-split deferral. `CommitShape` (R6.15,
§6 P6.8) is still built as IR data rather than emitter control flow — that part of Part 13.4's own
argument for *why* E7 will eventually be buildable ("the participation points have to be enumerable by
something other than the emitter before a provider can opt into them") is this phase's business; adding
the opt-in contract itself, gated on an actual durable-provider proposal appearing, is not.

---

## 4. Posture

Extends ADR 0059 as amended by ADR 0309, the same standing properties every track on this trajectory
has run under: behaviour-preserving by default (a slice that changes observable behaviour states so,
ships a fixture, gets a CHANGELOG entry); short-lived branches, one PR per slice; no per-increment ADRs
beyond what §11 front-loads; every slice citing `Closes-Rule:`.

---

## 5. The completion criterion

Same principle as every prior track on this trajectory: a slice is complete when the old path is
**deleted**, not when the new home merely exists alongside it. Here: `ast_importers` reads **0** —
unlike R3.5's own 4/4 floor (four `#[cfg(test)]` assertion strings with a structural reason to survive
retirement), no analogous carve-out is known for this probe today. That is a claim to verify at
completion, not assume now: if a lowering-pass unit test needs to hand-construct an `Ast` node directly
rather than parsing real source text (this codebase's own established preference — see
`bynk-check/src/secrets.rs`'s own test-authoring note from P5.5, "parsing real source rather than
hand-building a `Handler`"), the probe could carry its own small residue the way `emit_diagnostics` and
`fs_below_driver` both did. Confirm, don't assume, per §9's own "the evidence ages" discipline.

`bynk-emit/src/emitter/lower.rs`'s `stmts: &mut Vec<String>`-adjacent AST-walking functions either no
longer exist or call only into `bynk-emit::ir`'s lowering pass; every declaration-emission site reads
`IrItem`, never `AgentDecl`/`ServiceDecl`/`ActorDecl`/`TypeDecl` directly.

**P6.9's own grounding pass (#1167) found a stated exclusion is needed here, not merely a possible
future carve-out.** `ast_importers`'s own definition (`xtask/src/greenfield_status.rs`) counts every
file in `bynk-emit/src` whose contents match the literal string `bynk_syntax::ast` — a set that, by
construction, includes `bynk-emit::ir`/`ir/lower.rs` themselves, since an `Ast → Ir` lowering pass
reading `bynk_syntax::ast` is not a defect this track is closing, it is the pass's entire job.
Confirmed live: the probe's own crate-wide grep reads **11** files as of P6.9, not the **9** this
section's own estimate carried forward (that count predates P6.1, #1141 — `ir.rs`/`ir/lower.rs` did not
exist yet when it was taken). This is a probe/goal mismatch, not a track failure: the prose criterion
above (`emitter/`'s AST-walking functions gone) remains the right target and is unchanged by this
correction — but the probe itself needs a stated exclusion for `bynk-emit::ir`'s own legitimate import
(or a differently-scoped successor probe reading only `emitter/`) before "`ast_importers` = 0" can be
read as this track's own true finish line, rather than a bar this track's own IR module structurally
cannot clear.

**#1176 lands that stated exclusion**, settling the "or" above: a small, named exclusion list
(`ir.rs`, `ir/lower.rs`) rather than a path-prefix rule scoped to `emitter/`. A prefix rule was
rejected — `project.rs`/`project/tests_emit.rs` also import `bynk_syntax::ast` today
(`EmitProjectCtx` holding `ActorDecl`/`AgentDecl` fields directly, and test/suite emission reading
`TypeRef`/`HandlerKind`), and that import *is* the still-open R6.13 defect this probe exists to track
(§6's P6.6 row), not a lowering-pass import; scoping to `emitter/**` would have silently excluded
both files along with `ir/`'s legitimate two. `ast_importers` now reads **9** with the exclusion
applied (11 minus `ir.rs`/`ir/lower.rs`) and can structurally reach **0** once every remaining
counted file's AST-declaration reads move to `IrItem`.

---

## 6. Slice decomposition

§3 is settled; the slices below are the candidate decomposition Q5 argues for — provisional in detail
(each becomes its own increment proposal, cut and possibly re-scoped as it lands, per
`design/tracks/README.md`'s own lifecycle step 3), not provisional in shape.

| Slice | What lands | Rules | Gated on |
|---|---|---|---|
| **P6.0** | `Callee` classification added to `bynk-check`'s checked output (extends `TypedCommons`/`CheckedProgram`) — the taxonomy from Part 6.5 (`Fn`/`Value`/`Ctor`/`Refine`/`Unsafe`/`Static`/`Method`/`Kernel`/`Query`/`Store`/`Capability`/`Agent`/`Cross`/`CrossCap`/`Intrinsic`), resolved once during checking, consumed (not re-derived) by every later slice; the `Kernel { recv, op }` case is generated from the one machine-readable kernel table rather than re-deriving a sixth hand-synced copy | R6.10 (partial), R6.11 | §3 settled (Q4) |
| **P6.1** | `bynk-emit::ir` module: `IrExpr`/`IrStmt`/`Callee` core types (Part 6.2) and the `CheckedProgram → Ir` lowering skeleton for the node kinds needing no new resolution — `Const`, `Local`, `Global`, `Record`, `Field`, `List`, `Block`, `If`, `And`/`Or`/`Not`, `Return`, `Await`/`Send`/`Pure` | R6.1 (partial) | §3 settled (Q2, Q3) |
| **P6.2** | `Call`/`Lambda` lowering driven by P6.0's `Callee`, replacing `lower_method_call`/`lower_call`'s two hand-synced ordered-guard dispatchers; a store write reached through a `Callee::Store` op is detected by matching the callee, not a name — closes the name-matched-receiver defect `block_writes_state` still carries (§9) | R6.5, R6.10, R6.12 | P6.0, P6.1 |
| **P6.3** | The desugaring table (Part 6.4): `Question`, `Is`, `Implies`, `RecordSpread`, `Expect`, `Val`, `Observation`, `Trace`, `Wire`, `EffectPure` as one exhaustive, normative match — closes the `let x = match risky()? { … }` miscompile class directly (§6.3's worked example) | R6.7, R6.8, R6.9 | P6.1 |
| **P6.4** | Pattern IR (`IrPat`, `IrArm`, `Exhaustive`) per Part 5.1 — closes `pattern_match_tests`'s `Vec<String>` destruction (R5.1), records the or-pattern binding mode and guard/refinement ordering as recorded IR properties rather than emission-time discoveries | R5.1, R5.4–R5.11 | P6.1 |
| **P6.5** | `Match` lowering wired to P6.4's `IrPat` — ties the core `Match`/`IrArm` node from P6.1 to the pattern IR, deciding the lowering form (R5.2) and arm independence (R5.3) once rather than re-deriving either at three emission sites | R5.2, R5.3 | P6.1, P6.4 |
| **P6.6** | Declarations as `IrItem` (Part 6.6): `Type`/`Fn`/`Agent`/`Service`/`Actor`/`Capability`/`Provider` — closes the emitter reading AST declarations directly (`EmitProjectCtx`'s 28 fields, 5 with no readers) | R6.13 | P6.1–P6.3 |
| **P6.7** | Store-field state shape and index tables derived in the IR (`StoreFieldIr`, `StoreKindIr`) rather than at emission time | R6.14 | P6.6 |
| **P6.8** | `CommitShape` as IR data, not emitter control flow | R6.15 | P6.6, P6.7 |
| **P6.9** | Handler-invocation origin-independence — no IR node branches on caller kind | R6.16 | P6.6, P6.8 (needs its own investigation at proposal time — this settling pass did not trace the shipped wrapper-selection rule R6.16's own rationale names in full) |

**Completion probe:** `ast_importers` = **0** (§5) — live today at **9**, gated
(`greenfield_status_table_is_current`). §5's own P6.9 correction (#1167) named why the prior,
unexcluded crate-wide count could never reach 0 while `bynk-emit::ir` exists at all; #1176 closed that
gap with a named exclusion for `ir.rs`/`ir/lower.rs`, so the probe can now genuinely reach 0 as the
remaining slices land. The prose criterion (§5) is still the true target — a reader of this or any
later P6.x slice should not read "last row in the table" as "this slice reaches the completion probe."

---

## 7. Out of scope — forward references, not refusals

| Item | Phase | Entry condition |
|---|---|---|
| The `bynk-ts` tree and printer | 7 | this track's probe (`ast_importers`) reads 0 (§5) |
| Carving `bynk-ir`/`bynk-lower` as their own crates, per the reference's own target graph (Part 10) | 7 | `bynk-ts` exists and gives the IR a genuine second consumer — R10.3's own trigger, not this track's appetite (§3.3, Q3) |
| Severing `bynk-emit`'s dependency on `bynk-check` | 7 or later | the IR is proven complete enough (post this track) that `bynk-emit`'s remaining `Ir → TsProgram` logic never falls back to a `bynk-check` type — not one of this phase's own reference rules (trajectory §3 omits R10.1/R10.2 from phase 6's list) |
| Part 14's E7 (durable capability-provider transactional participation) | *unopened — no trigger yet* | a real durable `Idempotency` (or equivalent) provider proposal appearing, not the worked exercise alone (§3.6, Q6) |
| Incrementality — query granularity, `UnitSignature`, the firewall | 8 | phases 0, 3 and 4 complete (already true; phase 8 itself waits on phase 7 per the trajectory's stated order) |

---

## 8. Keeping the reference true

No new probe infrastructure is needed for this phase's gate — `ast_importers` already exists, already
runs in CI (`greenfield_status_table_is_current`), and already reads the value this track needs to
drive to zero. One correction worth raising under review, ahead of the generator: Appendix D's R6.1
row and the trajectory's own phase-6 prose both predate this settling pass's R4.9/R6.1 finding (§3.2,
Q2) and should note the panic-not-fallback discipline once P6.1 lands, the same routine update
`project-model.md` §3.5 and `semantics-in-the-checker.md` §8 both made ahead of the generator for their
own stale rows.

---

## 9. Risks

**This is the largest, lowest-confidence phase on the trajectory, and this settling pass confirmed
that rather than shrinking it.** Trajectory §8 names exactly this risk: "A phase's estimate is wrong by
a large factor … the phase boundary is the stopping point, and the trajectory's value is what has
already landed, not what remains." Ten candidate slices is this track's best current answer, not a
guarantee — P6.2 (the `Callee`-driven call/lambda rewrite) and P6.6 (declarations as `IrItem`) are the
two most likely to need their own mini-settling pass once their own shape is examined closely, the same
way P5.4 needed one for test/integration-suite processing.

**P6.0 (`Callee` classification in `bynk-check`) is a load-bearing prerequisite every other slice
depends on, and it lands in a different crate than the rest of this track's work.** Getting its shape
wrong is expensive to unwind across nine dependent slices — worth its own careful review independent of
this settling pass's own confidence in Q4's answer.

**R6.5's data-loss defect (the ADR-0076 security/safety trigger, partially met) stays live until P6.2
lands.** `block_writes_state`'s name-matched receiver — the "strongest single argument in the document"
for R6.5 per the reference's own text — is not closed by this settling pass; it is closed by a specific
future slice. Named here so a reviewer of any interim state knows the gap is real, not latent.

**The evidence ages.** Every fact, line number and quotation in this doc was measured against `main`
at the commit this settling PR is opened from, 9 August 2026. Re-check before a slice proposal cites
one, per every prior track's own §9 — this doc's own §1 is a direct demonstration of why that
discipline exists, having found and corrected an imprecision in the spine issue that opened this same
track only hours earlier.

---

## 10. What this phase causes

Per the trajectory's own "6 before 7" ordering: phase 7 (`bynk-ts`) needs a stable `Ir`/`IrItem` shape
to print against, the same way phase 6 needed phase 5's checking to have already left the emitter. This
phase's own settled Q3 sharpens what phase 7 inherits precisely: not a blank slate, but a named entry
condition (`bynk-ir`/`bynk-lower` carved out of `bynk-emit` once `bynk-ts` is the IR's real second
consumer) and a named, deferred extension point (Part 14's E7) that phase 7 does not have to solve
either, only keep open for whichever phase eventually does.

---

## 11. ADRs

Per ADR 0167 step 2, load-bearing, hard-to-reverse decisions land before slicing. Three do, with this
settling pass (numbers assigned at merge by the stamp; referred to by letter until then):

- **ADR-A** — the IR and its lowering pass land inside `bynk-emit` as new modules; `bynk-ir`/`bynk-lower`
  are not carved as separate crates this phase, per R10.3's own "at the moment the dependency appears"
  discipline — named as phase 7's entry condition, not refused. §3.3 (Q3).
- **ADR-B** — `Callee` classification is checking work and lands in `bynk-check` (P6.0), not
  `bynk-emit`'s lowering pass — new work this phase commissions, not scope the retired phase-5 track
  missed. §3.4 (Q4).
- **ADR-C** — lowering driven from a certified `CheckedProgram` enforces R6.1's totality by
  construction via a single total walk that `.expect()`s each `expr_ty` lookup rather than falling
  back on a miss, matching `lower_workers_cross_context_call`'s own
  `bynk.emit.unresolved_cross_context_signature` precedent — scoped to that path only, since
  `tests_emit.rs`'s non-certified `TypedCommons` producer needs the existing fallback to stay;
  `expr_types` stays a `HashMap`, and R4.9's `IndexVec` conversion is filed as optional, non-blocking
  residue. §3.2 (Q2).

---

## 12. Retirement

Mirrors every prior track on this trajectory: retires when §6's probe (`ast_importers`) reads its named
floor (**0**, per §5 — confirm no test-fixture residue survives, the same caution every prior floor
needed) and every slice named to reach it has landed. The retirement PR removes this doc, appends its
closing summary to `../archive/retired-tracks.md`, and closes the spine issue
([#1137](https://github.com/accuser/bynk/issues/1137)).
