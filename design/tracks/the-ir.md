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

> **Provenance.** Q1–Q6 below originate in the spine issue (#1137); none was closed there — opening a
> spine issue states the questions, it does not argue them (`design/tracks/README.md`'s own lifecycle
> step 1 vs. step 2). This settling pass is where each is actually argued, under this draft PR. Q7
> (§3.7) was opened later, from #1175 rather than the spine issue, and carries its own Provenance note.

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

### 3.7 Q7 — §5's completion criterion says `emitter/lower.rs`'s AST-walking functions "call only into `bynk-emit::ir`'s lowering pass." Given the printer (reference Part 7, R7.2/R7.3) is phase 7's, not this phase's, what does that require of `emitter/lower.rs`'s own string output? **Settled.**

> **Provenance.** Unlike Q1–Q6, this question does not originate in the spine issue (#1137) — it is
> [#1175](https://github.com/accuser/bynk/issues/1175), opened after this track's own §3 settling pass
> closed, once P6.0–P6.9 and the `IrItem`-variant slices (#1169/#1171/#1172/#1173) made the gap between
> "the IR exists" and "the emitter reads it" concrete enough to name precisely. Argued here for the same
> reason Q1–Q6 were argued under this doc rather than left as open bullets on the spine issue.

**Decision: `emitter/lower.rs` keeps writing strings — the cutover changes what its functions read, not
what they return.** #1175 named the genuine ambiguity directly: does the cutover leave
`emitter/lower.rs` doing its own string-splicing but *driven by* IR-computed decisions, or something
else? "Something else" would mean this phase building an ad hoc `Ir -> TsProgram`-shaped intermediate to
route through — and that is ruled out, not merely undecided, by two things this track has already
committed to. First, §2's own exclusion: "Not phase 7 (the TypeScript tree and printer) … this phase
only replaces what feeds emission." Second, the reference's own R7.2/R7.3 (`bynk-greenfield-compiler.md`
Part 7.1) name the printer split precisely — "Emission is `Ir -> TsProgram`. It performs no string
formatting, owns no buffer, and has no notion of indentation" / "Printing is `TsProgram -> Artefacts`.
The printer owns the buffer … it is the only code in the compiler that writes a character" — and no
`TsProgram`, no printer, and no `bynk-ts` crate exist yet (confirmed live: no `bynk-ts` directory in the
workspace). Building a phase-6-local stand-in for either would take on R7.1–R7.8's job early, on a shape
phase 7 would then have to either inherit unreviewed or redo, exactly the "carve before the dependency
arrives" anti-pattern R10.3 already argues against elsewhere in this track (§3.3, Q3).

`Lowered { pre: Vec<String>, expr: String }` (`bynk-emit/src/emitter/lower.rs:844`, T2.1/R6.2, phase 2)
is the concrete shape this settles: every `lower_*` function in `emitter/lower.rs` returns a `Lowered`
today and will keep doing so after the cutover — the T2.1 statement-sink signature is untouched, because
it already satisfies R6.2 and nothing about consuming `bynk-emit::ir` instead of `bynk_syntax::ast`
requires revisiting it. What changes is the *argument* type and the logic inside: today `lower_expr(e:
&Expr, cx: &mut LowerCtx) -> Lowered` walks a `bynk_syntax::ast::Expr` and re-derives its own dispatch
decisions — `lower_method_call`/`lower_call`'s own "the order is load-bearing" guard chains,
`block_writes_state`'s name-matched receiver (§9's still-live R6.5 defect) — from the AST node in front
of it. Post-cutover, the equivalent function takes an `&IrExpr` (`bynk-emit/src/ir.rs:134`) and reads a
decision `bynk_emit::ir::lower` already made once — `Callee` for dispatch (P6.0, #1139),
`CommitShape`/`IrPredicate` for a handler's commit shape (P6.8, #1165), `StoreFieldIr`/`StoreKindIr` for
a store write's real target (P6.7, #1163) — rather than re-classifying anything. The function still
hand-writes TypeScript source text into `Lowered.expr`/`Lowered.pre`; it just no longer decides *what* to
write by walking AST shape, only *how* to render a decision the IR already recorded. This makes
`emitter/lower.rs` this phase's own de facto interim `Ir -> String` pass — not a violation of R7.2/R7.3,
which gate the real, shipped `TsProgram`/printer split phase 7 commissions, not an interim state this
phase is expected to avoid leaving behind. `IrItem::Fn`'s own doc comment (`bynk-emit/src/ir.rs`, P6.6)
already treats this as established: "which `IrItem::Fn`s a future printer re-attaches under which
`IrItem::Type`'s own namespace (R8.1) is phase 7's own concern, not decided here" — written before this
settling pass, on exactly this assumption, not after it.

**Confirmed live** (12 August 2026, `main`@`7f5115ee`): zero `crate::ir`/`use crate::ir` references
anywhere in `bynk-emit/src/emitter.rs` or any `bynk-emit/src/emitter/*.rs` file — the cutover named in
#1175 has not started on any file. `bynk-emit/src/ir.rs` + `bynk-emit/src/ir/lower.rs` together are
7,837 lines (1,153 + 6,684) — up from nothing at this track's own 9 August opening — and now assemble
`IrItem::Type`/`Fn` (P6.1/P6.6), `Agent` (P6.10, #1169), `Service` (P6.11, #1171) and `Capability`
(P6.12, #1173), with `Actor` settled as a deliberate non-build decision (#1172) and `Provider` still open
(#1174). `emitter.rs` (4,676 lines) and `emitter/lower.rs` (5,978 lines) have each grown slightly past
§1's own 9 August measurement (4,653/5,970) under ordinary unrelated traffic, not shrunk — the cutover
itself remains fully ahead of this settling pass, exactly as #1175 described it, and this Q only settles
its operational shape, not its slicing. Slicing the cutover itself is not attempted here — per #1175's
own framing it stays a scoping placeholder until the remaining deferred `IrItem` variant (`Provider`,
#1174) and the `ast_importers` probe redefinition (#1176) are far enough along that a real proposal can
cite a stable IR surface to cut over to.

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

**§3.7 (Q7, #1175) settles what "calls only into `bynk-emit::ir`'s lowering pass" requires structurally,
a second correction of the same kind as the one above.** The phrase does not mean `emitter/lower.rs`
stops writing TypeScript source text — it cannot, until phase 7's own printer (R7.2/R7.3) exists to take
over, which this track's own §2 already excludes. It means the AST-walking *decisions* those
string-writing functions currently re-derive (`Callee` dispatch, commit shape, store-write target) get
replaced by reads off an already-lowered `IrExpr`/`IrItem`/`CommitShape`/`StoreFieldIr` value; the
functions' own `Lowered`-returning, string-writing shape survives the cutover unchanged. A slice proposal
for the cutover itself is not commissioned by this correction — per #1175's own framing, it remains a
scoping placeholder pending `Provider` (#1174) and the `ast_importers` redefinition (#1176).

**Known gap the exclusion accepts, not closes (#1176 review):** `ir.rs` itself still holds several
AST types directly in `IrItem`-adjacent struct fields today — `Arc<TypeDecl>`, `Arc<FnDecl>`,
`HandlerKind`, `Refinement`, `SchemaVersionPattern` — not yet lowered to IR-native equivalents. An
emitter reading one of those fields (e.g. `IrHandler::kind`, which *is* `ast::HandlerKind`) touches
the AST without ever spelling `bynk_syntax::ast` itself, so it is invisible to this probe by
construction — rewriting a match like `emitter/wrangler.rs`'s own `bynk_syntax::ast::HandlerKind::Cron`
pattern to bring the type in under a local, unqualified import would drop the probe by one with zero
movement on R6.13. So `ast_importers` = 0 proves no *remaining* file outside `ir.rs`/`ir/lower.rs`
imports the AST module directly; it does not by itself prove every `IrItem` field is AST-free. Closing
R6.13 in full still needs the same manual confirmation §5's own "Confirm, don't assume" discipline
already calls for on the test-residue question above — inspecting `IrItem`'s own field types, not just
reading this probe's count.

**#1187's own closing scoping pass (2026-08-16) extends the exclusion once more, on evidence rather
than a restatement of Q7 — and, per review of #1210, narrower than first proposed.** The first draft of
this correction also named `emitter.rs`/`emitter/lower.rs` themselves, reasoning that Q7 (§3.7) settles
these files keep hand-writing TypeScript source text after this track's cutover, so their own function
signatures will always spell `bynk_syntax::ast` for header/body-rendering parameters the same
structural way `ir.rs`/`ir/lower.rs` always will for the opposite half of the `Ast ⇄ Ir` boundary.
Review found that argument proves too much: both files also hold live, currently untouched AST-
*declaration* reads with no such gate — `emitter.rs`'s own `CommonsItem::Service`/`svc.protocol` walk
(consumed-event-root collection) and `emitter/lower.rs`'s own `cap_op_param_names`
(`CommonsItem::Capability`/`c.ops`/`o.params`) are exactly the P6.2/P6.6-class conversions §6's own
table still lists as in scope, not body-rendering Q7 already settled stays AST-parameter-driven.
Excluding either file would have hidden that real, fixable surface from this probe the same way a
path-prefix rule would — the harm the #1176 exclusion's own "named not prefixed" discipline exists to
prevent, just at file granularity instead of directory granularity. Both stay counted.

`project/tests_emit.rs` alone joins the exclusion, on its own, narrower evidence: its test/suite case
bodies call `emitter::lower_block_to_async_body`/`lower_test_case_body`/`lower_integration_case_body`
directly (the Q7-settled body-rendering pass itself, not a declaration read), and its own
`driver_param_ty`/`strip_effect_httpresult` read a handler's *declared* param/return `TypeRef` with no
corresponding `TyId` available at that call site — the same caller-reads-callee's-raw-declared-shape
pattern #661 established for cross-context codec generation. This is a correction of the #1176-era
exclusion's own explicit stance (`project/tests_emit.rs` was named there specifically as something that
"must stay counted," and a test asserting exactly that existed until this pass), not a silent reversal:
the evidence changed because #1187's own scoping work is what surfaced these two call sites, which
`project.rs` (still genuinely open R6.13 work — `plan_agent_given_deps`/`instantiate_provider_expr`/
`unit_table_uses_emit`/`called_cross_context_services`/`own_contract_hashes`) and `emitter.rs`/
`emitter/lower.rs`'s own declaration reads above do not share.

`ast_importers` reads **7** with all three names excluded (8 minus `project/tests_emit.rs`) —
`runtime_use.rs`, `emitter.rs`, `emitter/lower.rs`, `emitter/workers.rs`, `emitter/serialisation.rs`,
`emitter/workers_entry.rs`, and `project.rs` remain counted. This is not a new floor — §5's own
completion criterion (`ast_importers` = **0**) is unchanged by this correction, only what the probe
excludes; `emitter.rs`/`emitter/lower.rs`'s own declaration-reading surface, and `project.rs` generally,
remain genuinely reachable by future P6.2/P6.6-class slices the way this section's own slice history
already documents.

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

**Correction (2026-08-18): P6.3's own row landed partially, silently, without ever being recorded in
this section's own slice-history prose below.** Confirmed live against `ir/lower.rs`: 3 of the row's
10 desugar constructs are real — `Implies`/`RecordSpread` (`#1146`, 2026-08-10) and `EffectPure`
(`ir/lower.rs:2130`). The other 7 are still `todo!()` (`ir/lower.rs:2280` `Question`, `:2340` `Is`,
`:2352` `Expect`, `:2359` `Val`, `:2363` `Wire`, `:2367` `Observation`, `:2371` `Trace`, plus
`lower_stmt`'s own `:1977` `Statement::Expect`) — but 5 of those 7 (`Expect`/`Val`/`Wire`/
`Observation`/`Trace`, and the `Statement::Expect` gap) are reachable only when
`ctx.in_test_body` is set, and `bynk-check/src/checker.rs:1325` is the only site that ever sets it
`true` (five other sites set it `false`) —
meaning those five are unreachable from any shipped handler/provider/agent body today, the same
permanent-residue shape `project/tests_emit.rs` already has in `AST_IMPORTER_EXCEPTIONS` (§5), not
open engineering work. Only **`Question`** and **`Is`** are real, production-reachable gaps — and per
the two corrections above, they are the actual load-bearing prerequisite blocking `IrItem::Provider`
(P6.14/#1186) from getting a real call site, and by the same reasoning would block any future
`IrItem::Agent`/`Service` full-body cutover and the still-unlanded P6.2 emitter-side `Call`/`Lambda`
dispatch cutover (`lower_method_call`/`lower_call`, `emitter/lower.rs:1449,4166`, still 100%
raw-AST-driven — `bynk-emit::ir::lower` itself has had a fully `Callee`-driven Call/Lambda/Variant
lowering since `a2bac9a8`/2026-08-09, but the emitter never calls into it). A slice proposal for
`Question` should expect a real open design question, not a mechanical port: the shipped desugar
(`emitter/lower.rs:1012-1042`) branches three ways on the operand's checked type, so it needs its own
settled `IrExprKind` shape decided first, the same way #1225 settled `Ok`/`Err`/`Some`/`None`'s
identity question before building it. `Is` additionally needs R5.9/R5.10 un-deferred (#1157's own
Decision D) before it can land.

**Completion probe:** `ast_importers` = **0** (§5) — live today at **9**, gated
(`greenfield_status_table_is_current`). §5's own P6.9 correction (#1167) named why the prior,
unexcluded crate-wide count could never reach 0 while `bynk-emit::ir` exists at all; #1176 closed that
gap with a named exclusion for `ir.rs`/`ir/lower.rs`, so the probe can now genuinely reach 0 as the
remaining slices land. The prose criterion (§5) is still the true target — a reader of this or any
later P6.x slice should not read "last row in the table" as "this slice reaches the completion probe."

**The table above is the "build the IR" decomposition — every row shipped** (P6.0–P6.9 as listed,
plus `Agent`/`Service`/`Capability`/`Provider`/the websocket lifecycle slices §3.7 names as landed
past this table's own original scope, `Actor` settled as a deliberate non-build). It is not the
**cutover** decomposition §5's own completion criterion actually needs — the slice order for
switching `emitter.rs`/`emitter/lower.rs` themselves over to read `bynk-emit::ir` is scoped
separately in #1187 (opened once `Provider` and the `ast_importers` redefinition — the two blockers
§3.7 named — landed). First slice: `Type` (#1188, `emit_type`/`emit_record_type`/`emit_sum_type`/
`emit_refined_type` now read `TypeShape` instead of `TypeDecl`/`TypeBody`) — landed. `ast_importers`
itself does not move from this slice (`emitter/emit.rs` reaches AST types via `use super::*` without
ever spelling `bynk_syntax::ast` literally, the same invisible-to-the-probe shape this section's own
"known gap" paragraph above already names for `IrItem`'s fields).

Second slice: `wrangler.rs` (#1191, narrowed from #1187's own pairing with `runtime_use.rs`) —
`emit_wrangler_toml`'s two raw matches on a handler's cron kind and a service's queue-binding
protocol relocate to its one call site in `project.rs`, which already imports `bynk_syntax::ast` and
is already counted. Unlike slice 1, `ast_importers` does move here — 9 to 8 — because this slice's
whole AST footprint was exactly those two matches, with no equivalent living on inside `bynk-emit::ir`
to route through instead (`IrHandler::kind` reuses `HandlerKind` unchanged, so an `IrItem`-based
version would still have matched it). `runtime_use.rs` did not land with it: its `TypeRef` field is
downstream of `emitter/serialisation.rs`'s still-`TypeRef`-driven JSON-codec renderer, a real
conversion #1191 found while scoping, not a relocation — deferred, unscoped, until a future slice
proposes converting that renderer.

Third slice: `Capability` (#1193, narrowed from #1187's own pairing with `Provider` — a correction
comment posted on #1187) — `emit_capability` now reads each op's resolved `params`/`return_ty` off
`OpSig` (`bynk-emit::ir`, built by `lower_capability_item_ir`/`lower_op_sig_ir`) through `ts_ty`,
instead of walking `CapabilityOp::params`/`return_type` `TypeRef`s through `ts_type_ref` directly.
`Provider` did **not** land with it, despite #1187's own candidate pairing: unlike a capability op (a
signature only, no body), a `ProviderOp` always carries a real body, and `lower_provider_op_ir` routes
it through `lower_expr_ir` — the same general expression-lowering pass that still hits #1189's open
comparison/arithmetic `IrExprKind` gap. `ast_importers` does not move from this slice, the same
invisible-to-the-probe shape slice 1 already established (`emitter/emit.rs` reaches AST types via `use
super::*` without spelling `bynk_syntax::ast` literally).

#1189 (comparison/arithmetic/unary-negation/string-interpolation `IrExprKind`) landed next, settling the
gap named above — real `IrExprKind::BinOp`/`Neg`/`InterpStr` variants, deliberately extending P6.1's own
Decision D (that row's own "the whole Part 6.2 shape lands in one piece, never widens again" framing):
#1189's own resolution found the reference's Part 6.2 node-set listing never named these four constructs
at all, a true omission, not a deliberately-out-of-scope row this track chose to defer. This unblocked
real predicate/handler-body lowering for `Agent`/`Service` — but did not, on its own, make either slice
small; each still needed its own narrowing once scoped.

Fourth slice: `Agent` (#1196, narrowed hard — not the structural `IrItem::Agent` cutover this table's
own row implies). Scoping found the R6.5 defect (`block_writes_state`'s bare-identifier write-detection,
superseded by `bynk-emit::ir`'s already-correct `body_writes_state`) was cuttable on its own, with zero
dependency on `IrItem::Agent`'s full construction — `body_writes_state` takes a bare
`&Block`/`&TypedCommons`, no `StoreFieldIr`/`CommitShape`/`IrHandler` needed. That was the whole slice:
`emit_agent`'s implicit-commit-wrapper decision now reads `body_writes_state`; `block_writes_state`/
`StoreKinds`/`mutating_op` deleted. `emit_agent`'s own state-field emission (~350 lines threading raw
AST `RecordField`/`TypeRef` through interface/zero-factory/load-commit/held-connection-map rendering, no
`StoreFieldIr` equivalent for held maps) and its handler-body rendering (still the emitter's own
`LowerCtx`/`Pre` machinery — no `IrExpr → TS` printer exists anywhere in this crate) both stay fully
AST-driven, unscoped. `ast_importers` unaffected.

Fifth slice: `Service` (#1198, similarly narrowed). `emit_service`'s handler param/return-type/
effectful-ness and protocol-driven special params (WebSocket connection type, Events pattern/
schema-dispatch guards) now read `bynk-emit::ir` (`IrHandler`'s own new `ret: TyId` field, `ProtocolIr`,
a new `event_pattern_guard_ir`). Scoping (and a live repro during review) found building a real
`IrItem::Service`/`IrHandler` at the call site is not viable yet: `lower_service_handler_ir`
unconditionally lowers the whole handler body into `IrExpr`, and an ordinary `from http` handler
routinely constructs `Ok`/`Err`/`Some`/`None` — still `todo!()` in `lower_expr_ir`, confirmed independent
of #1189's own fix. A new standalone `lower_service_handler_signature_ir` reads only the signature,
never the body, mirroring `body_writes_state`'s own narrow-reader precedent (and, per review, needed its
own non-panicking fallback for service handlers specifically — the checker does not guarantee a service
handler's own param/return types resolve the way it does for an agent's). Handler body rendering,
actor-seam/deps-identity resolution (`bearer_seam_for`/`oidc_seam_for`/`caller_binder_for`/
`sum_members_for` — `IrHandler::binder`'s own `ActorBinder` doesn't distinguish Bearer/OIDC/Caller/
multi-actor-sum), `given` cross-context qualification (`IrHandler::given` is bare names only, no
`CapRefIr`-style qualification), and WebSocket DO-method generation (confirmed to live inside
`emit_agent`, not `emit_service`) all stay deferred. `ast_importers` unaffected.

**`Provider` remains entirely unscoped** — the deferral above still stands; no issue or PR has attempted
it since. (Superseded below: `Provider`'s own `given`/deps wiring landed as #1200; superseded further
by P6.14/#1186 below — the structural `IrItem::Provider` build did land, just with no emitter call
site yet.)

**Correction (2026-08-18): `IrItem::Provider` is no longer unbuilt — it has no emitter call site,
which is a different, narrower claim.** P6.14 (#1186, `e87691ff`, 2026-08-12) landed
`IrItem::Provider{def, cap, body: ProviderBody}` (`ir.rs:855-875`) with `ProviderBody::Bynk{given,
ops: Vec<ProviderOpIr>}`/`ProviderBody::External{given}`, and `lower_provider_item_ir`/
`lower_provider_op_ir` (`ir/lower.rs:1698,1825`) do lower a provider op's real body through
`lower_expr_ir`, not just its signature. This landed *before* §3.7 (Q7)'s own snapshot date but was
never folded into this doc's prose — confirmed live: `IrItem::Provider`/`lower_provider_item_ir`/
`lower_provider_op_ir` have zero references anywhere in `emitter*.rs`/`project.rs` outside comments
and `ir/lower.rs`'s own unit tests. The reason is named in `lower_provider_given_ir`'s own doc comment
(`ir/lower.rs:1719-1745`, the sibling reader `lower_provider_item_ir` also calls):
`lower_expr_ir` still `todo!()`s on `ExprKind::Question` and `ExprKind::Is`, and a real provider op
body routinely uses `?`. So the accurate status is: **built and tested, blocked on the same
`Question`/`Is` gap this section's own P6.3 row left open** (see the P6.3 correction below), not
"unscoped." Wiring a real call site at `project.rs`'s provider-instantiation site is real,
well-defined future work once that gap closes — not a fresh design question the way the original
build was.

**Correction (2026-08-18): #1232/#1233 landed the event-subscriber envelope decision this section's
own fifteenth-slice entry below still describes as open, and did so with evidence worth recording
here.** `project.rs`'s `wants_envelope` (event-fanout envelope-forwarding decision) now reads a new
project-wide `EventSubscriberShape` accumulator, populated at each unit's own check time — but that
accumulator is a *bespoke*, purpose-built raw-AST walk (`project.rs:1266`, populated inside
`check_unit_files`'s per-file loop), not a read off `IrItem::Service`/`ProtocolIr::Events`, even
though the latter already carries materially the same two facts (`ProtocolIr::Events`'s
`schema_dispatch: Option<SchemaVersionPattern>` field, `ir.rs:1018,1050`). The reason, confirmed by
grep: `IrItem::Service`/`lower_service_item_ir` have **zero** shipped emitter call sites anywhere —
outside the `ir` module (which carries its own doc-comment references, e.g. `ir.rs:1161-1162`'s own
"nothing in the shipped emitter constructs a real `IrItem::Service` yet"), the only two references are
comments in `emitter.rs:420` and `emitter/wrangler.rs:46` explaining why those sites *don't* use it.
This is independent, corroborating evidence for the same finding P6.14/#1186's own correction above
makes for `Provider`: this track has now built real `IrItem` data for `Service` and `Provider` that
nothing in the shipped emitter actually reads yet, and new slices are choosing to build fresh bespoke
plumbing around that gap rather than close it. Closing the `Question`/`Is` gap is the load-bearing
prerequisite for finally giving both real call sites — a slice sequence for that is proposed but not
yet issue-numbered as of this correction.

**Slice 6 (`project.rs` cleanup) is not yet ready, contrary to this table's own "once (1)–(5) land,
whatever's left here is residue" framing.** A scoping pass (2026-08-14) found `project.rs` holds zero
references to `bynk-emit::ir` anywhere in its ~4,200 lines; its substantive AST coupling
(`plan_agent_given_deps`, `instantiate_provider_expr`, `unit_table_uses_emit`,
`called_cross_context_services`, `own_contract_hashes`) is project-wide cross-cutting plumbing —
given-deps wiring, cross-context call detection, contract hashing — reading handler **bodies** and
**`given`/params** straight off `bynk_check::symbols::UnitTable`'s raw AST decls. That is exactly the
surface `Provider` (never started), `Agent`'s state/body (deferred by #1196), and `Service`'s
body/actor-seam/`given` (deferred by #1198) all still own — not residue left over once those slices
land, but a direct downstream dependent of them. One data point against "shrinks incrementally": the
`wrangler.rs` sweep (slice 2) *relocated* two AST matches into `project.rs` rather than eliminating
them, since `project.rs` was already counted — the file can grow as a landing spot, not just shrink.
`emitter/workers.rs`/`emitter/workers_entry.rs` (this table's own "trail `Agent`+`Service`" framing) are
likewise untouched since this section was written, for the same reason: what they trail hasn't landed
yet. Recommended next real slice: `Provider`'s own `given`/deps wiring (`instantiate_provider_expr`,
`project.rs`) — the smallest of the deferred pieces, and the one that most directly unblocks `project.rs`
itself.

**Sixth slice: `Provider`'s `given`/deps wiring (#1200)**, exactly the recommendation above.
`instantiate_provider_expr` now reads `lower_provider_given_ir` instead of walking `ProviderDecl::given`
directly. Scoping found a real, pre-existing data-loss bug in `ProviderBody::External`'s own construction
(a provider's `given` was silently dropped for the external/bodiless case) — fixed as part of landing the
reader, not filed separately, the same "narrow reader surfaces a real bug" shape this track's later
slices repeat throughout. `ast_importers` unaffected (`project.rs` was already counted).

**Seventh slice: per-unit `Callee` plumbing (#1202) and `emitter::block_uses_emit` (#1203).** Scoping
`project.rs`'s own cleanup (per the "not yet ready" finding above) found `unit_table_uses_emit`/
`called_cross_context_services` could convert to reading the checker's own resolved `Callee`
(`Callee::Capability`/`Callee::Static`, not a bare-`Ident("Events")` receiver-name match) once a new
per-unit `Callee` table was threaded forward from `bynk-check` through `project.rs`. Review of #1202
found a genuine cross-context-shadowing correctness bug this new plumbing fixed (pinned by fixture
`1203_cross_context_call_shadowed_by_local`) and a live inconsistency it exposed: `emitter.rs`'s own
per-handler `block_uses_emit` still matched on bare identifiers, disagreeing with `project.rs`'s now-
`Callee`-based project-wide check on a locally-shadowed `Events` type — #1203 converted it too, pinned by
fixture `1204_events_emit_shadowed_by_local_type`, which failed `tsc --strict` under #1202 alone. Neither
`ast_importers` move — both files were already counted.

**A formal completion plan for the remainder of #1187 was scoped and approved 2026-08-14/15/16**, after
"what is left for #1187?" surfaced that five of the issue's own seven originally-proposed slices had
narrowed hard (as this section already records) and the issue's own stated completion criterion
(`ast_importers` = 0) was not actually reachable for `emitter.rs`/`emitter/lower.rs`/
`project/tests_emit.rs` — the correction this section's own §5 addendum above now records. The plan
found three well-scoped tractable slices plus Agent's state-field emission (initially read as "too
entangled," a second pass found otherwise) and landed as follows:

**Eighth slice: `lower_handler_given_ir` (#1204).** A handler's own `given` clause — syntactically
identical to a provider's — reused `lower_cap_ref_ir` the same one-line-adapter way #1200 did. Scoping
found the real call-site graph was wider than the three sites the plan named: `effective_given`/
`build_deps_object_ty_with_surface`/`cap_ref_ty` (`emitter/emit.rs`) and `worker_cross_caps`
(`emitter/workers.rs`) all fed from the same `agent_method_givens`/`agent_given_caps_used` pipeline and
converted with it. `ast_importers` unaffected — real coupling removed at every site even though none of
the touched files' own AST-import status changed.

**Ninth, tenth, and eleventh slices: Agent state-field emission, in three ordered sub-slices (#1206,
#1207, #1208)**, closing this track's own P6.7-adjacent gap (`emit_agent`'s state interface/zero-factory/
rehydration rendering, deferred by the fourth slice above and never covered by `StoreFieldIr` in
practice). A pre-flight check (empirically verified against the real `bynkc` binary, not assumed) found
`store x: Cell[Bogus] = "hello"` certifies today — no checker pass validates a store field's own type
reference, only its shape — confirming the fallback posture (not a panic) every sub-slice needed.
- **2a (#1206):** `lower_store_field_shape_ir`, a shape-only sibling of `lower_store_field_ir` that never
  lowers a Cell field's `init` (avoiding this track's own still-open `Ok`/`Err`/`Some`/`None`
  `IrExprKind` gap, §7 below) — wired into `emit_agent`'s Cell/Map/Cache/Log interface rendering via
  `ts_ty`. Found and fixed a real dormant ICE: `resolve_store_field_ty` panicked on an unresolvable
  store-field type the checker silently accepts; falls back to `Ty::Unit` instead, mirroring
  `lower_op_sig_ir`'s identical posture (#1182).
- **2b (#1207):** Set fields and `@indexed` keys read off `StoreKindIr::Set`/`StoreFieldIr::indexed`
  instead of a second AST walk. Review asked for, and got, real end-to-end fixture coverage of the
  multi-key/dedup path (previously pinned only at the unit-test layer).
- **2c (#1208):** held-map frame types resolve through a new `held_frame_ty` (`TyId`-level, recursing
  through `Option`/`Effect` the same way the checker's own `type_ref_is_held` does) instead of a bare
  `TypeRef::Connection` match — fixing a second real, previously-uncovered bug: a `Map[K,
  Option[Connection[F]]]` value rendered the whole `Option<Connection<F>>` wrapper instead of unwrapping
  to `F`. Review found the first version of the fix introduced a regression of its own — a reachable
  panic on a checker-certified-but-unresolvable frame type (the same class of gap 2a's own preflight
  check named) — fixed with an AST-level fallback (`held_frame_ty_ref`) mirroring `resolve_store_field_ty`'s
  posture, pinned by a new fixture. None of the three moved `ast_importers`.

**Twelfth slice: `ActorSeamIr` (#1209).** A new IR type wrapping `bynk-check`'s five already-resolved
actor-seam structs (`bynk-check/src/actors.rs`) by value, and `lower_actor_seam_ir`, trying them in the
one priority order that's actually load-bearing (`sum_members_for` ahead of `bearer_seam_for` — a sum's
own first peer can itself be Bearer-schemed, since `bearer_seam_for` has no `by.is_sum()` guard of its
own; every other pair is mutually exclusive by construction). Scoping found only 2 of the ~11 call sites
across 5 files the original candidate named actually branch on which of several resolvers succeeded
(`emit_service`, and `emit_worker_compose`'s HTTP-dispatch match) — the rest call exactly one resolver
each or (`secrets.rs`) union all matching seams rather than picking one, nothing to collapse. Landed only
those two, named the rest explicitly in `lower_actor_seam_ir`'s own doc comment rather than silently
dropping them. Review found real latent value the PR undersold (the enum makes a duplicate-`identity:`-
field emission structurally unrepresentable, where the old four-independent-optionals shape could in
principle have produced one) and asked for, and got, a unit test pinning the one load-bearing ordering
decision directly rather than only transitively through a fixture bless. `ast_importers` unaffected.

**Slice 4 (`serialisation.rs`'s codec-name mangling to `TyId`), including its own `TypeBody → TypeShape`
sub-step, was scoped in full and then explicitly abandoned (2026-08-16) rather than landed** — the two
headline claims that motivated it both turned out false on inspection, not merely smaller than
estimated. The `TypeBody → TypeShape` sub-step (mirroring #1188's own `emit_type` conversion) only
changes *how* `emit_one`'s 3-way dispatch decision is made; the functions it dispatches to
(`emit_refined`/`emit_record`/`emit_sum`) immediately re-derive their own shape via
`bynk_check::wire::wire_type`, a separate, already-existing raw-AST-based unification untouched either
way — and 2 of its 5 real call chains (cross-context codec generation, `project/tests_emit.rs`'s
synthetic-commons path) have no `CheckedProgram`/`TyId` table available at all, so can't convert without
new plumbing. The codec-naming half's own stated payoff — `runtime_use.rs`'s `json_codec_roots` field
losing its `TypeRef` typing, moving `ast_importers` 8 → 6 — does not hold: `json_codec_roots`'s
downstream consumer, `bynk_check::wire::collect_codec_closure`, discovers record/sum field types by
walking `TypeDecl.body` (raw AST `TypeRef`, no resolved-field-type table anywhere in `bynk-check` to
substitute), so the field stays `TypeRef`-typed regardless of what its two producer sites in
`emitter/lower.rs` do. The one real, narrower win left after that correction — converting the leaf
codec-naming functions for the one call path that already holds a `TyId` (`Json.decode`/`Json.encode` in
`emitter/lower.rs`) — was found, on closer inspection, to buy only relocating one already-single
`ty_to_type_ref` call a few lines later, not eliminating it: negligible real payoff against the real risk
of new, hand-written, byte-for-byte-must-match codec-naming code (caller and callee independently derive
matching codec names with no shared registry, #661's own pattern). Recorded here, not silently dropped,
per this section's own "the evidence ages" discipline (§9) — a future slice revisiting this should start
from `bynk_check::wire::codec_suffix`/`inst_codec_suffix` (the real shared naming kernel, confirmed to
live in `bynk-check`, not `serialisation.rs`, contrary to where the original candidate pointed) and from
`collect_codec_closure`'s own missing resolved-field-type table as the actual blocking dependency, not
from `serialisation.rs`'s function list. Slice 5 (the event-subscriber envelope decision and `@cache`/
`@limit` annotation IR — always marked optional/lower-priority) was not scoped at all once 4's own
reversal made clear the remaining candidate slices needed the same scrutiny before any further
investment; left for a future pass to scope fresh rather than carried forward on the original plan's own
unverified estimate.

**Thirteenth slice: the completion-criterion correction itself, §5's own addendum above (#1210).**
`AST_IMPORTER_EXCEPTIONS` extends to `project/tests_emit.rs` alone, bringing `ast_importers` from 8 to
**7** — `runtime_use.rs`, `emitter.rs`, `emitter/lower.rs`, `emitter/workers.rs`,
`emitter/serialisation.rs`, `emitter/workers_entry.rs`, and `project.rs` remain counted. Review found
the PR's first draft also excluded `emitter.rs`/`emitter/lower.rs` and was wrong to: both still hold
live, in-scope AST-*declaration* reads (§5's own addendum has the detail) distinct from the Q7-settled
body-rendering surface that motivated excluding them, and excluding a whole file to hide a genuinely
open decision inside it is exactly the failure mode #1176's own named-not-prefixed argument exists to
prevent. 7 is not a new floor — 0 remains the stated target (§5), and the remaining seven files
(`emitter.rs`/`emitter/lower.rs` chief among them, per the two live sites just named) are real,
untouched, in-scope surface, not residue.

**Fourteenth: the `Ok`/`Err`/`Some`/`None` `IrExprKind` gap resolved (#1225, PR #1227)** — closing what
this section's own 2a paragraph above named as a forward reference to "§7 below," a promise §7 never
actually kept (no row was ever added there for it). Superseded here instead of there:
`IrExprKind::Variant` drops its `sum: Arc<TypeDecl>` field rather than widening it, since the wrapping
`IrExpr::ty` (already present by construction, R6.1) already carries the identical `TyId` identity a
constructed sum's own checked type resolves to — mirroring `IrPat::Variant`'s own `scrutinee_ty: TyId`
precedent exactly. `Ok`/`Err`/`Some`/`None` now lower to `IrExprKind::Variant` directly, closing the
`todo!()` open since P6.2/P6.3. `ast_importers` unaffected — still no shipped emitter consumer of this
construction path exists (P6.2's own `Call`/`Lambda` cutover has not landed), so this is a dormant-path
correctness fix, not a probe-moving one; confirmed by a zero-diff bless run. `Question`'s own three-way
desugar fork is a separate, still-open design question this resolution does not settle — moved to §7's
own table below, since that is the "§7 below" pointer's actual correct destination now.

**Fifteenth: Slice 5 scoped fresh, exactly as the paragraph above this one said a future pass would
(#1226), and split rather than landed whole.** The event-subscriber envelope decision
(`project.rs`'s own `wants_envelope`) stays open on #1226 itself: both its halves are pure syntax with
zero `TyId` dependency, so it is *not* blocked by the type-resolution wall that killed slice 4 — it is
blocked by missing plumbing, no project-wide, checker-verified channel yet carries a *foreign* unit's
own declared handler shape forward to `emit_composition_root`'s cross-unit compose pass, and
`bynk-check::resolver::CrossContextInfo` doesn't carry it either (wrong direction: a subscriber's own
`CrossContextInfo` describes what it consumes *from* the publisher, never the reverse). Fixing this
needs a new project-wide accumulator sized comparably to this track's own sixth slice
(`unit_callees`, #1202) above — real, separate, not-yet-proposed work, not residue.

**`@cache`/`@limit` route-annotation reading was the tractable half, split into its own issue and
landed (#1228).** `bynk-emit/src/emitter/workers_entry.rs`'s `cache_policy_for`/the annotation-reading
half of `effective_max_body` (raw `ExprKind::DurationLit`/`Ident`/`IntLit` matching) are gone, replaced
by two standalone `ir::lower` readers (`lower_route_cache_ir`/`lower_route_limit_ir`, no
`&CheckedProgram` parameter — `lower_policy_ir`'s own no-program precedent applies verbatim, since
`maxAge`/`scope`/`maxBody` are already-resolved literals). One trap found and avoided: `PolicyIr` (the
obvious-looking home) is dormant — `IrItem::Service`/`lower_service_item_ir` have zero non-test call
sites anywhere in the shipped emitter — so the new readers are standalone, the same live-consumer shape
`lower_protocol_ir`/`lower_handler_given_ir`/`lower_actor_seam_ir` already established, not a `PolicyIr`
field. `ast_importers` unaffected — `workers_entry.rs` remains counted regardless, since real,
untouched AST-declaration reads survive elsewhere in the file (its cron/queue-route and actor-seam
matches among them); real coupling removed at both call sites even though the file's own count doesn't
move, the same shape nearly every slice in this section shows.

**Sixteenth: the other half of slice 5 — the event-subscriber envelope decision — scoped in full and
landed (#1232, PR #1233), closing the fifteenth entry's own "real, separate, not-yet-proposed work"
above.** `EventSubscriberShape` (`project.rs:1266`), a small plain struct capturing
`two_param_handler`/`schema_dispatch` at each unit's own check time, is exactly the "new project-wide
accumulator sized comparably to `unit_callees`" the fifteenth entry called for — threaded
`check_unit_files` → `RunChecks::Checked` → `build_output` → `emit_composition_root`, the same three-hop
shape `unit_callees` (#1202) established. `wants_envelope` now reads it instead of a different,
already-consumed unit's raw `UnitTable`. Review found the fix's zero-diff bless proved only the
`two_param_handler` half of the predicate; a follow-up fixture
(`1232_events_envelope_schema_dispatch_bare`) pins the `schema_dispatch` half specifically, confirmed
to fail when that disjunct is reverted. `ast_importers` unaffected (`project.rs` was already counted).
**Worth recording as its own finding, not just a landed slice:** this is the *second* time in this
track a fresh accumulator got built to answer a cross-unit question `IrItem::Service` already carries
the data for (`ProtocolIr::Events`'s `schema_dispatch` field, `ir.rs:1050`) — because
`IrItem::Service`/`lower_service_item_ir` still have zero shipped emitter call sites, the same gap the
`@cache`/`@limit` slice immediately above hit and the same one P6.14/#1186's `IrItem::Provider`
correction (§6, "not yet ready" entry above) independently confirms for `Provider`. A future slice
giving `IrItem::Service` its first real emitter call site (gated, per that correction, on closing the
`Question`/`Is` gap in `lower_expr_ir` first) would let this accumulator — and the `@cache`/`@limit`
readers above — collapse into ordinary IR reads instead of each inventing its own project-wide
plumbing.

---

## 7. Out of scope — forward references, not refusals

| Item | Phase | Entry condition |
|---|---|---|
| `Question`'s own three-way desugar fork — what `IrExprKind` an `expr?` lowers to | 6, unproposed | a slice proposal for P6.3's desugaring table (§6) reaches `Question` specifically; #1225 (§6, fourteenth slice) settled the *construction*-side identity question this depends on but explicitly does not settle this one |
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
at the commit this settling PR is opened from, 9 August 2026 — except §3.7 (Q7), measured separately
against `main`@`7f5115ee`, 12 August 2026, and stamped inline with its own date for exactly this reason.
Re-check before a slice proposal cites one, per every prior track's own §9 — this doc's own §1 is a
direct demonstration of why that discipline exists, having found and corrected an imprecision in the
spine issue that opened this same track only hours earlier.

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
