# The TypeScript tree and printer — migrating to the greenfield reference (phase 7)

- **Status:** **Settled — Slicing on merge.** §3's five questions were argued under a settling review
  on this branch. Q3 and Q4 changed shape during this settling pass, in the direction of more evidence,
  not more doubt — see the provenance note at the head of §3. Merging settles **direction**; it is not a
  build authorisation. Each slice is still an ordinary increment proposal, and `accepted` on that
  sub-issue is the approval to build.
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
- **Relates:** reference rules R7.1–R7.8 in full — **Q3 settles that all eight, including R7.1's
  `Any` elimination, are achievable within this track**, not partially deferred. Plus R8.1
  (totality over `IrItem`), R8.20 (folded into Arc A's wrangler work, same defect as R7.6), and —
  **per Q4** — the five R8 rules closed as a natural byproduct of the tree/printer conversion
  (R8.3, R8.6, R8.8, R8.10, R8.16's emission half) and two rules needing their own named slices
  (R8.2, R8.14). The remaining twelve R8 rules (R8.5, R8.7, R8.9, R8.11–R8.13, R8.15, R8.17–R8.19,
  R8.21, R8.22) already read closed and carry only a verify-only pass; R8.4 needs a cheap
  confirming slice; R8.16's data-model half (a typed `ProjectGraph`) stays phase 8's, per phase
  4's own deferral.
- **Precedent:** `the-ir.md` (retired) is this track's direct predecessor on the same trajectory.
  Its own §7 ("Forward references") named this phase's entry condition explicitly, and amended it
  mid-flight (P6.58): not `ast_importers` reading 0, but reading its own re-settled floor, 5 —
  `bynk-emit/src/emitter.rs`, `emitter/emit.rs`, `emitter/lower.rs`, `emitter/workers.rs`,
  `emitter/workers_entry.rs` — because the renderer family living in those five files (`ts_base`,
  `ts_type_ref*`, `ty_to_type_ref`, `pred_condition_and_message`) is *this* track's own future
  surface, not phase 6's unfinished work. `the-typescript-tree.md` is the twelfth track to run
  the ADR 0167 flow from the start, after `compiler-architecture.md` (sixth),
  `identity-and-totality.md` (seventh), `content-ownership.md` (eighth), `project-model.md`
  (ninth), `semantics-in-the-checker.md` (tenth), `the-ir.md` (eleventh).

### ADR 0076 trigger check

| Trigger | Met? |
|---|---|
| Spans several increments | **Yes, decisively.** §6's settled slice count is 37, tightened from the provisional 35–45 range — against phase 6, sized 20 and delivered in 59 slices, against a smaller emitter surface than this phase inherits |
| Surface not yet settled | **No** — the destination (R7.1–R7.8) is fully specified in the reference. §3 closes the migration *method*: the escape hatch's shape and forcing function (Q2), the crate-carve sequencing (Q1), R7.1's real scope (Q3) and R8's real boundary (Q4) |
| Security/safety boundary | **No** — this phase's motivating defects (source-map corruption, `wrangler.toml` substring coupling) are correctness and maintainability failures, not security gates. `TsType::Any` elimination narrows a real type-safety gap (finding #18) but is not a new authorization boundary |

**One of three**, same count as every internal-architecture track on this trajectory before it.

---

## 1. Why now

**Phase 6's own closing note opens this phase by name and amends its own entry condition to do
so.** `the-ir.md`'s §7 recorded `bynk-ts` as a forward reference gated on `ast_importers` reading
0; P6.58 (`b22752a1`) re-settled that to "reads its own named floor" once the track's research
found the renderer family in `emitter.rs`/`emit.rs`/`lower.rs`/`workers.rs`/`workers_entry.rs`
cannot leave `bynk-emit` until `bynk-ts` exists to receive it. The floor, 5, is where phase 6
retired (`79053b09`); it is where this phase begins.

**Phase 7's own probe was never measured; measured for the first time on this branch, and one
figure needed correcting during settling.** §3.0's baseline table (30 July 2026) records "not
measured". Measured against the working tree (19–20 August 2026):

| Reading | Value |
|---|---|
| `write!`/`writeln!`/`format!` in `bynk-emit` | **1,709** total — `format!` 853, `writeln!` 845, `write!` 11 |
| …of which genuinely TypeScript-producing | **~1,540** |
| Emitter subtree (`emitter.rs` + `emitter/**`) | **21,742 LOC**, 52% of `bynk-emit`'s 41,953-line total |
| `TsType`/`TsProgram`/`Artefacts` Rust types | **none exist** — today's carrier is `pub struct CompiledFile`, `bynk-emit/src/project.rs:97` |
| `bynk-ts` crate | **does not exist** — 17 workspace members today |
| `as any` casts (raw grep) | **42**, corrected during settling from an earlier count of 48 (Q3) |
| …of which real emission sites | **~24** — the rest are Rust comments/doc-comments (11), one unrelated English phrase (`ir/lower.rs:3290`, "same as any other subexpression" — not TypeScript), hand-written runtime `.ts` files (5, R7.7's territory not R7.1's), and one test-fixture input string |
| Bare `: any` annotations missed by an `as any` grep | **19**, found independently during settling (Q3) — a probe scoped to `as any` alone under-covers R7.1 |

`design/bynk-greenfield-compiler.md` §15.2 prices the printer itself at "2,000–3,000 lines written
before the first emitted character" — a cost phase 6 never paid, since `Ir` is a value type, not
a writer. Phase 6 was sized 20 (relative) and took 59 slices against a smaller, better-scoped
surface (the IR's *shape*, not its rendering).

**Two defects this phase closes are live in shipped code, not hypothetical.** R7.6's own
rationale is not abstract: `emitter/wrangler.rs:34` defines
`KV_NAMESPACE_ID_PLACEHOLDER = "<KV_NAMESPACE_ID>"`, and `bynk/src/deploy/config.rs:195,260,263`
and `bynk/src/deploy/ledger.rs:365` text-match and `.replace()` against it;
`bynk-strip/src/lib.rs:136-139` does the same for `main = "index.ts"` → `main = "index.js"`. This
is also literally R8.20's own defect (Appendix B: "a deploy placeholder broken by a reformat, the
KV id literal") — the two rules share one fix, folded into a single Arc A slice (§6). R7.4's
rationale — wrong, not missing, source-map offsets — is finding #4 (Medium, confirmed):
`record_span(out.len(), …)` in `emitter/source_map.rs` "has no idea which buffer `out` is," so
IIFE-local offsets corrupt the map.

**One finding appears to have aged out already.** Finding #17 reported ~300 lines of harness
TypeScript as Rust string literals beside an `include_str!` of `runtime.ts`. In the current tree,
`emitter.rs:94` and five sites in `project/tests_emit.rs` already load real `.ts` files, and
`bynk-emit/runtime/tsconfig.json` type-checks them under `strict`. R7.7 is substantially closed
already — P7.1 (§6) is a verification pass, not construction.

---

## 2. What this track is not

- **Not phase 8 (incrementality).** Query granularity, `UnitSignature`, the firewall are
  untouched here.
- **Not re-opening phase 6's IR shape.** Q3 settles that no `as any`/bare-`any` site needs an IR
  extension — `IrExpr`/`IrItem`/`Callee`/`CommitShape` stay as phase 6 left them. The one residual
  (§3.3) needs a new *runtime-exported* type, not a new IR field.
- **Not rebuilding R8's emission semantics wholesale.** Q4 settles that twelve of twenty-one R8
  rules already read closed and need only a verify-only pass; five more close as a byproduct of
  Arc B/C's own conversion work, with no separate construction. Only R8.2 and R8.14 are genuinely
  separate scope.
- **Not a full crate re-graph.** Only `bynk-ts` (Q1: carved up front) and, once its own trigger is
  met, `bynk-ir`/`bynk-lower` are this phase's crate moves.
- **Not phase 6's own residue by name.** The five files at `ast_importers`'s floor are this
  phase's surface *because* phase 6 argued them there — this track inherits that argument, it
  does not re-litigate it.

---

## 3. Design questions — settled

> **Provenance: Q3 and Q4 changed shape during this settling pass, in the direction of more
> evidence, not more doubt.**
>
> The draft treated Q3 (`TsType::Any` elimination) as a live risk that could re-open phase 6 —
> `emitter/workers.rs:577`'s documented `: any` policy read as a warning sign. A full,
> site-by-site classification of every `as any` occurrence (not a sample) found the opposite: the
> real count was smaller than measured (42, not 48; ~24 real emission sites once comments and
> runtime `.ts` files are excluded), and every real site resolves without touching the IR — most
> to `unknown` or a locally-derivable structural type, a few to a generated index-signature type
> using data the IR already carries, and a small named residual (2–3 sites) needing a new
> *runtime*-exported type rather than a checker/IR change. `workers.rs:577`'s comment is a
> parameter-provenance argument, unaffected by phase 6, and survives as the reason those specific
> wrappers keep a (now-typed) escape rather than as evidence against full elimination.
>
> The draft treated Q4 (R8's scope) as answerable with a coarse "closed vs. open" split. A
> rule-by-rule audit against the current tree — the same discipline `semantics-in-the-checker.md`
> applied to `emit_diagnostics` — found the true shape is three-way, not two-way: twelve rules
> closed outright, five closed *behaviourally* but sourced from the AST or ad-hoc collection
> rather than the IR (naturally finished by this track's own conversion, not separate work), and
> only two (R8.2, R8.14) genuinely open and separately scoped. One rule (R8.16) splits down the
> middle with phase 8, which already owns its data-model half by name.
>
> Q1 and Q2 resolved close to the draft's own framing, each sharpened by real precedent this pass
> found rather than reasoned abstractly.

### 3.1 Q1 — Is `bynk-ts` carved as a crate up front, or built as a module inside `bynk-emit` and carved later? **Settled.**

**Decision: carved up front, in the first Arc B slice (P7.5).** This codebase has exactly two
prior R10.3-shaped precedents, and both were carved up front, never built in-module first:
`bynk-strip` (commit `868fda94`, #385 — created new, in the same PR as its only consumer) and
`bynk-render` (commit `b56f22de`, #251, `crate-decomposition` track slice 6 — created new, in the
same PR that moved seven existing renderer functions out of `bynkc` into it; its own module doc
states the load-bearing invariant directly, `cargo tree -p bynk-render` is `bynk-syntax` +
`ariadne` only, enforced structurally). `bynk-render`'s shape is the closer analogy to `bynk-ts`:
both exist to enforce a boundary invariant ("only this code writes/renders") that a `pub(crate)`
module cannot self-enforce — exactly finding #42's failure mode (33 of 38 world-reachable
`bynk_emit::emitter` items `pub` only to reach a sibling), live evidence that "carve later"
doesn't reliably happen in this codebase once code is already crate-internal-convenient. Phase
6's `ir.rs`-in-module choice is not a counter-precedent: ADR 0332 deferred that split because *no
second consumer existed at all*, a materially different condition from a boundary invariant
needing structural enforcement from day one. Mechanical cost is trivial — `bynk-ts` depends on
nothing but `bynk-syntax` (for `Span`), no circular-dependency risk, one new workspace member.

### 3.2 Q2 — Does the migration need an escape hatch, and what forces it to a named floor? **Settled.**

**Decision: yes — `TsStmt::Verbatim { origin: VerbatimOrigin, text: String }`, sealed
constructor, statement granularity, plus a companion textual lint.** None of this trajectory's
own prior migration techniques transfer directly, and understanding why sharpens the design.
Phase 3's "parallel-data" technique (`identity-and-totality.md`, recovered pre-retirement) kept
old and new *representations* live simultaneously because every consumer action was the same
kind (read a map) regardless of which table backed it — no behaviour ever depended on which
representation was live. Phase 6's IR migration (`the-ir.md`) used no bridge type at all — AST-
and IR-reading call sites simply coexisted, tracked by a per-file import count, because the
*output-producing mechanism* (raw `writeln!`) never varied regardless of which input model fed
it. Phase 7 is the first phase where the writer itself, not an input representation, must become
singular — R7.2–R7.4 exist to make exactly that singular. Letting unconverted sites keep calling
raw `writeln!` during migration would be a literal violation, not a benign representation choice,
so neither prior technique applies as-is. `Verbatim` is the minimal construct that keeps the
*writer* singular (the printer still owns buffer/indentation/offset arithmetic for a `Verbatim`
block, so R7.3/R7.4 hold from the slice that introduces it) while letting *content* stay
unconverted — closer in spirit to the shelved CodeWriter proposal's module-by-module,
golden-gated sequencing (worth citing directly as sequencing precedent) than to either prior
technique.

**A real gap this settling pass found and closes:** a byte-golden fixture is blind to what's
*inside* an opaque `Verbatim` block — the same shape of weak evidence ADR 0198 named directly
("the gate is green" was "the weakest possible evidence" after a defect survived 331 negative
fixtures for 60 increments because the fixture format asserted category strings, never the
attributed value). A `Verbatim` block could silently contain `enum`/`: any`/`namespace` and every
golden fixture would stay green, defeating R7.1's "cannot be typed" claim while reading as done.
**P7.5 (§6) therefore also builds a textual lint over `Verbatim` content** — forbidding the banned
constructs (`enum`, `namespace`, decorators, constructor parameter properties, `: any`/`as any`)
by pattern match on the wrapped text, run in CI alongside the golden fixtures, not instead of
them.

**Floor:** as already settled (not one of §3's open items — this predates the branch): named at
settling, not discovered mid-track. `ts_writes` retires at **0** outside `bynk-ts` and outside a
`Verbatim` construction; `verbatim_origins` retires at an **argued floor**, expected small (per
§1's five-file residue and Q3's finding that `Any`-elimination needs no residual `Verbatim`
coverage of its own); `verbatim_sites` retires at **0** — see §5 for why `verbatim_origins` alone
cannot be the floor (a review of the settling PR caught this: distinct variants don't bound
residual volume, and this third probe closes that gap).

### 3.3 Q3 — Is `TsType::Any` actually eliminable across all current `as any`/bare-`any` sites, or does eliminating some re-open phase 6? **Settled — and the finding is better than the question assumed.**

**Decision: full elimination is achievable within this phase, with one small, named residual
deferred to R7.7's runtime-typing work, not treated as a phase-6-reopening risk.** A full,
site-by-site classification (not a sample) found the real surface smaller and more tractable than
the draft's 48-site framing suggested:

- Raw `as any` grep hits: 42, not 48 (measurement correction) — only ~24 are real TypeScript
  emission sites; the rest are Rust comments, one unrelated English phrase, hand-written runtime
  `.ts` files (R7.7's territory), and a test-fixture string.
- A grep scoped to `as any` alone **under-covers R7.1**: 19 bare `: any` annotations exist
  independently (e.g. `workers_entry.rs:771`, `serialisation.rs:1026`, several sites in
  `emit.rs`'s history driver) — spot-checked and confirmed independently during this settling
  pass. P7.0's `ts_any` probe (§6) must scan for both patterns, not `as any` alone.
- Of the real sites: **~20 are narrowable with zero IR work** — most resolve to `unknown` (event
  payloads, queue message bodies with no declared type — R7.1 forbids `Any`, not `unknown`),
  several to a locally-derivable structural or marker type (duck-typed codec checks, thrown-value
  inspection), and a handful (`(this as any)[methodName]`-style dynamic handler dispatch, 3 sites)
  to a *generated index-signature type* built from data the IR already carries (the resolved
  handler set) — new emission code, not new IR data.
- **A small residual, 2–3 sites** (`serialisation.rs:802,1026` — runtime-owned error families
  `ValidationError`/`JsonError`/`HttpResult`/`QueueResult`) genuinely need a real exported
  TypeScript type for those runtime types before `any` can go. This is R7.7's business (the
  hand-written runtime as real, type-checked TS source), not R7.1's tree work, and is named here
  explicitly so it is deferred on purpose rather than discovered as a surprise inside an Arc C
  slice.
- `emitter/workers.rs:577`'s documented `: any` policy is a parameter-provenance argument (params
  mix codec-produced and route/query-string values) — unaffected by phase 6's IR, and not
  overturned by this decision: those wrappers gain a real structural type instead of `any`, not a
  removal of the wrapper's own design.

### 3.4 Q4 — How much of R8.1–R8.22 is this track's to close? **Settled.**

**Decision: three-way split, not the draft's assumed two-way one.** A rule-by-rule audit against
the current tree, not the review's stale findings, found:

- **Twelve rules already read CLOSED**, needing only a verify-only pass: R8.5 (commons
  re-branding, `emitter.rs:1633`), R8.7 (`loadState` merge, `emit.rs:3212-3229`, matches ADR 0124
  D4 precisely), R8.9 (single factory helper, low confidence — one `makeAgent` found, no
  exhaustive search for a duplicate path), R8.11 (`deps` type derivation already reads the IR's
  `CapRefIr`, `emit.rs:1847`), R8.13 (boundary-only verification, all four named properties
  confirmed in `workers.rs`), R8.15 (`http_value_serialiser` no longer exists — single dispatch
  confirmed), R8.17 (sorted route table, `workers_entry.rs:117-122`), R8.18 (contract-hash
  ordering, `workers_entry.rs:404`), R8.19 (typed TOML escaping, `wrangler.rs:162`, with a
  dedicated injection test), R8.21 (`is_effectful_return` as the one shared predicate), R8.22
  (`result ?? null`, `emit.rs:3690`). **R8.12 is closed under its own current text but
  self-superseding — flagged, not silently assumed:** it's closed today because only `Call`
  wrappers get real param types by design (R8.12's own rationale already documents the carve-out);
  R7.1 landing is what changes this rule's meaning, and its "closure" needs to be re-confirmed as
  intentional completion, not regression, in the same slice that lands full `Any` elimination.
- **Five rules read PARTIAL — behaviourally correct, structurally sourced from the AST or ad-hoc
  collection rather than the IR — and close as a natural byproduct of this track's own Arc B/C
  work, needing no separate slice:** R8.3 (`is_opaque` is real `TypeShape` data, but emission
  still branches on it at five call sites rather than reading a pre-decided shape — the tree/
  printer conversion is exactly what removes the branch), R8.6 (`CommitShape` exists precisely as
  specified in `ir.rs:1444` but has zero consumers — `emit.rs` still re-derives the same
  distinction independently, a gap the IR's own IR's doc comment already names), R8.8 (invariant/
  transition ordering is behaviourally exact but iterates the raw `AgentDecl` rather than an
  IR-carried set), R8.10 (handler key mangling is a single pure function, but the rule's own
  "with a stated inverse" requirement is unmet — no inverse function exists), R8.16 (emission
  half — per-consumer surface generation is already correct, `project.rs:3100-3175` — but the
  underlying `unit_consumes`/`unit_tables: HashMap` isn't a typed `ProjectGraph`; **that data-model
  half is phase 8's, named as deferred there since phase 4's own retirement note** — this track
  owns only confirming the emission behaviour stays correct once its own conversion lands).
- **Two rules read genuinely OPEN and are this track's own separately-scoped work:** R8.2 (the
  brand prefix is computed at emission from `ctx.owning_context`, `emit.rs:57-58`, not read from a
  recorded brand — a real Arc D slice) and R8.14 (the JSON codec collector still walks raw
  `ExprKind::MethodCall` over AST-shaped commons items, `emitter.rs:1046`, with its own doc
  comment recording that an IR conversion was investigated and declined at P6.56 — worth
  revisiting once `bynk-ts` exists, since a tree-node-based collector may succeed where an
  IR-based one was declined; a real Arc D slice, not assumed to close automatically).
- **R8.20 is not separate scope** — it's literally the same defect as R7.6 (the KV-namespace
  placeholder), already folded into Arc A's P7.4.
- **R8.4 is unresolved with full confidence** (numeric-refinement guard ordering looks correct at
  every site checked, but no exhaustive call-graph proof) — a cheap, one-slice verification, Arc
  D.

---

## 4. Posture

Extends ADR 0059 as amended by ADR 0309, the same standing properties every track on this
trajectory has run under: behaviour-preserving by default; short-lived branches, one PR per
slice; no per-increment ADRs beyond what §11 front-loads; every slice citing `Closes-Rule:`.

---

## 5. The completion criterion

**Three probes, not two — a gap a PR review caught in the two-probe version and this revision
closes directly.** The original pairing was gameable: `ts_writes` reading 0 the instant every
`format!`/`writeln!` call is redirected into a `Verbatim`'s `text:` field is *correct*, not a
loophole — it's the literal truth of R7.3 ("the printer... is the only code that writes a
character"; a `format!` call building an in-memory string for a `Verbatim` payload writes no
output character, the printer does that later). The real gap was `verbatim_origins` alone:
**variants are not sites.** `VerbatimOrigin` having only 1–3 surviving variants says nothing about
volume — three variants could each cover a handful of residual sites, or each wrap an entire
409-line file (`emitter/emit.rs`) or 372-line file (`emitter/lower.rs`) — precisely the two
largest, least-decomposed files in the whole surface, and the two Arc C schedules last with
"likely several slices," i.e. the ones most likely to arrive at the finish line undecomposed if
nothing else forces the count down. `ts_writes = 0` and `verbatim_origins ≤ 3` is therefore
satisfiable by wrapping those two files wholesale into one variant each and never touching them
again — trajectory §2's own failure mode ("a phase that half-lands leaves two paths reachable"),
reached through a green gate.

- **`ts_writes`** — TypeScript-producing `write!`/`writeln!`/`format!` outside `bynk-ts` **and**
  outside a `Verbatim` construction. Retires at **0**. Once every site routes through at least
  `Verbatim`, this reads 0 by design — that is the intended, immediate win of landing the hatch
  (P7.5), not a way to appear finished while unconverted.
- **`verbatim_origins`** — count of distinct `VerbatimOrigin` enum variants still constructed.
  Retires at an **argued floor**, expected small (1–3), named file-by-file at retirement the way
  `ast_importers`'s floor of 5 was. Measures how many *families* of residue remain, not their
  size.
- **`verbatim_sites`** (new — closes the gaming gap) — count of distinct `Verbatim::new`-style
  construction call sites in `bynk-emit` source, line-scanned the same way `hoist_sinks` counts
  `stmts: &mut Vec<String>` occurrences. Retires at **0**: every call site converting to a real
  tree node is what Arc C's own per-file slices are actually for, and this is what makes that
  progress visible and gate-checkable — `verbatim_origins` alone cannot distinguish "3 variants,
  12 residual call sites" from "3 variants, 900 residual call sites, two files never
  decomposed." A single slice landing `emitter/emit.rs` or `emitter/lower.rs` as one `Verbatim`
  wrap would read as 1 new variant and *hundreds* of new `verbatim_sites` — visibly not
  conversion, and rejectable at review on that basis alone.

`ts_writes` and `ts_any` are buildable now, against a real baseline (§1's own measurement), and
land in **P7.0**. `verbatim_origins` and `verbatim_sites` scan for patterns
(`VerbatimOrigin`/`Verbatim::new`) that don't exist in source until **P7.5** builds them — and,
per this codebase's own probe-fixture discipline (every probe needs a fixture exercising its
stated hazard, not just its happy path, the precedent T0.0's own "Done when" set), a probe can't
be meaningfully fixture-tested against a type that doesn't exist yet. So they're built in P7.5,
alongside `Verbatim`/`VerbatimOrigin` themselves, not stubbed early in P7.0. `xtask/src/
greenfield_status.rs` has 13 probes today; P7.0 adds two, P7.5 adds two more. Per Q3, the `ts_any`
trend probe must scan for `as any` **and** bare `: any` — a probe scoped to the former alone
under-counts. Per Q2, `Verbatim` content also needs a textual lint (banned constructs, run in CI)
as a companion to the golden fixtures, since golden output alone is blind
to what a `Verbatim` block hides.

---

## 6. Slice decomposition

§3 is settled; all slices below are accepted, buildable, not forward references. **37 slices**,
tightened from the provisional 35–45 range: Arc A gained one slice (Q3's finding that most `any`
narrowing is a plain text change, doable before the tree exists), Arc D gained precision (two
named rule-closing slices, R8.2/R8.14, replacing an unscoped placeholder).

**Arc A — independent of the tree, land immediately (5 slices)**

| Slice | What lands | Rules | Gated on |
|---|---|---|---|
| **P7.0** | `ts_writes` and `ts_any` (scanning `as any` **and** bare `: any`, per Q3) gated probes in `xtask/src/greenfield_status.rs`, following `hoist_sinks`'s line-scan pattern, excluding `xtask` from its own count | instrumentation | — |
| **P7.1** | Verification pass confirming R7.7 is closed (§1's finding) — every runtime `.ts` file is `include_str!`'d and CI-type-checked; correct the doc if so | R7.7 | — |
| **P7.2** | Narrow the ~20 classified-narrowable `as any`/bare-`: any` sites (§3.3) to `unknown`, structural or marker types, and generated index-signature types where the IR already carries the data — plain `writeln!`-level text changes, no tree required | R7.1 (partial, ahead of the tree) | §3.3 |
| **P7.3** | `TomlDocument` + minimal TOML printer; `emit_wrangler_toml` (`wrangler.rs:49`) returns a document, not a `String` | R7.8 (part) | — |
| **P7.4** | **Closes R7.6 and R8.20** — `bynk/src/deploy/config.rs:195,260,263`, `deploy/ledger.rs:365`, `bynk-strip/src/lib.rs:136-139` read typed documents instead of text-matching `KV_NAMESPACE_ID_PLACEHOLDER` and `main = "index.ts"` | R7.6, R8.20 | P7.3 |

**Arc B — the crate and the spine (5 slices)**

| Slice | What lands | Rules | Gated on |
|---|---|---|---|
| **P7.5** | `bynk-ts` crate carved up front (§3.1) — `TsProgram`/`TsNode`/`Span`, `Verbatim`/`VerbatimOrigin` plus its companion textual lint (§3.2), printer owning buffer/indentation/offsets, `SourceMap` relocated from `emitter/source_map.rs` | R7.2, R7.3, R7.4, R10.1, R10.3 | §3.1, §3.2 |
| **P7.6** | `Artefacts { docs: BTreeMap<PathBuf, Document> }` replaces `CompiledFile` (`project.rs:97`) | R7.8 | P7.5 |
| **P7.7** | Named, tested printer policy | R7.5 | P7.5 |
| **P7.8** | `TsStmt`/`TsExpr`/`TsType`/`TsDecl` per §7.1's sketch — no `Any`/`enum`/`namespace`/decorator/param-property representable; lower-risk than the draft assumed, since P7.2 already narrowed most `any` sites ahead of this slice | R7.1 | P7.5 |
| **P7.9** | `ts_type_ref*`/`ts_base`/`ty_to_type_ref`/`pred_condition_and_message` (`emitter.rs:978,4075,4088,4098,4115,4130,4408`, ~124 call sites) become `bynk_ts::TsType` constructors — first real `ts_writes`/`ast_importers` drop | R7.1, R7.2 | P7.8 |

**Arc C — conversion, smallest blast radius first (~19 slices)**

`contracts.rs` + `secrets.rs` + `runtime_use.rs` → `events_fanout.rs` → `serialisation.rs`
(includes closing §3.3's 2–3-site residual by exporting real runtime types for
`ValidationError`/`JsonError`/`HttpResult`/`QueueResult`, per R7.7) → `workers.rs` →
`workers_entry.rs` → `emitter/lower.rs` (several slices) → `emitter/emit.rs` (several slices,
also finishing R8.3/R8.6/R8.8's structural half per §3.4) → `emitter.rs` + `project.rs` →
`project/tests_emit.rs` converts its 130 byte-golden assertions to node assertions **last**. Each
slice deletes its `VerbatimOrigin` variant and is checked against the P7.5 textual lint, not
golden fixtures alone (§3.2).

**Arc D — settling (~8 slices)**

Provisionally lettered, not numbered — Arc C's own slice count is an estimate (~19), so fixed
`P7.N` numbers here would silently claim a range Arc C's real slices will actually occupy. Real
`P7.N` numbers are assigned sequentially as each slice is actually cut, in landing order, the same
convention every prior track on this trajectory used.

| Slice | What lands | Rules | Gated on |
|---|---|---|---|
| **P7.d1** | `bynk-ir`/`bynk-lower` carved as crates — ADR 0332's named trigger (`bynk-ts` as a genuine second consumer) met once Arc B lands | R10.3 | P7.5 |
| **P7.d2** | R8.2 — brand string recorded once (R4.10), read at emission rather than computed from `ctx.owning_context` | R8.2 | Arc C substantially landed |
| **P7.d3** | R8.14 — the JSON/boundary codec collector unified into one collector over `bynk-ts` tree nodes, revisiting P6.56's declined IR-based attempt now that a tree exists to collect over | R8.14 | P7.8 |
| **P7.d4** | R8.4 verification — confirm numeric-refinement guard ordering across every emission site, not just the ones spot-checked during settling | R8.4 (verify) | — |
| **P7.d5** | Severing `bynk-emit`'s dependency on `bynk-check`, if Arc C's conversion leaves one — tentative, no direct evidence gathered this settling pass | — | Arc C landed |
| **P7.d6** | R10.4 surface enumeration — finding #42's 33-of-38 spurious `pub` in `bynk_emit::emitter` | R10.4 | Arc C landed |
| **P7.d7** | R10.2 verification — `bynk-lsp` stops linking emission code it never executes (finding #39) | R10.2 | P7.d1 |
| **P7.d8** | Retirement review: verify-only pass on the twelve R8 rules already closed (§3.4), explicitly confirming R8.12's self-supersession lands as intended, not regression; all three probes' final floors named file-by-file | R8.5,7,9,11–13,15,17–19,21,22 (verify) | Arc C landed |

---

## 7. Out of scope — forward references, not refusals

| Item | Phase | Entry condition |
|---|---|---|
| Incrementality — query granularity, `UnitSignature`, the firewall | 8 | this track's probes (`ts_writes` = 0, `verbatim_origins` at its argued floor, `verbatim_sites` = 0) settle |
| R8.16's data-model half — a typed `ProjectGraph` | 8 | named by phase 4's own retirement note; this track only verifies R8.16's emission behaviour stays correct |
| A further crate re-graph beyond `bynk-ts`/`bynk-ir`/`bynk-lower` (e.g. R10.5's `bynk-driver` consolidation) | *unopened — no trigger yet* | named in the reference (Part 10) but not this phase's own invariant |

---

## 8. Keeping the reference true

Four probes need building, split across two slices (§5): `ts_writes` and `ts_any` in **P7.0**,
scoped to `as any` **and** bare `: any` per §3.3's own correction — mirroring `the-ir.md`'s own
P6.0 being real infrastructure, not ceremony. `verbatim_origins` and `verbatim_sites` in **P7.5**,
alongside the `Verbatim`/`VerbatimOrigin` types they measure, since a probe can't be
fixture-tested before its target type exists. `design/bynk-greenfield-compiler.md`'s Appendix D
carries no R7/R8 rows yet (its own note: "most of phases 6–8... have no live probe yet"), so this
settling pass adds none there — a future pass, once these probes exist and phase 7 is further
along, is the natural point to add them, not this one.

---

## 9. Risks

**The `Verbatim` hatch is blind to its own content without the textual lint (§3.2).** A byte-
golden fixture proves nothing about what's inside an opaque `Verbatim` block — P7.5 must ship the
lint alongside the hatch, not as a follow-up, or Arc C's own conversion work has no way to detect
a banned construct smuggled through unconverted.

**§3.3's residual (2–3 sites) is a named dependency on R7.7's runtime-typing work, not a Arc-C-
internal task.** If the runtime types for `ValidationError`/`JsonError`/`HttpResult`/
`QueueResult` aren't exported in time, the `serialisation.rs` conversion slice either blocks or
ships with a residual `any` — named here so it's tracked, not discovered late the way phase 5's
`secrets.rs` open item was.

**Golden-test churn is the whole safety net for Arc C, and it is finite.** Every conversion slice
must produce byte-identical output; if a conversion changes a byte, the phase loses its only cheap
correctness check for ~1,540 sites simultaneously. `project/tests_emit.rs` converts to node
assertions last, not first.

**R8.12 self-supersedes at the exact moment R7.1 lands (§3.4).** A reviewer checking Arc B's `Any`
elimination against Appendix B's own "closed" marking for R8.12 could misread the change as a
regression rather than the rule's own intended completion — P7.d8 exists to make this explicit,
not assumed.

**A completion criterion stated as counts, not volume, is gameable — found in PR review, fixed in
§5.** The original two-probe pairing (`ts_writes` = 0, `verbatim_origins` at a small floor) was
satisfiable by wrapping the two largest, least-decomposed files (`emit.rs`, `lower.rs`) into one
`VerbatimOrigin` variant each and never touching them again — trajectory §2's own failure mode,
reached through a green gate. `verbatim_sites` (§5, retiring at 0) closes this: it counts
construction call sites, not variants, so a wholesale wrap of a large file is visible as hundreds
of new sites rather than one new variant. Named here as a risk this track's *own* review process
already caught once — evidence the discipline works, not just an assumption that it does.

**The evidence ages.** Every fact, line number and count in this doc was measured against `main`
at commit `79053b09` on 19–20 August 2026; the `verbatim_sites` addition (§5) was measured against
the same commit, added after a PR review on 20 August 2026 found the two-probe version gameable.
Re-check before a slice proposal cites one.

---

## 10. What this phase causes

Phase 8 needs phases 3, 4 **and** 7 (trajectory §4): "3 and 4 before 8. The firewall needs stable
identity and a project model that is not ambient" — and §4's ordering diagram shows 7 feeding 8
directly, since a query firewall over an emitter that still writes strings ad hoc has no stable
unit to memoize against. This phase's `Artefacts` (R7.8) is plausibly phase 8's `UnitSignature`
target for the emit side, and R8.16's deferred data-model half (a typed `ProjectGraph`) is
literally phase 8's own scope per phase 4's retirement note — worth phase 8's own settling review
treating these as linked, not independently rediscovering the connection.

---

## 11. ADRs

Per ADR 0167 step 2, load-bearing, hard-to-reverse decisions land before slicing. Four do, with
this settling pass (numbers assigned at merge by the stamp; referred to by letter until then):

- **ADR-A** — `bynk-ts` is carved as a crate in the first Arc B slice (P7.5), not built in-module
  and carved later. §3.1 (Q1).
- **ADR-B** — the migration escape hatch is `TsStmt::Verbatim{origin: VerbatimOrigin, text:
  String}`, statement granularity, sealed constructor, with a companion textual lint over its
  content run in CI alongside golden fixtures, and completion tracked by three probes —
  `ts_writes`, `verbatim_origins`, `verbatim_sites` (§5) — not two, after PR review found the
  two-probe version gameable by a wholesale, undecomposed wrap of a large file. §3.2 (Q2). Likely
  the most load-bearing of the four — it's what makes every Arc C slice individually safe to ship
  and individually safe to stop after.
- **ADR-C** — `TsType::Any` is eliminated in full within this phase; the 2–3-site residual
  (runtime-owned error types) is named and deferred to R7.7's runtime-typing work, not treated as
  open-ended or as grounds to re-open phase 6's IR. §3.3 (Q3).
- **ADR-D** — R8's scope splits three ways: five rules close as a byproduct of Arc B/C (no
  separate slice), two rules (R8.2, R8.14) get named Arc D slices, R8.20 folds into Arc A's P7.4,
  R8.16's data-model half stays phase 8's, and the twelve already-closed rules get a verify-only
  pass with R8.12's self-supersession flagged explicitly. §3.4 (Q4).

---

## 12. Retirement

Mirrors every prior track on this trajectory: retires when `ts_writes` reads 0,
`verbatim_origins` reads its own argued floor, and `verbatim_sites` reads 0, with every surviving
site named file-by-file in the closing summary. The retirement PR removes this doc, appends its
closing summary to
`../archive/retired-tracks.md`, and closes the spine issue
([#1293](https://github.com/accuser/bynk/issues/1293)).
