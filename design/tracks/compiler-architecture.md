# Compiler architecture — migrating to the greenfield reference

- **Status:** **Settling.** §3's questions are answered but the answers have not been reviewed — see
  the provenance note at the head of §3, which claims the track README's step-2 carve-out from the
  outset. Awaiting a settling PR. Merging this doc settles **direction**; it is not a build
  authorisation. Each slice is an ordinary increment proposal, and `accepted` on that sub-issue is
  the approval to build.
- **Spine:** [#996](https://github.com/accuser/bynk/issues/996)
- **Theme:** **phases 0, 1 and 2** of
  [`../bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md) — the seams, the paydown, and
  the typed hoist. The trajectory's endpoint is the current compiler rebuilt on
  [`../bynk-greenfield-compiler.md`](../bynk-greenfield-compiler.md); this track walks the first
  three phases of it, without changing the language.
- **Phase boundaries are safe stopping points** (trajectory §2). This track can be the last one
  opened and what it leaves behind is coherent.
- **Relates:** [ADR 0059](../decisions/0059-refactor-track-posture.md) (refactor-track posture — this
  track extends it and amends its acceptance gate), [ADR 0076](../decisions/0076-feature-track-posture.md)
  (track posture), [ADR 0167](../decisions/0167-feature-tracks-run-github-native.md) (GitHub-native
  flow), [ADR 0099](../decisions/0099-crate-layering-dependency-direction.md) (the layering this
  track finishes), [`../reviews/2026-07-27-compiler-pipeline-review.md`](../reviews/2026-07-27-compiler-pipeline-review.md)
  (the evidence), [`../bynk-1.0-definition.md`](../bynk-1.0-definition.md) (why the sequencing works).
- **Precedent:** `crate-decomposition.md` (retired, v0.60–v0.66, ADRs 0099–0102) and
  `increment-allocation.md` (retired, ADR 0206) are both prior tracks whose theme was internal
  architecture rather than language surface. The track artefact is not reserved for features.

### ADR 0076 trigger check

| Trigger | Met? |
|---|---|
| Spans several increments | **Yes** — two tiers, eleven slices |
| Surface not yet settled | **No** — the destination is specified in full; what was unsettled is *how far to go*, which §3 has now closed |
| Security/safety boundary | **Discharged.** T1.2 (the `ResolvedCommons` constructor) re-enabled three checker gates, one of which its own comment calls "the primary boundary guarantee the threat model names". §6.0 measured it as **already landed** by ordinary work. The trigger was met when this track was drafted and is recorded, not claimed. |

**One of three — so the count fails, and the count is the wrong test here.** ADR 0076's triggers are
written for a *language feature*, where "surface not yet settled" and "security boundary" are the two
things that distinguish a track from a proposal. An internal-architecture track has no language
surface to be unsettled, and its security content is incidental rather than definitional. Applied
literally, the check would have rejected `crate-decomposition.md` too.

**The track is warranted on a different ground, stated so it can be argued with:** §3's six decisions
— the library's purpose, the emit-ABI posture, the acceptance gate, phase 3's trigger, the freeze
scope, in-place versus adjacent — are **not per-slice and not per-phase**. They govern eleven slices
here and constrain six later phases, and they need a home that is neither a proposal (which closes)
nor the trajectory (which holds the endpoint, not the method). That home is a track doc, which the
README defines exactly as "the living map the per-slice proposals are cut from".

If that argument does not hold for a reviewer, the honest fallback is available and cheap: run Tier A
as ordinary proposals, and open a track for Tier B alone. The decisions would then need relocating —
most naturally into ADR-B and ADR-D, which already carry the two that matter most.

---

## 1. Why now, and why this is not "more Bynk"

**The language surface does not move.** No `.bynk` syntax change, no semantic change, no new
construct, no tutorial rewrite, no per-increment ADR beyond the four named in §11. Every slice is
behaviour-preserving or carries an explicitly recorded behaviour change (§4).

**The 1.0 window points the other way from intuition.** `bynk-1.0-definition.md` is explicit that
stability does *not* freeze "the emitted TypeScript. The compile target is an implementation detail,
not part of the frozen contract; the codegen may improve within a 1.x release as long as documented
behaviour holds," and that the tooling surface is "versioned with the toolchain, not the
language-stability promise." So the compiler gets *freer* at 1.0, not more constrained. The window
closing at 1.0 is the language surface, which this track does not touch.

**And 1.0 is the event the July review was waiting for.** Its refusal of a query engine was an
explicit conditional — "inverting the pipeline **before the phase sequence has settled** would be
paying a large fixed cost for a problem that does not yet bite." A 1.0 stability commitment *is* the
phase sequence settling. The condition expires on its own; §3.4 makes that a decision rather than a
drift.

**The payoff that is least speculative is reviewability.** The barriers to outside contribution are
the same list as the architectural defects, item for item: "the crate names do not describe the
layering… a contributor reading the crate docs will be misled about where to make a change";
`bynk-check::check_record` matches two item kinds while 110 diagnostic codes live in
`bynk-emit/src/project/validate.rs`; every edit to `lower.rs` rebuilds and relinks six crates; a
dispatcher's doc comment says "the order is load-bearing" about an order duplicated in another crate;
233 "keep in sync" comments are 233 rules that cannot be taught by the compiler and must therefore be
taught in review, every time.

---

## 2. What this track is not

- **Not a rewrite.** There is no v2 branch and no parallel compiler. Every slice lands on `main`.
- **Not the whole reference.** This track covers phases 0–2 of the trajectory. The IR, the
  TypeScript tree and printer, and the crate re-graph are **later phases with their own tracks**
  (§7) — not refusals, and not this track's work.
- **Not a 1.0 blocker.** Nothing here gates the 1.0 release except the two decisions in §3.1–§3.2,
  and those are decisions, not work.

---

## 3. Design questions — answered 30 July 2026, review pending

> **Provenance, stated plainly: this settling has not been reviewed.**
>
> ADR 0167 step 2 puts the settling phase inside a **draft PR**, where the open questions are closed
> "under line-anchored review", and marking the PR ready "asserts the questions are closed". These
> six were closed in a design conversation instead. The prose below is therefore *proposed* settling,
> not *tested* settling.
>
> The track README records this exact failure and its remedy: `idempotency-capability.md`'s settling
> PR (#922) "was marked ready for review and merged 55 seconds later with no review (`reviews: []`)",
> and the step-2 carve-out applied — the track's real phase stayed **Settling** past the merge until a
> genuine re-settling pass (#924, #927) closed its questions for real.
>
> **This track claims that carve-out from the outset.** Its phase is Settling until a settling PR has
> actually tested the assertion, whatever this document's status line says. Better to claim it than
> to have it noticed.
>
> The two decisions most worth a second reader are **D2** (the emit-ABI posture, because it interacts
> with a 1.0 commitment already made) and **§3.6** (running phases 0–2 ahead of 1.0, because it is
> the only decision here that changes what happens to the release).

### 3.1 D1 — What is `bynkc`-the-library for? **Settled.**

`bynkc` has both a `[lib]` and a `[[bin]]`. The library is a 91-line re-export facade; nothing in the
workspace depends on it (`bynk/Cargo.toml` says so explicitly, and `bynk-ide` was demoted to a
dev-dependency in Wave 5 §5.3, citing review finding #41). But `bynkc` **is published**, so its
`pub use bynk_check::{actors, builtin_names, checker, expr_types, firstparty, hints, index,
kernel_methods, locals, requirements, resolver, store_ops}` and `pub use bynk_emit::{emitter, project}`
put `bynkc::checker::Ty` and `bynkc::resolver::*` on crates.io as public API of a released crate.

ADR 0059's escape hatch — "the crate is pre-1.0 with only in-repo consumers" — expires at 1.0. If
nobody decides, the crate graph freezes by default and phases 4–5 become a breaking change. The review:
this "re-attaches at the top exactly the constraint the decomposition into leaf crates existed to
remove."

**Decision: narrow the library. Delete the fourteen whole-module re-exports; keep the item
re-exports.** `[lib]` stays. `cargo install bynkc` is unaffected. `bynkc`'s integration tests move to
leaf-crate imports (`bynk_check::checker::Ty`), which is the correct import in any case.

*Rejected: `publish = false`* — it would break `cargo install bynkc`, which ADR 0101 preserves
deliberately for CI/build determinism. *Rejected: deleting `[lib]`* — the library is load-bearing for
the integration test suite's namespace, and the ~30 item re-exports (`CompileOptions`,
`compile_project`, `BuildTarget`, `strip_project_to_js`, the renderers) are a small, legitimate API
for a published compiler.

*Why this and not more:* the `bynk` driver is to `bynkc` what `cargo` is to `rustc` (ADR 0083), and
cargo ships `cargo build` while `rustc` stays installable — a pinned compiler binary without the
driver's Node orchestration is precisely what CI determinism wants. And once the module re-exports
are gone, deleting `[lib]` altogether is trivial if it ever becomes wanted.

**Lands as:** slice **T-D1**, carrying **ADR-A**. It changes a published surface, so it needs its own
`level` and changelog row.

**Named follow-on, deliberately unfiled: `bynk build`.** The driver gaining a build verb, with
`bynkc` receding to the CI/build-determinism role ADR 0101 already reserves for it. It is recorded
here because it was the reasoning that decided D1's shape and a later reader would otherwise assume
it had been forgotten. It is *not* filed: it is user-facing CLI surface, and this track's whole
premise is that internal architecture is a different category from surface additions. D1's decision
does not depend on it — the cargo/rustc analogy stands on CI determinism alone — so `bynk build` can
be proposed whenever it is wanted, or never, without reopening anything here.

### 3.2 D2 — Is the emit ABI published? **Settled.**

ADR 0086 calls the first-party bindings and the runtime "part of the compiler's **emit ABI**" —
coupled to `Result`/`Option` tag layout, `JsonError`, `Uuid.of`, `FetchError` — and defers publishing
them as `@bynk/*` packages "gated on runtime-ABI stability (≈1.0)". That collides with the 1.0
definition's freeing of the emitted TypeScript: both cannot hold once a third party authors a
capability binding, because a binding is hand-written TypeScript constructing `Ok(…)` and reading
`.tag === "Err"` that the Bynk compiler never type-checks against a changed emit shape.

**Decision: do not publish before 1.0 — and record the eventual shape now.** When the bindings are
published, the shape is **lockstep versioning with the compiler** (an exact-version dependency, not a
range) plus a **fail-closed runtime-version check** at load. This is ADR 0200's contract-hash pattern
applied to the ABI: a mismatch is refused rather than tolerated, so a skew is a legible error rather
than a silent miscompile at runtime.

Recording the shape costs nothing now and is the whole point of the decision: publishing on
semver-stable terms would freeze the emit ABI at 1.0 by implication, and that is a *language
stability* decision that would have to be argued in the 1.0 record, not arrived at by shipping one
third-party adapter.

**Lands as:** **ADR-C**, in the settling increment. No code.

### 3.3 Q3 — What is the acceptance gate? **Settled.** *(amends ADR 0059)*

ADR 0059's standing property 1 is "behaviour-preserving… The acceptance gate is the existing golden
fixtures passing **byte-identical and unedited**." That worked for the `project.rs` and `checker.rs`
splits because their movers were pure string and path helpers. It does not work here: `emitter.rs`
and `lower.rs` have almost no pure helpers — their units are `fn(&mut String, &Ast, &mut LowerCtx)` —
so a Tier B change would land with whole-file goldens, one crate up, driven from disk, as its only
net, and a broken golden is an entire emitted TypeScript file failing in `bynkc` rather than in the
crate being refactored.

Worse, the corpus has two precedents for structural changes passing a green gate. ADR 0198: "331
negative fixtures and not one of them can observe which file a diagnostic was blamed on — so the
identity could be wrong for every split project and every test would still pass. **It did, and they
did.**" And its verdict on exactly this question: "'the gate is green' is the weakest possible
evidence here." ADR 0201, converting keyed sinks by grep: "converting it looked right and was not…
the failure is a **hang**, not an assertion."

**Decision.** The gate becomes, per tier:

| Tier | Gate |
|---|---|
| 0 | its own artefacts exist and are exercised (§6, T0 criteria) |
| 1 | byte-identical goldens **plus** a crate-local fixture per behaviour change; a slice that changes a diagnostic must add an `expected_diagnostics.txt` assertion |
| 2 | crate-local fixtures via T0.1 **plus** a named regression fixture per closed defect **plus** byte-identical goldens |
| later phases | as Tier B, plus the phase completion probe (§8) reading zero |

**Lands as:** **ADR-B**, in the settling increment. Must land before Tier A opens.

### 3.4 Q4 — Does phase 3 open at all, and on what trigger? **Settled.**

Phase 3 (node identity and totality) is the expensive one, and the July review's verification pass
explicitly killed a full `NodeId` retrofit. The review also named three cheaper steps as "the
migration scaffolding if the retrofit ever happens."

**Decision.** Phase 3 does not open on appetite. It opens when Tier B is complete **and** one of these
fires:

- a defect class recurs in the lowering after being patched once at a different site;
- a documented language-level semantic property — short-circuit, evaluation order, atomicity — is
  found violated by the emitter rather than by the checker;
- **the emitter's in-file test-line ratio does not rise in the two releases after Tier A completes**;
- a second consumer of the emitted artefact appears (a second target, a debugger, a course).

The third trigger is deliberately phrased as a *hypothesis test*, not a threshold. The stated blocker
on emitter testing is the missing seam; Tier A closes its remainder. If density still does not move once the
seam exists, the blocker was structural — which is exactly the evidence phase 3 needs, and an
arbitrary percentage would not have supplied it.

### 3.5 Q5 — Reviewer bandwidth, and the freeze. **Settled.**

`bynk-adoption-sequencing.md` has a tooling freeze in effect. This track is not tooling, but it
consumes the same reviewer. ADR 0059's answer to the same problem was a **feature freeze** with
short-lived branches, on the grounds that "the hot files the track restructures… are the most-churned
in the crate (touched in 34/25/36 of the 40 commits preceding the track)."

**Decision: a scoped freeze, not a feature freeze, and only for Tier B.**

ADR 0059 needed a full freeze because its track restructured three hot files — `project.rs`,
`checker.rs` and `emitter.rs`. Tier B touches one: `lower.rs` and its callers. Tier A touches
neither `lower.rs` nor itself in any conflicting way, and runs without a freeze.

So: **no feature work lands in `bynk-emit/src/emitter/lower.rs` while T2.1 is open.** That is a
narrower and much cheaper commitment than ADR 0059's, and it is the only conflict the tier structure
actually creates.

### 3.6 Sequencing relative to 1.0 — **Settled.**

**Decision: Tiers 0, 1 and 2 run now, ahead of 1.0.**

Three reasons, in order of weight.

**The compiler that makes the 1.0 promise should not contain a known miscompile.** `let x = match
risky()? { … }` currently early-returns into a synthetic arrow and silently evaluates to the `Err`
object — "a miscompile of ordinary code with no diagnostic and no assertion." Issuing a compatibility
promise on top of that is an odd thing to do, and Tier B is what closes it.

**T1.2's behaviour change is free before 1.0 and expensive after.** Re-enabling `.raw`, `T.unsafe(…)`
and owner-only event emission will reject programs that compile today. Pre-1.0 that is an ordinary
increment with a changelog row; post-1.0 it is a compatibility event on a security-relevant gate.

**The cost is smaller than it looks, because 1.0 is already gated on a track that has not started.**
Gate 2 (`deploy`) is satisfied; gate 3 (state migrations) is "the second adoption blocker (**track to
be opened**)". So "let 1.0 wait" is not a delay against an imminent release — the real question is
ordering against the state-migrations track, and the answer follows from §3.5: Tiers 0–1 conflict
with nothing, and Tier B's scoped `lower.rs` freeze is cheapest in exactly the window before a
storage-shaped track opens.

**Recommended order:** Tier A (T0.0 first) → Tier B under the scoped freeze → open the
state-migrations track → 1.0. Tiers 3 and 4 remain gated on §3.4 regardless.

---

## 4. Posture — how this track runs

Extends ADR 0059, whose four standing properties carry, amended:

1. **Behaviour-preserving by default.** Where a slice changes observable behaviour — T1.1 changes
   `bynk check`'s exit code, T1.2 re-enables three gates that will start rejecting programs that
   compile today — it says so in its proposal, ships a fixture asserting the new behaviour, and gets
   a CHANGELOG entry. A behaviour change is allowed; an unrecorded one is not.
2. **Acceptance gate per §3.3**, not byte-identical goldens alone.
3. **Short-lived branches, one PR per slice.** No integration branch. ADR 0059's reasoning holds and
   is stronger here.
4. **No per-increment ADRs.** The four in §11 land up front; slices cite them and the rule ids.

Two additions specific to this track:

5. **Every slice cites the rule ids it closes**, in a `Closes-Rule:` trailer. This is what makes §8
   possible.
6. **A slice that leaves both paths reachable is not done.** See §5.

### 4.1 In-place, not adjacent

**Every slice lands in the existing crates. There is no parallel tree, no `bynk2/`, no
feature-flagged second pipeline on `main`.**

ADR 0059 already rejected the adjacent shape for the reason that applies here: "A long-lived
integration branch accumulating the whole track would therefore conflict catastrophically the moment
the freeze lifts and feature work resumes in those files" — the hot files being "the most-churned in
the crate (touched in 34/25/36 of the 40 commits preceding the track)." A parallel crate tree is a
long-lived integration branch with extra steps.

Three further costs, in order of weight. **It violates §5 by construction, at maximum scale** — both
paths reachable for the whole duration, deliberately, ending in the big-bang swap the tier structure
exists to avoid. **Every language increment lands twice** or the new tree falls behind, which is the
same structural-lag argument the course-feasibility work used to keep the course in-tree ("a language
change breaking an exercise breaks the build IN THE SAME PR — a same-repo property"). **And a
sub-folder buys naming, not isolation** — workspace members are still built, linted, MSRV-checked,
and need `deny.toml` and release-workflow entries, so it carries separation's bookkeeping with none
of its benefit.

Two smaller consequences: fork-and-swap destroys the line-level git continuity that makes this
codebase's archaeology possible, and an adjacent tree means a contributor must know which pipeline is
live — the exact confusion §1 says this track exists to remove.

**What adjacent genuinely buys is differential testing, and that is available without forking.** For
Tier B, keep the old lowering path behind a `cfg` **on the branch only**, run the fixture corpus
through both, diff the emitted TypeScript, and delete the old path before merge. Stronger than
byte-identical goldens on one pipeline, because it tests the new against the old on every input in
the corpus — and the second path never reaches `main`.

**Per-tier technique:**

| Tier | Technique |
|---|---|
| A | straight in-place |
| B | in-place, with the branch-local differential harness above; the completion probe proves the fork collapsed |
| phase 3 | *parallel data, single pipeline* — `ExprId` allocated alongside `Span`, both tables populated, consumers migrated one at a time, the `Span`-keyed table deleted last. T3.1 and T3.2 are its scaffolding |
| phases 4–5 | `git mv` plus import fixes |

**The one case where adjacent would be right, recorded so it is not rediscovered by momentum:** if
§7's exclusions are ever reopened — a typed IR, a TypeScript tree, a printer — that is a *different
program*, not a change to this one, and forking would be the honest way to build it. Adjacent becomes
the right question **if and only if** §7 is reopened, and it should be asked again then rather than
assumed either way.

---

## 5. The completion criterion

The thesis of the retrospective this track descends from is that every regret in the corpus is a
correct abstraction introduced as *available* while the old path stayed reachable — `UnitInfo` beside
the parallel maps, `CompileOptions` collapsed at the public boundary only, the overlay plumbed with
two bypasses, `expr_children` with twelve adopters and three holdouts.

**A tier is complete when its old path is deleted and a mechanical probe reads zero.** Not when the
new path exists. Not when most callers have moved. Each tier below states its probe, and §8 runs
them.

A half-migrated compiler is worse than either endpoint. This project's own history is the evidence.

---

## 6. Slice decomposition

### 6.0 Measured baseline — 30 July 2026, v0.245.0

**Read this before the slice tables.** Every number below was measured against the working tree, not
taken from the July review. The review is v0.237.1; eight of the slices originally drafted for this
track had **already landed** in the eight versions since.

| Probe | Reads | State |
|---|---|---|
| `CompileOptions.sources` | present | ✅ landed |
| `bynk-emit/src/testkit.rs` | present | ✅ landed |
| `strip_project_to_js` in tests | 7 sites | ✅ landed |
| `ReportKind::Warning` | present in `bynk-syntax/src/error.rs`, with a comment describing the old defect | ✅ landed |
| `ResolvedCommons {` in `bynk-emit` | **0** (`::new` = 9) | ✅ landed — the security slice |
| `groups` / `test_groups` / `kinds` / `integration_groups` | all `BTreeMap` | ✅ landed |
| `http_value_serialiser` | **0** live (1 comment mention); `serialise_ref_via` = 7 | ✅ landed |
| `CompileOptions` | `#[derive(Clone)]` | ✅ landed |
| `joinOn` / `groupBy` in `kernel_methods.rs` | present | ✅ landed |
| `type_refs_match` | **0** live (3 comment mentions) | ✅ landed |
| `expr_children` | **34 uses across 8 files** (23/5 on 30 July; new consumers in `bynk-check` and `bynk-lsp`) | ✅ largely landed |
| `is_fully_drained` | 5 occurrences | ✅ landed |
| — | | |
| `[workspace.lints]` | **absent** | ⬜ open |
| `expected_contains` / `_absent` / `_diagnostics` | **3 / 2 / 1**, against **419** `expected_error` | ⬜ format exists, adoption ~1% |
| `tree-sitter-bynk/tests/conformance.rs` | exists, both-parsers-agree, **scoped to the type surface**, case-driven | ⬜ mechanism built, totality open |
| `std::fs` below the driver | `bynk-emit` 4 files, `bynk-ide` 5 files, `bynk-fmt` 1 | ⬜ open |
| `stmts: &mut Vec<String>` | **32** | ⬜ open |
| `contains("await` in `lower.rs` | **1** | ⬜ open |
| `HashMap<Span` | **27** | ⬜ phase 3 |
| `Ty::Error` | absent | ⬜ phase 3 |
| `Span` fields | `{ start: usize, end: usize }` — no `FileId` | ⬜ phase 3 |
| `certify` | **0** | ⬜ phase 3 |
| `bynk-project` crate | absent | ⬜ phase 4 |
| `bynk-ide` → `bynk-emit` | present | ⬜ phase 4 |
| unique `bynk.*` codes in `bynk-emit` | **200** registered (`bynk-check`: 206). A `bynk.*` literal grep returns 206/212; six in each crate are commons or namespace paths, not codes — see §8 | ⬜ phase 5 |
| `bynk_syntax::ast` imports in `bynk-emit` | **13 files** | ⬜ phase 6 |

**Two conclusions change this track's shape.**

**Phases 0 and 1 are roughly 85% complete, and nobody planned them.** The paydown happened as
ordinary work over eight versions. That is good news and it is also the argument for the trajectory:
small defects get fixed by normal churn; structural ones do not.

**The layering is not shrinking.** The review counted 190 codes originating in `bynk-emit`; the same
crate now carries **200** codes that are actually in the registry. The counting methods still differ —
the review's 190 was not derived the way this 200 was — so treat it as directional. But there is no
shrinkage, and probably growth. Phase 5 gets further away on its own.

### Decision slices (unordered, independent of the tiers)

| Slice | What lands | Rules |
|---|---|---|
| **T-D1** | The fourteen whole-module re-exports deleted from `bynkc/src/lib.rs`; `bynkc`'s integration tests repointed at the leaf crates. Carries **ADR-A**. | R10.4 |

### Tier A — Phases 0 and 1, the remainder

**Tiers 0 and 1 are merged**, because the measurement removed the distinction between them. The two
tiers were separated by *dependency* — Tier 0 enabled, Tier 1 stood alone — and the enabler that
justified the split, the in-memory `sources` seam, has already landed. Of what remains, **only T0.0
gates anything**: it gates the completion criteria, not the work. Everything else is independent
paydown that can land in any order.

Keeping two tiers whose only difference was a dependency that no longer exists would be exactly the
bookkeeping this track exists to correct. The phase numbering in
[`../bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md) is unaffected — phases 0 and 1
remain distinct *states*; this is one tier of work that closes the remainder of both.

| Slice | What lands | Rules | Size |
|---|---|---|---|
| **T0.0** | **`cargo xtask greenfield-status`** — the probe harness of §8, with exact probe definitions, a committed status table, and a CI check that fails when it is stale. **First; everything else is unordered.** | §8 | ~200 lines |
| **T0.2′** | *Adoption*, not construction. The three assertion granularities exist and have six users against 419 category-string fixtures. Convert the fixtures where attribution or a targeted property is the thing under test — starting with the thirteen `Roots::Split` fixtures ADR 0198 named as unobservable | R11.2 | medium |
| **T0.3** | `[workspace.lints]` with `clippy::wildcard_enum_match_arm` at `warn`; the inventory recorded on the spine; `deny` per crate as each is cleaned | R2.12 | one manifest edit + an inventory |
| **T0.4′** | Widen `tree-sitter-bynk/tests/conformance.rs` from cases to **totality**: every file the compiler's parser accepts (all of `examples/`, the vendored first-party `.bynk`, every positive fixture) has zero ERROR *and* zero MISSING nodes; every parse-error negative fixture is rejected by both. The both-parsers-agree mechanism is already built and correct — this widens its corpus | R11.7 | ~40 lines |
| **T0.7** | The residual filesystem reads below the driver: 4 files in `bynk-emit`, 5 in `bynk-ide`, 1 in `bynk-fmt`. `bynk-ide` has **two unrelated reasons** to touch disk, not one: `completion.rs`'s `cached_project_unit` path (#62, 3 sites) and `symbols.rs`'s `find_declaration_cross_file` / `describe_symbol_cross_file`, which read directly and bypass that cache entirely. Two fixes, not one | R2.3 | medium |
| **T1.6′** | Generate the `method_not_found` text *from* the registry, and make the drift test **bidirectional**. The vocabulary drift itself is fixed (`joinOn`/`groupBy` now present); the one-directional test and the second copy are not | R6.11, R11.6 | small |
| **T1.7′** | Verify the residue: the store-field shadowing guard, `linearity.rs`'s `let` save/restore, and the `unit_info` map kind. `block_writes_state` now descends via `expr_children`; `type_refs_match` is gone | R2.11, R6.5 | small — may close as verification |
| **T1.8** | `NonEmpty` → `MinLength(1)`, `Positive` → `InRange(1, ∞)`, `NonNegative` → `InRange(0, ∞)` normalised inside `canon_refinement`. **Verified 31 July:** `bynk-check/src/contract.rs`'s `canon_predicate` emits `"NonEmpty"`, `"Positive"` and `"NonNegative"` as distinct literals and never folds them, so this is work, not verification | R12.2 | small |

*Struck — landed since v0.237.1:* T0.1 (the in-memory seam), T0.5 (the erasability gate), T1.1
(`ReportKind::Warning`), **T1.2 (the security slice — `ResolvedCommons` now has its constructor and
zero hand-rolls in `bynk-emit`, so the three gates are back on)**, T1.3 (ordered maps), T1.4 (the
parallel serialiser), T1.5 (`CompileOptions: Clone`), T1.9 (the trivia drain assertion), and most of
T1.7.

**Completion probes:** `std::fs` files below the driver = 0; `[workspace.lints]` present with
`wildcard_enum_match_arm` at `deny` and zero allowances; the conformance totality assertions in CI;
`xtask greenfield-status` green.

### Tier B — Phase 2, the typed hoist

The single highest-value structural change, and the only one in this track that touches the emitter's
substrate. It is a **signature** change, not a representation change: it does not commit the project
to the IR, and nothing about the IR is reachable without it.

| Slice | What lands | Rules |
|---|---|---|
| **T2.1** | `lower_expr` returns `Lowered { pre: Vec<String>, expr: String }` instead of taking a sink; threaded across ~90 functions. The two ternary throwaway vectors, the `simple_expr` gate and its `debug_assert!`s are deleted | R6.2 |
| **T2.2** | `maybe_async_iife`'s `contains("await ")` scan replaced by a flag computed during lowering; the `if`-IIFE path uses it too | R6.4 |
| **T2.3** | `lower_and_with_is` can no longer splice statements into a string; `lower_bin_op`'s general path no longer shares one vector across operands | R6.3 |

**Closes in one change:** the dropped-statements bug, the spliced-statements bug, the
`let x = match risky()? { … }` miscompile, and the short-circuit violation the type-system spec says
"developers can rely on".

**Completion probe:** `rg 'stmts: &mut Vec<String>' bynk-emit/` returns zero.

**Regression fixtures required:** one per closed defect, named in the slice proposals.

*Phases 3 and 4 previously appeared here as Tiers 3 and 4. They are other tracks' work; §7 carries
them as forward references with their entry conditions. Their slice decompositions are cut by those
tracks, not pre-empted here — an unopened phase whose slices are already written is a wish list.*

---

## 7. Out of scope for this track — forward references, not refusals

This track covers **phases 0, 1 and 2** of
[`../bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md). The items below are later phases
of the same trajectory, each with its own track, opened when the previous phase's probe reads zero.
They are listed here so a proposal citing one is recognised as out of *this track's* scope rather
than out of the programme's.

| Item | Phase | Entry condition |
|---|---|---|
| Identity and totality (`ExprId`, `FileId`, `Ty::Error`, `certify`) | 3 | this track's Tier B probe reads zero |
| The project model as a phase (`bynk-project`) | 4 | phase 3 complete |
| Semantics in the checker (`validate.rs` dissolves) | 5 | phase 4 complete |
| The full IR (reference Part 6) | 6 | phase 5 complete |
| The TypeScript tree and printer (reference Part 7) | 7 | phase 6 complete |
| Incrementality (query granularity, the firewall) | 8 | phases 3 and 4 complete |

*Note on an earlier revision.* Before the trajectory document existed, this section recorded the IR,
the printer and the crate re-graph as **refusals with triggers**. That was wrong bookkeeping:
recording an intention as a refusal is the inverse of the failure §15.1 of the reference names. They
are phases, and their triggers are entry conditions.

**Genuinely refused**, and carried with their triggers in the reference's Part 15: tuples, effect
inference, a diagnostic error enum, a rowan CST, and salsa-the-framework (as distinct from the
demand-driven architecture, which is phase 8).

---

## 8. Keeping the reference true

A 23,000-word reference describing a compiler you are not fully building becomes a museum piece
within two releases unless something binds it to the code. Two bindings, both cheap.

**Rule citations.** Every implementing PR carries a `Closes-Rule: R4.11, R4.6` trailer, the way
increments already cite ADR numbers.

**`cargo xtask greenfield-status`.** Regenerates the reference's Appendix D from those citations plus
a set of mechanical probes, and fails CI if the committed table is stale.

| Probe | Reads | Closes |
|---|---|---|
| `workspace_lints` | `[workspace.lints]` present, `wildcard_enum_match_arm` level | R2.12 |
| `wildcard_arms` | count of `_ =>` arms over compiler-owned enums | R2.12 |
| `fs_below_driver` | `std::fs` mentions in crates below the driver | R2.3 |
| `options_sources` | `CompileOptions` has a `sources` field | R2.3 |
| `hoist_sinks` | `stmts: &mut Vec<String>` occurrences in `bynk-emit` | R6.2 |
| `span_keyed_maps` | `HashMap<Span` occurrences | R2.4 |
| `emit_diagnostics` | `bynk.*` literals in `bynk-emit` **cross-referenced against the registry** | R3.5 |
| `ide_emit_edge` | `bynk-ide` → `bynk-emit` in the manifest | R10.2 |
| `keep_in_sync` | "in sync" / "mirrors" / "parity" / "must match" comment count | P2, trend only |
| `test_density` | in-file test-line ratio per crate | R11.1, and §3.4's trigger |
| `ast_importers` | `bynk_syntax::ast` importers in `bynk-emit` | R6.13 (phase 6) |
| `fixture_kinds` | users of each assertion granularity | R11.2 |

**Three probes need care beyond a grep, and the third was found the hard way.**

`emit_diagnostics` must count codes *originating* in `bynk-emit`, not code literals appearing there —
the registry lives in `bynk-syntax`, so a naive count over that crate returns 456 and means nothing.
**Crate-scoping is necessary and not sufficient**: within `bynk-emit`, six `bynk.*` literals are
commons or namespace paths (`bynk.locale`, `bynk.cloudflare`, `bynk.toml`, `bynk.synthetic`,
`bynk.bynk`, `bynk.schema.lock`) and six more in `bynk-check` (`bynk.list`, `bynk.map`,
`bynk.string`, `bynk.locale.types`, and two others). The probe must therefore **cross-reference each
literal against `bynk-syntax/src/diagnostics.rs`'s `REGISTRY`**, not pattern-match. Pattern-matching
inflates both crates by exactly six.

`wildcard_arms` must scope to compiler-owned enums; `_ =>` over a foreign type is legitimate.

**Every "reads zero" probe must exclude comments.** `type_refs_match` and `http_value_serialiser` are
both genuinely gone from the code and both leave comment residue — three mentions and one — so a
plain `rg` reports a closed rule as open. This is the inverse of the over-reporting failure and it is
the more likely one, because deleting a mechanism and explaining why in a comment is good practice.
A probe that under-reports trains its readers to ignore it.

A probe that over-reports is worse than no probe, because it reports a rule closed that is not.

The last two are trends, not gates — but `test_density` is one of §3.4's phase-3 triggers, so it needs
to be measured continuously rather than reconstructed later. **The probe set is the part that will
need care**: a probe that greps loosely will report a rule closed that is not.

---

## 9. Risks

**The one that matters: stopping at 60%.** Every regret in the corpus is exactly this. §5's
completion criterion and §8's probes exist for it, and they are the only mitigations that do not
depend on discipline.

**Tier B conflicts with feature work in `lower.rs`.** Answered by §3.5's scoped freeze — narrower
than ADR 0059's, and the only conflict the tier structure creates.

**T1.2 breaks programs that currently compile.** Correctly — three gates are off. It needs the
CHANGELOG entry, a fixture per gate, and a note in the migration guide. §3.6 puts it before 1.0
precisely because a behaviour change on a security-relevant gate is an ordinary increment pre-1.0 and
a compatibility event after it.

**The evidence base ages — measured, not hypothesised.** A probe sweep on 30 July 2026 found that
**nine of the fourteen slices originally drafted for Tiers 0 and 1 had already landed** in the eight
versions since the review, including the security slice. See §6.0. This is not a caveat; it is the
single most useful thing the sweep produced, and it took three minutes. Every slice proposal
re-checks its finding against the tree, and **T0.0 exists so that check is mechanical rather than
remembered.**

The corollary is the more uncomfortable half: over the same eight versions, registered `bynk.*` codes
in `bynk-emit` went from a counted 190 to a measured 200. The paydown fixes itself; the layering does
not.

**And the probes disagree with each other, which is worse than drift.** An independent re-measurement
on 31 July — **same commit, v0.245.0, nothing landed in between** — reproduced most of §6.0 exactly
and differed on four rows: `expr_children` (23/5 vs 34/8), `is_fully_drained` (6 vs 5),
`type_refs_match` and `http_value_serialiser` (both `0` vs comment residue), and `emit_diagnostics`
(206 vs 200 once the registry is consulted). None of it moves a tier or a claim. All of it says the
same thing: **a probe described in prose is ambiguous enough that two careful readings disagree.**
That is the argument for T0.0 in its strongest form, and it is why T0.0 ships an executable
definition and a committed table rather than a convention.

**The reference is a single 23,000-word file.** It matches the corpus convention
(`bynk-type-system.md` is 125KB, `bynk-design-notes.md` 239KB), but a reader looking for one rule
needs Appendix B or D as the index, not the body. If that proves painful, the split is by part and it
is cheap.

**Scope creep from the reference into the track.** §7 exists for this. A proposal citing a rule
outside Tiers 0–4 is out of scope by this doc, not by taste.

---

## 10. What "transformational" means here, honestly

Worth calibrating, because the tiers do not deliver the same thing.

**Tier A is paydown, and most of it has already happened** — see §6.0. Nine of the fourteen slices
originally drafted for phases 0 and 1 landed as ordinary work between v0.237.1 and v0.245.0,
including the security slice. What remains is the probe harness, the lint table, fixture adoption, conformance
totality, the residual filesystem reads, and three verification items. The review's verdict stands
and is now nearly discharged: "**Nothing here suggests remediation.** This is paydown, and most of
the highest-value items are small."

**This makes Tier B the track's centre of gravity, not its climax.** Four defect classes closed by one
signature change, including a miscompile of ordinary code with no diagnostic. Still not
transformational — a correctness change with a small ergonomic dividend — but it is now most of what
this track is *for*.

**Tiers 3–4 are where "transformational" lives**, and they are also where the cost is. Node identity
and totality are what make the editor path and the batch path one program; layering is what makes the
crate names true and therefore what makes the codebase reviewable by someone who did not write it.
Both are gated in §3.4 and §6 precisely because their value is real and their cost is the kind that
gets underestimated at the moment of most enthusiasm.

The honest summary: **Tiers 0–2 are worth doing on their own merits, now, regardless of what follows.
Tiers 3–4 are worth doing if the triggers fire, and the triggers exist so that "later" is a decision
rather than a drift.**

---

## 11. ADRs to land up front

Per ADR 0167 step 2 — the load-bearing, hard-to-reverse decisions, landed before slicing. Numbers are
assigned at merge by the stamp (ADR 0206), so these are drafted as `## ADR:` blocks in
`design/pending/`.

**In the settling increment** (`design/pending/compiler-architecture-settle.md`, `level: patch`, no
code):

- **ADR-B — The refactor acceptance gate, amending ADR 0059 property 1.** §3.3. Must land before
  Tier A opens.
- **ADR-C — The emit ABI publication posture.** §3.2. Arguably belongs to the packaging track;
  filed here because nobody else has claimed it, and it freezes by default if unclaimed.
- **ADR-D — The lowering substrate.** The record R0.1 says should have existed from the start: we
  lower to text on purpose, here is what it costs, here is Tier B's amendment, and here are the
  triggers (§3.4) that would open phase 3. This is the missing artefact the whole retrospective points
  at, and landing it is worth doing **even if no other slice ships**.

**In its own slice:**

- **ADR-A — `bynkc`-the-library's purpose.** §3.1, riding **T-D1**, because narrowing a published
  surface needs its own `level` and changelog row.

---

## 12. Retirement

The track retires when Tier B is complete and §3.4's trigger has been settled — whether or not Tiers
3 and 4 have opened. A track that stays open waiting for a gated tier is a track that never retires.
If a trigger fires later, that is a new track citing this one and the reference.

The retirement PR removes this doc, appends its closing summary to `../archive/retired-tracks.md`,
and closes the spine.
