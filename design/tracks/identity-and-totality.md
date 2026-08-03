# Identity and totality — migrating to the greenfield reference (phase 3)

- **Status:** **Settled — Slicing on merge.** §3's seven questions were argued under a settling
  review the same day this draft opened. Q1 was **reversed**: the draft's initial recommendation
  (allocate `ExprId` directly) is overturned in favour of the review's cheaper scaffolding, landed
  first and on its own merits — see the provenance note at the head of §3. Merging settles
  **direction**; it is not a build authorisation. Each slice is still an ordinary increment
  proposal, and `accepted` on that sub-issue is the approval to build.
- **Spine:** [#1046](https://github.com/accuser/bynk/issues/1046)
- **Theme:** **Phase 3** of [`../bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md) —
  node identity independent of position, every side table total, the editor consuming a program that
  does not compile. The trajectory's endpoint is the current compiler rebuilt on
  [`../bynk-greenfield-compiler.md`](../bynk-greenfield-compiler.md); this track walks phase 3 of it.
- **Phase boundaries are safe stopping points** (trajectory §2). Phases 0–2 already retired
  ([#996](https://github.com/accuser/bynk/issues/996)) leave a coherent compiler regardless of
  whether this phase lands.
- **Relates:** [ADR 0311](../decisions/0311-the-lowering-substrate.md) (D3, the trigger that opened
  this spine), [ADR 0309](../decisions/0309-refactor-acceptance-gate-per-tier.md) (the acceptance
  gate this track's §3.4 asks whether to amend), reference rules R2.2, R2.4, R2.5, R3.10, R4.1–R4.3,
  R4.9, [the 2026-07-27 pipeline review §1.3](../reviews/2026-07-27-compiler-pipeline-review.md)
  (`Span` is node identity — the finding this whole phase closes),
  [`../archive/retired-tracks.md`](../archive/retired-tracks.md)'s `compiler-architecture.md` closing
  entry (records why the trigger fired).
- **Precedent:** `compiler-architecture.md` (retired) is this track's direct predecessor on the same
  trajectory — its §4.1 named this phase's scaffolding "T3.1 and T3.2" as a forward reference without
  detailing it, and its §7 carried phase 3 as an entry-conditioned forward reference rather than a
  refusal. This doc is where both become real. `identity-and-totality.md` is the seventh track to run
  the ADR 0167 flow from the start (spine issue first, doc via a settling draft PR), after
  `compiler-architecture.md`, and continues its internal-architecture theme.

### ADR 0076 trigger check

| Trigger | Met? |
|---|---|
| Spans several increments | **Yes** — trajectory §5 rates this phase relative size 8, the largest attempted so far; §6 below names at least seven slices, two detailed and five forward-referenced |
| Surface not yet settled | **No** — the destination (reference Parts 2–4) is fully specified; what's open is sequencing and gate shape, which §3 exists to close |
| Security/safety boundary | **Incidental.** R3.10's "editor never calls `certify`" is adjacent to correctness, not a threat-model boundary the way phases 0–2's `ResolvedCommons` constructor was |

**One of three, same as `compiler-architecture.md`'s own count, and the same argument applies.**
ADR 0076's triggers are written for a language feature; an internal-architecture track's surface has
nothing to be "unsettled" about and its security content, where it exists, is incidental. The warrant
here is the one the tracks README states directly: §3's questions below govern every slice in §6 and
constrain phases 4–8, and that governance needs a home that is neither a proposal (which closes) nor
the trajectory (which holds the endpoint, not the method).

---

## 1. Why now

**Phase 2's probe reads zero.** `hoist_sinks` (`rg 'stmts: &mut Vec<String>' bynk-emit/`) has read 0
since T2.1 landed, confirmed at `compiler-architecture.md`'s retirement (3 August 2026, spine #996).

**§3.4's trigger fired, by ADR 0311's own wording.** The retirement summary in
`../archive/retired-tracks.md` records it precisely: ADR 0311 D3's test-density trigger starts its
two-release clock "after a crate-local test seam exists" (v0.238.0, #954) — not after Tier A
completed, which is what `compiler-architecture.md`'s own §3.4 paraphrased it as. `bynk-emit`'s
test-line density has read flat at 8.3–8.5% for the whole precisely-tracked window since. D3's first
trigger — a defect class recurring after being patched once at a different site — is recorded in the
ADR itself as already having fired once, at `maybe_async_iife`; Tier B's own T2.3
(`emitted_early_return`) then reproduced the identical shape a third time, built as a direct copy of
T2.2's `emitted_await` mechanism, after the ADR naming the pattern was already on file.

**`compiler-architecture.md` already named the entry condition and it is met.** Its §7 carried this
phase as: "Identity and totality (`ExprId`, `FileId`, `Ty::Error`, `certify`) | 3 | this track's Tier
B probe reads zero." Tier B's probe (`hoist_sinks`) reads zero.

**The completion probe already exists and is already CI-gated.** `span_keyed_maps` — whole-repo
occurrences of `HashMap<Span`, excluding `xtask`'s own probe source — was built by T0.0
(#999/#1000) and is diffed by `greenfield_status_table_is_current`. It currently reads **27**
(`design/greenfield-status.md`, confirmed by direct grep against `3691d6a3`). This phase does not need
new probe infrastructure to know when it is done.

---

## 2. What this track is not

- **Not the naive full `NodeId` retrofit the review explicitly killed.** The review's own words: "a
  large change across a 2,806-line AST [now 2,993] and three consumer crates." §3.1 (Q1) is where
  this track decides how much of the full reference shape it commits to.
- **Not a rewrite.** In-place, using the parallel-data technique `compiler-architecture.md` §4.1
  named for this exact phase: the new identity allocated alongside the old, both populated, consumers
  migrated one at a time, the old table deleted last.
- **Not phases 4–8.** Carried as forward references in §7, not refusals, per the same discipline
  `compiler-architecture.md` §7 used for this phase before this doc existed.
- **Not a language surface change.** No `.bynk` syntax or semantics moves.

---

## 3. Design questions — answered 3 August 2026, argued under a same-day settling review

> **Provenance: one of these seven changed under review.**
>
> Draft PR #1052 opened this file with all seven questions stated as open, per ADR 0167 step 2. The
> settling review that followed tested each recommendation against the rest of this document and the
> tree rather than accepting it on the strength of its own argument — the discipline
> `compiler-architecture.md`'s own settling review modelled, which overturned that track's D2.
>
> **Q1 is the one that moved, and it moved against the draft's own recommendation.** The draft argued
> for allocating `ExprId` directly, on the ground that the review's `ExprKey(Span)` scaffolding was
> "scoped for a world where phase 3 wasn't open" and that world no longer holds. The review found that
> argument proves too much: it would justify skipping *every* piece of migration scaffolding this
> corpus has ever used, including Tier B's own `Lowered` signature change and phase 0's `sources` seam,
> both of which were "small, safe, and load-bearing for what came after" rather than the end state
> itself. `ExprId`-at-parse touches the parser, every AST-node consumer across (at least) the seven
> crates §3.3 counts, and the arena/generational-index question (does a re-parse invalidate every
> `ExprId` a client holds?) that R2.4 itself does not answer. `ExprKey(Span)` touches none of that — it
> is a type-alias-shaped change over the exact ~11 signatures the review already enumerated — and it is
> independently complete: it closes the #844 defect class and the else-less-`if` collision on its own,
> whether or not `ExprId` ever lands. Shipping it first is not a detour from phase 3's invariant; it is
> the same "parallel data, single pipeline" technique §4.1 of the retired track named, applied one
> level earlier than the draft applied it.
>
> The remaining six (Q2 through Q7) were examined and stand as written, with their recommendations
> promoted to decisions.

### 3.1 Q1 — Does this track commit to the full retrofit now, or open on the review's scaffolding alone? **Settled — reversed under review.**

The review's verification pass named three cheap steps as "the migration scaffolding **if the
retrofit ever happens**": newtype the key as `ExprKey(Span)` (~11 signatures), give blocks their own
map, add a debug-only uniqueness check via `ast::expr_children`, and replace the
`_ => "unknown".to_string()` miss branch with a loud internal error. `compiler-architecture.md` §4.1
named this "T3.1 and T3.2" without detailing it, and #1046's own body says that's where this doc
starts.

But the trajectory's phase-3 invariant is the *full* reference shape — `ExprId` at parse (R2.4),
`FileId` on `Span` (R2.2), interned `Ty` with a real `Error` variant (R4.1–R4.3), `expr_ty` as a total
`IndexVec` (R2.5/R4.9), `certify` as sole constructor (R3.10). `ExprKey(Span)` alone closes none of
R2.2/R4.1–R4.3/R3.10, and only nominally addresses R2.4: it is still keyed by position, not an arena
index — R2.4's own rationale names position-as-identity as the defect class, and wrapping the
position in a newtype doesn't change what it *is*, only how it reads at call sites.

**The scaffolding was scoped for a world where phase 3 wasn't open.** That was this draft's original
argument for skipping straight to `ExprId`, and the settling review's provenance note above is where
it was tested and reversed.

**Decision: land `ExprKey(Span)` first, as its own complete slice, on its own merits — not as a detour
en route to `ExprId`.** T3.1/T3.2 (§6) ship the review's scaffolding exactly as specified: the newtype,
block-local maps, the uniqueness assertion, the loud internal error. This closes the #844 defect class
and the else-less-`if` collision immediately, and it is the "parallel data" half of the migration
technique applied to the *key type* before it is applied to the *table shape* (T3.4). True `ExprId`
allocation at parse time is **not refused** — it is renamed **T3.4** in §6 and stays gated on T3.1
having proven the per-consumer-crate migration mechanics (§3.3) across all seven readers first. If
that proof goes badly, the cost paid is one newtype's worth of scaffolding, not a parser change with
seven crates mid-migration.

### 3.2 Q2 — Sequencing among the reference's four identity/totality pieces **Settled.**

Four pieces; the reference specifies the end state, not the path through it for *this* codebase:

1. `FileId` on `Span` (R2.2)
2. `ExprId` + total `expr_ty: IndexVec` (R2.4, R2.5, R4.9)
3. `Ty` interned + `Ty::Error` (R4.1–R4.3)
4. `certify` as sole constructor of `CheckedProgram` (R3.10)

Dependency read, to test under review:

- **R4.9's totality can't be met before R4.3 exists.** An `IndexVec` has a value at every index. Today
  an unresolved expression's type is `None` — "an error was already pushed; do not pile on" — and the
  write site guards on `Some`, so `None` is never recorded at all (R4.3's own rationale). A total
  table needs somewhere honest to put "resolution failed here," which is exactly what `Ty::Error` is
  for. Build the total table before the error variant exists and the totality guarantee is fiction.
- **R4.3 (add one variant) does not require R4.1/R4.2 (full interning).** `Ty` already exists
  (`checker.rs:50`); adding `Error` to it is cheap and independently valuable — it closes the
  `None`-swallows-the-diagnostic problem on its own, ahead of and decoupled from the much larger
  `TyId`/intern-table change.
- **R2.4 (`ExprId` at parse) has no dependency on the `Ty` side** — it's a syntax-tree change, not a
  type-system one. Can run in parallel with or ahead of R4.x.
- **R2.2 (`FileId` on `Span`) is independent of both** — it's `Sources`/multi-file plumbing, not
  intra-file node identity. Lowest coupling of the four; could be its own early, small slice.
- **R3.10 (`certify`) is the capstone.** It needs a total `expr_ty` to gate on and a real `Ty::Error`
  to reject, so it lands last by construction.

**Decision.** Order confirmed, amended only to insert Q1's `ExprKey`-first step ahead of R2.4 proper:
`ExprKey(Span)` (T3.1/T3.2) → R2.2 `FileId` and R2.4 real `ExprId` in parallel (T3.5, T3.4 — neither
gates the other) → R4.3 `Ty::Error` (T3.3, unblocks R4.9) → R4.9 the total table (folded into T3.4
once `ExprId` exists) → R4.1/R4.2 interning (T3.6, its own larger slice) → R3.10 `certify` last (T3.7).

### 3.3 Q3 — What does "parallel data, single pipeline" mean across this map's actual readers? **Settled.**

`compiler-architecture.md` §4.1 named the technique for this phase but never applied it to real call
sites — not this track's business at the time it was written. Measured against `3691d6a3`,
`expr_types`/`TypedCommons` — the span-keyed channel R4.9 replaces — is read in **seven crates**, not
the three the review's retrofit-cost sentence named (that sentence counted AST-consumer crates for
the full `NodeId` question, a different count from this one): `bynk-check` (the producer — 6 files),
`bynk-emit` (7 files, the emission consumer), `bynk-ide` (4 files), `bynk-lsp` (7 files, including
`hover.rs` — the exact site review §1.3 names as observably wrong today: "hover over an else-less
`if`'s then-branch reports `()` where the branch is `Effect[()]`"), `bynk-syntax` (1 file),
`bynk-wasm` (1 file), and `bynkc`'s own test suite. Seven migration boundaries, not three — the slice
count in §6 needs re-checking against whichever number survives review.

**Decision: the map goes dual for the whole migration window** — both the old keying and the new one
populated and symmetric, deleted-last on the old side, one cutover slice per consumer crate above.
Applied first to T3.1 (both `HashMap<Span, Ty>` and `HashMap<ExprKey, Ty>` populated during the T3.1
window, `Span` form deleted once every one of the seven crates reads `ExprKey`), then again when T3.4
introduces the real `IndexVec<ExprId, TyId>` alongside whatever T3.1 left in place.

### 3.4 Q4 — Does ADR 0309's tier table need amending for this phase's gate? **Settled.**

ADR 0309 defines four tiers — Enablers / Paydown / Structural / Layering — mapping "Structural" to
Tier B and "Layering" to phase 5. Phase 3 has no named tier. It resembles Structural (crate-local
fixtures over the `sources` seam, a named regression fixture per closed defect, byte-identical
goldens) but its blast radius differs in kind, not just size: Tier B touched one crate's internals
(`bynk-emit`); this phase touches the **read** side of crates that ship developer-facing behaviour
today — `bynk-ide`/`bynk-lsp`'s hover, completion, and live diagnostics. A byte-identical-emission
gate says nothing about whether hover regresses.

**Decision: ADR 0309 is amended, not superseded.** The gate's *shape* stays Structural
(fixture-backed, byte-identical goldens, per-defect regression); the amendment adds one requirement:
each migrated consumer crate in §3.3's seven-crate list carries an LSP-surface fixture (hover,
completion, or diagnostic-shape, as applicable) in addition to any emitted-TypeScript one. Lands as
a pending ADR block riding this settling pass (§11), amending ADR 0309 the same way ADR 0309 itself
amended ADR 0059 property 1.

### 3.5 Q5 — Freeze scope **Settled.**

Tier B's freeze (`compiler-architecture.md` §3.5) was one file, `lower.rs`, for the duration of T2.1.
This phase's footprint is wider: 12 of the 27 `HashMap<Span` sites live in `bynk-check/src/checker.rs`
alone, with the rest split across `expr_types.rs`, `checker/calls.rs`, `checker/linearity.rs`
(`bynk-check`) and `validate.rs`, `project.rs`, `tests_emit.rs` (`bynk-emit`) — and `checker.rs` is
the file every later compiler stage calls through. Recommendation to test: a scoped freeze on
`checker.rs` specifically, for the duration of the R2.4/R4.9 slices (T3.1, T3.2, T3.4 in §6), on the
same narrow-scoping reasoning §3.5 used originally — touch one contended file, freeze that one file.
No freeze proposed for R2.2 (`FileId`) or R4.1–R4.3 (`Ty`), which touch less-contended surface and can
proceed unfrozen per Q2's ordering.

**Decision: confirmed as recommended.** No feature work lands in `bynk-check/src/checker.rs` while
T3.1 or T3.2 is open. Lifts automatically when T3.2 merges, the same mechanism §3.5 of the retired
track used for `lower.rs`.

### 3.6 Q6 — Sequencing relative to 1.0 and the state-migrations track **Settled.**

The retirement summary that opened this spine already states the independence claim: phase 3 "closes
into a new phase-3 track ([#1046]) rather than the state-migrations track [`compiler-architecture.md`]
§3.6 anticipated running next; the two are independent and neither blocks the other." That holds at
the file level — this phase touches the checker and the editor path, not `deploy.rs` or storage code.
What wants testing here rather than repeating is whether this phase's *size* (§3.7 below) competes for
the same reviewer bandwidth a state-migrations track would need — a scheduling question, not a
technical one.

**Decision: confirmed independent; not re-litigated further here.** If reviewer bandwidth becomes the
binding constraint later, that is a scheduling call for whoever opens the state-migrations track, not
a reason to slow this one now.

### 3.7 Q7 — Slicing granularity given the size **Settled.**

Trajectory §5 rates this phase relative size 8 against Tier B's 3 and Tier A's combined 2 — the
largest phase attempted so far, at medium confidence ("the review killed a naive retrofit; §6's
parallel-data technique is the mitigation"). §6 below decomposes a first pair (T3.1, T3.2, matching
the review's named scaffolding, gated on Q1) and names five more without detailing them. Needs
settling: whether five undetailed forward-references is the right grain, or whether some of them
(R2.2 in particular, per Q2's independence finding) should be detailed now since nothing gates them.

**Decision: keep T3.3–T3.7 as forward references for now.** §6's T3.1/T3.2 are the only slices this
settling pass details to signature level — cutting five more before T3.1 has even landed is exactly
the "an unopened phase whose slices are already written is a wish list" failure
`compiler-architecture.md` §7 named. Revisit the grain once T3.1/T3.2 ship and §3.3's seven-crate
migration mechanics are proven in practice, not just argued.

---

## 4. Posture

Extends ADR 0059 as amended by ADR 0309, the same five standing properties `compiler-architecture.md`
ran under: behaviour-preserving by default (a slice that changes observable behaviour — e.g.
`Ty::Error` starting to suppress a diagnostic class differently than today's `None` does — states so,
ships a fixture, gets a CHANGELOG entry); the acceptance gate per §3.4 above once settled; short-lived
branches, one PR per slice; no per-increment ADRs beyond what §11 front-loads; every slice citing
`Closes-Rule:`.

---

## 5. The completion criterion

Same principle as `compiler-architecture.md` §5: a slice is complete when the old path is **deleted**
and the mechanical probe reads zero — not when the new path merely exists alongside it. Here: 
`span_keyed_maps` reaches 0, and no `Option<Ty>` recovery path remains where R4.3 says `Ty::Error`
belongs. This codebase's own history — bug #844, the else-less-`if` aliasing, `is_binding_cache`
serving stale narrowings on collision (`checker.rs:1426`) — is the standing argument against leaving
`HashMap<Span, Ty>` reachable behind a feature-complete `IndexVec` "for now."

---

## 6. Slice decomposition

§3 is settled; T3.1 and T3.2 below are accepted, buildable slices, not forward references.

### Scaffolding — named by `compiler-architecture.md` §4.1 as T3.1/T3.2, detailed and settled here

| Slice | What lands | Rules | Gated on |
|---|---|---|---|
| **T3.1** | `ExprKey(Span)` newtype (Q1's decision) across the review's ~11 signatures in `bynk-check/src/checker.rs`, `expr_types.rs`, `checker/calls.rs`, `checker/linearity.rs` and `bynk-emit`'s readers; blocks split into their own map, out of the shared keyspace the `block.span != block.tail.span` guard (`checker.rs:2614`) currently patches around; dual-map migration per Q3 (both `HashMap<Span, Ty>` and `HashMap<ExprKey, Ty>` populated, old form deleted once all seven consumer crates in §3.3 have cut over) | R2.4 (partial — see §3.1) | — settled, ready to slice |
| **T3.2** | A debug-only uniqueness check at the end of `check_record`, walking the unit via `ast::expr_children` and asserting no two typed nodes share a key (the guard at `checker.rs:381-393` generalised into an assertion, per the review's exact proposal — would have caught #844 on introduction); replace the remaining `_ => "unknown".to_string()` fallback (`lower.rs:3025` — down to one live site as of `3691d6a3`, from the three-plus the review counted; sweep the sibling `.unwrap_or_else(|| "unknown".to_string())` at `lower.rs:1848` in the same pass) with a loud internal error | R2.4 | T3.1 |

### Forward-referenced, not yet detailed (§3.7/Q7's decision: kept coarse until T3.1/T3.2 prove the mechanics)

| Slice | What it names | Rules | Gated on |
|---|---|---|---|
| **T3.3** | `Ty::Error` as a real variant; `expr_types` records it instead of omitting the entry on `None` | R4.3 | T3.1 landed |
| **T3.4** | Real `ExprId` allocated at parse, replacing `ExprKey(Span)`; `expr_ty` as a total `IndexVec<ExprId, TyId>` replacing the keyed map entirely, migrated crate-by-crate per §3.3's seven-crate list | R2.4, R2.5, R4.9 | T3.1 (mechanics proven), T3.3 |
| **T3.5** | `FileId` on `Span`; `Sources` as the compiler's one view of file contents | R2.2 | independent — can run in parallel with T3.1–T3.4 |
| **T3.6** | `Ty` interned (`TyId`, `Copy`/`Hash`/`Ord`) | R4.1, R4.2 | T3.3 |
| **T3.7** | `certify` as the sole constructor of a `CheckedProgram`; the editor stops routing a non-compiling file through the batch-checking path | R3.10 | T3.4, T3.6 |

Deliberately not decomposed further, for the reason `compiler-architecture.md` §6's own forward
references gave one phase up: "an unopened phase whose slices are already written is a wish list."
T3.3 onward exist so a proposal citing one is recognised as this track's future work, not unclaimed
scope — they are not proposals themselves.

**Completion probe:** `span_keyed_maps` = 0. Already built (T0.0, #999/#1000) and CI-gated
(`greenfield_status_table_is_current`); reads **27** as of `3691d6a3`.

---

## 7. Out of scope — forward references, not refusals

Carried forward from `compiler-architecture.md` §7's rows for phases 4–8, since this track now owns
phase 3 directly rather than forward-referencing it:

| Item | Phase | Entry condition |
|---|---|---|
| The project model as a phase (`bynk-project`) | 4 | this track's probe (`span_keyed_maps`) reads zero |
| Semantics in the checker (`validate.rs` dissolves) | 5 | phase 4 complete |
| The full IR (reference Part 6) | 6 | phase 5 complete |
| The TypeScript tree and printer (reference Part 7) | 7 | phase 6 complete |
| Incrementality (query granularity, the firewall) | 8 | phases 3 and 4 complete |
| Publishing the emit ABI: the `@bynk/*` package, the three ADR 0200 layers, the build-time enumeration guard | *packaging track* | ADR 0310 merged; not gated on this track's phases |

---

## 8. Keeping the reference true

No new probe infrastructure is needed for this phase's gate — `span_keyed_maps` already exists,
already runs in CI, and already reads the number this track needs to reach zero (§6). Two additions
worth raising under review, not yet decided: a probe for `Ty::Error` adoption (today: absent, per
Appendix D of the reference) and a probe for `certify` call-site count (today: 0, matching the same
Appendix D row).

---

## 9. Risks

**The largest phase attempted so far.** Relative size 8 against Tier B's 3 (trajectory §5). §3.7 (Q7)
is where slicing granularity gets tested against that, rather than assumed.

**The first phase whose migration touches the editor's read path, not only the batch emitter.** Tier
B could regress only `bynkc`'s output; this phase can regress `bynk-lsp` hover, completion and
diagnostics live in an editor session. §3.4 (Q4)'s proposed gate amendment exists specifically for
this.

**Totality has a bootstrapping order.** §3.2 (Q2) names it directly: `IndexVec`'s totality guarantee
(R2.5) needs somewhere honest to record "this node's resolution failed," which doesn't exist until
`Ty::Error` (R4.3) lands. Building the total table first reproduces the exact "`None` swallows the
diagnostic" failure R4.3's own rationale describes, one layer further from the source.

**Seven consumer crates, not three.** §3.3 (Q3) counts `expr_types`'s actual readers against the
review's "three consumer crates" framing (which measured something else — the AST-retrofit cost, not
this map) and finds more. §6's slicing needs re-checking against whichever count survives review, the
same discipline `compiler-architecture.md` §6.0 applied to its own inherited numbers ("nine of the
fourteen slices... had already landed").

**The evidence base ages, and already has.** The `_ => "unknown".to_string()` fallback count dropped
from the review's "3155, 3189, 3245 and neighbours" to one live site (`lower.rs:3025`) by the time
this doc was measured against `3691d6a3` — paydown happening as ordinary work, the same pattern
`compiler-architecture.md` §6.0 found for its own inherited findings. Every number in this doc was
measured 3 August 2026; re-check before a slice proposal cites one.

---

## 10. What "transformational" means here

In the trajectory's own words (§3, Phase 3): "This is the phase that makes the batch path and the
editor path one program." Not an ergonomic dividend the way Tier B's four-defects-in-one-change was —
a precondition for phase 8 (incrementality needs stable interned identity and total side tables per
trajectory §4) and for phase 4's project model sitting cleanly below both check and emit.

---

## 11. ADRs

Per ADR 0167 step 2, the load-bearing, hard-to-reverse decisions land before slicing. Two do, with
this settling pass (`level: patch`, no code); their numbers are assigned at merge by the stamp
(ADR 0206), so this doc refers to them by letter until they exist — the pattern
`compiler-architecture.md` §11 used for ADRs 0309–0312.

- **ADR-A — the phase-3 migration technique: `ExprKey(Span)` scaffolding first, real `ExprId` and
  totality later, gated on the scaffolding proving out.** §3.1 (Q1). Reverses this doc's own initial
  draft recommendation under review — the reversal is the ADR's content, not an embarrassment to
  paper over.
- **ADR-B — ADR 0309's Structural tier gains an LSP-surface-fixture requirement for consumer crates
  that ship developer-facing behaviour.** §3.4 (Q4). Amends ADR 0309 the same way ADR 0309 amended
  ADR 0059 property 1.

Lands as `design/pending/identity-and-totality-settling.md`.

---

## 12. Retirement

Mirrors `compiler-architecture.md` §12: this track retires when §6's probe (`span_keyed_maps`) reads
zero and every slice named to reach it has landed — whether or not phase 4 has opened. The retirement
PR removes this doc, appends its closing summary to `../archive/retired-tracks.md`, and closes the
spine (`Closes #1046`).
