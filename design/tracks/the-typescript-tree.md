# The TypeScript tree and printer — migrating to the greenfield reference (phase 7)

- **Status:** **Draft — Settling.** Spine open. This file is drafted for a settling draft PR
  (tracks/README.md step 2). Nothing here is decided — every question in §3 is open, each with
  the investigation it needs, not yet argued under review. Marking that PR ready for review
  asserts §3's questions are closed.
- **Spine:** [#1293](https://github.com/accuser/bynk/issues/1293).
- **Theme:** **Phase 7** of [`../bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md) —
  emission produces nodes; one printer writes every character. `bynk-ts` holds the TypeScript
  tree, the printer and the source map. The tree omits `enum`, `namespace`, decorators,
  constructor parameter properties and `TsType::Any`; `Artefacts` becomes a keyed set of typed
  documents. The trajectory's endpoint is the current compiler rebuilt on
  [`../bynk-greenfield-compiler.md`](../bynk-greenfield-compiler.md); this track walks phase 7 of
  it.
- **Phase boundaries are safe stopping points** (trajectory §2). Phase 6 is retired
  ([#1137](https://github.com/accuser/bynk/issues/1137), retired 19 August 2026) and leaves a
  coherent compiler regardless of whether this phase lands.
- **Relates:** reference rules R7.1–R7.8 in full (`bynk-greenfield-compiler.md` Part 7, lines
  1207–1289), plus R8.1 (totality over `IrItem`, no wildcard arm) and the R8 rules governing how
  output is *constructed* — R8.19 (typed string escaping), R8.20 (typed deploy placeholders),
  R8.21 (uniform `async`), R8.22 (no `undefined` in generated JSON). The remaining R8 rules
  (R8.2–R8.18: brand strings, `unsafe` as a `TypeShape` field, numeric refinement ordering,
  commons re-branding, commit shapes, `loadState` merge, invariant/transition ordering, factory
  helpers, handler key mangling, `deps` derivation, the internal-door wrapper, boundary
  verification, codec collection and dispatch, the compose root, the sorted route table, contract
  hash validation) are chiefly emission-*semantics*, much of it plausibly already settled by phase
  6's `IrItem`/`Callee` work — **see Q4: re-verified still true, not rebuilt, unless found open.**
- **Precedent:** `the-ir.md` (retired) is this track's direct predecessor on the same trajectory.
  Its own §7 ("Forward references") named this phase's entry condition explicitly, and amended it
  mid-flight (P6.58): not `ast_importers` reading 0, but reading its own re-settled floor, 5 —
  `bynk-emit/src/emitter.rs`, `emitter/emit.rs`, `emitter/lower.rs`, `emitter/workers.rs`,
  `emitter/workers_entry.rs` — because the renderer family living in those five files (`ts_base`,
  `ts_type_ref*`, `ty_to_type_ref`, `pred_condition_and_message`) is *this* track's own future
  surface, not phase 6's unfinished work. `the-typescript-tree.md` would be the thirteenth track
  to run the ADR 0167 flow from the start, after `compiler-architecture.md` (sixth),
  `identity-and-totality.md` (seventh), `content-ownership.md` (eighth), `project-model.md`
  (ninth), `semantics-in-the-checker.md` (tenth)... `the-ir.md` (retired) was the eleventh, so
  this is the twelfth.

### ADR 0076 trigger check

| Trigger | Met? |
|---|---|
| Spans several increments | **Yes, decisively.** Trajectory §5 rates this phase relative size 15, confidence *low* — against phase 6, sized 20 and delivered in 59 slices, against a smaller emitter surface than this phase inherits. §1 below measures phase 7's own probe for the first time (it was "not measured" at §3.0's baseline) at ~1,540 TypeScript-producing sites across 21,742 lines, 52% of `bynk-emit`. The size question is one of §3's open items (Q5), but "several increments" is not in doubt at any plausible answer |
| Surface not yet settled | **No** — the destination (R7.1–R7.8: tree shape, printer ownership, source-map production, `Artefacts` as typed documents) is fully specified in the reference. What's open is the migration *method* — whether an escape hatch exists, where it lives, what forces it to a floor (Q2), and whether the crate is carved before or after conversion begins (Q1) |
| Security/safety boundary | **No** — this phase's motivating defects (source-map corruption via unaccounted offsets, finding #4; `wrangler.toml` coupling by substring match, R7.6's own rationale) are correctness and maintainability failures, not security gates. `TsType::Any` elimination narrows a real gap (`tsc --strict` disarmed by 48 `as any` casts, R7.1's own rationale, finding #18) but this is a type-safety improvement, not a new authorization or capability boundary |

**One of three**, same count as every internal-architecture track on this trajectory before it
(`compiler-architecture.md`, `identity-and-totality.md`, `content-ownership.md`,
`project-model.md`, `semantics-in-the-checker.md`, `the-ir.md`). The warrant is the tracks
README's own: §3's questions govern the migration method — the crate-carve sequencing, the escape
hatch's shape and its forcing function, and R8's real boundary — none of which the trajectory
document or the reference spec settles on their own.

---

## 1. Why now

**Phase 6's own closing note opens this phase by name and amends its own entry condition to do
so.** `the-ir.md`'s §7 recorded `bynk-ts` as a forward reference gated on `ast_importers` reading
0; P6.58 (`b22752a1`) re-settled that to "reads its own named floor" once the track's research
found the renderer family in `emitter.rs`/`emit.rs`/`lower.rs`/`workers.rs`/`workers_entry.rs`
cannot leave `bynk-emit` until `bynk-ts` exists to receive it — a circularity the 0-target could
not resolve. The floor, 5, is where phase 6 retired (`79053b09`); it is where this phase begins.

**Phase 7's own probe was never measured.** §3.0's baseline table (30 July 2026) records "TypeScript-producing `write!`/`format!` outside `bynk-ts` = not measured". Measured now, against the
working tree (19 August 2026):

| Reading | Value |
|---|---|
| `write!`/`writeln!`/`format!` in `bynk-emit` | **1,709** total — `format!` 853, `writeln!` 845, `write!` 11 |
| …of which genuinely TypeScript-producing | **~1,540** (excludes 130 test-assertion sites in `project/tests_emit.rs`, 32 TOML sites in `emitter/wrangler.rs`, 5 diagnostic sites in `ir/lower.rs`) |
| By file (top): `emitter/emit.rs` 409 · `emitter/lower.rs` 372 · `emitter/serialisation.rs` 237 · `emitter/workers_entry.rs` 192 · `emitter/workers.rs` 137 · `emitter.rs` 99 · `project.rs` 44 · `emitter/events_fanout.rs` 41 | |
| Emitter subtree (`emitter.rs` + `emitter/**`) | **21,742 LOC**, 52% of `bynk-emit`'s 41,953-line total |
| `as any` casts in `bynk-emit/src` | **48**, clustered in `emitter/workers.rs` (params/payload wrappers, e.g. `:690,709,747,931,1004,1041,1131,1151`) and `emitter/serialisation.rs` (codec casts, `:314,317,802,941,1026`) |
| `TsType`/`TsProgram`/`Artefacts` Rust types | **none exist** — today's carrier is `pub struct CompiledFile { .. output_path: PathBuf .. }`, `bynk-emit/src/project.rs:97` |
| Other crates producing TypeScript text | **0** (one false positive on inspection: `bynk-fmt/src/fmt.rs:2953` formats *Bynk* lambdas, not TypeScript) |
| `bynk-ts` crate | **does not exist** — 17 workspace members today |

`design/bynk-greenfield-compiler.md` §15.2 prices the printer itself at "2,000–3,000 lines written
before the first emitted character" — a cost phase 6 never had to pay, since `Ir` is a value type,
not a writer. Phase 6 was sized 20 (relative) and took 59 slices against a smaller, better-scoped
surface (the IR's *shape*, not its rendering). **The trajectory's own sizing of phase 7 at 15 is
not credible against these numbers** (Q5).

**Two defects this phase closes are live in shipped code, not hypothetical.** R7.6's own
rationale — "downstream consumers couple to nodes, never to emitted text" — is not abstract:
`emitter/wrangler.rs:34` defines `KV_NAMESPACE_ID_PLACEHOLDER = "<KV_NAMESPACE_ID>"`, and
`bynk/src/deploy/config.rs:195,260,263` and `bynk/src/deploy/ledger.rs:365` text-match and
`.replace()` against it; `bynk-strip/src/lib.rs:136-139` does the same for
`main = "index.ts"` → `main = "index.js"` — "reformat that line's spacing and `--emit js`
silently produces a `wrangler.toml` pointing at a file that no longer exists" is not a
hypothetical failure mode, it is the shape of the code today. R7.4's rationale — wrong, not
missing, source-map offsets — is finding #4 (Medium, confirmed): `record_span(out.len(), …)`
in `emitter/source_map.rs` "has no idea which buffer `out` is," so IIFE-local offsets corrupt the
map.

**One finding appears to have aged out already.** Finding #17 (review, `#17`) reported ~300 lines
of harness TypeScript as Rust string literals beside an `include_str!` of `runtime.ts`. In the
current tree, `emitter.rs:94` (`include_str!("emitter/runtime.ts")`) and five sites in
`project/tests_emit.rs` (`:1894,1902,2748,2752,2764`, each `include_str!`-ing a file under
`emitter/test_runtime/`) already load real `.ts` files, and `bynk-emit/runtime/tsconfig.json`
type-checks `src/**` and `test/**` under `strict`. R7.7 may be substantially closed already —
**verify before scheduling a slice for it** (§6 candidate P7.1 is a verification pass, not
construction, pending this).

**The "`tsc --strict` disarmed at two places" is the `as any` casts, not config.** Emitted
`tsconfig.json` sets `strict: true`/`noImplicitAny: true` (`emitter.rs:141`,
`bynk-emit/runtime/tsconfig.json:10`); there are zero `@ts-ignore`/`@ts-nocheck` anywhere in the
repo. `emitter/workers.rs:577` documents keeping `: any` as *deliberate* policy on several of the
48 sites — this is the tension Q3 exists to resolve, not an oversight to sweep away casually.

---

## 2. What this track is not

- **Not phase 8 (incrementality).** Query granularity, `UnitSignature`, the firewall are
  untouched here — carried forward in §7, not built.
- **Not re-opening phase 6's IR shape.** `IrExpr`/`IrItem`/`Callee`/`CommitShape` are settled;
  this phase's business with the IR is reading it, not changing its shape — except where Q3 finds
  `TsType::Any` elimination genuinely needs a type the IR does not carry, in which case that
  specific gap is named and scoped narrowly, not treated as license to redesign the IR.
- **Not rebuilding R8's emission semantics wholesale.** R8.2–R8.18 are assumed closed by phase 6
  or by ordinary paydown until Q4's re-verification finds otherwise. This track's job is
  converting *how* R8's mappings are written (`Ir -> TsProgram -> Artefacts`, not
  `Ir -> String`), not re-deriving *what* they map to.
- **Not a full crate re-graph.** Only `bynk-ts` (and, if Q1 settles that way, `bynk-ir`/
  `bynk-lower`, whose absent second consumer was ADR 0332's stated reason for deferring them) are
  this phase's crate moves. R10.5 (command bodies to `bynk-driver`) and R10.6/R10.7 are untouched.
- **Not phases 6's own residue by name.** The five files at `ast_importers`'s floor are this
  phase's surface *because* phase 6 argued them there — this track inherits that argument, it
  does not re-litigate it.

---

## 3. Design questions — open

None of the five below has been argued under a settling review yet. Each states the investigation
already done and the tension it leaves.

### 3.1 Q1 — Is `bynk-ts` carved as a crate up front, or built as a module inside `bynk-emit` and carved later, the way `ir.rs` was in phase 6 (ADR 0332)?

Phase 6 built the IR as `bynk-emit/src/ir.rs`/`ir/lower.rs`, inside the existing crate, and
deferred the `bynk-ir`/`bynk-lower` split explicitly for want of a second consumer (R10.3's own
"carve prospectively when a dependency arrives that only one consumer needs" — there wasn't one).
The same argument does not obviously transfer to `bynk-ts`: R7.3's invariant ("the printer... is
the only code in the compiler that writes a character") is a boundary a `pub(crate)` module cannot
enforce on itself — finding #42 (33 of 38 world-reachable `bynk_emit::emitter` items are `pub`
only to reach a *sibling* module) is exactly the failure mode of "boundary enforced by convention,
not by the crate graph." Carving `bynk-ts` immediately also manufactures the second IR consumer
ADR 0332 was waiting for, which would let phase 6's own deferred crate split happen inside this
phase rather than needing a further trigger later.

**Needs settling:** carve `bynk-ts` in the first slice (Arc B, before any conversion begins), or
follow phase 6's own precedent of building in-crate and carving once the boundary has proven
itself under real use?

### 3.2 Q2 — Does the migration need an escape hatch, and what forces it to a named floor rather than open-ended use?

~1,540 sites cannot convert in a single slice without violating trajectory §2: "a phase that
half-lands leaves two paths reachable... the failure mode every regret in this corpus shares."
Converting all of them atomically is not credible either, given phase 6's own 59-slice experience
on a smaller surface. A statement-granularity, sealed-constructor node —
`TsStmt::Verbatim { origin: VerbatimOrigin, text: String }`, tagged by a closed `VerbatimOrigin`
enum with one variant per named residue family — is one candidate: the printer still owns the
buffer and the offset arithmetic for a `Verbatim` block, so R7.3/R7.4 hold from the slice that
introduces it, and the closed enum makes "how much residue is left" a compile-time, not a
grep-time, question. The risk this question exists to catch: an *expression*-level hatch would
compose invisibly inside nodes the tree claims cannot express `as any`/`enum`, defeating R7.1
while still reading as "done." A *document*-level hatch (`Document::TsText(String, SourceMap)`)
would be a second writer with no path back, which R7.8 explicitly rejects for exactly this reason
(`wrangler.toml` under the old `(String, SourceMap)` shape was "unsatisfiable" for R7.6, the
review that produced R7.8).

**Needs settling:** does a hatch exist at all, and if so at what granularity; what is its forcing
function (a floor named now, e.g. "`ts_writes` retires at 0 outside `bynk-ts`, `verbatim_origins`
retires at an argued floor, expected small" — the completion-criterion approach settled for this
track already, per §5); and is one closed enum variant per residue family the right shape, or too
coarse to prevent the ratchet stalling on one large, un-decomposed family?

### 3.3 Q3 — Is `TsType::Any` actually eliminable across all 48 current sites, or does it re-open phase 6?

`emitter/workers.rs:577` documents keeping `: any` as *deliberate* policy on several wrapper
sites. R7.1 forbids the variant outright — "the tree contains no... `TsType::Any`." Some of the 48
casts are plausibly narrowable with types the checked program already carries (parameter/payload
wrappers where the real shape is known but was cast away for convenience); others may need a type
the IR does not yet construct (e.g. a genuinely dynamic dispatch site where the real type is
decided at runtime by a discriminant the IR doesn't preserve as a static alternative). If the
latter exist, closing R7.1 fully would mean extending the IR — reaching back into phase 6's
settled scope, which trajectory §6's own review frame (question 5, "does it foreclose a later
phase") and this track's own §2 both flag as needing to be argued explicitly, not discovered
mid-slice.

**Needs settling:** a classification of all 48 sites (a spike, not a design argument) into
"narrowable with existing IR types" versus "needs an IR extension," before any slice touching
`emitter/workers.rs` or `emitter/serialisation.rs` is scheduled. **This is the one finding in this
track's own research that could justify not opening large conversion work yet** — it does not
block opening the track itself, since Arc A's independent slices (§6) do not depend on its
answer.

### 3.4 Q4 — How much of R8.1–R8.22 is this track's to close, versus already closed by phase 6 or genuinely belonging elsewhere?

The trajectory names both R7.1–R7.8 and R8.1–R8.22 as phase 7's reference rules
(`bynk-compiler-trajectory.md` §3, "Reference rules: R6.1, R6.5, R6.7–R6.16, R5.1–R5.11" for phase
6 by contrast shows the pattern of a phase claiming rules a prior phase partly pre-settles). R8 is
chiefly emission-*semantics* (commit shapes R8.6, codec dispatch R8.14/R8.15, the compose root
R8.16, the sorted route table R8.17, contract-hash validation R8.18) — work that plausibly moved
substantially once `IrItem`/`Callee`/`CommitShape` existed as real types in phase 6, rather than
waiting for phase 7's tree/printer split. Treating all 22 R8 rules as open by inheritance, the way
the trajectory's own phase-7 row reads literally, risks the track claiming — and being measured
against — work it does not actually need to do.

**Needs settling, rule by rule against the current tree** (the same discipline phase 5's own §1
applied to `emit_diagnostics`, and phase 6's own spine issue applied before proposing a
decomposition): which of R8.2–R8.18 already read closed; which are this track's business because
they concern *how* output is constructed (already carried in this doc's front matter as R8.19–
R8.22, tentatively); and whether any found genuinely open but out of this track's natural scope
should be named as a forward reference (§7) instead of absorbed.

### 3.5 Q5 — A fresh slice count, not the trajectory's stale "15, low confidence" sizing.

§1 measures ~1,540 TypeScript-producing sites across 21,742 lines — a larger, less-decomposed
surface than phase 6 attacked at a relative size of 20 (59 slices). The trajectory's phase-7 row
predates any real measurement of this phase's own probe. Following the same "recount before
proposing a decomposition" discipline the-ir.md's own spine issue (#1137, Q5) used for itself:

**Needs settling:** a slice budget stated with its own argument (§6 sketches 35–45 as a working
estimate — Arc A ~4, Arc B ~5, Arc C ~20, Arc D ~8 — but this is provisional pending Q1–Q4, since
each answer changes Arc B's and Arc C's real shape), replacing the trajectory's own "15, low"
figure once this settles.

---

## 4. Posture

Extends ADR 0059 as amended by ADR 0309, the same standing properties every track on this
trajectory has run under: behaviour-preserving by default (a slice that changes observable
behaviour states so, ships a fixture, gets a CHANGELOG entry — Arc C's whole premise depends on
this holding, since byte-identical output is its only safety net); short-lived branches, one PR
per slice; no per-increment ADRs beyond what §11 front-loads; every slice citing `Closes-Rule:`.

---

## 5. The completion criterion

As settled already (not one of §3's open items): **the floor is named at settling, not discovered
mid-track the way phase 6's P6.58 had to.** Two probes, not one, following Q2's `Verbatim`
proposal:

- **`ts_writes`** — TypeScript-producing `write!`/`writeln!`/`format!` outside `bynk-ts`. Retires
  at **0**. This is the trajectory's own literally-stated probe and is not negotiable by this
  track.
- **`verbatim_origins`** (new, contingent on Q2) — count of distinct `VerbatimOrigin` enum
  variants still constructed. Retires at an **argued floor**, named file-by-file in the retirement
  summary the way `ast_importers`'s floor of 5 was — expected small (1–3) based on §1's file
  list, but not asserted here before the conversion work that would justify a number.

Both probes need adding to `xtask/src/greenfield_status.rs` (13 probes exist today,
`ast_importers` at `:1485` and `hoist_sinks` at `:1203` are the closest precedents for the
line-scan implementation) — candidate slice P7.0 (§6).

---

## 6. Slice decomposition — candidate, pending §3

None of the below is accepted yet; §3 governs both which of these actually ship and their order.
Arc A does not depend on Q1–Q3's answers and could begin immediately once this track settles; Arc
B and Arc C's real shape depends on Q1 (crate timing) and Q2 (hatch shape).

**Arc A — independent of the tree, land immediately**

| Slice (candidate) | What it does | Rules | Gated on |
|---|---|---|---|
| **P7.0** | `ts_writes` and `verbatim_origins`-precursor (`ts_any` trend) gated probes added to `xtask/src/greenfield_status.rs`, following `hoist_sinks`'s line-scan pattern and excluding `xtask` itself from its own count | instrumentation | — |
| **P7.1** | Verification pass on R7.7 (§1's finding that it may already be closed) — confirm every runtime `.ts` file is `include_str!`'d and CI-type-checked, correct the doc if so, or name the residue if not | R7.7 | — |
| **P7.2** | Classify all 48 `as any` sites (Q3's spike) — narrowable vs. needs-IR-extension | investigates Q3 | — |
| **P7.3** | `TomlDocument` + minimal TOML printer; `emit_wrangler_toml` (`emitter/wrangler.rs:49`) returns a document, not a `String` | R7.8 (part) | — |
| **P7.4** | **Closes R7.6** — `bynk/src/deploy/config.rs:195,260,263`, `deploy/ledger.rs:365`, `bynk-strip/src/lib.rs:136-139` read typed documents/nodes instead of text-matching `KV_NAMESPACE_ID_PLACEHOLDER` and `main = "index.ts"` | R7.6 | P7.3 |

**Arc B — the crate and the spine**

| Slice (candidate) | What it does | Rules | Gated on |
|---|---|---|---|
| **P7.5** | `bynk-ts` crate exists (Q1) — `TsProgram`/`TsNode`/`Span`, `Verbatim` (Q2), printer owning buffer/indentation/offsets, `SourceMap` relocated from `emitter/source_map.rs` | R7.2, R7.3, R7.4, R10.1, R10.3 | Q1, Q2 |
| **P7.6** | `Artefacts { docs: BTreeMap<PathBuf, Document> }` replaces `CompiledFile` (`project.rs:97`) — every TS document is a `TsProgram`, initially wrapping one `Verbatim` node | R7.8 | P7.5 |
| **P7.7** | Named, tested printer policy (readability as a property with a name and a test) | R7.5 | P7.5 |
| **P7.8** | `TsStmt`/`TsExpr`/`TsType`/`TsDecl` per §7.1's sketch; builders; no `Any`/`enum`/`namespace`/decorator/param-property representable | R7.1 | P7.5 |
| **P7.9** | `ts_type_ref`/`ts_type_ref_qualified`/`ts_type_ref_qualified_multi`/`ts_type_ref_with`/`ts_base`/`ty_to_type_ref`/`pred_condition_and_message` (`emitter.rs:978,4075,4088,4098,4115,4130,4408`, ~124 call sites, plus the sibling at `emitter/serialisation.rs:1440`) become `bynk_ts::TsType` constructors — first real `ts_writes` and `ast_importers` drop | R7.1, R7.2 | P7.8 |

**Arc C — conversion, smallest blast radius first**

`contracts.rs` + `secrets.rs` + `runtime_use.rs` (~10 sites) → `events_fanout.rs` (41) →
`serialisation.rs` (237, pending Q3's classification for its `as any` sites) → `workers.rs` (137,
same) → `workers_entry.rs` (192) → `emitter/lower.rs` (372, likely several slices) →
`emitter/emit.rs` (409, likely several slices) → `emitter.rs` (99) + `project.rs` (44) →
`project/tests_emit.rs` converts its 130 byte-golden assertions to node assertions **last** — they
are the migration's only real safety net and must stay byte-golden until everything upstream has
converted. Each slice deletes its `VerbatimOrigin` variant, if Q2 settles that shape. R8.1's
no-wildcard totality folds into whichever slice owns each `IrItem` arm rather than standing alone.

**Arc D — settling**

`bynk-ir`/`bynk-lower` carved as crates, if Q1 didn't already fold this into P7.5 (ADR 0332's
named trigger — `bynk-ts` as a genuine second consumer — is met once Arc B lands); severing
`bynk-emit`'s dependency on `bynk-check`, if Q4 finds it genuinely this phase's; R10.4 surface
enumeration (finding #42's 33-of-38 spurious `pub`); R10.2 verification that `bynk-lsp` stops
linking emission code it never executes (finding #39, ~25k lines); residual R8.x items Q4 finds
open; the retirement review naming both probes' final floors.

**Working slice-count estimate (Q5): 35–45** — Arc A ~4, Arc B ~5, Arc C ~20 (provisional,
depends on how finely the two largest files split), Arc D ~8. Provisional pending §3.

---

## 7. Out of scope — forward references, not refusals

| Item | Phase | Entry condition |
|---|---|---|
| Incrementality — query granularity, `UnitSignature`, the firewall | 8 | this track's probes (`ts_writes` = 0, `verbatim_origins` at its argued floor) settle; trajectory §4 also names phases 3 and 4 as preconditions, already true |
| Any R8 rule Q4 finds genuinely open but out of this track's natural scope | *unopened — named only if Q4 finds one* | Q4 |
| A further crate re-graph beyond `bynk-ts`/`bynk-ir`/`bynk-lower` (e.g. R10.5's `bynk-driver` consolidation) | *unopened — no trigger yet* | named in the reference (Part 10) but not this phase's own invariant |

---

## 8. Keeping the reference true

Two probes need building for this phase's gate — `ts_writes` and, if Q2 settles the `Verbatim`
shape, `verbatim_origins` — neither exists today (`xtask/src/greenfield_status.rs` has 13 probes,
none TypeScript-write-shaped). P7.0 is this track's own first slice for exactly that reason,
mirroring `the-ir.md`'s own P6.0 being real infrastructure/classification work, not ceremony.

One correction this track's settling pass should carry: the trajectory doc's own §3.0 baseline
row for phase 7 ("not measured") and §5's sizing ("15, low confidence") are both stale against
§1's own first measurement — a routine update, the same way `project-model.md` §3.5 corrected a
stale Appendix D row ahead of the generator, and `the-ir.md`'s own spine issue did for phase 6's
sizing before it opened.

---

## 9. Risks

**Q3's answer may re-open phase 6.** If any of the 48 `as any` sites need a type the IR does not
construct, closing R7.1 fully means extending `IrItem`/`TyId` — work this track's own §2
explicitly says is not its job to do casually. P7.2 (the classification spike) exists to surface
this before large slices are scheduled, not after.

**Golden-test churn is the whole safety net, and it is finite.** Every Arc C slice must produce
byte-identical output against `project/tests_emit.rs`'s existing goldens; if a conversion changes
a byte, the phase has lost its only cheap correctness check for ~1,540 sites simultaneously. This
is why tests_emit.rs converts to node assertions *last*, not first (§6).

**§2's half-landing risk is real, and `Artefacts` is the hinge.** Before P7.6 lands, stopping
anywhere in Arc A or early Arc B is safe by construction — nothing has changed downstream
consumers' contract yet. After P7.6, stopping mid-Arc-C leaves a partially-treed emitter; this is
survivable *only* because every unconverted site routes through `Verbatim`, which the printer
still owns per R7.3/R7.4 from the moment it exists. If Q2 settles on *no* hatch, this risk becomes
much sharper — the phase would have no safe stopping point between P7.6 and full Arc C
completion, which trajectory §2 treats as the failure mode this whole discipline exists to avoid.

**The evidence ages.** Every fact, line number and count in this doc was measured against `main`
at commit `79053b09` on 19–20 August 2026. Re-check before a slice proposal cites one, per every
prior track's own §9.

---

## 10. What this phase causes

Phase 8 (incrementality) is named, by the trajectory's own dependency graph (§4), as needing
phases 3, 4 **and** 7: "3 and 4 before 8. The firewall needs stable identity and a project model
that is not ambient" — but §4's ordering diagram also shows 7 feeding 8 directly, since a query
firewall over an emitter that still writes strings ad hoc has no stable unit to memoize against.
This phase's `Artefacts` (R7.8) is plausibly phase 8's `UnitSignature` target for the emit side,
though the trajectory does not say so explicitly — worth naming as a question for phase 8's own
settling review, not decided here.

---

## 11. ADRs

Per ADR 0167 step 2, load-bearing, hard-to-reverse decisions land before slicing. Candidates,
pending §3 actually closing under review (numbers assigned at merge by the stamp, per every prior
track's own convention of referring to them by letter until then):

- **ADR-A** — whether `bynk-ts` is carved as a crate in the first Arc B slice or built in-module
  and carved later (§3.1, Q1). Likely the most load-bearing of the set — it decides whether R10.2
  and the `bynk-ir`/`bynk-lower` carve can ride this phase or need their own later trigger.
- **ADR-B** — the migration escape hatch's existence, shape and forcing function (§3.2, Q2).
  Directly determines whether this phase has a safe stopping point anywhere between "the crate
  exists" and "conversion is complete" (§9).
- **ADR-C** — the two completion probes' final floors, once Arc C's real shape is known (§5)
  — likely written late, near retirement, the same way `the-ir.md`'s own floor-of-5 argument was
  P6.58, not a day-one ADR.

---

## 12. Retirement

Mirrors every prior track on this trajectory: retires when `ts_writes` reads 0 and
`verbatim_origins` (if it exists, per Q2) reads its own argued floor, with every surviving site
named file-by-file in the closing summary. The retirement PR removes this doc, appends its closing
summary to `../archive/retired-tracks.md`, and closes the spine issue
([#1293](https://github.com/accuser/bynk/issues/1293)).
