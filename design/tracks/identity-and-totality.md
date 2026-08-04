# Identity and totality — migrating to the greenfield reference (phase 3)

- **Status:** **Settled — Slicing.** §3's seven questions were argued under a same-day settling
  review; Q1 was reversed once there (toward `ExprKey(Span)` scaffolding), then **reversed again**
  while slicing T3.1 (§1.1), once implementation work found the scaffolding had already been built —
  and already rejected — by a commit that predates this track. Eight slices have shipped for real:
  T3.0 (extending an existing debug-only check to handler bodies), T3.3a (`Ty::Error` as a real,
  correctly-handled variant), T3.3b (`expr_types` made total — **R4.3 closed in full**, at two
  write choke-points rather than the ~81-function conversion originally estimated), T3.4 (real
  `ExprId` at parse, `expr_types` re-keyed by it — **R2.4 closed for expressions**; caught and fixed a
  genuine cross-file collision bug before it shipped, see §9), T3.5 (real `FileId` on `Span`,
  stamped at the lexer — **R2.2 closed**, at one choke point rather than the 160-construction-site
  conversion originally estimated; see §9), T3.6a (`Hash`/`Ord` on `Ty`/`NamedKind`/`BaseType` —
  **half of R4.2 closed**, a 3-line derive diff touching zero call sites; the other half, real `TyId`
  interning for `Copy`, is **not** a choke-point fix — split out as T3.6b, its own larger effort, see
  §9), and T3.7a/T3.7b (`certify`/`CheckedProgram`, single-file then project/batch path — **R3.10
  fully closed**; T3.7a's own belief that the project/batch path needed a separate design pass on
  emission ordering was itself re-checked while implementing T3.7b and found to be a conflation, see
  §9). `span_keyed_maps` is 3, down from 27 —
  the remainder is `Ctx::pattern_binding_types`, a deliberate, principled exclusion (§6), not residue.
  T3.6b (real `Ty` interning, confirmed genuinely large — see §9) is the only slice left unbuilt.
  Merging settled **direction**; it is not a build authorisation.
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

### 1.1 Correction found while slicing T3.1: more of §3's premise had already shipped than §3 knew

Implementation work on T3.1 surfaced a fact this doc's first two revisions missed, despite both
claiming to measure directly against the tree: **commit `43abc242`** ("Wave 8: leisure batch for the
compiler pipeline review", #960, 28 July 2026 — landed *before* `compiler-architecture.md` even
retired, let alone before this spine opened) already shipped batch **8.5** of the review's own
findings **#28/#46** — the exact finding §3.1's `ExprKey(Span)` argument was built on:

- Fixed the else-less-`if` span-aliasing bug **at its root cause** (a zero-width span for the
  synthesised else-block/`UnitLit`, `bynk-syntax/src/parser/expressions.rs:1582-1589`), not merely
  worked around it.
- Added the debug-only uniqueness check the review proposed — but scoped to `check_record`'s
  `CommonsItem::Fn` loop only (`checker.rs:317-329`, using `ast::expr_children`).
- Replaced **9 of 10** `_ => "unknown".to_string()` emitter fallbacks with a loud internal error. The
  one remaining site (`lower.rs:3025`, `join_other_elem_ts`) is a **documented, deliberate**
  soft-fallback for an already-diagnosed program, not residue — and the `.unwrap_or_else(|| "unknown"…)`
  at `lower.rs:1848` this doc's first revision flagged as "a sibling to sweep in the same pass" is
  unrelated: it defaults a missing type-argument name, not an `expr_types` miss. Both claims in this
  doc's earlier revisions were wrong; §9 records the correction as a risk realised, not silently fixed.
- **Explicitly rejected the `ExprKey(Span)` newtype**, "confirmed with the user mid-implementation":
  `expr_types` is read directly by "22 files across 6 crates," and the newtype "changes no behavior —
  it's migration scaffolding for a future `NodeId` retrofit, not a bug fix."

**This reopens and re-settles Q1 a second time**, in §3.1 below — the first settling review's decision
(land `ExprKey(Span)` as an independent slice) is itself now superseded, by [ADR
0313](../decisions/0313-phase-3-scaffolding-before-retrofit.md)'s successor ADR, on the strength of a
prior author having already reached, and acted on, the same conclusion this correction reaches.

**A real, distinct gap was found and closed in the same pass.** `43abc242`'s uniqueness check runs
only from `check_record`'s loop over top-level `CommonsItem::Fn` items. `check_handler_body` and
`check_body` — `bynk-emit`'s own entry points for service/agent handler bodies and test-case bodies,
called directly from `validate.rs`/`tests_emit.rs`, entirely outside `check_record`'s loop — carried
**no uniqueness protection at all**, for the bulk of real Bynk programs (handler bodies, not free
functions). This is shipped as this track's first landed slice — see §6, "T3.0".

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

### 3.1 Q1 — Does this track commit to the full retrofit now, or open on the review's scaffolding alone? **Settled — reversed twice.**

> **Second reversal, §1.1's correction.** Everything below this line through "Decision: land
> `ExprKey(Span)` first…" is this doc's **first** settling review, preserved rather than deleted —
> the same "keep the record of it" discipline `compiler-architecture.md` §3.6 used for its own spent
> reasoning. That decision is **overturned again**, by evidence, not by re-argument: `43abc242`
> already shipped the review's uniqueness-check and loud-error scaffolding (scoped to `check_record`
> only) and already **explicitly rejected the `ExprKey(Span)` newtype**, for the reason this doc's
> first reversal argued *against* — "changes no behavior… migration scaffolding … not a bug fix." A
> prior author reached this settling review's conclusion first, under real implementation pressure,
> and recorded why. **Final decision: no standalone `ExprKey(Span)` slice.** The one genuinely open
> gap `43abc242` left — the uniqueness check never reaching `check_handler_body`/`check_body` — is
> real, valuable, and closed directly (§6, "T3.0"), without the newtype. What remains of phase 3's
> R2.4/R2.5/R4.9 invariant needs the real thing: `ExprId` allocated at parse, done as its own slice
> when someone is prepared to take on a parser change touching all seven consumer crates at once — not
> pre-designed here, for the same "wish list" reason §3.7 (Q7) already gives.

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

**Decision.** Order confirmed as originally read, with §3.1's second reversal removing the `ExprKey`
step this decision briefly inserted ahead of it (no code shipped under it, so nothing to unwind): R2.2
`FileId` and R2.4 real `ExprId` in parallel (T3.5, T3.4 — neither gates the other) → R4.3 `Ty::Error`
(T3.3, unblocks R4.9) → R4.9 the total table (built into T3.4 directly, since there is no `ExprKey`
predecessor to fold it into) → R4.1/R4.2 interning (T3.6, its own larger slice) → R3.10 `certify` last
(T3.7).

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
Applies once, when T3.4 introduces the real `IndexVec<ExprId, TyId>` — §3.1's second reversal removed
the `ExprKey(Span)` predecessor this decision originally described as the first of two dual-map
passes, so there is now only the one.

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
`checker.rs` specifically, for the duration of the R2.4/R4.9 slice (T3.4 in §6), on the
same narrow-scoping reasoning §3.5 used originally — touch one contended file, freeze that one file.
No freeze proposed for R2.2 (`FileId`) or R4.1–R4.3 (`Ty`), which touch less-contended surface and can
proceed unfrozen per Q2's ordering.

**Decision: confirmed as recommended, not yet in effect.** No feature work should land in
`bynk-check/src/checker.rs` for the duration of T3.4, whenever it opens — the same mechanism §3.5 of
the retired track used for `lower.rs` during T2.1. §3.1's second reversal means nothing currently open
needs this freeze yet: T3.0 (§6) was small enough not to warrant one, and T3.4 hasn't been sliced.

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
parallel-data technique is the mitigation"). §6 names five forward references (T3.3–T3.7) without
detailing them to signature level. Needs settling: whether that is the right grain, or whether some of
them (R2.2 in particular, per Q2's independence finding) should be detailed now since nothing gates
them.

**Decision: keep T3.3–T3.7 as forward references for now.** §1.1's correction is itself evidence for
this, not just a reason stated in the abstract: this settling pass detailed a scaffolding pair
(`ExprKey(Span)`) to signature level before checking whether it was still needed, and it was not.
Detailing T3.3–T3.7 now, before any of them has a slice open, risks the same mistake at larger scale.
Revisit the grain once one of T3.3–T3.7 actually opens and the seven-crate migration mechanics (§3.3)
are proven in practice against *that* slice, not argued in advance of it.

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

§3 is settled. §1.1's correction retires the `ExprKey(Span)` slice pair before it was ever built and
replaces it with the one real gap found while preparing it.

### Landed

| Slice | What shipped | Rules | Status |
|---|---|---|---|
| **T3.0** | Extended `43abc242`'s debug-only uniqueness check (finding #28) to `check_handler_body` and `check_body` — previously covered only `check_record`'s top-level-`fn` loop, leaving service/agent handler bodies and test-case bodies with no collision protection. Same per-call `seen` set granularity `43abc242` chose, for the same reason (a multi-file commons legitimately re-checks a handler more than once). `bynk-check/src/checker.rs` | R2.4 (verification) | **Shipped** — `cargo test --workspace` green, no existing fixture triggers the extended assertion |
| **T3.3a** | `Ty::Error` added as a real variant on `enum Ty`, and given deliberate (not default/panicking-by-omission) semantics at every *exhaustive* match over `Ty` the compiler's own exhaustiveness check found: `compatible`/`structurally_compatible_inner` treat it as compatible with anything in both positions (R4.3's "assignable to and from everything"); `unify` binds it trivially; `substitute`/`contains_var`/`contains_flexible_var`/`rebrand_return_type` treat it as the leaf it is; `display` renders `<type error>`; `json_codable` treats it as codable (no cascading `json_uncodable`, matching "suppresses all downstream diagnostics that mention it"); `ty_to_type_ref` returns `None` (no codec); `ts_ty` — reached only if a checked, emission-bound program somehow contains one, which R4.3 says should never happen — raises a loud internal error (finding #28's own convention) rather than silently emitting a type for it. **Measured, not assumed:** the compiler's own exhaustiveness check found exactly 11 sites needing an arm (9 in `bynk-check`, 2 in `bynk-emit`) out of 900+ raw `Ty::` mentions across the two crates — most of those are constructors and non-exhaustive matches, not sites this change touches. `bynk-ide`/`bynk-lsp`/`bynk-wasm` needed no changes at all. | R4.3 (scaffolding) | **Shipped** — `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`, `cargo test --workspace` all clean (one pre-existing, environment-only failure, unrelated) |
| **T3.3b** | `expr_types` made total for real, at both of its actual write choke points — `type_of`'s own `ctx.expr_types.insert(expr.span, …)` (`checker.rs`) and `type_of_block`'s equivalent for a block's own span — changed to record `Ty::Error` rather than skip the entry when the computed type is `None`. **This closes R4.3's externally-observable content in full**, not partially: every expression `type_of` is called on (directly or through its ~90 recursive/nested self-calls) now gets an `expr_types` entry, whichever of the checker's internal paths produced `None` and for whatever reason. Verified with a new regression test (`a_diagnosed_resolution_failure_records_ty_error_instead_of_nothing`, `bynk-emit/src/project.rs`) asserting the actual behaviour, not just absence of failure: a diagnosed `type_of` failure (`bynk.types.uninferable_element_type` on an empty `[]` with no inferable element type) now records `<type error>` at its span instead of nothing. | R4.3 | **Shipped** — full workspace build/clippy/fmt/test clean, including the `bynkc` byte-identical golden fixture corpus (unchanged output — this is purely additive on the *recording* side) |
| **T3.4** | Real `ExprId(u32)` allocated at parse — `id: ExprId` added to `Expr`, populated by a monotonic counter on `Parser<'a>` (`Parser::alloc_expr_id`, mirroring its existing `brace_depth` field), one allocation point so no two parser-produced nodes ever share an id. `expr_types`/`partial_expr_types` re-keyed from `HashMap<Span, Ty>` to `HashMap<ExprId, TypedExpr>` (`TypedExpr { span, ty }` — the value carries its own span so LSP consumers keep a position-shaped answer without a second map). `is_binding_cache` (genuinely `Expr`-keyed — condition-sub-expression memoisation, not pattern-related despite living beside `Ctx`'s pattern-binding field) converted the same way. Pattern-bound identifier types (`Pattern::Binding`, three sites) **deliberately kept `Span`-keyed** in a new, narrowly-scoped `Ctx::pattern_binding_types` field — `Ident` has no `ExprId` and giving it one would touch every identifier construction site in the workspace, not just the handful that bind; this is exactly the `PatId`/`ExprId` split the reference draws (Part 2), out of scope on purpose. **Measured, not assumed:** the compiler's own missing-field errors enumerated 46 real parser construction sites (of ~59 raw `Expr {` matches — the rest were struct *definitions* and function signatures, not constructions) plus 3 post-parse synthetic sites (`ExprId::SYNTHETIC`); the `expr_types` re-keying touched 61 read/write call sites, 49 of them the single uniform shape `expr_types.get(&X.span)` → `.get(&X.id).map(\|te\| &te.ty)`. | R2.4, R2.5, R4.9 | **Shipped** — see §9 for a real bug this slice found and fixed before it shipped, not after |
| **T3.5** | Real `FileId(u32)` on `Span` (`pub file: FileId`), stamped at the one place a `Span` is ever first minted from real source bytes — the lexer. `Span::new(start, end)` kept its existing two-argument signature, now defaulting `file: FileId::UNKNOWN`; a new `Span::new_in(file, start, end)` is the real-identity constructor. `lexer::tokenize`/`tokenize_expanding_holes` became thin wrappers around new `tokenize_in`/`tokenize_expanding_holes_in` entry points that thread `file` through every one of the lexer's internal span constructions (13 sites in the main scan loop, plus the `scan_str`/`scan_hole`/`split_interp` string-and-interpolation helpers it calls). `bynk-emit`'s `phase_parse`/`parse_sources` gained a `next_file_id: u32` counter threaded exactly like T3.4's `next_expr_id` — one `FileId` allocated per source file, at the same choke point. The interpolation-hole re-lexer (`Parser::parse_hole_expr`) needed no new field: it already receives the hole's own `Span`, which now carries the right `file` once the lexer is fixed, so its `tokenize(src)` call became `tokenize_in(src, hole.file)`. **Every other `Span` construction site in the workspace — the parser, the checker, the IDE/LSP single-document call sites, all tests — needed no change at all**, because they safely default to `FileId::UNKNOWN` exactly as T3.4's non-origin `Expr` construction sites defaulted to zero threading. | R2.2 | **Shipped** — see §9 for a real bug this slice's own safety-net effect (not finding #28 this time, but the same "a total field exposes a latent assumption" pattern) caught in the LSP rename validator before it shipped |
| **T3.6a** | `Hash, PartialOrd, Ord` added to `Ty`, `NamedKind`, and `BaseType`'s derive lists. **Measured, not assumed** (see §9): unlike every prior slice, T3.6 as a whole (real `TyId` interning, R4.1's `Copy` requirement) genuinely has no choke point — `Ty` is minted at ~200+ scattered sites and recursively pattern-matched in ~25-30 functions across four crates, so the naive size estimate was, unusually, roughly *right* for that half. But R4.2's `Hash`/`Ord` half is unrelated to interning: `Ty`'s recursive fields (`Box<Ty>`, `Vec<Ty>`, `String`, tuples) already support derived `Hash`/`Ord` with zero manual trait impls to conflict with, so adding the derives is a 3-line diff touching zero of the workspace's 958 `Ty::` call sites. Exposed one unrelated, pre-existing `clippy::nonminimal_bool` warning in `bynk-emit/src/project/validate.rs` (`!opt.is_some_and(f)` → `opt.is_none_or(!f)`) that a full downstream rebuild surfaced for the first time; fixed alongside since it now blocks `-D warnings`. | R4.2 (half) | **Shipped** |
| **T3.7a** | `CheckedProgram`/`certify` (`bynk-check/src/checker.rs`), scoped to the single-file compile path. `certify(program: TypedCommons, diagnostics: Vec<CompileError>) -> Result<CheckedProgram, Vec<CompileError>>` rejects on any error-severity diagnostic (reusing `bynk_syntax::partition_by_severity`, the same split `check_record` already applies) and otherwise wraps `program`; `CheckedProgram` has no public constructor and no way back to a bare `TypedCommons`, so the only way to obtain one is `certify`. `emitter::emit` (`bynk-emit/src/emitter.rs`) — confirmed, not assumed, to have exactly one production caller (`bynk-emit/src/lib.rs`'s `compile_with_warnings`) — now takes `&CheckedProgram` instead of `&TypedCommons`; `compile_with_warnings` calls `certify` right after `checker::check(...)?` and unwraps with a loud internal-error panic on the (structurally unreachable, since `check` already gated on hard errors) `Err` path, rather than silently trusting the caller not to skip the check. Originally scoped to exclude the project/batch path on the belief that its per-unit emission was too "speculative" to gate the same way — **that belief was itself re-checked and found wrong, see T3.7b below and §9.** | R3.10 (single-file half) | **Shipped** |
| **T3.7b** | `CheckedProgram`/`certify` for the project/batch path. `bynk-emit/src/project.rs`'s `check_unit_files` loop already runs three per-unit gates before reaching emission (`check_record`'s `Ok`, `check_context_constraints`, `check_context_declarations`'s `blocks_emission`) — `certify` is called right after the last of them, wrapping the unit's `typed` before it's handed to `emit_unit`, whose `typed: &TypedCommons` parameter became `program: &CheckedProgram` (one `let typed = program.program();` line covers its two internal uses). **T3.7a's own stated reason for excluding this path turned out to be a conflation, caught by reading `check_unit_files`'s actual control flow rather than trusting the earlier summary** — see §9. `emit_unit` and `emitter::emit_project` each confirmed (by grep, not assumed) to have exactly one caller, so this is the same single-choke-point shape as T3.7a, not the multi-site sweep the original framing implied. Verified against the full multi-file/project test corpus, not just unit tests: `e2e.rs`'s `bless_positive_fixtures`/`positive_fixtures`/`negative_fixtures`, `multi_file_commons_barrel.rs`, `events_workers_wiring.rs`, and every other project-path integration test passed with zero hits on the internal-error panic guarding `certify`'s (structurally unreachable) `Err` arm. | R3.10 (project/batch half — **R3.10 fully closed**) | **Shipped** |

**Why this closed in two edits, not ~81 functions and ~193 call sites — the estimate directly above
this table (preserved, not deleted, as an example of the same "measure before trusting an estimate"
discipline §9 keeps naming) was based on a conflation.** Converting the checker's *internal* recovery
convention — every one of the ~81 `Option<Ty>`-returning functions changing signature to `Ty`, every
`?`/`.or(...)` call site updated to match — genuinely is that large, and is **not** what was done here
and **not** required by R4.3's own wording. R4.3 says `expr_types` must be total; it does not say the
checker's internal control-flow idiom must change. `type_of` and `type_of_block` each write into
`expr_types` at exactly one place, after all of their internal `Option`-returning logic has already
run to completion — so making *that* write total closes the gap for every internal `None`-producing
path at once, with zero changes to any of the ~193 call sites, because none of them read `expr_types`
to get their value — they read `type_of`'s **return value**, which this change does not touch. Verified
directly, not assumed: `check_field_access`'s `let recv_ty = type_of(receiver, None, ctx)?;` still
short-circuits on `None` exactly as before, because `type_of`'s return value is unchanged — only what
gets *recorded* changed. This is why the whole fix is two match-arm-shaped edits and a total workspace
test run, not a multi-day internal rewrite.

### Not built: the scaffolding pair this doc's first revision specified

`ExprKey(Span)` and its uniqueness-check/loud-error companions are **not** being built as a separate
slice — §3.1's second reversal explains why: `43abc242` already shipped the uniqueness check and 9/10
loud-error replacements, and already rejected the newtype on the same grounds this doc's first
reversal argued against. **`T3.1` and `T3.2` are retired, unused, not repurposed** — skipped outright
rather than renumbered, so every cross-reference to `T3.3`–`T3.7` elsewhere in this doc (§3.2, §3.5,
§9) keeps meaning exactly what it already said.

### Forward-referenced, not yet detailed — the actual remaining reference-shape work

| Slice | What it names | Rules | Gated on |
|---|---|---|---|
| **T3.6b** | Real `Ty` interning: `TyId` as the only currency above the intern table, `Ty` values constructed only by the interner, `Copy`-cheap. A genuinely larger, multi-session effort — see §9's T3.6a/T3.6b split for why this one, unlike every other slice in this track, does not have a hidden choke point that shrinks it, and §9's settling-review addenda for the real decomposition (`bynk-check` first as one atomic PR — `Ty`'s field definition can't change gradually — then `bynk-emit`, then `bynk-ide`/`bynk-lsp`; `compatible`/`unify`/`substitute` alone are 149 call sites, confirmed contained to `bynk-check`), the resolved `Types`-table-ownership question (owned per `check_record` invocation, carried on `TypedCommons`/`CheckedProgram` — confirmed safe by checking how `compose_unit_symbols` actually merges cross-unit declarations, not assumed), and the one small, real prerequisite (`Ty`'s `Hash`/`Eq`/`Ord` test coverage) already closed alongside this review, not left as findings with no code to show for them | R4.1, R4.2 (`Copy` half) | independent — no scaffolding-first increment exists and the ownership design is now settled (investigated and resolved across four passes, see §9); ready to execute as the large, multi-crate rewrite tiers 1–3 describe |

T3.6b above is not built (T3.0, T3.3a, T3.3b, T3.4, T3.5, T3.6a, T3.7a, T3.7b, above, are). Deliberately not decomposed to
signature level, for the reason `compiler-architecture.md` §6's own forward references gave one phase
up: "an unopened phase whose slices are already written is a wish list." Each exists so a proposal
citing it is recognised as this track's future work, not unclaimed scope — none is a proposal itself,
and (per §1.1's correction) none should be assumed still-accurate without re-measuring against the
tree first — the same discipline that caught the `ExprKey(Span)` premise being stale, found T3.3a's
real 11-site scope instead of trusting a 900+-mention grep count, found T3.3b closable in two edits
instead of the ~81-function estimate, found and fixed a real cross-file collision bug in T3.4
before it ever shipped, found T3.5 closable at one choke point (the lexer) instead of the
160-construction-site conversion its own settling estimate called for, found — the discipline's first
negative result — that T3.6's `Copy`-via-interning half genuinely has *no* choke point and is
roughly as large as its own settling label already said, while still finding a real, small,
independently-shippable slice (T3.6a) inside it that a same-sized-or-nothing framing would have
missed, found T3.7's "gated on T3.6" itself was stale — a list-position artifact, not a
re-derived dependency, once R3.10's actual rationale was read directly, and then — a second-order
instance of the same discipline, this time applied to a conclusion this track had itself just
written down — found that T3.7a's own reason for excluding the project/batch path (§9) was a
conflation between per-unit certification and whole-build atomicity, letting T3.7b ship in the same
session rather than waiting on a "design pass" that turned out not to be needed. See §9 for all of
the above.

**Completion probe:** `span_keyed_maps` = 3 (down from 27). `HashMap<Span` now appears only in
`Ctx::pattern_binding_types`'s own type signature (3 sites, deliberately, per T3.4's row above) — not
0, but the remaining 3 are a stated, principled exclusion, not residue. `IndexVec<ExprId, TyId>`
(R2.5/R4.9's literal "IndexVec, i.e. total" wording) is not yet built — `expr_types` is
`HashMap<ExprId, TypedExpr>` today, which T3.3b already made *functionally* total (every id anyone
would query has an entry) without being an `IndexVec` structurally. Whether that gap is worth closing
on its own is an open question for whoever picks this phase back up, not decided here.

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

**T3.4 found a real, silently-miscompiling bug in itself, before it shipped — recorded here in full
because catching it is the whole point of this phase, not a footnote to it.** Making `expr_types`
`ExprId`-keyed exposed that `bynk-emit`'s `collect_unit_methods` merges a type's methods from sibling
files into the file that declares the type (so a type's methods surface together regardless of which
file wrote them), *before* a single `check_record` call — and each file was parsed by its own,
independently-zero-based `Parser`. Two files of similar size are *likely*, not just theoretically able,
to produce colliding `ExprId`s once merged into one `expr_types` map. Finding #28's debug-only
uniqueness assertion (extended to handler bodies in T3.0) caught it immediately on real fixtures
(`bynkc/tests/fixtures/positive/64_full_time_commons`, `65_money_uses_time`) the first time the full
test suite ran against the re-keyed map — a silent type-coercion drop (`self.raw` losing its
`(self as number)` cast) that byte-identical golden tests alone would eventually have caught (the
fixture's expected output would differ) but with no indication of *why*, and only for the specific
files that happened to collide. The fix: `ExprId` allocation is now a single counter threaded across
every file one project parse touches (`bynk-emit`'s `phase_parse`/`parse_sources`, via new
`parse_*_from` entry points in `bynk-syntax` that seed and hand back a running counter — the existing,
zero-based entry points are unchanged thin wrappers, so none of the ~20 single-file callers this doc's
T3.4 row measured needed touching), not reset per file. The cached first-party synthetic units
(`bynk.bynk`, `bynk/map.bynk`, five more — parsed once per *process* behind a `OnceLock`, reused across
every later compile regardless of that compile's own file count) get their own fixed, disjoint,
1M-wide reserved id ranges instead, since a `OnceLock`-cached parse can't participate in a live
per-compile counter. Full workspace test suite (including the two fixtures above) verified green
after the fix, on the same run that would have caught a wrong fix immediately. This is not a caveat on
T3.4's totality claim — it's evidence the uniqueness assertion this track has been building on since
T3.0 does exactly what it says on the tin, on a real defect, before a user ever saw it.

**The largest phase attempted so far.** Relative size 8 against Tier B's 3 (trajectory §5). §3.7 (Q7)
is where slicing granularity gets tested against that, rather than assumed.

**The first phase whose migration touches the editor's read path, not only the batch emitter.** Tier
B could regress only `bynkc`'s output; this phase can regress `bynk-lsp` hover, completion and
diagnostics live in an editor session. §3.4 (Q4)'s proposed gate amendment exists specifically for
this.

**Totality had a bootstrapping order — resolved, not just avoided.** §3.2 (Q2) named it: `IndexVec`'s
totality guarantee (R2.5) needs somewhere honest to record "this node's resolution failed," which
didn't exist until `Ty::Error` (R4.3) landed. T3.3a then T3.3b landed in that order for exactly this
reason, and a future literal `IndexVec<ExprId, TyId>` (should one still be built — see T3.4's
completion-probe note in §6) already has `Ty::Error` to put at a missing index instead of needing to
invent the same answer under different, more gated conditions later.

**Seven consumer crates, not three.** §3.3 (Q3) counts `expr_types`'s actual readers against the
review's "three consumer crates" framing (which measured something else — the AST-retrofit cost, not
this map) and finds more. §6's slicing needs re-checking against whichever count survives review, the
same discipline `compiler-architecture.md` §6.0 applied to its own inherited numbers ("nine of the
fourteen slices... had already landed").

**The evidence base ages, and already has — and this doc's own first two revisions are the proof, not
just the warning.** §1.1 records what happened: both revisions correctly noted the
`_ => "unknown".to_string()` count had dropped to one live site, but **neither asked why** before
building a slice plan on top of the observation. The why — `43abc242` had already shipped the
review's own uniqueness-check-and-loud-error proposal, and had already rejected the newtype this
doc spent two settling passes deciding to build — was one `git log -S` away. The remaining site was
also mischaracterised as residue to sweep ("the sibling `.unwrap_or_else`... in the same pass") when
it is a different, unrelated fallback. `compiler-architecture.md` §6.0 found its inherited numbers
stale; this doc found its *own freshly-measured* numbers stale, one commit deep, because a number
that moved was read as "paydown already happened here too" without checking what actually moved it.
The corrective discipline this risk names for future slices: when a number has already moved between
the review and today, find the commit, not just the new value — the commit tells you what's still
open, the value alone doesn't.

**T3.3 ("small, independent") was bigger than its one-line description suggested, and the *shape* of
the risk was measured wrong on the first pass.** `Ty::` appears **702 times across 9 files in
`bynk-check`** and **202 times across 6 files in `bynk-emit`** — 900+ raw mentions, which is what this
doc's previous revision read as the danger: wildcard match arms silently mishandling a new variant.
**That fear was addressed directly rather than argued around**: T3.3a (above) added the variant and
let the compiler's own exhaustiveness check enumerate every *exhaustive* match that needed an arm —
**11 sites**, not 900+, because the overwhelming majority of those mentions are constructors
(`Ty::Base(x)`, `Ty::List(...)`) or matches that already carry a wildcard and were checked by hand
(none found mishandling `Error` in a way that would matter, since nothing constructs it yet). The
wildcard-audit risk this section originally raised was real in principle but not in this codebase's
actual shape — the type system did the enumeration work a manual audit would have done, faster and
completely.

**T3.3b then repeated the same lesson at a different layer, and this time the estimate directly above
was wrong for a structural reason, not a measurement one.** The ~81-function, ~193-call-site figure
conflated two different problems: making `expr_types` *total* (what R4.3 actually requires) with
converting the checker's *internal* `Option<Ty>` recovery convention throughout (a much larger,
separate thing R4.3 does not require). A classification pass over ~350 internal `None`-sites — tried,
and documented as unreliable even after real spot-checking found a missed syntax pattern and a
confirmed off-by-one-window false negative — was solving the wrong-shaped problem. The actual fix was
two write-site edits (`type_of`, `type_of_block`), because both funnel every internal `None`, from
whichever of those ~350 sites produced it, through one place before anything downstream ever sees it.
Verified, not assumed: `check_field_access`'s `type_of(receiver, …)?` still sees the exact same
`Option<Ty>` return value as before the change — only what gets *written* to `expr_types` changed, so
none of the ~193 call sites needed touching. The general form of the mistake, now seen twice in this
track (T3.3a's wildcard-audit fear, T3.3b's call-site-conversion fear): estimating a fix's size from
the shape of the *problem statement* rather than the shape of the *codebase's actual choke points*.

**T3.5 ("lowest coupling... could be its own early, small slice", Q2 in §3.2) was re-measured at
implementation time and found smaller than the settling-review estimate directly above, for the same
structural reason T3.3b was: the estimate counted every `Span` *construction* site (160 of them — 50
struct literals, 110 `Span::new(...)` calls) as needing to change, but R2.2 only requires that a `Span`
carry its *real* file identity when one exists — not that every construction site supply one. Almost
none of those 160 sites mint a `Span` from raw source bytes; they copy, offset, or merge a `Span` that
some earlier `Span` already carried, or (in the checker, the IDE, and every test) build a synthetic
span with no real file to attach in the first place. The one place a `Span` is *first* minted from
real bytes is the lexer — 13 sites in its main scan loop plus the `scan_str`/`scan_hole`/`split_interp`
string-and-interpolation helpers it calls, all reached from exactly one production entry point
(`bynk-emit`'s `parse_sources`, per file). Threading a `next_file_id: u32` counter through
`phase_parse`/`parse_sources` there — the same shape T3.4's `next_expr_id` already used — and giving
the lexer a `tokenize_in(source, file)` entry point (with `tokenize(source)` becoming a thin wrapper
defaulting to `FileId::UNKNOWN`) closes R2.2 for every span that matters: `Span::offset`/`Span::merge`
already propagate whatever `file` their input carried, so a span derived from a lexer-stamped span
inherits the real id for free, exactly as T3.3b's derived reads inherited totality for free. The one
non-lexer site needing deliberate attention was the interpolation-hole re-lexer
(`Parser::parse_hole_expr`), and it needed no new field — it already receives the hole's own `Span`,
so it just reads `hole.file` instead of restarting at `FileId::UNKNOWN`. The other ~150 sites needed
zero changes, verified by a full build, not assumed.

**This slice's own totality caught a second real bug, of a different shape than finding #28's T3.4
collision.** Two LSP rename tests (`a_rename_in_one_project_ignores_a_dirty_buffer_in_another`,
`a_multi_file_rename_stamps_a_dirty_non_cursor_file_at_its_current_version`) started failing the
instant `Span` gained a real `file` field — not because the rename logic was wrong, but because its
validator (`index_queries::remap_site`, `bynk-lsp`) rebuilt a shifted post-edit `Span` with
`Span::new(start, end)`, which now defaults to `FileId::UNKNOWN` where the index it was being compared
against carried the file's real, freshly-lexed id. The rename validator's core check
(`ProjectIndex::equals_modulo_rename`) compares `SiteRef`s — `(path, span)` pairs — by derived
equality, and `Span`'s equality now includes `file`; a remapped site that silently downgraded to
`UNKNOWN` no longer matched. This was latent the moment `file` was added to `Span`'s definition, not
introduced by the lexer threading — `remap_site` is a T3.5-adjacent site nobody had reason to touch
until totality made the mismatch observable. Fixed by preserving `site.span.file` through the remap
(`Span::new_in(site.span.file, start, end)`) instead of dropping it. Confirms the same lesson finding
#28 established for `ExprId`: giving an identity field real, non-default values across a slice is
itself a verification pass over every consumer that compares by it — this is why T3.3a, T3.3b, T3.4,
and now T3.5 have each shipped a full `cargo test --workspace` as part of "done," not as a formality.

**T3.6 is where this discipline's honesty gets tested the other direction: not every slice shrinks
under measurement, and pretending otherwise would be the same mistake in reverse.** `Ty`
(`bynk-check/src/checker.rs`) derives `Debug, Clone, PartialEq, Eq` today, with `Box<Ty>`/`Vec<Ty>`
recursion exactly as the reference doc's rationale describes — every `Ty` is a fully-owned deep tree,
deep-cloned on every `.clone()`, never a map key (a workspace-wide check found zero existing
`HashMap<Ty, _>`/`HashSet<Ty>` — every `Ty`-adjacent map today is keyed on something else instead,
e.g. `Span` or `String`). Direct measurement, mirroring T3.3a/T3.3b/T3.5's method: 958 raw `Ty::`
mentions across 30 files in 4 crates (`bynk-check`, `bynk-emit`, `bynk-ide`, `bynk-lsp`). Unlike those
three slices, this time the raw count was not an overestimate of the real scope, because `Ty` is
structurally unlike an `ExprId`/`FileId`-style opaque leaf identity in two ways at once: it is
**minted at ~200+ scattered ad hoc sites** (only `Ty::Named` has any central builder — `List`,
`Option`, `Result`, `Map`, `Effect`, `Fn`, and every base type are written as inline variant literals
wherever a kernel or operator needs one, e.g. 173 `Ty::Base(...)` literals alone across 14 files), and
it is **recursively pattern-matched** in ~25-30 functions across all four crates that destructure it to
decide real compiler behaviour — `compatible`, `unify`, `substitute`, `display`, `json_codable`,
`ty_to_type_ref`, hover/completion rendering, and more. Interning doesn't relabel these call sites the
way re-keying `expr_types` by `ExprId` did; `match ty { Ty::Option(inner) => … }` becomes `match
types.get(id) { Ty::Option(inner_id) => …types.get(*inner_id)… }` at every recursive step of every one
of those ~25-30 functions — an added interner-lookup indirection in the function *body*, not a
mechanical signature edit. There is no single write choke-point standing in for all of them, because
there is no single place "a type gets read" the way there was a single place "an expression's type
gets recorded" (T3.3b) or "a token gets minted" (T3.5).

**But R4.2's literal wording — "`Ty` and `TyId` are `Copy`-cheap, `Hash` and `Ord`" — bundles two
independent asks, and only one of them shares `Ty`'s hard shape.** `Hash`/`Ord` need nothing from
interning: `Ty`'s recursive fields (`Box<Ty>`, `Vec<Ty>`, `String`, tuples) already support derived
`Hash`/`Ord` the moment their element types do, with no manual trait impl anywhere to conflict —
confirmed by shipping it: adding `Hash, PartialOrd, Ord` to `Ty`, `NamedKind`, and (already-`Copy`)
`BaseType`'s derive lists was a 3-line diff, and `cargo build --workspace` came back clean with zero
of the 958 call sites touched (**T3.6a**). `Copy` is the one demand that cannot be satisfied this way
— `Box`, `Vec`, and `String` are non-`Copy` by construction, so satisfying it is not a shortcut away
from full interning, it *is* full interning. That half — real `TyId`, the intern table, `Copy` — is
split out as **T3.6b**, scoped honestly as its own larger, multi-session effort (an estimated
150-300+ genuine touch sites, a much higher true-positive fraction of the raw count than any prior
slice saw, and a real share of them needing interner-lookup logic changes, not mechanical rekeying),
not forced into this track's usual "measure it down to a choke point" shape just because every
previous slice fit that shape.

**Shipping T3.6a also surfaced a real but unrelated latent issue**, the same way T3.5 exposed
`remap_site`: adding the derives forced a full downstream rebuild of `bynk-emit`, and `clippy`'s fresh
lint pass over `bynk-emit/src/project/validate.rs` (unchanged by this slice) reported a genuine
`nonminimal_bool` finding — `!opt.get(i).is_some_and(f)` is more directly written
`opt.get(i).is_none_or(!f)` — that a stale incremental cache had been masking. Confirmed by a clean
rebuild of `bynk-emit` on unmodified `main`: zero warnings before, the same warning every time after,
with or without a `touch`. Fixed alongside, since it now blocks `-D warnings` in CI regardless of
whether this slice caused it.

**T3.7's "Gated on: T3.6" turned out to be a stale artifact of list position, not a re-derived
dependency — caught the same way §1.1 caught the `ExprKey(Span)` premise going stale.** §3.2's own
"Decision" paragraph gives the actual technical reasoning for R3.10 landing last: "It needs a total
`expr_ty` to gate on and a real `Ty::Error` to reject" — both shipped, in T3.3b and T3.3a respectively.
R3.10's full rationale (`bynk-greenfield-compiler.md` §3, R3.10) names a `TypedProgram`, a `Diagnostic`
list, and a `CheckedProgram` constructible only by `certify` — nothing about `Copy`, `Hash`, `Ord`, or
interning. The "→ R4.1/R4.2 interning (T3.6…) → R3.10 certify last (T3.7)" ordering in that same
paragraph reused a four-item enumeration's numeric order, and nobody re-checked it once T3.6 split into
T3.6a/T3.6b. A direct search turns up no `Ty`-keyed hash or ordered collection anywhere in the checker
today — T3.6a's `Hash`/`Ord` derive isn't even exercised by anything yet, R3.10 included.

**What R3.10 actually needs, measured against the tree as it stands:** two real "must gate, then emit"
choke points, not a scan across the workspace — `bynk-emit/src/lib.rs`'s single-file
`checker::check(resolved)?` and `bynk-emit/src/project.rs`'s `finish_build` (the project/batch path,
behind a `RunChecks::Bailed`/`Checked` match and a `mode == Mode::Build` gate). Every "must degrade
gracefully" editor caller (`bynk-ide`'s `analyse_project_with`/`analyse_in_memory*`, already
`Mode::Analyse`) stays on the total `RecordCheck`/`TypedCommons` output directly, per R3.10's own "the
editor never calls `certify`" rule — no change needed there. `TypedCommons` (`bynk-check/src/checker.rs`)
is already, in spirit, the single-file `TypedProgram`: `checker::check` already returns
`Result<TypedCommons, Vec<CompileError>>`, gated on error-freedom. The literal gap is narrower than
"introduce a typed program" — it's that `TypedCommons`'s fields are `pub` and nothing stops a second,
non-`check`-derived `TypedCommons` from reaching the emitter, so the invariant holds by convention
today, not by construction. Two genuine hand-built `TypedCommons` literals already exist outside the
checker, both test-only (`bynk-emit/src/emitter/lower.rs`'s `empty_commons()`, `bynk-emit/src/project/
tests_emit.rs`'s two synthetic builders) — a real design question for whoever slices this next: does
`certify` wrap `TypedCommons` in a new, privately-constructed `CheckedProgram` (leaving `TypedCommons`
itself exactly as constructible as today, so those test fixtures need no change), or do
`TypedCommons`'s fields go private (which would break all three and require them a checker-side
test-only bypass)? That boundary — not `Ty` interning — is the one open question worth resolving before
T3.7 is sliced for real; not decided here, per this track's own discipline of not writing slices further
than the tree currently supports measuring.

**T3.7a answered that open question the same way T3.4/T3.5 resolved theirs — by attempting the
smaller half and finding it genuinely small, then discovering the other half's real shape only once
code was in hand.** Chose the wrapper: `pub struct CheckedProgram(TypedCommons)` (private field, no
`Deref`, one `.program()` accessor, no way back to a bare `TypedCommons`), constructed only by
`certify(program: TypedCommons, diagnostics: Vec<CompileError>) -> Result<CheckedProgram,
Vec<CompileError>>` — reusing `bynk_syntax::partition_by_severity`, the exact split `check_record`
already applies, rather than inventing a second error-severity test. This leaves the three existing
test-only hand-built `TypedCommons` literals (`emitter/lower.rs`'s `empty_commons()`,
`project/tests_emit.rs`'s two synthetic builders) untouched — none of them call `emitter::emit`
directly (confirmed by grep, not assumed), so none needed to route through `certify`.
`emitter::emit`'s signature changing from `&TypedCommons` to `&CheckedProgram` touched exactly one
production call site (`bynk-emit/src/lib.rs`'s `compile_with_warnings`) — confirmed by grep before
changing it, not assumed from the doc comment's claim that `emit` is single-file-only — with the same
zero-ripple shape as T3.4/T3.5's other choke-point fixes: `cargo build --workspace` and `cargo test
--workspace --no-run` both came back clean on the first try, meaning no other call site anywhere in
the workspace (including every test crate) constructs a `TypedCommons` and feeds it to `emit` outside
that one path.

**T3.7a's own reason for excluding the project/batch path was itself wrong, and re-checking it — not
just accepting it — is what let T3.7b ship in the same session.** T3.7a's PR reasoned that
`emit_project`'s per-unit call happens "before that unit's build-wide gate is finally decided," so
wrapping it in `CheckedProgram` would misrepresent an unfinished decision as certified. Rereading
`bynk-emit/src/project.rs`'s actual control flow (not the earlier summary of it) found this conflated
two different properties. `check_unit_files`'s per-unit loop *does* fully decide "is this unit's own
typed data free of any known error" before reaching emission — three real gates run in sequence
(`checker::check_record`'s `Ok`, `check_context_constraints`, `check_context_declarations`'s
`blocks_emission`, each a `continue` on failure) and `emit_unit` is called only after all three pass.
What *is* true is that **later**, unrelated phases — `process_tests`, `process_integration_tests`,
`check_platform_lock` (gated on `errors.is_empty()`, confirmed to run only after `check_unit_files`
returns) — can still add error-severity diagnostics that fail the whole build. But `finish_build`'s
final `if !errors.is_empty()` arm discards the *entire* `RunChecks::Checked` state, `compiled` output
included, unconditionally — so a late platform-lock failure already can't let an earlier unit's TS
reach the caller, regardless of whether that unit went through `certify`. That's whole-build atomicity,
an existing and already-correct property, separate from R3.10's actual claim ("no error node reaches
the emitter" — a per-node/per-unit property, not a per-build one). Conflating the two made a real,
available slice look architecturally blocked when it wasn't.

Once separated, T3.7b was the same shape as every other choke-point slice in this track: `certify`
called once, right after `check_unit_files`'s third gate, wrapping `typed` before it reaches
`emit_unit` — whose own `typed: &TypedCommons` parameter became `program: &CheckedProgram`, one
`.program()` accessor covering its two internal uses (`emitter::emit_project`, `collect_handler_labels`).
`emit_unit` and `emitter::emit_project` each confirmed, by grep, to have exactly one caller — not
assumed from a doc comment, the same discipline T3.7a itself used for `emitter::emit`. Verified against
the full multi-file project test corpus, not just unit tests, since this is the path every multi-file
`.bynk` project (workers targets, event fan-out, multi-file commons, the whole `e2e.rs` fixture corpus)
actually runs through: all green, and the internal-error panic guarding `certify`'s structurally
unreachable `Err` arm never fired once. R3.10 is closed in full; only T3.6b remains for this track.

**T3.6b's own settling review — the one its §6 row says it needs before slicing further — found a
genuine, evidence-backed negative result, investigated twice independently for the same conclusion.**
The question asked was the one every prior slice in this track answered yes to: is there a
"parallel data" first increment — the technique §2 names for this whole track, and the exact shape
that worked for T3.4 (`ExprId` alongside `Span`) and T3.5 (`FileId` alongside the rest of `Span`) —
small, safe, and *actually used*, not just defined and left dormant? A standalone `TyId`/intern-table
struct (`Types { intern(&mut self, ty: Ty) -> TyId, resolve(&self, id: TyId) -> &Ty }`) is trivially
buildable today on T3.6a's already-shipped `Hash`/`Eq`/`Ord` derives, without touching `Ty`'s own
`Box<Ty>`/`Vec<Ty>` fields. But buildable is not the same as populated: a workspace-wide search found
**zero** `HashMap<Ty,_>`/`HashSet<Ty>` anywhere in `bynk-check`/`bynk-emit`/`bynk-ide`/`bynk-lsp` —
confirming, independently, T3.6a's own note above that its derives are "not even exercised by anything
yet." R3.13 (query memoisation), the concrete beneficiary R4.1's own rationale names for a cheap
interned key, is confirmed unbuilt (`bynk-greenfield-compiler.md`'s gap table: "no query
decomposition... phase 8"). The one real `Vec<Ty>`-as-dedup-guard found in the whole tree
(`seen_sources` in `checker/refinements.rs`, bounded by a handful of declared `embeds` clauses per
type) is real but too small to carry genuine signal — converting it would be exactly the "hollow
shipped infrastructure nothing uses" pattern this section exists to warn against, not a legitimate
slice. Even a dual-population step at `type_of`/`type_of_block`'s existing write choke point (the
exact site T3.3b/T3.4 populated) would still ship dormant, because nothing downstream reads a `TyId`
without the same ~25-30-function rewrite that is T3.6b's actual scope — unlike `ExprId`/`FileId`,
which populated an identity into a place *already read* by real code (`expr_types`'s own re-keying;
`remap_site`'s existing `Span` comparison), `Ty` has no equivalent already-read comparison waiting for
a cheap key today. **Conclusion, stated plainly rather than left as an open question to re-ask next
session: no scaffolding-first slice exists. T3.6b starts as the real rewrite or not at all.**

The real decomposition, once someone does pick this up: `Ty`'s own field definition
(`Box<Ty>`/`Vec<Ty>` → `TyId`) is one Rust type definition that must change atomically, breaking every
one of `bynk-check`'s own ~20-23 pattern-matching functions (`display`, `substitute`, `contains_var`,
`contains_flexible_var`, `unify`, `compatible`, `structurally_compatible_inner`,
`rebrand_return_type`, `variants_of`, the `peel_to_*`/`is_*`/`join_*` family, `check_pattern` and its
neighbours, `checker/linearity.rs`'s `held_value`/`storage_value_is_held`) simultaneously — there is
no way to migrate `bynk-check`'s own consumers one function at a time mid-change the way `ExprId`
threaded through the parser file-by-file. The crate boundary is therefore the smallest real slice, not
an artificial one chosen for convenience — and it is a larger boundary than the function count alone
suggests: `compatible`, `unify`, and `substitute` are `pub fn`s taking `&Ty` with no context parameter
today, called from **149 sites** (86 + 25 + 38, direct grep, `bynk-check` only) nested throughout the
checker's call graph, not just from the ~20-23 sites that pattern-match `Ty` directly. If `Ty`'s
recursive fields become `TyId`, resolving a child during recursion needs a `&Types` handle, which these
three signatures — and therefore all 149 call sites — must gain access to, most of them nested many
frames below `check_fn`/`type_of` rather than beside a `Ctx` already in scope at the point of the call.
Confirmed contained to `bynk-check`, not a cross-crate cascade: only **one** external call site exists
for all three combined (`bynk-emit/src/emitter/lower.rs:224`, `compatible(e, f_err)`) — consistent with,
and now measured evidence for, "tier 1 is `bynk-check` alone." **Tier 2 is `bynk-emit`**'s ~7 functions (`ty_to_type_ref`,
`ts_ty`, `payload_field_ty`, `mock_value`, `list_ok_elem_ts`, `decode_map_key`, `literal_base_of_ty`),
gated on tier 1 landing; **tier 3 is `bynk-ide`/`bynk-lsp`**'s ~6 functions (`variants_for_ty`,
`named_type_target`, the `Ty::Named` destructuring in `symbols.rs`), the smallest function count but
the highest regression risk since it's the developer-facing hover/completion surface, and per ADR
0314 (already adopted by this track, §3.4) needs its own LSP-surface fixture, not just the emission
goldens tiers 1–2 can lean on.

**An open design question the tier system above does not yet answer, found while scoping whether tier
1 could actually start: who owns the `Types` table once it exists, and how does a value built against
it reach a consumer in a different crate?** If `Ty`'s recursive fields become `TyId`, a `Ty` value only
tells you its own top-level variant — resolving anything nested inside it (what a `Ty::Option` actually
contains) requires the *same* `Types` table it was interned against. `TypedExpr.ty: Ty`
(`TypedCommons::expr_types`'s value type, T3.4) is exactly such a value, read by ~61 call sites in
`bynk-emit` alone (T3.4's own count) plus `bynk-ide`'s completion/hover code and `bynk-lsp`'s
`index_queries.rs`. Every one of those becomes unable to fully resolve a `Ty` it's handed unless it
also has the `Types` table that produced it in scope — which means the real blast radius is not
contained to `bynk-check`'s own ~20-23 functions (tier 1) at all; it is *every* existing `Ty` consumer
across all four crates, including the ~13 functions the tier 2/3 estimate already named plus every
plain (non-recursing) reader of `TypedExpr.ty` that tier 2/3's function count didn't count because it
doesn't pattern-match `Ty`, just holds and forwards it. Answering this — a single project-wide
`Types` table threaded everywhere, `TypedCommons` carrying its own alongside `expr_types`, or some
other shape — is real design work with real tradeoffs (a global table risks cross-file/cross-compile
identity confusion, echoing exactly the R2.2/R2.4 lesson this track already learned about `Span`/
`ExprId`; a per-`TypedCommons` table means `TyId`s aren't comparable *across* units, which `unify`/
`compatible` calls spanning a `uses` boundary may need).

**Resolved by checking, not assuming, how cross-unit type references actually flow today.**
`bynk-emit/src/project.rs`'s `compose_unit_symbols` — the function that builds each unit's own
`combined_types`/`combined_fns`/`combined_methods` before that unit is checked — merges `TypeDecl`
(the immutable AST-level declaration) from transitively-`uses`d units into the checking unit's own
resolved input; it never shares an already-*checked* `Ty` value across units. Each unit's own
`check_record` call independently constructs its own `Ty` graph from that merged declaration set,
including for cross-unit types — `named_ty(decl)` builds a fresh `Ty::Named` from the shared
`Arc<TypeDecl>` every time a unit needs to represent that declaration as a type, regardless of which
unit originally declared it. `unify`/`compatible`/`substitute` therefore only ever compare `Ty` values
produced within the *same* `check_record` invocation — cross-unit `TyId` comparison across two
separate `check_record` calls never happens in the current architecture, so the "not comparable across
units" risk of a per-unit table isn't a real risk at all.

**Decision: `Types` is owned per `check_record` invocation** — created fresh at its entry, threaded
through via `Ctx` (already pervasive through every function that would need it), and carried out as a
new field alongside `expr_types` on `TypedCommons`/`RecordCheck`, exactly the shape `ExprId`'s own
counter took in T3.4. `CheckedProgram` (T3.7a/T3.7b) already exists as the boundary object handed from
`bynk-check` to `bynk-emit` for precisely this reason — extending it to carry `types: Types` alongside
its wrapped `TypedCommons` is the natural, already-built seam for getting the interner across the
crate boundary, not a new mechanism. `bynk-ide`/`bynk-lsp`'s `Mode::Analyse` path (which never calls
`certify`) would need the equivalent carried on `RecordCheck`/`ExprTypeSink` directly. This is now a
settled design decision, not an open question — tier 1 (and 2, and 3) can be scoped and executed with
confidence against it; implementing it is still the large, multi-function, multi-crate undertaking
tiers 1–3 already describe, which is why it remains future work rather than attempted in this pass.

**One more shortcut was checked and ruled out before concluding tier 1 has no smaller real slice:
converting `Ty`'s recursive variants to `TyId` one at a time (`Ty::Option(Box<Ty>)` →
`Ty::Option(TyId)` alone, say, leaving `Result`/`Effect`/`List`/etc. as `Box<Ty>`) does not shrink the
dominant cost.** `compatible`, `unify`, `substitute`, `display`, and the other generic recursive
functions each handle every `Ty` variant in one function body — Rust has no way to give a function a
signature that only sometimes needs a `&Types` parameter depending on which variant a particular call
happens to touch. The moment *any* variant's match arm needs to resolve a `TyId`, that function's
*signature* changes, which cascades to all of its callers regardless of which variant they were
matching — the same 149 call sites for `compatible`/`unify`/`substitute` alone that a full migration
would touch. Per-variant migration only reduces the smaller cost (construction/match-site fixes for
that one variant, real but not dominant); it does not reduce the threading cost, which is paid in full
the moment the first variant converts. This rules out the one remaining idea for shrinking tier 1
further — the size is real, not an artifact of how the prior measurements framed the question.

The one real, small, immediately-shippable piece this review *did* find — closed in this same commit,
not deferred as a bare finding — is test coverage for the specific failure mode unique to interning
that nothing in the existing corpus covers: whether two `Ty` values built through different
construction paths but structurally identical hash and compare equal (the property `intern()`'s dedup
will depend on), and whether structurally different ones don't collide. `bynk-check/src/checker.rs`'s
new `ty_hash_eq_ord_tests` module pins exactly this — `map_entry_ty` (a real constructor) against its
own equivalent raw `Ty::Named` literal, a deeply nested type built twice, and `HashSet` insertion
behaviour in both directions (dedup and non-collision). The byte-identical golden fixture corpus
(`bynkc/tests/fixtures/positive`, 394 dirs) and the 422-dir negative corpus catch a broken
`compatible`/`unify`/`ts_ty` if it changes what compiles or what emits, but neither was authored to
test interning's specific dedup-vs-collision property — this was a real, zero-coverage gap, not a
belt-and-suspenders addition.

## 10. What "transformational" means here

In the trajectory's own words (§3, Phase 3): "This is the phase that makes the batch path and the
editor path one program." Not an ergonomic dividend the way Tier B's four-defects-in-one-change was —
a precondition for phase 8 (incrementality needs stable interned identity and total side tables per
trajectory §4) and for phase 4's project model sitting cleanly below both check and emit.

---

## 11. ADRs

Per ADR 0167 step 2, the load-bearing, hard-to-reverse decisions land before slicing.

**Landed** (this settling pass, `level: patch`, no code — `design/pending/identity-and-totality-settling.md`):

- **[ADR 0313](../decisions/0313-phase-3-scaffolding-before-retrofit.md) — the phase-3 migration
  technique: `ExprKey(Span)` scaffolding first, real `ExprId` and totality later.** §3.1 (Q1)'s first
  reversal. **Superseded** — see below.
- **[ADR 0314](../decisions/0314-refactor-acceptance-gate-lsp-surface-requirement.md) — ADR 0309's
  Structural tier gains an LSP-surface-fixture requirement** for consumer crates that ship
  developer-facing behaviour. §3.4 (Q4). Amends ADR 0309 the same way ADR 0309 amended ADR 0059
  property 1. **Stands** — unaffected by §1.1's correction.

**Lands with this correction** (`level: patch`, no code — `design/pending/identity-and-totality-t3-correction.md`):

- **ADR-C — ADR 0313 is superseded: no standalone `ExprKey(Span)` slice.** §1.1, §3.1's second
  reversal. Records that `43abc242` had already shipped ADR 0313's proposed scaffolding (minus the
  newtype itself) and had already reached, and acted on, ADR 0313's rejected conclusion — evidence a
  prior author had, and this settling pass didn't check for before writing ADR 0313. Per
  `design/decisions/README.md` ("a reversal is a new record that supersedes the old one"), ADR 0313
  stays on file, unedited, with a superseded pointer added to its file and index row once this ADR's
  number is known.

---

## 12. Retirement

Mirrors `compiler-architecture.md` §12: this track retires when §6's probe (`span_keyed_maps`) reads
zero and every slice named to reach it has landed — whether or not phase 4 has opened. The retirement
PR removes this doc, appends its closing summary to `../archive/retired-tracks.md`, and closes the
spine (`Closes #1046`).
