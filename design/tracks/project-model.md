# The project model as a phase — migrating to the greenfield reference (phase 4)

- **Status:** **Draft — Settling.** Spine open, settling draft PR open
  ([#1108](https://github.com/accuser/bynk/pull/1108)). Nothing here is decided — every question in §3
  is open, each with the investigation it needs, not yet argued under review. Marking the PR ready for
  review asserts §3's six questions are closed; merging settles direction only, not build authorisation.
- **Spine:** [#1107](https://github.com/accuser/bynk/issues/1107)
- **Theme:** **Phase 4** of [`../bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md) —
  discovery, the unit graph, contract hashes and the schema registry live below both `bynk-check` and
  `bynk-emit`, in their own crate; `bynk-ide` repoints at it and drops its `bynk-emit` edge. The
  trajectory's endpoint is the current compiler rebuilt on
  [`../bynk-greenfield-compiler.md`](../bynk-greenfield-compiler.md); this track walks phase 4 of it.
- **Phase boundaries are safe stopping points** (trajectory §2). Phase 3 is retired
  ([#1046](https://github.com/accuser/bynk/issues/1046)) and leaves a coherent compiler regardless of
  whether this phase lands.
- **Relates:** reference rules R3.7, R3.8, R3.9, R3.11, R10.2; [ADR 0200](../decisions/0200-cross-context-contract-hash.md)
  (the only `ContractHash`-shaped type in the tree today — §3's Q4 asks whether it is this phase's
  business); [ADR 0147](../decisions/0147-structural-test-ness-and-flat-paths.md) (the flat-`include`
  manifest schema `Roots` still partially models the pre-0147 shape of, per R3.9);
  [`../archive/retired-tracks.md`](../archive/retired-tracks.md)'s `identity-and-totality.md` closing
  entry (the passage that names this phase as what phase 3's retirement opens).
- **Precedent:** `identity-and-totality.md` (retired) is this track's direct predecessor on the same
  trajectory. Its own closing summary states the handoff explicitly: "What this track opens: phase 4
  (the project model as its own crate, `bynk-project`) … entry-gated on this track's own probe
  (`span_keyed_maps`) reading zero — met, modulo the stated `Ctx::pattern_binding_types` exclusion."
  `project-model.md` is the ninth track to run the ADR 0167 flow from the start, after
  `compiler-architecture.md` (sixth), `identity-and-totality.md` (seventh) and `content-ownership.md`
  (eighth) — the third in a row on the internal-architecture theme rather than language surface.

### ADR 0076 trigger check

| Trigger | Met? |
|---|---|
| Spans several increments | **Yes, if narrowly** — trajectory §5 rates this phase relative size 3 (medium confidence), the smallest attempted since phase 1; §6 below names six candidate slices, none detailed to signature level pending §3 |
| Surface not yet settled | **No** — the destination (reference §3.2, R3.7–R3.11) is specified; what's open is which parts of today's ~22k-line `bynk-emit/src/project.rs` module tree are "the project model" versus orchestration that legitimately stays put, which §3 exists to close |
| Security/safety boundary | **No** — this phase moves data-shape and crate-boundary code; it touches no capability gate the way phases 0–2's `ResolvedCommons` did |

**One of three, same count as `compiler-architecture.md` and `identity-and-totality.md` before it, same
argument.** The warrant is the tracks README's own: §3's questions govern six candidate slices and
constrain what phase 5 inherits (§2 below — moving `validate.rs` before this phase settles means moving
it twice), and that governance needs a home that is neither a proposal nor the trajectory document
itself.

---

## 1. Why now

**Phase 3's probe reads at its documented floor.** `span_keyed_maps` (`HashMap<Span` below the driver)
reads **3** as of this draft — confirmed live (`cargo xtask greenfield-status`, 6 August 2026), all three
`Ctx::pattern_binding_types` in `bynk-check/src/checker.rs`/`linearity.rs`. `identity-and-totality.md`'s
own retirement recorded this exact residue as a **deliberate exclusion** (the `PatId`/`ExprId` split the
reference draws on purpose), not open work, and stated in the same breath that phase 4's entry condition
is "met, modulo" it.

**The reference's own appendix already measures this phase's distance, and it hasn't moved since the
review.** `bynk-greenfield-compiler.md`'s Appendix D:

| Rule | Reads | Distance |
|---|---|---|
| R3.7 | no `bynk-project` crate | medium |
| R3.8 | `read_project_paths` still total | small |
| R3.9 | `Roots` still models the removed role split | small |
| R3.11 | schema registry read/written ambiently | small — thread two values |
| R10.2 | `bynk-ide` → `bynk-emit` edge present, for `analyse_project` alone | medium — extract `bynk-project`, repoint |

Confirmed live: `bynk-ide/Cargo.toml`'s own comment states the dependency's whole reason — `"analyse_project
(the non-bailing project analysis) lives in the project driver"` — and `bynk-project` does not exist as a
crate (`ls bynk-project` — absent; not a workspace member).

**The causal chain the trajectory names is visible in the code, not just asserted.** R3.5's rationale
(phase 5) reads: "context-level semantics were written in `project.rs` because that is where the project
model was, and the project model was in the emitter because emission needed it first. … Extracting
`bynk-project` as its own phase *below* both check and emit is what makes phase 5 the only place context
checking can live." `bynk-emit/src/project.rs`'s own top-of-file doc confirms the entanglement it names:
discovery, resolution, checking and emission are one two-pass pipeline by design ("Resolve, type-check,
and emit each unit" — one pass), and the file imports `crate::emitter` and calls three `validate::`
functions directly at its own top level. Phase 4 is the phase that has to pull the *model* out of that
pipeline without also, incidentally, dragging the *checking* out with it — that second move is phase 5's,
named already so it isn't repeated.

**R3.11's "ambiently" reads as already half-closed, and the live half tells this track where the seam
already is.** `schema_registry.rs`'s own doc states "this module touches no disk" — `parse`/`serialize`
are pure, threaded explicitly through `CompileOptions::schema_registry` (an input) and
`ProjectOutput::schema_lock` (an output), with the actual read/write already relocated above `bynk-emit`
into `bynk-driver/src/schema_lock.rs`. That is R3.11-shaped work already done for the **CLI** path. What
hasn't happened — visible in `bynk-driver/src/discovery.rs`'s own doc comment, quoted in full at §3's Q3
below — is the same threading for the **LSP/analysis** path, which is also exactly why `bynk-ide` still
needs `bynk-emit`'s `analyse_project_with` directly. R3.7's "two project models" and R3.11's "ambient"
verdict may be the same defect read through two different rules, which §3 needs to settle before slicing
assumes they're independent.

---

## 2. What this track is not

- **Not phase 5's semantics extraction.** `validate.rs` (5,019 lines, the context-level checks R3.5
  moves to `bynk-check`) is untouched here. Moving it before `bynk-project` exists would mean moving it
  twice — the trajectory's own "4 before 5" ordering argument (§4 of the trajectory doc).
- **Not the fixture-format work `tests_emit.rs` (5,922 lines) implies.** Its residence and shape are
  phase 0/seams territory (broad fixture-kind adoption), not this phase's.
- **Not necessarily the full `R3.2` `ProjectGraph { units, files, edges, contract }` struct on day one.**
  §3's Q1 is where this track decides how much of that destination shape it commits to versus extracts
  as today's name-keyed, `HashMap`-based logic with the typed-index upgrade deferred — the same
  scaffolding-versus-retrofit question `identity-and-totality.md`'s Q1 answered for phase 3, asked fresh
  here because the trajectory's own sizing ("cheap … one function's worth of work") reads like the
  narrow answer, and the reference's struct sketch reads like the full one.
- **Not a manifest schema change.** R3.8/R3.9 ask for a typed, `Result`-returning reader and a `Roots`
  that matches ADR 0147's flat `include` list — not a new `bynk.toml` shape.
- **Not phases 6–8.** Carried as forward references in §7, not refusals, per the discipline
  `compiler-architecture.md` §7 and `identity-and-totality.md` §7 both used for phases not yet open.

---

## 3. Design questions — open

None of the six below has been argued under a settling review yet. Each states the investigation already
done and the tension it leaves; the settling draft PR closes them, the way `identity-and-totality.md` §3
closed its own seven (one of them reversed under review, six confirmed as drafted).

### 3.1 Q1 — Full `ProjectGraph` now, or extract today's shape and defer the typed-index upgrade?

The reference (`bynk-greenfield-compiler.md` §3.2) specifies the destination struct:

```rust
struct ProjectGraph {
    units:    IndexVec<UnitId, Unit>,
    files:    IndexVec<FileId, UnitId>,
    edges:    Vec<(UnitId, UnitId, EdgeKind)>,
    contract: IndexVec<UnitId, ContractHash>,
}
```

No `UnitId`, `ProjectGraph`, or `IndexVec`-keyed graph exists anywhere in the tree today (`grep -rn
"ProjectGraph\|UnitGraph\|struct Unit\b"` — zero hits outside this spec). What exists is
`bynk-emit/src/project/graph.rs`'s `detect_consumes_cycles`, keyed by `HashMap<(String, String), …>` and
plain unit-name strings, plus `symbols.rs`'s `ProjectIndex` (a separate, LSP-facing index also keyed by
name). Building the reference's typed graph from scratch is real design work, not extraction; extracting
the existing name-keyed logic into a new crate unchanged is comparatively mechanical.

The trajectory's own sizing text argues for the narrow reading: relative size 3 (versus phase 3's 8),
"Cheap, and it is the *cause* the next phase treats," and the bynk-ide repoint named as "one function's
worth of work." Nothing in that text claims the `IndexVec<UnitId, …>` upgrade ships in this phase.

**Needs settling:** does this track's probe (`ide_emit_edge` absent) require the typed `ProjectGraph`, or
only a `bynk-project` crate holding today's logic with `bynk-ide` repointed at it — leaving the `UnitId`
upgrade a named forward reference the way phase 3 named `ExprId`-at-parse before committing to it?

### 3.2 Q2 — Module boundary: what actually moves, and what does `bynk-project` export?

`bynk-emit/src/project.rs` and its eight submodules total 21,985 lines. Excluding `validate.rs` (5,019,
phase 5's) and `tests_emit.rs` (5,922, phase 0's), the candidate surface is `project.rs` itself (6,369),
`discovery.rs` (446), `graph.rs` (325), `paths.rs` (634), `schema_registry.rs` (908), `consistency.rs`
(178), `symbols.rs` (1,472) and `diagnostics.rs` (202) — roughly 10.5k lines. But `project.rs` is not a
clean project-model file: it imports `crate::emitter`, calls `validate::` directly, and is the hub every
submodule and `validate.rs` alike reach through via `use super::*` — there is no existing internal
boundary between "project-model fact" and "orchestration that also drives checking and emission."

Two sub-questions this needs to answer together:

1. Which of `discovery.rs`/`graph.rs`/`paths.rs`/`schema_registry.rs`/`consistency.rs`/`symbols.rs` move
   whole, which need splitting (project.rs's orchestration functions — `compile_project`, `run_checks` —
   almost certainly don't move at all), and does `symbols.rs`'s `ProjectIndex` belong in `bynk-project` or
   stay an LSP-facing concern in `bynk-ide`/`bynk-lsp`?
2. What does `bynk-project` actually export? A blanket `pub use project_model::*` re-acquired by
   `bynk-check` and `bynk-emit` would reproduce, one layer down, the exact defect R10.4 already closed in
   `bynk-emit` (#41/#42 — "of thirty-eight world-reachable items … exactly five have an external user").

**Needs settling:** the module list per crate, and whether `bynk-project`'s public surface gets its own
enumerated-and-reviewed pass (R10.4) as part of this track or as a fast-follow.

### 3.3 Q3 — The two discovery paths R3.7 was measured against

`bynk-driver/src/discovery.rs`'s own doc comment names this directly, unprompted, as still live:

> "`bynk-emit`'s own on-disk discovery (`project::discover_bynk_files`) and `read_source`'s overlay-miss
> fallback are **not** removed, and can't be yet: `analyse_project_with` (the LSP's own analysis path,
> `bynk-emit/src/project.rs`) hardcodes `discovered: None` and passes an overlay that deliberately covers
> only open editor buffers — it depends on that exact fallback to see every other project file … Closing
> `discovery.rs` for good needs all of that migrated first, which is bigger than either #1077 or #1079's
> issue text currently describes."

So today there are, in effect, two discovery implementations: the CLI's (`bynk-driver::discovery`, walking
above `bynk-emit` since #1077, feeding `CompileOptions.sources`) and the LSP/IDE's (`analyse_project_with`
inside `bynk-emit`, still reading through the overlay-miss fallback the content-ownership retirement named
as one of `bynk-emit`'s three permanent, documented `fs_below_driver` exceptions). R3.7's rule — "a unit's
identity is its declared qualified name … a file contributes to exactly one unit" — reads as a claim about
one discovery process, not two that could in principle disagree.

**Needs settling:** does phase 4 unify on one discovery implementation both the CLI and the LSP call
through `bynk-project`, or does the extraction carry today's two-path split into the new crate unchanged,
leaving the unification a named forward reference? If unification is in scope, this is very likely the
same root cause as Q5 below (the LSP path never threads a `SchemaLock` value either), which would fold
into one slice rather than two.

### 3.4 Q4 — Where does `ProjectGraph.contract: IndexVec<UnitId, ContractHash>` actually come from?

The only `ContractHash`-shaped type anywhere in the tree is `bynk-check/src/contract.rs` — the ADR 0200
cross-context wire-contract hash, stamped beside `X-Bynk-Caller` so a `deploy` can fail closed on skew.
Its own doc states the precondition plainly: it canonicalises **resolved** types, reached through
`combined_types_for`/`CrossContextService`, both checker outputs. It is necessarily downstream of
type-checking — the opposite side of the dependency the reference's phase-4 line item asks for
("Discovery, the unit graph, contract hashes and the schema registry live below both check and emit").

Three readings, none yet tested:

1. `ProjectGraph.contract` is the *same* ADR 0200 hash, and the reference's "below both check and emit"
   framing is approximate — the field is populated late, after checking, even though the struct itself is
   defined in the project-model layer.
2. It's a genuinely different, pre-resolution structural hash (declared shape only, no type resolution)
   that phase 4 has to design from nothing.
3. Per the trajectory's own phase-8 aside — "ADR 0200's contract hash is already `UnitSignature`'s
   identity function pointed at a different problem" — this field is actually phase 8's `UnitSignature`
   work, and the reference's struct sketch names it a phase early.

**Needs settling** before `contract` is built, stubbed, or deferred in §6's slicing; reading 3 would mean
this field is out of scope for this track entirely, which changes what "the project model" means for
phase 4's purposes.

### 3.5 Q5 — Is R3.11's remaining distance the same defect as Q3?

§1 already found the CLI path's schema-registry threading (`CompileOptions::schema_registry` →
`bynk-driver/src/schema_lock.rs`) R3.11-shaped and apparently complete. `analyse_project_with` — the LSP
path — takes no `SchemaLock` parameter at all, per its signature (`analyse_project_with(roots: &Roots,
overlay: &HashMap<PathBuf, String>) -> ProjectAnalysis`, `bynk-emit/src/project.rs:970`).

**Needs settling:** confirm (or refute) that R3.11's "ambiently" verdict is scoped to the analysis path
only, and if so, whether closing it rides the same slice as Q3's discovery unification (same caller, same
missing parameter) rather than being counted as separate work.

### 3.6 Q6 — Freeze scope

`identity-and-totality.md` §3.5 froze one contended file (`checker.rs`) for the duration of its two
scaffolding slices. This phase's candidate surface (Q2) is smaller in slice count but the hub file
(`project.rs`, 6,369 lines) is reached by every submodule and by `validate.rs` via `use super::*` — a
wider blast radius on one file than phase 3's freeze covered, if the whole hub is in motion at once.

**Needs settling:** whether a scoped freeze on `project.rs` (or on whichever of its functions Q2 decides
move) is warranted for the extraction's duration, and how that interacts with phase 5 work, if any,
starting before this track retires (trajectory §2 permits phases to be abandoned, not to interleave
mid-slice — this question is about freeze scope, not about starting phase 5 early).

---

## 4. Posture

Extends ADR 0059 as amended by ADR 0309 and by `identity-and-totality.md`'s pending LSP-surface-fixture
amendment (§3.4 there), the same standing properties every track on this trajectory has run under:
behaviour-preserving by default (a slice that changes observable behaviour states so, ships a fixture,
gets a CHANGELOG entry); the acceptance gate per whichever tier §3 settles on; short-lived branches, one
PR per slice; no per-increment ADRs beyond what §11 front-loads; every slice citing `Closes-Rule:`.

---

## 5. The completion criterion

Same principle as every prior track on this trajectory: a slice is complete when the old path is
**deleted**, not when the new crate merely exists alongside it. Here: `bynk-ide/Cargo.toml`'s `bynk-emit`
dependency line — and the comment justifying it — are gone, `bynk-project` is a real workspace member, and
the manifest-level check R10.2 asks for (a CI gate on the dependency graph, not just a probe reading zero
once) exists.

---

## 6. Slice decomposition — forward-referenced only

§3 is unsettled, so nothing below is detailed to signature level, per the same discipline
`identity-and-totality.md`'s §3.7 (Q7) used for its own undetailed slices: cutting slices before the
questions that shape them are closed is "an unopened phase whose slices are already written," which
`compiler-architecture.md` §7 named as a wish list, not a plan.

| Slice | What it names | Rules | Gated on |
|---|---|---|---|
| **P4.0** | `bynk-project` crate skeleton; today's discovery/graph/paths/schema-registry/consistency logic moved in with minimal reshaping | R3.7, R3.8, R3.9 | Q1, Q2 |
| **P4.1** | `symbols.rs`'s `ProjectIndex` assembly relocated or left, per Q2 | — | Q2 |
| **P4.2** | Unify the CLI and LSP discovery paths on one implementation inside `bynk-project`, if Q3 decides that's in scope | R3.7 | Q3 |
| **P4.3** | Thread `SchemaLock` through the analysis path, if Q5 finds it's separate from P4.2 | R3.11 | Q5 |
| **P4.4** | `ProjectGraph.contract` — designed, stubbed, or explicitly deferred to phase 8, per Q4's reading | R3.2 (partial) | Q4 |
| **P4.5** | `bynk-ide` repoints at `bynk-project`; `bynk-emit` dependency and its justifying comment deleted; CI dependency-graph gate added | R10.2 | P4.0–P4.4 |

**Completion probe:** `ide_emit_edge` = absent. Already built (T0.0's probe harness, `xtask
greenfield-status`) and CI-gated (`greenfield_status_table_is_current`); reads **present** as of this
draft.

---

## 7. Out of scope — forward references, not refusals

Carried forward from `identity-and-totality.md` §7's rows for phases 5–8, since this track now owns
phase 4 directly:

| Item | Phase | Entry condition |
|---|---|---|
| Semantics in the checker (`validate.rs` dissolves) | 5 | this track's probe (`ide_emit_edge`) reads absent |
| The full IR (reference Part 6) | 6 | phase 5 complete |
| The TypeScript tree and printer (reference Part 7) | 7 | phase 6 complete |
| Incrementality (query granularity, the firewall; needs a real `UnitSignature`, which Q4's reading 3 would place here) | 8 | phases 3 and 4 complete |
| Publishing the emit ABI: the `@bynk/*` package, the three ADR 0200 layers, the build-time enumeration guard | *packaging track* | ADR 0310 merged; not gated on this track's phases |

---

## 8. Keeping the reference true

No new probe infrastructure is needed for this phase's gate — `ide_emit_edge` already exists, already
runs in CI (`greenfield_status_table_is_current`), and already reads the value this track needs to flip.
One addition worth raising under review, not yet decided: a probe for `bynk-project` crate existence
itself (today: a `find`/manifest-membership check would do; not yet wired as a gated row).

---

## 9. Risks

**Two of six open questions (Q3, Q5) may collapse into one slice, or may not.** Treating them as
independent when they share a root cause would double-count size; treating a genuinely separate concern
as one slice would under-scope it. §3 needs to settle this before §6's slice count is trusted.

**Q4 is the one most likely to surprise.** The reference names "contract hashes" as this phase's, but the
only hash-shaped type in the tree is downstream of checking by construction (ADR 0200), and the
trajectory's own words about phase 8 read like a direct admission that the reference's struct sketch may
have misplaced this field. If reading 3 (§3.4) is correct, this phase's rule coverage (R3.2, partial) is
smaller than the reference's struct literally suggests, and phase 8's scope is correspondingly larger than
trajectory §5 currently prices it at.

**`project.rs` is the hub, not a leaf.** Q2's finding — `use crate::emitter`, direct `validate::` calls,
`use super::*` reached by every sibling and by `validate.rs` itself — means this is not a
move-the-files exercise. The trajectory's "cheap" sizing may hold for the `bynk-ide` repoint (§6's P4.5)
while understating P4.0–P4.2's actual design cost, the same shape of miscalibration `identity-and-totality.md`
§9 found in the review's "three consumer crates" undercount for its own phase.

**The evidence ages.** Every number and quotation in this doc was measured against the tree on 6 August
2026. Re-check before a slice proposal cites one, per every prior track's own §9.

---

## 10. What this phase causes

In the trajectory's own words: "Cheap, and it is the *cause* the next phase treats. Context semantics
live in the emitter because the project model does." This phase's value is not chiefly its own rule
closures (R3.7–R3.9, R3.11, R10.2 are all rated small-to-medium distance) — it's that phase 5 cannot start
cleanly without it. Moving `validate.rs` while the project model it depends on still lives in `bynk-emit`
would mean moving context-level semantics twice: once to follow the project model out, once for real.

---

## 11. ADR candidates — not yet decided

Per ADR 0167 step 2, load-bearing, hard-to-reverse calls land before slicing. None of the three below is
settled; each depends on a §3 question closing first, and none should be numbered until the settling PR
that closes its question is ready to merge.

- **Candidate — the phase-4 scope commitment (full `ProjectGraph` vs. extraction-with-deferred-typing).**
  §3.1 (Q1).
- **Candidate — `bynk-project`'s enumerated public surface, extending R10.4 one layer down.** §3.2 (Q2).
- **Candidate — unifying CLI and LSP discovery onto one implementation.** §3.3 (Q3). Load-bearing and
  hard to reverse: `analyse_project_with` has 100+ call sites across `bynk-ide`'s inline tests,
  `bynk-lsp/tests` and `bynkc/tests` (per `bynk-driver/src/discovery.rs`'s own count), so whichever
  discovery shape this settles on is expensive to revisit later.

---

## 12. Retirement

Mirrors every prior track on this trajectory: retires when §6's probe (`ide_emit_edge`) reads absent and
every slice named to reach it has landed. The retirement PR removes this doc, appends its closing summary
to `../archive/retired-tracks.md`, and closes the spine issue once one exists.
