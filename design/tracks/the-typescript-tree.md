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

**R7.7 is closed — confirmed by P7.1 (#1298), not just aged out.** Finding #17 reported ~300 lines
of harness TypeScript as Rust string literals beside an `include_str!` of `runtime.ts`. Verified
against the current tree: every runtime-shaped `.ts` file is `include_str!`'d from a real file —
`emitter.rs:94` (`runtime.ts`), five sites in `project/tests_emit.rs` (`test_runtime/*.ts`), and
four in `bynk-check/src/firstparty.rs` (the vendored first-party bindings) — and no hand-written
runtime TypeScript survives as a Rust string literal anywhere in `bynk-emit/src` (a search of every
`r#"..."#` raw string found each one is a *Bynk* source fixture for compiler tests, not
TypeScript). Coverage is real CI enforcement, not just a config file claiming strict mode:
`bynk-emit/runtime/tsconfig.json` sets `strict`/`noImplicitAny: true`; `.github/workflows/ci.yml`'s
`runtime` job runs `tsc -p tsconfig.json` plus a bundle-drift guard; `bynkc/tests/tsc_verify.rs`
additionally stages every positive fixture project — exercising the runtime, the `test_runtime`
helpers and the first-party bindings together — into one tree and runs `tsc --strict --noEmit`
over it, with `BYNK_REQUIRE_TSC=1` set in CI so a missing `tsc` hard-fails rather than silently
skipping. Neither Appendix B (historical rationale, not a live-status tracker — R7.7's row stays
accurate regardless of current closure) nor Appendix D (which carries no R7.7 row at all — it does
carry R7.1/R7.3/R7.8 rows, correcting this track's own earlier assumption that it had none; R7.1's
own row is now stale against this track's own Q3 finding, 55 sites not "two", flagged for a later
slice, out of P7.1's own scope) needs a change for R7.7.

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

**P7.2 (#1300, landed) re-settles this estimate a second time — the real number is neither ~20 nor
50.** #1300's own filing already corrected the settling-time "~20 narrowable" to a fresh,
exhaustive 55-site count (50 narrowable, 2 residual, 3 deferred) before implementation began.
Implementation itself found roughly half of the 50 "narrowable now" sites were not: a real
`tsc --strict` pass over all 270 positive fixtures (Decision B, exactly as the proposal specified)
caught several classes the file-level read missed — a bare/unqualified type colliding with a
`handlers`-namespace name or a DOM-ambient global of the same name; `unknown`/`unknown[]` breaking
downstream type inference for callers that need the real element or field type (the query-algebra
methods, a discriminated-union field access); a local fallback type colliding with a differently
shaped real imported type of the same name; and, in three places, a commons-imported (rebranded)
type reaching a compose-layer boundary without the brand-assertion cast `workers_entry.rs`'s own
`on call` dispatch already carried for exactly this gap (`brand_assertion`, threaded to the `on
http` and queue dispatch paths that lacked it — a real, previously-latent `tsc --strict` gap this
narrowing surfaced, not introduced, since it was invisible while those parameters were still
`any`). Each such site was re-classified DEFERRED on the spot, per the proposal's own named risk,
with a comment at the site recording what was tried and why. **Landed: `ts_any` 55 → 31** (24
sites narrowed for real; 2 residual plus 29 deferred, up from the proposed 2 + 3 — the
implementing PR carries the full site-by-site accounting, not repeated here). The completion
criterion (§5) is unaffected: `ts_any`'s eventual floor is still argued at retirement, not fixed
at filing time, and every deferred site already carries its own named reason rather than a silent
drop.

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

`ts_writes` and `ts_any` were buildable against a real baseline (§1's own measurement) without
`Verbatim` existing, and landed in **P7.0** (#1296, shipped — `xtask/src/greenfield_status.rs`
carries 15 probes today, gated on `ts_writes` = 1641, `ts_any` = 55). `verbatim_origins` and
`verbatim_sites` scan for patterns (`VerbatimOrigin`/`Verbatim::new`) that don't exist in source
until **P7.5** builds them — and, per this codebase's own probe-fixture discipline (every probe
needs a fixture exercising its stated hazard, not just its happy path, the precedent T0.0's own
"Done when" set), a probe can't be meaningfully fixture-tested against a type that doesn't exist
yet. So they're built in P7.5, alongside `Verbatim`/`VerbatimOrigin` themselves, not stubbed early
in P7.0. Per Q3, the `ts_any` probe scans for `as any` **and** bare `: any` (and, per review of
#1297, `any` in generic type-argument position — widened after landing found the narrower
definition under-counted a live case) — a probe scoped to `as any` alone under-counts. Per Q2,
`Verbatim` content also needs a textual lint (banned constructs, run in CI) as a companion to the
golden fixtures, since golden output alone is blind to what a `Verbatim` block hides.

---

## 6. Slice decomposition

§3 is settled; all slices below are accepted, buildable, not forward references. **37 slices**,
tightened from the provisional 35–45 range: Arc A gained one slice (Q3's finding that most `any`
narrowing is a plain text change, doable before the tree exists), Arc D gained precision (two
named rule-closing slices, R8.2/R8.14, replacing an unscoped placeholder).

**Arc A — independent of the tree, land immediately (5 slices)**

| Slice | What lands | Rules | Gated on |
|---|---|---|---|
| **P7.0** (#1296, landed) | `ts_writes` (1641) and `ts_any` (55, widened for generic-position `any` per review of #1297) gated probes in `xtask/src/greenfield_status.rs`, following `hoist_sinks`'s line-scan pattern, excluding `xtask` from its own count | instrumentation | — |
| **P7.1** (#1298, landed) | Verification pass — R7.7 confirmed closed, §1's own hedge firmed up with the evidence; no Appendix B/D change needed (checked, not stale for R7.7) | R7.7 | — |
| **P7.2** (#1300, landed) | Narrowed 24 of the 50 classified-narrowable `as any`/bare-`: any` sites (§3.3) to `unknown`, real declared/qualified types, or generated structural types; the other 31 (2 residual + 29 deferred, up from the proposal's 3) named and left as `any`, each with a site comment recording why — `ts_any` 55 → 31 | R7.1 (partial, ahead of the tree) | §3.3 |
| **P7.3** (#1303, landed) | `TomlDocument`/`TomlBlock`/`TomlValue` + a minimal TOML printer (`emitter/toml_doc.rs`, new); `emit_wrangler_toml` (`wrangler.rs:56`) returns a document, not a `String` — its one caller (`project.rs:2272`) prints it. String-escaping (`escape_toml_basic_string`) relocated to the printer and applied unconditionally to every string value, not just the two call sites (`crons`/`queues`) that used to remember to call it. Zero-diff across the whole `wrangler.toml` golden corpus, confirmed in the implementing PR | R7.8 (part) | — |
| **P7.4** (#1305, landed) | **Closes R7.6 and R8.20** — three structural read/write functions added to `bynk-emit::emitter::wrangler` (`set_wrangler_main`, `materialise_kv_namespace_id`, `wrangler_needs_kv_materialisation`), parsing an *existing* `wrangler.toml` via `toml::Table` (the construction-side `TomlDocument`, P7.3, builds fresh — this reads back what was already emitted). `bynk/src/deploy/config.rs`/`deploy/ledger.rs` and `bynk-strip/src/lib.rs` call them instead of text-matching `KV_NAMESPACE_ID_PLACEHOLDER`/`main = "index.ts"`; `bynk-strip` needed no new production dependency (the parse lives in `bynk-emit`, which it already depends on). Nothing here runs at compile time — zero diff across the whole `bless_positive_fixtures` corpus | R7.6, R8.20 | P7.3 |

**Arc B — the crate and the spine (5 slices, all landed as of P7.9/#1315)**

| Slice | What lands | Rules | Gated on |
|---|---|---|---|
| **P7.5** (#1307, landed) | `bynk-ts` crate carved up front (§3.1), depending on `bynk-syntax` only (`cargo tree -p bynk-ts` confirmed). `TsProgram`/`TsStmt` (one variant, `Verbatim`, sealed behind a single `TsStmt::verbatim` constructor); `VerbatimOrigin` seeded with the three origins Arc C's own first slice needs (`Contracts`/`Secrets`/`RuntimeUse`), not the whole ~19-slice schedule; a printer owning one buffer for the whole tree (no splicing, so it records source-map checkpoints straight from each statement's own span); the companion textual lint over `Verbatim` content (§3.2), unit-tested against all six banned constructs. `SourceMap` relocated from `emitter/source_map.rs` unchanged — `bynk-emit`'s current splice-based lowering keeps calling the identical API from its new crate path; the printer uses the same type a second, simpler way (no `merge`). `verbatim_origins`/`verbatim_sites` gated probes added, both reading **0** — no real `bynk-emit` construction site builds a `TsProgram` in this slice; that's Arc C's own first slice. Zero-diff across `bless_positive_fixtures` (nothing here runs at compile time) | R7.2, R7.3, R7.4, R10.1, R10.3 | §3.1, §3.2 |
| **P7.6** (#1309, landed) | `Artefacts { docs: BTreeMap<PathBuf, Document> }` **replaces** `CompiledFile`/`ProjectOutput.files` outright — not aliased, per the accepted proposal's own "Done when". `Document` (`Ts(bynk_ts::TsProgram)` / `Toml(TomlDocument)` / `Json(String)` / `Js(String)` / `SourceMap(String)` / `DebugSidecar(String)`) is the one typed storage every real producer/consumer now reads and writes: `bynk-driver::output` (`write_output`/`write_compiled_file`/`expected_output_paths`, consolidated to one sibling-path derivation), `bynk-driver::test_runner`, `bynk-strip::strip_project_to_js` (dispatches on `Document` directly — `wrangler.toml`'s `main` patch is now a structural in-tree `TomlDocument::set_main` mutation, not a print-then-`toml_edit`-reparse, closing R7.6 the way Decision E named as an opportunity), `bynk-wasm::compile_inner`, and `bynkc`'s re-exports plus its six directly-coupled `tests/*.rs` fixtures. `Document::Js` is new relative to the proposal's own sketch — `bynk-emit` never produces it, only `strip_project_to_js`'s own output, kept distinct from `Ts` so a stripped artefact can never round-trip back through the TypeScript printer/lint. Each of the 11 TS-producing `bynk-emit::project`/`project/tests_emit.rs` construction sites wraps its still-`String`-producing content in its own literal `TsStmt::verbatim(VerbatimOrigin::NotYetConverted, ..)` call — not funnelled through one shared helper, which would collapse `verbatim_sites` to 1 regardless of how much is actually unconverted. `wrangler.toml`'s real `TomlDocument` (Decision E) reaches `Artefacts.docs` unstringified; `emitter::print_toml_document` now runs only at the write boundary (`Document::text()`, or `bynk-driver::output`'s own dispatch). `verbatim_origins`/`verbatim_sites` (P7.5's own gated probes) move from 0/0 to **1/11**. Zero-diff across `bless_positive_fixtures`/`positive_fixtures`/`tsc_verify`. First implementation attempt kept `CompiledFile` alive as a parallel flattened-text view rather than replacing it — caught in review against the accepted proposal's own explicit "gone, not aliased" line before merge, and redone as a full cutover; worth remembering as a precedent that "behaviour-preserving" doesn't license softening an accepted scope's own stated done-condition without going back for a fresh decision | R7.8, and (via `strip_project_to_js`'s `set_main`) closes the last textual-reparse corner of R7.6 | P7.5 |
| **P7.7** (#1311, landed) | Names the printer's *current* readability guarantee as R7.5's own "policy with a name and a test" — deliberately not a target formatting spec for P7.8's own not-yet-built tree. A `# Readability policy (R7.5)` doc block in `bynk-ts/src/printer.rs` states the one real formatting decision the printer makes today (one generated line per statement, always, regardless of the wrapped `Verbatim` text's own trailing whitespace) and names the boundary explicitly: a statement's own interior formatting is `bynk-emit`'s pre-tree output, not a printer decision, until Arc C gives the printer real nodes. The two existing statement-separation tests (`prints_every_statement_in_order`, `a_statement_missing_its_own_trailing_newline_still_gets_its_own_line`, both P7.5/#1307) get a doc comment tying them to the named policy by rule, not just by review-history archaeology. No behaviour change — `print`'s own logic is untouched, zero diff across `bless_positive_fixtures`/`positive_fixtures`, `ts_writes`/`verbatim_origins`/`verbatim_sites` unaffected. The rest of R7.5 (indentation, blank-line placement, brace style) stays open, deliberately, until P7.8 gives the printer real structured nodes to have an opinion about — writing that policy now would be prose speculating about a shape nobody has built yet, the same trap `bynk-ts/src/lib.rs`'s own module doc already named for the tree's structure, applied here to its formatting | R7.5 (part — today's real surface only) | P7.5 |
| **P7.8** (#1313, landed) | `TsStmt`/`TsExpr`/`TsType`/`TsDecl` — not the full §7.1 sketch as literally written (a variant-name list with almost no field-level design), but the subset `bynk-emit/src/emitter/events_fanout.rs` concretely needs, grounded against that file's own real shape (see the Arc C schedule correction below — this is where the correction was found). Adds `TsExpr`/`TsType`/`TsDecl` to `bynk-ts::program`; `TsStmt` gains `Const`/`Let`/`ExprStmt`/`Return`/`If`/`ForOf`/`TryCatch`/`Block` alongside `Verbatim`, plus `Continue` — a second real gap found beyond the accepted proposal's own variant list (`events_fanout.rs` uses a bare `continue;` twice; `TryCatch` was the first gap, already named in the proposal itself, since §7.1's own sketch has no `try`/`catch` at all). `TsType::Named` carries type arguments (`Record<K, V>`/`Promise<T>`), a second sketch gap the proposal found. The printer (`bynk-ts::printer::print`) learns to render every new kind — real indentation, blank-line, and brace-placement decisions, all named in its own extended readability-policy doc block (R7.5), matching exactly what `events_fanout.rs` itself does (2-space indent; one blank line between top-level declarations except adjacent `import`s; no blank between a class's fields and its constructor, one before each method; `if`/`for...of` bodies brace when the body is a `Block`, print inline otherwise). Verified by constructing `EventsFanoutDO`'s own class (field, constructor, `fetch` method — every control-flow and expression shape the real file uses) as real nodes and asserting the printed text is byte-identical to the real emitter's own `write!`-built text, transcribed directly — proving the algebra is sufficient for its own grounding case, not merely plausible. First implementation pass found one real gap the exercise couldn't close (no `Assign` statement existed, so the constructor's own `this.env = ...;` needed a `const`-binding stand-in in the test rather than the real statement) — caught in review before merge: the constructor's body wasn't actually catalogued by the accepted proposal's own grounding work (it detailed the `fetch` method, not the constructor), a third real gap beyond the proposal's own variant list. Closed by adding `TsStmt::Assign { target: TsExpr, value: TsExpr }` (mechanical, matching the existing `Const`/`Let` pattern exactly) and updating the grounding test to use it — now genuinely byte-identical against the real constructor body, not an approximation. `bynk-emit` unchanged — nothing there builds a new node yet; `ts_writes`/`verbatim_origins`/`verbatim_sites` unaffected | R7.1 | P7.5 |
| **P7.9** (#1315, landed) | The first slice where `bynk-emit` itself changes, and Arc B's last. Corrected the track doc's own stale P7.9 citation before implementation (three ways: `ty_to_type_ref` builds a different Bynk-internal type, not TS text; `pred_condition_and_message` builds a runtime-check expression + message, not a type; both omitted, the citation also omitted `ts_ty`, the dominant real builder). `ts_type_ref`/`ts_type_ref_qualified`/`ts_type_ref_qualified_multi`/`ts_type_ref_with`/`ts_ty` (`emitter.rs`) construct a real `bynk_ts::TsType` internally and print it through a new `bynk_ts::print_type`, instead of hand-`format!`-ing text — every function keeps its exact `-> String` signature, every one of its ~110-115 real callers unchanged. (`ts_base` itself is untouched — still an unchanged string-table lookup, `Review of #1315/#1316` caught it credited alongside the converted functions when it isn't one; now consumed by them instead of interpolated directly.) `TsType::Array` gains a `readonly` field; `TsType::Fn { params, ret }` added (positional `a0`/`a1`/… parameter naming, matching the pre-P7.9 convention exactly). Two further real gaps found during implementation, beyond the accepted proposal's own two (`readonly`/`Fn`): (1) `Query[T]`'s real shape wraps its whole function type in an *extra* outer paren pair (`(() => readonly T[])`), caught only by the zero-diff fixture check (`302_query_annotated_let`), not by reasoning about the algebra — closed by pre-rendering that one shape's text and carrying it as an inert `TsType::Named`, a deliberate, narrow representational choice rather than a new general paren-wrapper variant; (2) a resolved multi-actor sum (`Ty::ActorSum`) builds a genuine type-position union of tagged object literals (`{ tag: "a" } | { tag: "b", identity: T }`) — closed by adding `TsType::Union(Vec<TsType>)`, and each member printed the same opaque-`Named`-verbatim-text way as the `Query` fix (the members' own real separator is `, `, not the ordinary `TsType::Object` renderer's `; ` — caught by a direct test, not reasoning). First implementation pass stopped short of converting `ts_ty` at all rather than invent the `Union` variant unilaterally (flagged explicitly, `ts_writes` only reaching 1631); reviewed and completed in the same PR before merge, since the gap was real and grounded, not speculative — the same "close it, don't substitute around it" call this track made for P7.8's own `Assign` gap. Zero diff: `bless_positive_fixtures`/`positive_fixtures`/`tsc_verify`'s full strict-`tsc` corpus all pass unchanged; 23 new direct tests (16 for `ts_type_ref`, 7 for `ts_ty`, plus 3 in `bynk-ts` for the new `TsType` shapes) pin every real shape category against each function's exact pre-slice text. `ts_writes` 1642 → **1620** (22 — both families' own `format!`/`write!` calls, minus the few that legitimately remain for the `Query`/tag-literal opaque-text construction). `ast_importers` stays at 5, as the accepted proposal's own Framing predicted (`emitter.rs` imports 23 other AST items well beyond what this family touches) — not a regression, real future Arc C work. `verbatim_origins`/`verbatim_sites` unaffected | R7.1, R7.2 | P7.8 |

**Arc C — conversion, smallest blast radius first (~23-27 slices, revised again by #1335's own
step (2) split, #1337's own step (3) closure, and #1339's own step (2) closure — see the "Revised
estimate" paragraph below; was
~19 pre-Arc-C, ~12-18-more-from-here right after the design pass settled the `lower.rs` question,
honestly no longer close to the original ~19)**

`events_fanout.rs` (**landed**, Arc C's own slice 1, #1317 — see below) → `workers.rs`
(**landed**, Arc C's own slice 3, #1321 — see below) → `workers_entry.rs`
(**landed**, Arc C's own slice 4, #1323 — see below) → `emit_test_main` (**landed**, slice 5,
#1325) → `emit_composition_root` (**landed**, slice 6, #1327) → `emit_commons_barrel` (**landed**,
slice 7, #1329) → a design/decomposition pass (**landed**, #1331 — see "Third correction" below;
converts no code, settles the `lower.rs` question and names the order for everything after) →
`emit_doc_block` + `ts_type_params` (**landed**, slice 8, #1333 — closes step (1) of #1331's own
decomposition order, see "Fourth correction" below) → `emit_refined_checks` + `emit_pred_check`
(**landed**, slice 9, #1335 — corrects step (2)'s own scope, see the table row below) →
`emit_attached_methods` + `emit_forwarded_methods` + `emit_method` (**landed**, slice 10, #1337 —
closes step (3) named by function, not absolute line range, per slice 9's own established
convention; also unblocks step (2)'s own deferred `emit_type`/`emit_refined_type`/
`emit_record_type`/`emit_sum_type` trio — see the table row below) →
`emit_type`/`emit_refined_type`/`emit_record_type`/`emit_sum_type` (**landed**, slice 11, #1339 —
closes step (2) fully, the deferred dispatcher-plus-trio, see the table row below) → the
ICU-formatting cluster
(step (11), not yet proposed) → `emit_integration_module` + `emit_test_module` (unblocked by
#1331, not yet proposed) → `emit.rs`'s own remaining tree, leaf-to-root per #1331's own named order
(not yet proposed) → `emit_project` itself, last (not yet proposed). `project/tests_emit.rs`
converts its 130
byte-golden assertions to node assertions **last of all**, after everything above. Each slice is
checked against the P7.5 textual lint, not golden fixtures alone (§3.2). **`emitter/lower.rs`
itself does NOT appear in this ordering** — #1331 settled it as a deliberate, permanent exclusion
from Arc C's own scope, not a file waiting its turn; see "Third correction" below.
**Correction (found by slice 1's own grounding, #1317): "each slice deletes its own
`VerbatimOrigin` variant" is stale** — it describes P7.5's original per-file seeding
(`Contracts`/`Secrets`/`RuntimeUse`), but P7.6 (#1309) replaced that with one shared
`VerbatimOrigin::NotYetConverted` every current construction site uses; there is no file-specific
variant for a slice to delete. `verbatim_sites` (the call-site count) is what tracks each slice's
own progress instead.
**Second correction (#1319): `serialisation.rs` is removed from this ordering entirely.**
Grounded directly: `serialisation.rs` is 1821 lines / 237 `write!`-family sites, but structurally a
shared codec-generation *library* — 10 separate `pub(crate)` entry points, each a `&mut String`-
accumulator called from multiple sites inside `emitter.rs`'s own still-unconverted code — not
`events_fanout.rs`'s "one function, one file, one clean call site" shape. Converting it needs its
own dedicated design pass (does it ripple into `emitter.rs`'s own still-`writeln!`-based callers, or
need an as-yet-undesigned "sub-program merged into a splice point" mechanism?), not a slice sized
against the precedent — it will get its own increment proposal when that design work is ready, not
occupy this ordering's "slice 2" slot by default. `workers.rs` moves up to fill it: `emit_worker_
compose` has exactly one external caller (`bynk-emit/src/project.rs:2291`), the same clean shape
`events_fanout.rs` had. §3.3's own 2–3-site `ts_any` residual (`ValidationError`/`JsonError`/
`HttpResult`/`QueueResult` casting through `any`) landed independently of any tree-conversion slice
(#1319, `ts_any` 31 → 30 — review of the implementing PR found `HttpResult<T>`'s own bare-generic
cast failed `tsc --strict`, closed by casting through `unknown` uniformly, which incidentally also
made the pre-existing `Effect` case's own `any` visible to the probe for the first time; a real
correction, not a regression) — its own blocking dependency (a real, exported runtime type for each of
the four) turned out to already be satisfied (`bynk-emit/runtime/src/errors.ts`/`queue.ts`/
`http.ts`), just never wired into either header-assembly function's own conditional-import scan;
closed by naming the real type per `WireRef::UncheckedReason`/`TypeRef` arm instead of sharing one
`any` arm, and by adding a `QueueResult` field/type-declaration structural scan
(`file_mentions_queue_result`) to both `write_header` and `write_header_single` — the one gap among
the four that a purely handler-based check (`has_queue`) never covered, mirroring the scan
`JsonError`/`HttpResult` already had. Whichever future slice actually converts `serialisation.rs`
inherits real types at these two sites instead of a residual `any` to solve itself.

**Third correction (#1331, a design/decomposition increment — converts no code, landed as its own
accepted proposal the same way this track's own opening settled §3's design questions): the
`emitter/lower.rs` (several slices) → `emitter/emit.rs` (several slices) ordering this section
originally named is retired, not merely reordered.** Once slice 7 (#1329) landed, the real
remaining surface was re-grounded directly against the repo rather than trusted from this section's
own pre-Arc-C prose: `emit_project` (`emitter.rs:319-579`, 261 lines, one caller) is a thin
orchestrator whose direct calls span `emitter.rs`, `emitter/emit.rs` (4,776 lines, **72** top-level
functions, 410 write-macro calls), and — pulled in transitively through seven of `emit.rs`'s own
top-level functions (`emit_method`/`emit_free_fn`/`emit_contract_guarded_body`/`emit_provider`/
`emit_service`/`emit_agent`/`emit_ws_do_method`, via `emit_block_as_function_body_with_return`,
`lower.rs:201`) — `emitter/lower.rs` (6,210 lines, 372 write-macro calls). `emit_agent` alone
(`emit.rs:2638-3949`) is **1,312 lines**, the single largest function in the crate; its own
websocket-dispatch cluster (`emit.rs:3950-4776`) adds another **827** lines, so agent-plus-cluster
is **2,139** lines — 45% of `emit.rs`.

**`emitter/lower.rs` turned out to be categorically different from every other Arc C target, not
merely large.** It is the compiler's own **second code-generation pass** — general expression
lowering (`lower_method_call` alone is 1,044 lines), match-to-IIFE compilation, ten-plus
per-builtin-type "kernels" (`lower_numeric_kernel`/`lower_duration_kernel`/`lower_bytes_kernel`/
`lower_string_kernel`/`lower_option_kernel`/`lower_result_kernel`/`lower_effect_result_kernel`/
`lower_map_kernel`/`lower_list_kernel`/`lower_query_method`), if/binary-op/field-access/lambda/
record-spread lowering, and indexed-collection index-maintenance codegen — covering the entire
Bynk expression and statement grammar at once, not a bounded, file-specific set of real constructs
the way every other Arc C target (including ones that looked intimidating by line count alone,
like `workers_entry.rs`'s 1,660 lines) turned out to be once actually read. Converting it for real
would mean re-architecting the compiler's whole lowering strategy to be tree-native, comprehensive
language-surface work this track's own trajectory doc never scoped Arc C to cover — not "wrap this
function's existing string-building in real nodes," the scope every landed Arc C slice has actually
been. **Decision (#1331): `lower.rs`'s real output stays a `String` permanently, carried as opaque
pre-rendered text at its one well-defined splice boundary
(`emit_block_as_function_body_with_return`'s return value) wherever `emit.rs`'s own future slices
need it — the same "existing textual variant carries content this crate structurally can't build a
shape for" pattern `workers.rs`'s/`workers_entry.rs`'s own `deserialise_call`/`brand_assertion`/
`claim_predicate_to_js`, and #1327's own `__eventsDispatch` closure body, already established, just
applied once at this one splice boundary instead of many small ad-hoc ones. This does not block any
future `emit.rs` slice — each of `emit_method`/`emit_free_fn`/`emit_provider`/`emit_service`/
`emit_agent`/etc. converts its own *wrapper* (signature, decorators, surrounding declaration shape)
to real tree nodes while carrying its own spliced *body* as one opaque blob.**

**This is a real, permanent, deliberate narrowing of §1's own original "the tree omits nothing
real" framing (R7.1) — recorded here explicitly, not glossed over, and a corresponding note added
to `design/bynk-compiler-trajectory.md` itself (R7.1's own text) at #1331's own landing.**
Converting `lower.rs` for real remains a legitimate possible FUTURE track if ever justified, scoped
and argued on its own terms — a fundamentally larger, likely multi-month undertaking, not a handful
of remaining Arc C slices. **`emitter/lower.rs` is deliberately NOT added to
`xtask/src/greenfield_status.rs`'s `TS_WRITES_EXCLUDED_FILES`** — that list's own doc comment
already distinguishes `ir/lower.rs` (excluded: builds Rust-internal strings during checker→IR
lowering, never emitted syntax, "not part of the emitter's rendering code at all, despite the name
proximity to `emitter/lower.rs`, which is") from `emitter/lower.rs` itself, which the codebase's
own prior authors already marked as real, in-scope emission surface. Silently excluding it now
would reverse that documented distinction. Instead: **`ts_writes` keeps counting `lower.rs`'s own
real write-macro calls, and Arc C's own eventual retirement review names an argued, non-zero floor
for `ts_writes`** — the same honest-correction shape `verbatim_sites`'s own floor (at least 2,
named at slice 5) and `ast_importers`'s own floor (5, named at phase 6's own retirement) already
took, rather than chasing an unreachable 0.

**The real decomposition order for what remains, leaf-to-root** (given the `lower.rs`-stays-opaque
decision, `emit.rs`'s own remaining work is converting its 72 functions' own *wrapper*
construction, not their spliced bodies). **Correction, review of #1332: the first draft of this
list gave two ranges that silently double-claimed lines already assigned elsewhere, and omitted a
real cluster of small functions entirely — every range below was re-derived directly against
`emit.rs`'s own function boundaries, not estimated.**

**Fourth correction (#1333, landed): step 1's own list needed re-grounding too, the same "verify
every named function before scoping" lesson applied one level down.** Reading every one of step
1's 13 named functions in full found **9 are not TypeScript-emission code at all**:
`sorted_index_fields`/`topo_order_providers`/`sanitise_path_segment` are pure Rust logic (a
deterministic sort, a topological sort, a string sanitiser — no TS text anywhere); `http_handler_
method_name`/`http_handler_method_name_ir`/`http_handler_method_name_from_str`/`cron_handler_
method_name`/`queue_handler_method_name` build plain identifier-name strings via `format!`, not a
structured shape a `bynk-ts` node would represent any differently than the `String` they already
return; `collect_handler_labels`/`handler_op_label` build **JSON** for the debug-metadata sidecar
(ADR 0105), a genuinely different output *kind* entirely, structurally out of scope for `bynk-ts`
(which only represents TypeScript) the same way `emit_wrangler_toml` needed its own
`TomlDocument`, not a `TsProgram`. **None of these 9 is ever an Arc C target** — recorded here
explicitly, the same way the two permanent `verbatim_sites` (`adapter_bindings` content,
`emit_runtime_module`) are named, not silently dropped from a future slice's own scope. The
3-function ICU-formatting cluster (`emit_message_entry_renderer`/`emit_icu_placeholder`/
`emit_sub_message`, `emit.rs:864-1008`) is real TS-emission code — genuine IIFEs with real
branching, nested computed-key object literals — but far too substantial to be a "leaf helper";
**re-scoped to its own future dedicated slice**, the same way `emit_agent`'s own sub-tree (step 9
below) is already flagged as needing its own dedicated grounding pass. **Step 1's real remaining
scope was therefore exactly 2 functions** — `emit_doc_block` and `ts_type_params`, both **landed**
in #1333 (both kept their exact existing `-> String`/`out: &mut String` signatures, the P7.9
pattern, since neither owned its own `Verbatim` construction site — `verbatim_sites` unaffected by
this slice, only `ts_writes` moved). **Step (1) is closed by #1333** — its only remaining real
content, the ICU-formatting cluster, is tracked below as its own future step (11), not left as
step (1)'s residue. Steps (2)-(10) are
unaffected by this correction, their own numbering unchanged: (2) the type-declaration cluster
`emit_type`/`emit_refined_type`/`emit_refined_checks`/`emit_pred_check`/`emit_record_type`/
`emit_sum_type` (`emit_type` through `emit_sum_type`, immediately preceding `emit_attached_methods`
in file order — **named by function, not by absolute line range**: this span's own line-number
citation went stale twice in a row, first by 12 lines then, within #1335's own correcting commit,
by another 69 as that same slice's own new code shifted every later line — the second time is the
signal to stop citing absolute lines here at all; excludes `emit_doc_block`/`sorted_index_fields`/
`ts_type_params`, step (1)'s own former leaf helpers, which lived inside this same span but were
scoped separately and are now landed/excluded per the correction above). **Fourth correction
(#1335): step (2)'s own 6-function list does not convert as one unit.** All 6 genuinely build real
TypeScript (no repeat of step 1's "9 of 13 aren't emission code" surprise), but 3 of the 6
(`emit_refined_type`/`emit_record_type`/`emit_sum_type`) call `emit_attached_methods` (step (3)'s
own target, not yet converted) MID-CONSTRUCTION, splicing its own variable-count method-entry
output into the same `export const {...}` object literal before closing it — `bynk-ts::
TsObjectEntry`'s 4 existing variants (`Prop`/`Shorthand`/`Method`/`Spread`) can't represent "zero or
more pre-rendered entries appended by not-yet-converted code." **Landed in #1335: `emit_refined_
checks`/`emit_pred_check` only** (self-contained, no entanglement with `emit_attached_methods`) —
both kept their exact existing signatures, the P7.9/step-1 pattern (`verbatim_sites` unaffected,
only `ts_writes` moved). `emit_type` (a pure dispatcher, builds no TS text of its own) and the 3
object-builders (`emit_refined_type`/`emit_record_type`/`emit_sum_type`) are **deferred to a
follow-up slice, gated on a `TsObjectEntry::Raw`-style opaque-entries-carrier decision** (or on step
(3) landing first, if a future proposal prefers reordering over adding that carrier) — named
explicitly here so a future reader doesn't assume "step (2) = one slice" or that step (2) is fully
closed. `emit_pred_check`'s own real algebra need: `pred_condition_and_message` (`emitter.rs:4966`,
still unconverted) returns `(cond, msg)` as a raw `(String, String)` pair; `cond` is carried as
opaque `TsExpr::Ident` wrapped in the existing `Not`/`Paren`, matching the accepted proposal's own
Decision B — but `msg` is carried the SAME opaque way, **a real deviation from Decision B's own
"msg as an ordinary `TsLit::Str`"**: `PredKind::Matches`'s own message embeds `escape_ts_string`-
escaped pattern text directly, and running it through `TsLit::Str`'s own escaper a second time would
double-escape every backslash the pattern contains — a real, if currently unreachable (no fixture
uses a `Matches` pattern with a backslash), correctness bug closed proactively, the same "not
reachable today, real bug class tomorrow" discipline #1322/#1324 already established, pinned by a
dedicated test proving the guard would have caught it (temporarily reverted to `TsLit::Str`,
confirmed the message quadruples the pattern's backslash, restored). No new `bynk-ts` shapes needed
at all — `If`/`Return`/`Object`/`Unary`/`Paren`/`Str` all already existed. `ts_writes` dropped by
**8**, not the accepted proposal's own estimated 9 (9 real `writeln!` sites removed, offset by 1 new
`format!` call needed to build `msg`'s own pre-quoted opaque text) — a small, honestly-corrected
delta, the same class of probe-arithmetic correction #1329's own `ts_writes` finding required.
**Step (2) is fully closed by #1339 — the deferred dispatcher (`emit_type`) plus the 3 object-
builders (`emit_refined_type`/`emit_record_type`/`emit_sum_type`), unblocked by #1337's own
`emit_attached_methods` signature change** (see the table row below for the real algebra gaps and
the carrier reuse). **Step (3) is closed by #1337 — the method-emission cluster `emit_attached_methods`/
`emit_forwarded_methods`/`emit_method` (named by function, not by absolute line range, per the
same staleness lesson above; see the table row below for its own real gaps and its landed
signature change), which also unblocks step (2)'s own deferred trio** (`emit_type`/
`emit_refined_type`/`emit_record_type`/`emit_sum_type`) — `emit_attached_methods` now returns a
real `Vec<TsObjectEntry>` instead of writing text, so a future slice converting those three can
append the returned entries directly into its own `TsExpr::Object`, with no
`TsObjectEntry::Raw`-style opaque-entries carrier needed after all. **Step (4) is fully closed,
in two slices: #1351 landed `emit_free_fn`'s own outer wrapper (name/generics/params/return-type
— a real `TsDecl::Function`, which gained the `generics` field this needed); #1353 closed the
named remainder, `emit_contract_guarded_body`'s own precondition/postcondition guard `if`
statements and its trailing `return result;` (real nodes now — `bynk_ts` gained `TsStmtKind::
Throw`, its own first real gap) — the result-capturing IIFE line stays one opaque `TsStmtKind::Raw`
statement, a deliberate boundary, not a remaining gap (see the table row below for why).**
**Step (5) is closed by #1355: `emit_messages_bundle`'s own outer construction converts, with two
named opaque carve-outs — `messagesByLocale`'s own type annotation (a one-off shape `TsType::Fn`
can't name its own parameter for) and each message-entry's own renderer expression, still the
ICU-formatting cluster's own future scope (step (11), unaffected — see the table row below).**
**Step (6) is fully closed, in two slices: #1357 landed `emit_capability`'s own full conversion
(real generic, doc-commented interface methods — `bynk_ts::TsTypeMember::Method` gained
`generics`/`doc` — plus the real injection-token `const`); #1359 closed the named remainder,
`emit_provider`'s own per-op methods (each a real `bynk_ts::TsClassMethod` fragment, printed
through a new `bynk_ts::print_class_method` entry point, its own body source-mapped via a
per-method sub-builder/`merge`) — the class's own wrapper (header/`implements`/deps field+
constructor/closing brace) and the trailing factory `const` stay hand-written text, a deliberate
boundary, not a remaining gap (see the table row below for why).** (7) `emit_service`
(`emit.rs:1340-1708`, 369 lines) plus its
own direct helpers (`emit.rs:1709-2106`), likely 1-2 slices; (8) `emit_make_surface`
(`emit.rs:2107-2191`) plus the cross-context lowering cluster (`emit.rs:2192-2637`); (9)
`emit_agent` and its websocket-dispatch cluster (`emit.rs:2638-4776`, **2,139** lines — corrected
from an earlier 2,138, an inclusive-range off-by-one) — needs its OWN dedicated grounding pass
before a first slice is proposed against it, likely 3-5 slices given its real size, larger than any
single prior conversion in this whole track; (10) `emit_project` itself, last, once every function
it calls directly returns a real node instead of appending to a shared `String` — will NOT move
`ast_importers` (the same finding #1321/#1323 already made for `workers.rs`/`workers_entry.rs` —
`emitter.rs`'s own *input*-side AST walk is unchanged regardless of *output*-side conversion; state
this explicitly when that slice lands, don't assume a probe win); (11) *(added by #1333)* the
ICU-formatting cluster `emit_message_entry_renderer`/`emit_icu_placeholder`/`emit_sub_message`
(`emit.rs:864-1008`) — re-scoped out of step (1) once actually read in full (real IIFEs, real
branching, comparable complexity to already-landed slices, not a small independent utility); needs
its own grounding pass before a first slice is proposed against it, the same as step (9).
`emit_integration_module`/`emit_test_module` (`project/tests_emit.rs`, deferred by slice 7
specifically because of `lower.rs`'s own then-unresolved decision) are now **unblocked** —
proposable as ordinary slices, each carrying its own `lower_integration_case_body`-or-similar
output as opaque text per the pattern above.

**Revised estimate, corrected ten times now — by review of #1332 (the arithmetic), by #1333's
own step (1) closure (the real per-step sizing), by #1335's own step (2) split, by #1337's
own step (3) closure, by #1339's own step (2) closure, by #1351's own step (4) split, by #1353's
own step (4) closure, by #1355's own step (5) closure, by #1357's own step (6) split, and now by
#1359's own step (6) closure.** Steps (2)-(6) are all **fully landed** and entirely out of the
remaining-work sum. Summing the list directly: (8), (10) are one slice each = 2 (steps (2)-(6) all
fully landed, out of this sum entirely); step (7) is "1-2"; step (9) is "3-5"; step (11) (the ICU
cluster) is "1-2". Floor: 2 + 1 + 3 + 1 = **7**; ceiling: 2 + 2 + 5 + 2 = **11** — `emit.rs`'s own
remaining tree is now **roughly 7-11 slices** (down from 8-12, since step (6)'s own remainder is
landed). Total remaining from here: `emit.rs` (7-11) + the `tests_emit.rs` pair (2) —
#1331/#1332, slice 8 (#1333), slice 9 (#1335), slice 10 (#1337), slice 11 (#1339), slice 12
(#1351), slice 13 (#1353), slice 14 (#1355), slice 15 (#1357), and slice 16 (#1359) are all
already landed, no longer "remaining" — **roughly 9-13 more slices from here**. Arc C's own real
total (the **16** slices already landed — slice 1, the schedule-correction, slices
3/4/5/6/7/8/9/10/11/12/13/14/15/16 — plus the 9-13 above): **roughly 25-29** — unchanged, honestly,
because landing slice 16 itself was already counted inside the prior "remaining" figure as step
(6)'s own named remainder; landing it moves one slice from "remaining" to "landed" without
changing the real total. `emit_agent`'s own sub-decomposition (step 9) remains the single largest
source of variance in this range; the ICU cluster (step 11) and step (7) are smaller, secondary
sources.

| Slice | What lands | Rules | Gated on |
|---|---|---|---|
| **Arc C, slice 1 — `events_fanout.rs`** (#1317, landed) | `emit_events_fanout_do` (`bynk-emit/src/emitter/events_fanout.rs`) stops building TypeScript text with `writeln!`/`format!`/`write!` and instead constructs and returns a real `bynk_ts::TsProgram` — a deliberate signature change (`-> TsProgram`, not `-> String`), departing from P7.9's own "keep `-> String`, print immediately" posture, since this slice's whole point is the opposite: stop wrapping this file's output in `Verbatim` at all. `bynk-emit/src/project.rs`'s own construction site (`:2379`) now uses the real returned `TsProgram` directly as `Document::Ts`, no `Verbatim`. Most of the class portion's own node-construction code was already proven correct by P7.8's own grounding test (`bynk-ts/src/printer.rs`'s `prints_events_fanout_dos_own_class_byte_identical_to_the_real_emitter`) and moved over largely as-is, using the real `EVENTS_FANOUT_CLASS_NAME` constant, not that test's own placeholder `"EventsFanoutDO"` name. Two real gaps found and closed during implementation, beyond the accepted proposal's own authorised one (`TsStmt::Comment`, for the file's two-line header banner — every later Arc C slice will hit the identical need on its own first line): the dynamic `__eventRoutes` table renders as a **multi-line** object literal (one entry per line, each with its own trailing comma, closing brace at the statement's own indent) — TypeScript's ordinary multi-line convention, which nothing in `TsExpr` could represent (`TsExpr::Object`'s only existing shape is always single-line, matching real TypeScript's inline-literal convention, which is a *different*, real syntax position from this one). Closed by adding a `multiline: bool` field to `TsExpr::Object` (mirroring `TsType::Array`'s own `readonly` field) and a depth-aware `render_stmt_level_expr` wrapper — only statement/declaration-level renderers (which already carry `depth`) can render a multi-line object correctly; a `multiline` object reached through the ordinary depth-unaware `render_expr` recursion falls back to single-line rendering, a named, tested boundary, not silently wrong. Review caught two more real issues before merge: the empty-table case (a context can `ctx_uses_emit` while publishing only events nobody subscribes to) printed the tight `{}` shortcut instead of the pre-conversion code's own unconditional open/close-brace-on-separate-lines shape — fixed by dropping the shortcut entirely, `render_multiline_object` never takes it, matching the original `writeln!` code's own unconditional behaviour; and a bare `Comment` reaching `render_inline_stmt`'s fallback (e.g. as an `if`'s brace-free body) would have rendered as a `//` line comment that swallows everything after it on the same physical line, a real parse-error risk for a future slice — fixed with its own inline shape, a `/* text */` block comment, which cannot swallow anything. Verified by a fresh, direct test (`bynk-emit/src/emitter/events_fanout.rs`'s own `matches_the_real_fixtures_own_events_fanout_ts_byte_for_byte`) asserting the real, converted function's own output against a real fixture's `events_fanout.ts` byte-for-byte — not a hand-rebuilt tree standing in for it, closing the gap the P7.8-era tests left (most of them cover isolated *shapes* with synthetic data, e.g. only 2 of the real interface's 4 `envelope` fields, never this file's real assembled content) — plus a direct regression test for the empty-table case. `bless_positive_fixtures`/`positive_fixtures`/`tsc_verify`'s full strict-`tsc` corpus all pass unchanged. `verbatim_sites` 11 → **10** (this slice's own one call site); `ts_writes` 1620 → **1581** (`events_fanout.rs`'s own ~39 `writeln!`/`write!`/`format!` calls gone); `ast_importers` unaffected (this file was never one of the five counted) | R7.1, R7.2 | P7.8, P7.9 |
| **Arc C, slice 3 — `workers.rs`** (#1321, landed) | `emit_worker_compose` and its nine private helpers (`bynk-emit/src/emitter/workers.rs`) stop building TypeScript text with `writeln!`/`format!`/`write!` and instead construct and return a real `bynk_ts::TsProgram` (`-> (TsProgram, bool)`, the `bool` unchanged — `needs_request`, consumed by `emit_worker_entry`, unrelated to this slice's own node-algebra work). `bynk-emit/src/project.rs`'s own construction site drops its `Verbatim`/`NotYetConverted` wrap in favour of the real returned `TsProgram`, directly — the second `bynk-emit` construction site (after `events_fanout.ts`'s own, slice 1) to reach `Document::Ts` with no `Verbatim` at all. The largest single algebra extension of any Arc C slice so far: the accepted proposal's own Decision A named five new `bynk-ts` shapes (a `TsObjectEntry` enum — `Prop`/`Shorthand`/`Method`/`Spread` — replacing `TsExpr::Object`'s bare `Vec<(String, TsExpr)>` entries, since the compose surface's own dominant shape is one shorthand async method per wrapper, mixed with shorthand and spread entries elsewhere; `TsExpr::Arrow`, expression-body only; `TsExpr::OptionalMember`/`OptionalIndex`, separate variants rather than a flag on `Member`/`Index` — narrower, since a flag would have touched every one of `Member`'s already-many real call sites that never need one; an `optional: bool` third element on `TsType::Object`'s tuple entries, mirroring `TsParam::optional`'s existing precedent) plus Decision B (a `TsType`-returning twin of `ts_type_ref_qualified`, `ts_type_ref_qualified_ts_type` in `emitter.rs` — not a new structural walk, since `ts_type_ref_qualified` already builds a real `TsType` internally since P7.9 and only stringifies at its own last step; this function is that same build, minus the final `print_type` call). Real, further gaps found during implementation, beyond the accepted proposal's own five-shape catalogue (its own Framing checked `TsStmt`/`TsExpr`/`TsType` shapes but not `TsDecl` ones, and admitted the risk explicitly): `TsDecl::Function` (`compose.ts`'s own top-level `export function compose(...) { ... }` — there is no way to represent this file's *entry point* without one) and `TsDecl::TypeAlias` (the conditional `type DurableObjectNamespace = ...;` fallback), both direct, mechanical siblings of the already-existing `ConstDecl`/`Class`; `TsDecl::ImportNamespace` (`import * as X from "spec"`, structurally different from the existing braced named-imports form — `compose.ts` imports `handlers.js` and every referenced unit's binding module this way); a parameterless `catch { ... }` (ES2019's optional catch binding, needed by `emit_http_sum_wrapper`'s own raw-body read) — closed by widening `TsStmtKind::TryCatch`'s `catch_param` from `String` to `Option<String>`; and four new `TsBinaryOp` variants (`Or`/`And`/`StrictEq`/`StrictNotEq`) plus `TsUnaryOp::Typeof`, pervasive throughout the file's own header/tagged-result checks — closed alongside a real printer fix: with only `??` in the algebra, a nested `Binary` operand of another `Binary` was *always* parenthesized (documented as deliberately conservative, since nothing exercised the gap); `workers.rs`'s own real content nests a strictly-higher-precedence comparison inside `||`/`&&` (`__authz === null || !__authz.startsWith(...)`) with no parens in the byte-golden fixtures, so `render_binary_operand`'s own parenthesisation became precedence-aware (equal-or-lower precedence still always parenthesizes, preserving every existing pinned test; only a strictly-higher-precedence nested operator newly omits parens). Two further, genuinely out-of-scope dependencies — `bynk_check::actors::claim_predicate_to_js` (a different crate) and `workers_entry.rs`'s own still-`String`-returning `deserialise_call`/`brand_assertion` (Arc C's own *next* slice, not yet converted) — are carried as opaque pre-rendered text via `TsExpr::Ident`, the same "existing textual variant carries content this crate structurally can't build a shape for" precedent `ts_type_ref_to_ts_type`'s own `TypeRef::Query` arm already established for `TsType::Named` (`emitter.rs`), not a new pattern. `bynk-emit/src/project.rs` also gains `instantiate_provider_ts_expr`, a `TsExpr`-returning twin of the existing `instantiate_provider_expr` (added alongside, not replacing — `emit_composition_root`'s own Bundle-mode `compose.ts`, a different, still-unconverted document, still needs the `String` form). One real, historical-formatting-quirk regression found by the zero-diff fixture check, not by reasoning: the pre-conversion `deps` object's own empty case printed `{  }` (a double space, from its own `"{{ {} }}"` template with nothing interpolated), not the ordinary single-line object literal's tight `{}` shortcut — carried as its own opaque-text special case at that one construction site rather than changing the general (and elsewhere-correct, e.g. `events_fanout.rs`'s own `(env ?? {})`) shortcut. Zero diff: `bless_positive_fixtures`/`positive_fixtures` (all 116 `compose.ts` byte-golden fixtures) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged; 12 new `bynk-ts` unit tests pin every new shape (and the new precedence rule) directly. No isolated direct fixture test was added inside `workers.rs` itself matching `events_fanout.rs`'s own pattern — `emit_worker_compose`'s 13-parameter signature depends on whole-project, checker-derived data (`UnitTable`, cross-unit consume/alias/flatten maps) with no existing precedent anywhere in `bynk-emit` for hand-constructing or checker-deriving that data inside a crate-local unit test; the full `positive_fixtures`/`tsc_verify` suite already exercises the real, converted function against all 116 real fixtures byte-for-byte under real `tsc --strict`, a stronger guarantee than one isolated unit test could add. `verbatim_sites` 10 → **9** (this slice's own one call site); `ts_writes` 1585 → **1462** (`workers.rs`'s own ~230 `writeln!`/`write!`/`format!` calls gone, plus one more from review's finding 3 below); `ast_importers` stays at **5**, exactly as the accepted proposal's own Framing predicted — this file's *input*-side `use bynk_syntax::ast::{...}` walk (`Handler`/`ActorDecl`/`TypeRef`, …) is unchanged, only its *output* representation converted. Review (#1322) found three more real issues, all fixed before merge: (1) `TsExpr::Arrow` was missing from every parenthesisation rule (`needs_parens_as_operand`, `render_binary_operand`, `As`'s own local rule) — not reachable by any real content today (the one grounded `Arrow` site goes straight through the depth-unaware `render_expr` with no operand context), but the same #1314-finding-2 bug class, closed with three direct tests pinning the callee/binary-operand/as-operand shapes; (2) the initial `ts_any 30 → 27` reading, reported above in an earlier draft of this row, was a **probe artifact, not a real reduction** — all three `any`s are still emitted byte-identically, only their *Rust-source spelling* changed from `writeln!`-string to `TsType::named("any")`, which `xtask`'s `line_violates_ts_any` predicate didn't yet match; widened to also match the `named("any"` construction spelling (round three of the same under-counting failure mode §3.3 and review of #1297 already hit), restoring `ts_any` to its real, unchanged value of **30** — worth remembering for every later Arc C slice converting an `any`-emitting `writeln!` the same way; (3) the `env.<binding>` `Member` built at `workers.rs:1423` was a single property node whose own name (`format!("env.{binding}")`) lied about being one property when the printed text is really two chained accesses — fixed to use the file's own existing `member_chain` helper (structurally honest, byte-identical output), which is the one additional `writeln!`/`format!` call site `ts_writes`'s own final **1462** (not 1463) reflects | R7.1, R7.2 | P7.8, P7.9 |
| **Arc C, slice 4 — `workers_entry.rs`** (#1323, landed) | `emit_worker_entry` and its four helpers (`emit_scheduled_handler`/`emit_queue_handler`/`emit_http_route_dispatch`/`emit_call_handler_dispatch`, `bynk-emit/src/emitter/workers_entry.rs`) stop building TypeScript text with `writeln!`/`format!`/`write!` and instead construct and return a real `bynk_ts::TsProgram`. `bynk-emit/src/project.rs`'s own construction site drops its `Verbatim`/`NotYetConverted` wrap in favour of the real returned `TsProgram` directly — the third `bynk-emit` construction site to reach `Document::Ts` with no `Verbatim` at all. The accepted proposal's own Decision A named four new/extended `bynk-ts` shapes: `TsStmt::Switch`/`TsSwitchCase` (the file's own dominant control-flow shape — four real `switch` statements, the first genuinely new statement-*grouping* construct in the tree; every real `case` is `{ }`-blocked, `default` never is); `TsDecl::ExportDefault(TsExpr)` (the file's own top-level `export default { fetch, scheduled?, queue? }` — a bare-expression default export, which the existing `TsDecl::Export(Box<TsDecl>)` declaration-wrapper can't represent); `TsExpr::Conditional` (ternary — two real sites), with its own parenthesisation-rule coverage added proactively in all three existing rule sites (`needs_parens_as_operand`, `render_binary_operand`, `As`'s own local rule) rather than left for a review round to re-find the same #1314/#1322 bug class a third time; `TsType::Object` extended from a raw `(String, TsType, bool)` tuple to a `TsTypeMember` enum (`Prop { readonly, optional }` / `Method { params, ret }`), since a fourth positional `bool` next to `optional` would be genuinely ambiguous at call sites. Real gaps found during implementation beyond that list, each closed and flagged as a deviation: `TsDecl::ReExport { names, from }` (`export { A, B } from "spec";`, structurally distinct from both `Import` and `Export` — this file's own agent/fan-out-DO re-exports, a shape the proposal's own grounding never named); `TsStmtKind::Blank` (a bare blank line usable at any nesting depth, distinct from `print()`'s own top-level-only blank-line policy — three real points inside `fetch`'s own `try` block); `TsLit::Bool` (`CorsPolicy.credentials`/`SecurityPolicy.nosniff` are real booleans); `TsExpr::Paren` (an explicit, printer-preserved parenthesisation — the CORS-preflight guard's own path-match condition is always wrapped in `(...)` even when nothing about its own precedence requires it, a real case the ordinary precedence-derived rules correctly do *not* parenthesize on their own); `TsTypeMember::Index` (`{ [k: string]: JsonValue }`, textually distinct from the semantically-equivalent `Record<string, JsonValue>`, which the multi-param `on call` dispatch's own real cast needs); `TsBinaryOp::GreaterThan` (the request-body-ceiling guard's own `Number(__contentLength) > <cap>`, the one real relational, non-equality comparison anywhere in `bynk-emit`); `TsStmt::If`'s own `else_branch` (the queue-consumer ack/retry dispatch's real `if (...) msg.ack(); else { ...; msg.retry(); }` — `events_fanout.rs`'s own grounding never needed one, so P7.8 named omitting it a deliberate choice, not an oversight; this slice's own real content needs it for real); `TsStmtKind::InlineBlock` (`{ stmt; stmt; ...; }` on one generated line, reachable only as an `if`/`else` branch today — two real sites, both hand-written as compact one-liners in the pre-conversion text) — and `render_block_body`'s own special case rendering an `InlineBlock` `TryCatch` body with its braces on their own lines but its content still one compact line, the queue consumer's own real catch clause. A further real correction, found only by the zero-diff fixture check: the `As` arm's own "always parenthesize a nested `Await`" rule (added by P7.8, reasoned as a grammar requirement) was actually just one file's own historical text — `workers_entry.rs`'s real `await request.json() as JsonValue` has no parens (`as` binds looser than `await`, so none are grammatically needed), while `events_fanout.rs`'s own real `(await request.json()) as {...}` keeps its parens, and even *within this one file* the two real `await ... as T` sites disagree with each other (`/_bynk/call/`'s `args` has none; `/_bynk/event/`'s destructuring and the signature-seam's `__body_json` both do) — removed the automatic rule entirely, replaced by explicit `TsExpr::Paren` wraps at the three real sites that want it (two in this file, one in `events_fanout.rs`, all with an inline comment explaining why each one differs), pinned by a new test proving the corrected no-auto-parens behaviour and a new test proving `Paren` composes correctly under `As`. `render_binary_operand` also gained one more real refinement: a same-operator (not mixed) `||`/`&&` chain nested in itself no longer auto-parenthesizes (`typeof args !== "object" || args === null || Array.isArray(args)`, a genuine 3-term chain — the pre-slice "always parenthesize equal precedence" rule would have wrongly wrapped the first two terms) — `??` keeps its own pre-#1321 conservative choice unchanged, since nothing real needs it flattened. `deserialise_call`/`serialise_call`/`brand_assertion` stay `String`-returning per Decision B, carried as opaque `TsExpr::Ident` text at every call site, the exact precedent `workers.rs`'s own conversion already established for the same two functions. Two real implementation-time bugs caught only by the fixture diff, not by reasoning: a `for...of`'s own `try`/`catch` body was passed directly as the loop's body instead of wrapped in a `Block` — `render_branch`'s own brace-free inline fallback reset the whole nested try/catch to depth 0, matching neither this loop's real nesting nor the original's own braced shape; and the CORS-preflight `if`'s own then-branch was a bare `return` statement instead of a `block(vec![return_(...)])` — the original wraps it in braces on three lines, not one inline line. `verbatim_sites` 9 → **8** (this slice's own one call site); `ts_writes` 1462 → **1305** (`workers_entry.rs`'s own 191 `writeln!`/`write!`/`format!` calls gone); `ts_any` unchanged at **30** (no `any` construction anywhere in this file, confirmed); `ast_importers` stays at **5**, exactly as the accepted proposal's own Framing predicted — this file genuinely is one of the five counted (`use bynk_syntax::ast::{...}`, unchanged), and converting its own *output* representation does not remove its *input*-side AST walk. Zero diff: `bless_positive_fixtures`/`positive_fixtures` (all 116 `expected/workers/*/index.ts` byte-golden fixtures) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged; 15 new `bynk-ts` unit tests pin every new/extended shape, the corrected `Await`-under-`As` behaviour, the same-operator-chain flattening, and the `TryCatch`/`InlineBlock` combination directly. No isolated direct fixture test was added inside `workers_entry.rs` itself, the same precedent `workers.rs`'s own conversion (#1321) already set for the identical reason — `emit_worker_entry`'s own signature depends on whole-project, checker-derived `UnitTable` data with no existing precedent anywhere in `bynk-emit` for hand-constructing it in a crate-local unit test; the full `positive_fixtures`/`tsc_verify` suite already exercises the real, converted function against all 116 real fixtures byte-for-byte under real `tsc --strict` | R7.1, R7.2 | P7.8, P7.9 |
| **Arc C, slice 5 — `emit_test_main`** (#1325, landed) | `emit_test_main` (`bynk-emit/src/project/tests_emit.rs`) stops building `tests/main.ts` with `push_str`/`format!` and instead constructs and returns a real `bynk_ts::TsProgram`; `project.rs`'s own construction site drops its `Verbatim`/`NotYetConverted` wrap. This slice's own research corrected the track doc's stale `emit.rs`/`lower.rs` naming (§3.1/§6, both written before Arc C started): the real remaining surface after slice 4 was 5 construction sites, all in `project.rs`, not spread across those two files — `emit_project` (`emitter.rs:319-559`) is a 15-function **orchestrator**, genuinely the "emit.rs/lower.rs-scale, several slices" work the track anticipated, just realised as one fan-out function rather than two files, and needs its own dedicated multi-slice design pass (the same conclusion #1319 reached for `serialisation.rs`) — not proposed as a single slice; `emit_composition_root` (`project.rs:3074-3463`, the **Bundle**-target composition root, distinct from the already-converted **Workers**-target `emit_worker_compose`) is the next well-scoped target; `adapter_bindings[name].content` and `emit_runtime_module()` wrap genuinely unconvertible content (foreign/user-authored TypeScript and a committed npm build artefact respectively, neither ever generated by `bynk-emit`) — **a real correction to this track's own probe-floor claim**: §3's own text argues `verbatim_sites` retires at 0; these two sites are direct evidence it doesn't, the same way `ast_importers`'s own floor settled at 5 rather than 0 — the real floor is *at least 2*, to be named explicitly at Arc C's own retirement. Extends the node algebra with `TsExpr::TemplateLit` (template literals — `program.rs`'s own module doc had named this construct "unused in the grounding file" until now; its own static `parts` print with no escaping of their own, since a generic escaper would double the literal backslash of the two real pre-formed JS unicode escapes this function's own ✓/✗ output lines carry as six-ASCII-character text, not the actual glyph — the same "already a raw-text slot" reasoning `TsDecl::Import`'s own `names` field already uses), `TsLit::Raw` (the one real sibling gap: `const PREFIX = "integration · ";` needs the identical pre-formed-escape treatment for a whole literal, not just a template literal's own static segment), `TsExpr::Array`'s own `multiline` field (mirroring `TsExpr::Object`'s, for the real `modules` array — one `{ name, run }` entry per test, one per line), `TsDecl::Function`'s own `is_async` field (deferred at its own introduction in #1321: "`workers.rs`'s one real site is never async... add it when a future slice needs it" — this is that slice, `emit_test_main`'s own top-level `async function main()`), `TsStmtKind::Increment` (a bare postfix `<expr>++;` statement — `passed++;`/`failed++;`), and `TsStmtKind::If`'s own new `same_line_else` field (a second, real `} else {`-on-one-line convention alongside the fresh-line default #1323 established — all three of this function's own real `if`/`else` sites use it, none use the fresh-line form, the same class of cross-file formatting tension the `Await`-under-`As` correction (#1323/#1324) found for parenthesisation; only reachable when `then_branch` is a `Block` or `InlineBlock`, falling back to the fresh-line default otherwise). `verbatim_sites` 8 → **7**; `ts_writes` only 1305 → **1306** (a net *increase* despite eliminating ~90 lines of `push_str`-based emission — the pre-conversion code built almost all of its static TS text via bare `push_str`, which the probe never counted in the first place, only its handful of real `format!` calls; this slice's own tree-construction code uses 3 `format!` calls of its own, one more than the 2 it removed — an honest, probe-definitional artifact, not a regression, worth remembering the same way #1322's `ts_any` finding was: a probe's literal number can move for reasons adjacent to a slice's real intent); `ts_any`/`ast_importers` unaffected (`tests_emit.rs` was already in `AST_IMPORTER_EXCEPTIONS`, never counted). Zero diff: `bless_positive_fixtures`/`positive_fixtures` (all 69 `expected/**/tests/main.ts` byte-golden fixtures) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged; a direct fixture-backed test (`emit_test_main`'s simple `&[RunnableTest]`/`ImportExt` signature is hand-constructible, unlike `emit_worker_compose`'s/`emit_worker_entry`'s own whole-project `UnitTable` data) asserts the real function's output byte-identical to a real fixture's own `tests/main.ts`; 14 new `bynk-ts` unit tests pin every new/extended shape directly. | R7.1, R7.2 | P7.8, P7.9 |
| **Arc C, slice 6 — `emit_composition_root`** (#1327, landed) | `emit_composition_root` (`bynk-emit/src/project.rs`) — the **Bundle**-target composition root, distinct from the already-converted **Workers**-target `emit_worker_compose` — stops building `compose.ts` with `format!`/`push_str` and instead constructs and returns a real `bynk_ts::TsProgram` (`-> Option<TsProgram>`, the `None` case unchanged: "nothing to compose"). `project.rs`'s own construction site drops its `Verbatim`/`NotYetConverted` wrap — the fourth `bynk-emit` construction site to reach `Document::Ts` with no `Verbatim` at all; `verbatim_sites` now reads **6**, matching the *at least 2* permanent floor slice 5 already named (`adapter_bindings[name].content`, `emit_runtime_module()` — neither ever generated by `bynk-emit`, so neither is a future Arc C target) plus `emit_project`'s own still-unconverted, not-yet-designed multi-slice surface. The accepted proposal's own Decision A named two mechanical algebra additions: `TsExpr::Arrow` gains `is_async: bool` (mirroring `TsDecl::Function`'s own #1325 precedent — this function's real `__eventsDispatch` closure is `async (events: Array<...>) => {...}`); `instantiate_provider_ts_expr` (`project.rs`, the `TsExpr`-returning twin #1321 added for `workers.rs`'s own use) had its `workers_ns: bool` parameter restored (originally hardcoded to `true`, the only mode `emit_worker_compose`'s own call site used at the time) — this function's own two call sites both need `false` (Bundle mode has no per-Worker `handlers_` namespace split), matching `instantiate_provider_expr`'s real `String`-returning signature exactly, once a second real caller needed the other branch. Decision B's own real judgment call: the `__eventsDispatch` closure's dispatch body is a genuine block statement (`for`/`switch`/`try`-`catch` nested), not an expression — `TsExpr::Arrow` stays expression-body-only by design, and building a real block-body variant would have needed new "flatten every nested statement to one line" printer machinery, disproportionate to the 3 real fixtures that reach it — kept as opaque pre-rendered text (the same nested `format!` calls as before conversion) fed into a real `Arrow` node's `body` as one `TsExpr::Ident`, the same "opaque text carrier" precedent `workers.rs`'s/`workers_entry.rs`'s own `deserialise_call`/`brand_assertion`/`claim_predicate_to_js` outputs already use. One real, reachable formatting quirk carried proactively (not found by review this time — applied the identical, already-known #1321 lesson before it could recur): the pre-conversion `{ns}Deps` object's own empty case printed `{  }` (double space), not the tight `{}` shortcut — 7 real fixtures (`98_cross_context_call_with_alias` and others) hit it, a services-having context with no providers/cross-caps/emit/consumed-surface — carried as the same opaque-text special case `workers.rs`'s own conversion established. One real gap found only by the zero-diff fixture check: the pre-conversion code's own unconditional blank line between the last per-context `const` and the final `return` (`out.push('\n')`, run once regardless of loop iteration count) has no equivalent in the printer's own "blank line between top-level declarations" policy, which only separates entries in `TsProgram.stmts` itself, not statements inside one function body — closed with an explicit `TsStmt::blank(None)` push, `TsStmtKind::Blank`'s own precedent (#1323) reused rather than extending the top-level-only policy. `verbatim_sites` 7 → **6**; `ts_writes` 1306 → **1301**; `ts_any` unchanged at **30** (no `any` construction in this file); `ast_importers` unaffected — `project.rs` was never one of the five counted files (no direct `use bynk_syntax::ast` import). Zero diff: `bless_positive_fixtures`/`positive_fixtures` (all 39 `expected/compose.ts` byte-golden fixtures, including the 3 that exercise the `__eventsDispatch` closure) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged; 2 new `bynk-ts` unit tests pin `Arrow`'s `is_async` field directly (async and non-async). No isolated direct fixture test was added inside `project.rs` itself — `emit_composition_root`'s own signature (`unit_tables`/`unit_consumes`/`unit_consumes_aliases`/`unit_callees`/`unit_event_subscriber_shapes`/`adapter_bindings`/`unit_flattened`/`event_subscribers`, eight whole-project checker-derived maps) is at least as complex as `emit_worker_compose`'s/`emit_worker_entry`'s own, the same precedent both already established for the identical reason; the full `positive_fixtures`/`tsc_verify` suite already exercises the real, converted function against all 39 real fixtures byte-for-byte under real `tsc --strict`. Also deleted `ts_string_literal` (dead code once both of its own call sites converted to `TsLit::Str` via the local `str_lit` helper — no other caller existed). | R7.1, R7.2 | P7.8, P7.9 |
| **Arc C, slice 7 — `emit_commons_barrel`** (#1329, landed) | `emit_commons_barrel` (`bynk-emit/src/project/tests_emit.rs`) — the smallest slice in the whole track (49 lines, one function, two callers feeding one construction site) — stops building a multi-file `commons` unit's barrel module (`out/<name>.ts`, one `export *` line per constituent source file) with `format!`/`push_str` and instead constructs and returns a real `bynk_ts::TsProgram`; its own `Verbatim` wrap (nested inside its `Option<StagedFile>` return, not a separate return path) is gone. A post-slice-6 decomposition research pass found the true state of the remaining `verbatim_sites`: not all of them were `emit_project`'s own (a real correction to an earlier inherited assumption) — 3 were previously-untracked real targets, all in `tests_emit.rs`, never named by any prior slice's own research. This proposal converts the smallest of the three; the other two (`emit_integration_module`, `emit_test_module`) remain unconverted, deliberately deferred — `emit_integration_module` was read in full and found to be a genuine orchestrator splicing in `emit_system_http_support` (307 lines, larger than the function itself) and, critically, `emitter::lower_integration_case_body` (`lower.rs:128-171`) — real arbitrary-statement lowering, the same `lower.rs` machinery flagged as needing its own dedicated "opaque carrier vs. multi-slice conversion" design decision before any slice touches it; `emit_test_module` (524 lines) wasn't investigated in depth but flagged as likely sharing the same delegation-heavy shape, needing its own full read before a future proposal assumes it's simpler than it looks — the same lesson this slice's own scoping-down already demonstrated once. The accepted proposal's own Decision A named one new `bynk-ts` shape: `TsDecl::ReExportAll { from: String }` — a wildcard re-export (`export * from "spec";`), structurally distinct from the existing `TsDecl::ReExport` (which always carries a braced name list; an empty one would render the ill-formed `export {  } from "spec";`), mirroring `TsDecl::ImportNamespace`'s own "no braces, no name list, one bound target" shape on the export side. A real spacing gap found only by the zero-diff fixture check, not named by the proposal: the barrel's own real content is one header `Comment` immediately followed by one `export *` line per source file, every one of those lines adjacent with no blank line anywhere — a different spacing shape from `ReExport`'s own (which has no grouping rule of its own; `workers_entry.rs`'s own re-exports are each separated from their neighbours by a blank line). Closed by extending the printer's blank-line policy with two new grouping cases, scoped exactly to this new adjacency: consecutive `ReExportAll`s group with each other, and a `Comment` immediately before a `ReExportAll` groups with it too — an ordinary `Comment` before any other decl still gets its blank line, unchanged. `verbatim_sites` 6 → **5**; `ts_writes` 1302 → **1301** (this function's own one real `format!` call gone); `ts_any` unchanged at **30**; `ast_importers` unaffected (`project/tests_emit.rs` was already in `AST_IMPORTER_EXCEPTIONS`, never counted). Zero diff: `bless_positive_fixtures`/`positive_fixtures` (all 3 barrel-producing fixtures — `251_multi_file_commons_test`, `252_multi_file_commons_dotted_test`, `253_integration_multi_file_commons_uses` — plus the full positive corpus) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged; a direct fixture-backed test (`ParsedFile::new`'s own documented "general constructor — `bynk-emit`'s own tests use this" purpose makes the function's inputs hand-constructible, unlike `emit_worker_compose`'s/`emit_worker_entry`'s/`emit_composition_root`'s own whole-project `UnitTable`-derived data) asserts the real function's output byte-identical to `251_multi_file_commons_test`'s own real `expected/thing.ts`; 2 new `bynk-ts` unit tests pin the new decl's own rendering and the new blank-line grouping rule directly. | R7.1, R7.2 | P7.8, P7.9 |
| **Arc C, slice 8 — `emit_doc_block`/`ts_type_params`** (#1333, landed) | Closes step (1) of the design pass's (#1331) own decomposition order. Reading every one of step (1)'s 13 originally-named functions in full found **9 are not TypeScript-emission code at all** (`sorted_index_fields`/`topo_order_providers`/`sanitise_path_segment`: pure Rust logic; `http_handler_method_name`(×3)/`cron_handler_method_name`/`queue_handler_method_name`: plain identifier-name strings, not structured syntax; `collect_handler_labels`/`handler_op_label`: JSON for the debug-metadata sidecar, ADR 0105, a genuinely different output kind) — recorded here as permanently excluded from Arc C, the same way `adapter_bindings`/`emit_runtime_module` are. The 3-function ICU-formatting cluster is real but too substantial to be a "leaf helper" — re-scoped to its own future step (11), see above. **Real remaining scope: exactly 2 functions.** `emit_doc_block` (`bynk-emit/src/emitter/emit.rs`, 14 real callers) and `ts_type_params` (5 real callers) both keep their exact existing `-> String`/`out: &mut String` signatures — neither owned its own `Verbatim` construction site (both are shared helpers spliced into still-unconverted callers' buffers), so the P7.9 pattern applies: build a real node internally, print just that fragment, no caller changes, `verbatim_sites` unaffected. `bynk-ts` gains `TsStmtKind::DocComment(String)` (a JSDoc `/** ... */` block, distinct from `Comment`'s own `//`-per-line form, with its own escaping — a literal `*/` becomes `*\/`, issue #720 — and blank-line convention — a bare ` *`, no trailing space) and a new narrow `print_stmt(stmt, depth) -> String` entry point (mirroring `print_type`'s own "one fragment, not a whole document" scope, P7.9's precedent) — `emit_doc_block` now builds a `TsStmt::doc_comment` and prints it through `print_stmt`. `ts_type_params` needed no new `bynk-ts` shape at all, exactly as the accepted proposal's own least-prescriptive gap predicted — each type-param name now routes through the existing `TsType::named`/`print_type` (P7.9) instead of splicing the raw `&str` directly. `ts_writes` drops by **4** (`emit_doc_block`'s own 4 `writeln!` call sites — the accepted proposal's own citation undercounted this as 2; `ts_type_params`'s own wrapping `format!` legitimately stays, since it only joins/brackets already-printed names, not TS-shape construction), not the proposal's own estimated 3 — corrected here rather than left standing, the same "correct the honestly stated number" discipline this track has applied throughout. `verbatim_sites`/`ast_importers`/`ts_any` unaffected, confirmed, not claimed as wins. Zero diff: `bless_positive_fixtures`/`positive_fixtures` (every fixture containing a real `/**` JSDoc block or a declaration-level generic — 175+ fixtures) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged; new direct tests in both crates assert real output against real fixture content (`137_agent_instantiation_workers`'s own header comment; `816_locale_negotiation_no_bundle_regression`'s own `<A, B>` generic list) byte-for-byte, alongside the pre-existing `doc_block_tests` module's own escaping-regression coverage, confirmed still passing unchanged. | R7.1 | #1331 |
| **Arc C, slice 9 — `emit_refined_checks`/`emit_pred_check`** (#1335, landed) | Corrects step (2) of the design pass's (#1331) own decomposition order: all 6 named functions genuinely build real TypeScript, but 3 (`emit_refined_type`/`emit_record_type`/`emit_sum_type`) call `emit_attached_methods` (step (3)'s own target, not yet converted) mid-construction, splicing its own variable-count method-entry output into the same object literal before closing it — `bynk-ts::TsObjectEntry`'s 4 existing variants can't represent "zero or more pre-rendered entries appended by not-yet-converted code." **Real scope for this slice: the 2 self-contained functions.** `emit_refined_checks` and `emit_pred_check` (`bynk-emit/src/emitter/emit.rs`) both keep their exact existing `out: &mut String` signatures — neither owned its own `Verbatim` construction site, so the P7.9/step-1 pattern applies: build real nodes internally, print, no caller changes, `verbatim_sites` unaffected. `emit_type` and the 3 object-builders are deferred to a follow-up slice, gated on a `TsObjectEntry::Raw`-style opaque-entries-carrier decision (see the "Fourth correction" note above). No new `bynk-ts` shapes needed — every guard reduces to a real `TsStmt::If` wrapping a `TsStmt::Return`, built from `If`/`Return`/`Object`/`Unary`/`Paren`/`Str`, all already existing. `emit_pred_check`'s own `cond`/`msg` come from `pred_condition_and_message` (`emitter.rs:4966`, still unconverted, out of this slice's scope) as a raw `(String, String)` pair — `cond` carried as opaque `TsExpr::Ident` wrapped in `Not`/`Paren`, matching Decision B, but `msg` gets the SAME opaque treatment rather than the accepted proposal's own `TsLit::Str` — a real, deliberate deviation: `PredKind::Matches`'s own message embeds `escape_ts_string`-escaped pattern text directly, and running it through `TsLit::Str`'s own escaper a second time would double-escape any backslash the pattern contains. Not reachable by any fixture in the corpus today (no `Matches` pattern uses a backslash), so this is a proactively-found-and-closed latent bug, not a zero-diff-check catch — pinned by a dedicated test that was itself proven to catch the bug (temporarily reverted to `TsLit::Str`, confirmed the message quadruples the pattern's own backslash from one to four characters, restored). `ts_writes` drops by **8** (9 real `writeln!` call sites removed, offset by 1 new `format!` needed to build `msg`'s own pre-quoted opaque text) — not the accepted proposal's own estimated 9, corrected here. `ast_importers`/`ts_any` unaffected. Zero diff: `bless_positive_fixtures`/`positive_fixtures` (every fixture containing a real refined-type predicate check) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged; new direct tests assert both functions' real output against `254_multi_file_commons_workers_codec`'s own real `Cents.of` guard/check byte-for-byte, plus the dedicated `Matches`-backslash regression test. | R7.1 | #1331 |
| **Arc C, slice 10 — `emit_attached_methods`/`emit_forwarded_methods`/`emit_method`** (#1337, landed) | Closes step (3) of the design pass's (#1331) own decomposition order, and unblocks step (2)'s own deferred trio (see the table row above): `emit_attached_methods` (`bynk-emit/src/emitter/emit.rs`) changes signature from `out: &mut String` to `-> Vec<TsObjectEntry>` — a real signature change, not the P7.9/step-1 "keep `-> String`" pattern, since the whole point is making its output *appendable* into a real `TsExpr::Object` a future slice builds. `emit_method` likewise returns one real `TsObjectEntry::Method` instead of writing text; its own body is delegated wholesale to `emit_block_as_function_body_with_return` (`emitter/lower.rs:201`, the one splice boundary ADR `arc-c-lower-rs-permanent-exclusion` names a *permanent* exclusion) — captured at the exact same absolute indent the pre-conversion code always passed and carried as one opaque `TsStmtKind::Raw` statement, a new, narrow, unconditional-passthrough carrier deliberately NOT a reuse of `TsStmtKind::Verbatim` (which tracks *temporary* Arc C residue via the `verbatim_sites` probe — using it here would misrepresent a permanent exclusion as unfinished work, and wrongly move that probe). `emit_forwarded_methods` (no `emit_attached_methods`/`lower.rs` dependency at all) converts fully to real nodes, no opaque carrier — but its own pre-conversion `writeln!` built the WHOLE entry, signature and one-statement body alike, on ONE physical line, a real shape `TsObjectEntry::Method`'s own standard multi-line body rendering couldn't represent — caught only by the zero-diff fixture check (`255_context_uses_commons_static_method`), not reasoned about in the abstract. Closed by adding a new `inline: bool` field to `TsObjectEntry::Method` (`false` for every other real site in this tree; `true` only here), reusing the printer's own existing compact-statement machinery (`render_compact_stmts`, `TsStmtKind::InlineBlock`'s own sibling shape) rather than a second renderer. Two further real gaps found only by the zero-diff check, both outside the accepted proposal's own project-form-fixture-corpus search: **generics** — `Box.map<A, U>` (`402_generic_instance_method`, single-file form, outside that search) needed a new `generics: Vec<String>` field (bare names only, matching `ts_type_params`'s own rendering, not a full `Vec<TsParam>`); **doc** — `Timestamp.diff`/`Timestamp.add` (`65_money_uses_time`, project form, but the citation's own doc-comment search used the wrong marker, `///` rather than this language's real `---`-delimited block) needed a new `doc: Option<String>` field, rendered via the same `render_doc_comment` renderer `TsStmtKind::DocComment` already uses. `verbatim_sites` **unchanged** (none of the three functions owned a construction site) — confirmed, not claimed as a win. `ts_writes` drops by **17** (1301 → 1284) — well beyond the accepted proposal's own conservative "1 + 1" estimate, since converting each parameter also eliminated its own `format!` call, not just the two named `writeln!` sites; corrected honestly via a fresh probe run, not assumed. `ast_importers`/`ts_any` unaffected. Zero diff: `bless_positive_fixtures`/`positive_fixtures` (every fixture containing a real attached-method, forwarded-method, generic-method, or doc-commented-method entry) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged; new direct `bynk-ts` tests pin the `generics`/`doc`/`inline` fields directly, including one matching `255_context_uses_commons_static_method`'s own real inline `equals` entry byte-for-byte. | R7.1 | #1331, #1333, #1335 |
| **Arc C, slice 11 — `emit_type`/`emit_refined_type`/`emit_record_type`/`emit_sum_type`** (#1339, landed) | Closes step (2) of the design pass's (#1331) own decomposition order, fully: the dispatcher (`emit_type`) and all 3 object-builders slice 9 (#1335) deferred now build real `bynk_ts` nodes, unblocked by slice 10's (#1337) `emit_attached_methods -> Vec<TsObjectEntry>` signature change. All 4 functions keep their exact existing `out: &mut String` signatures (`emit_type`'s own pure-dispatch shape is unchanged too) — none owns a `Verbatim` construction site, the P7.9/step-1 pattern applies one level down: build real nodes internally, print, no caller changes. The accepted proposal's own Decision A named four `bynk-ts` algebra gaps, all landed: `TsDecl::Interface` gains `type_params: Vec<String>` (bare names) and `members` switches from a raw `(String, TsType)` tuple to `Vec<TsTypeMember>` (reusing the existing `TsTypeMember::readonly_prop`, since every real field `emit_record_type` builds is `readonly`); a new `TsType::Intersection(Vec<TsType>)` for `emit_refined_type`'s own `{base} & { readonly __brand: "..." }` branded alias; `TsType::Union` gains a `multiline: bool` field (via new `TsType::union`/`TsType::multiline_union` constructors) for `emit_sum_type`'s own real leading-pipe, one-variant-per-line discriminated-union shape, pinned byte-for-byte against the pre-conversion `writeln!` code's own asymmetric "space on the first line, pipe on every other" rule — `TsDecl::TypeAlias`'s own render arm special-cases this one shape's `=` line (a bare `\n`, no trailing space, matching the original exactly) rather than growing a general multiline-alias flag nothing else needs; `TsExpr::Arrow` gains `generics: Vec<String>` and `return_type: Option<TsType>` for `emit_sum_type`'s own generic payload-constructor arrows (`<T>(value: T): Sum<T> => (...)`) — a real gap beyond the proposal's own two-field citation for this trio, since every one of this file's own generic-payload arrows carries an explicit return-type annotation the arrow itself owns, not something inferred from its body; the object-literal body is wrapped in an explicit `TsExpr::Paren` — `Arrow`'s own renderer does not auto-parenthesise an object body, the same "explicit `Paren` always prints its own literal parens" precedent #1323 established. `TsDecl::TypeAlias` also gains `type_params: Vec<String>` (`emit_sum_type`'s own generic sums erase to `export type Foo<T> = ...`; `workers.rs`'s own real site, the `DurableObjectNamespace` fallback, stays empty). `emit_refined_type`'s own `of()` guard-block content (`emit_refined_checks`'s real, already-printed `TsStmt` output, unaffected by this slice) is carried as one `TsStmtKind::Raw` statement — a second, differently-reasoned use of the carrier #1337 added for `lower.rs`'s own permanent exclusion, documented explicitly as broadening (not narrowing) that variant's own justification: real, already-correctly-indented statement text a call site cannot restructure without changing scope it isn't the one converting, whether the underlying reason is permanent or, as here, just out of this slice's own scope. The printer's own `TsObjectEntry::Method` generics renderer is generalised, not duplicated a third/fourth time, into one shared `render_bare_generics`, now also used by `Interface.type_params`, `TypeAlias.type_params`, and `Arrow.generics`. `verbatim_sites` **unchanged** (5) — none of the 4 functions owned a construction site, confirmed, not claimed as a win. `ts_writes` drops by **20** (1284 → 1264), verified via a fresh `cargo xtask greenfield-status --apply`, not assumed. `ast_importers`/`ts_any` unaffected. Zero diff: `bless_positive_fixtures`/`positive_fixtures` (every fixture containing a real interface/refined-type/sum-type declaration) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged; a new direct test pins `emit_sum_type`'s own generic multiline-union and generic-arrow-constructor output against `406_generic_sum_envelope`'s own real `expected.ts` byte-for-byte (the pre-existing `generic_record_type_erases_its_type_parameter` test, unchanged, already covers `Interface.type_params` against a real fixture), plus new `bynk-ts` unit tests pinning the `Intersection`/multiline-`Union`/generic-`Interface`/generic-`Arrow` shapes directly. Step (2) of #1331's own decomposition order — the entire type-declaration cluster `emit_type`/`emit_refined_type`/`emit_refined_checks`/`emit_pred_check`/`emit_record_type`/`emit_sum_type` — is now fully closed. | R7.1 | #1331, #1335, #1337 |
| **Arc C, slice 12 — `emit_free_fn`** (#1351, landed) | Splits step (4) of the design pass's (#1331) own decomposition order the same shape #1335 split step (2): `emit_free_fn`'s own outer wrapper (`export {async}function {name}{generics}({params}): {ret} { <body> }`) builds a real `bynk_ts::TsDecl::Function` node; `emit_contract_guarded_body`'s own internal If/Const/Return-shaped guard construction (precondition checks, the result-capturing IIFE, postcondition checks, `return result`) does NOT convert — its signature and `writeln!`-based body are unchanged, named explicitly as step (4)'s own deferred remainder, not silently folded into "step (4) done." `emit_free_fn` keeps its exact existing `out: &mut String` signature — it never owned a `Verbatim` construction site (both real call sites, `emitter.rs:210`/`emitter.rs:384`, splice into `emit_project`'s own still-unconverted shared buffer). The accepted proposal's own Decision A gap: `bynk_ts::TsDecl::Function` had no `generics` field — added (bare names, matching every other real generics-list precedent in this crate); `params`/`return_type` route through the already-real `ts_type_ref_to_ts_type` (P7.9, #1315) instead of the opaque pre-printed `String` `ts_type_ref` returns, directly callable from `emit.rs` via `use super::*` (the same private-sibling-visibility precedent #1339 used for `ts_ty_to_ts_type`). The function's own body — whichever of the two still-unconverted sources built it — is captured into a fresh buffer and carried as one opaque `TsStmtKind::Raw` statement, the exact precedent #1337 established for `emit_method`'s own wholesale-delegated body. **Real bug found and fixed before merge, not by the accepted proposal's own grounding:** capturing the body into a local buffer for `Raw`-embedding broke source-map correctness — the pre-conversion code lowered the body directly into the shared `out` buffer, so its own `record_span` checkpoints were correct by construction (offsets relative to the buffer they'd actually land in); once the body lowers into an isolated `body_text` buffer instead, any checkpoint recorded during that lowering is relative to the WRONG (0-based, local) buffer, silently corrupting the map for every statement inside a free function — caught by `coverage_behaviour.rs`'s own integration tests (which read line coverage through the source map), not by any unit test in this crate. Fixed using the exact local-sub-builder-then-`merge` pattern `emit_service`'s own handler-body lowering already established (`LowerCtx::record_span`'s own doc had already named this exact hazard: "a caller building an IIFE into its own local `String`... before splicing it elsewhere must not call this with that buffer's own length") — a fresh `body_smb: RefCell<SourceMapBuilder>` collects checkpoints relative to `body_text`, then `merge`s into the real `source_map`, rebased at `body_text`'s own exact splice offset within the fully-printed declaration (computed by arithmetic against the printed string's own known length, not a text search, since `Raw`'s own text is spliced verbatim). `verbatim_sites` **unchanged** (5) — `emit_free_fn` never owned a construction site. `ts_writes` net drops by **3** (1264 → 1261: 4 real sites removed by the conversion itself, offset by 1 new `format!` from review of #1352's own finding 1, a `debug_assert!` naming the printed-text/`Raw`-splice invariant explicitly rather than trusting it silently), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. **Review of #1352 found 3 real, actionable gaps, all closed before merge:** (1) the `body_offset_in_printed` arithmetic silently encoded three cross-crate invariants with no guard — closed by the `debug_assert!` above; (2) the source-map fix itself had no *unconditional* regression test outside `coverage_behaviour.rs`'s own toolchain-gated integration tests — investigated directly: 3 of the 5 existing unconditional `bynkc/tests/source_map.rs` tests (no `tsc`/`node` dependency) already caught the bug when it was temporarily reintroduced to verify, so the real gap was narrower than the review's own framing — only the `guarded == true` (contract call-site guard) branch was genuinely untested, closed with a new `compile_reps_with_contracts` helper and a dedicated `contract_guarded_body_keeps_its_own_statement_lines` test, itself proven to catch the bug the same way (temporarily reverted, confirmed the failure, restored); (3) no `bynk-ts` unit test rendered a non-empty `TsDecl::Function.generics` directly — closed with `prints_a_generic_top_level_function_declaration`, matching #1339's own per-field test precedent. Zero diff: `bless_positive_fixtures`/`positive_fixtures` (every fixture containing a real free-function declaration — generic, async, and contract-guarded) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged; `coverage_behaviour.rs`'s integration tests (the source-map regression's own first real catch) pass. | R7.1 | #1331 |
| **Arc C, slice 13 — `emit_contract_guarded_body`** (#1353, landed) | Closes step (4) of the design pass's (#1331) own decomposition order, fully: the remainder #1351/#1352 split off — `emit_contract_guarded_body`'s own precondition/postcondition guards and its trailing `return result;` — now build real `bynk_ts` nodes. Each guard (`if (!(pred)) { const __e = new Error(\`msg\`); __e.name = "BynkContractError"; throw __e; }`) is a real `TsStmt::if_stmt` over a real `TsStmt::inline_block` of 3 real statements (`const_stmt`/`assign`/`throw_stmt`) — `bynk_ts::TsStmtKind::Throw` is this slice's own real gap (no bare `throw` existed anywhere in the tree before now), a mechanical sibling of `Return` (mandatory value, no `Option`), plus its own real, reachable `InlineBlock` use (unlike several of that fallback's neighbouring, not-yet-reachable variants). `pred` (from `Pre::lower`) and each hoisted `pre.stmts()` line stay opaque, the same `Ident`-wrapped-in-real-`Unary::Not`/`Paren` treatment #1335's own `emit_pred_check` established for its sibling predicate lowerer's `cond` — but for a *stronger*, different reason than that slice's own "temporarily unconverted" framing: `Pre` lives in `emitter/lower.rs` (`pre.lower` calls `lower_expr`, the general expression lowerer), the one splice boundary ADR `arc-c-lower-rs-permanent-exclusion` names a *permanent* Arc C exclusion — not machinery a future slice is expected to finish converting. The error message (already backtick-quoted, its own `\`clause\`` delimiters and `${param}=...` interpolations pre-baked as literal text by `param_dump`, unchanged) is carried as one opaque `TsExpr::template_lit` part with zero real `exprs` — `parts` prints verbatim, reproducing the exact byte output with no decomposition needed. **Real implementation-time bug found and fixed before this landed, not by the accepted proposal's own grounding:** the result-capturing IIFE's own body-lowering (`emit_block_as_function_body_with_return`) was initially captured into a fresh local buffer for `Raw`-wrapping, mirroring #1339's `emit_refined_type`/`emit_refined_checks` precedent — but that precedent is safe only because `emit_refined_checks` never touches source-map recording; `emit_block_as_function_body_with_return` calls `cx.record_span(out.len(), ...)` internally, so a local buffer's own 0-based length corrupts every checkpoint once its text is spliced into `out` at a non-zero offset — the *exact* bug #1352 already found and fixed one level up, reproduced one level deeper by this slice's own first draft. Caught immediately by the very `contract_guarded_body_keeps_its_own_statement_lines` test #1352 added for exactly this branch (proven by reintroducing the bug: the precondition guard's own checkpoint moved from source line 1 to line 6). Fixed per the accepted proposal's own Decision B: the whole result-capturing IIFE line is written directly into `out` — the SAME buffer `emit_free_fn`'s own `body_smb`/`merge` machinery (#1352) already expects — exactly as the pre-conversion code did, no local buffer, no `Raw`-wrapping needed at this level (that dance is only for content splicing into a *separate* tree structure `emit_contract_guarded_body` doesn't own; this function still just writes sequential text into its own `out: &mut String` parameter, the P7.9/step-1 pattern one level deeper than #1352's own conversion, no caller changes). `verbatim_sites` unchanged (5) — this function never owned a construction site. `ts_writes` net drops by **1** (1261 → 1260) — smaller than earlier slices' own drops, since most of this function's own line count is guard-message `format!` calls that stay exactly as they were (now arguments to `contract_guard_if_stmt` instead of `writeln!`, not eliminated). `ast_importers`/`ts_any` unaffected; `keep_in_sync` (a trend probe, not a gate) moved 226→227, noted plainly, not investigated further. Zero diff: `tsc_verify`'s full strict-`tsc` corpus pass unchanged; `bynkc/tests/integration_contracts_behaviour.rs`'s own unconditional `contracts_survive_the_integration_workers_recompile` test (real project-form contract-guarded coverage, found during this slice's own "Fixture coverage" check — the positive fixture corpus itself has none, but this dedicated integration test already does) passes unchanged; `bynkc/tests/source_map.rs`'s `contract_guarded_body_keeps_its_own_statement_lines` (added by #1352 for exactly this branch) continues to pass, now exercising real nodes instead of raw `writeln!` text. | R7.1 | #1331, #1335, #1351 |
| **Arc C, slice 14 — `emit_messages_bundle`** (#1355, landed) | Closes step (5) of the design pass's (#1331) own decomposition order: `emit_messages_bundle`'s own outer construction (`bynk-emit/src/emitter/emit.rs`) converts to real `bynk_ts` nodes, with two deliberate, named opaque carve-outs. **`messagesReferenceLocale`/`messagesLocales`/`export function render` convert fully** — no gaps: `TsExpr::As` (double-cast, `("tag" as string) as LocaleTag`, the inner cast wrapped in an explicit `TsExpr::Paren` since `As`'s own renderer does not auto-parenthesise a nested `As` operand, the same "explicit `Paren` always prints its own literal parens" precedent #1323 established — caught by the zero-diff fixture check, not reasoned about in the abstract), `TsExpr::Index`/`Conditional`/`TsBinaryOp::NullishCoalescing`/`TsStmt::If`/`Return` all already existed. **`messagesByLocale`'s own header/type-annotation/closing-brace stay hand-written text** (Decision B) — `Record<string, Record<string, (params: ReadonlyMap<string, MessageArg>) => string>>` needs its inner function type's one parameter named `params`, but `bynk_ts::TsType::Fn`'s own `params` are deliberately anonymous, printer-numbered positionally (`a0`, `a1`, …) — the same "an odd, one-off type shape stays opaque text" precedent P7.9 (#1315) already used for `Query[T]`'s own extra-paren-wrapped shape, not a general `TsType::Fn` redesign for this one call site. Each LOCALE's own entry (and each per-code entry nested inside it) IS a real `TsObjectEntry`, though — printed through the shared `bynk_ts::print_object_entry` fragment entry point directly into the hand-written wrapper, the same "return/print real entries into a still-hand-written enclosing literal" shape `emit_attached_methods`'s own callers already use (#1337) — chosen specifically because `emit_doc_block`'s own per-locale doc comment has no real node shape to intersperse with today (`TsObjectEntry::Prop` carries no `doc` field the way `Method`'s own does; adding one for this single narrow need would be disproportionate). Each per-code entry's own VALUE — `emit_message_entry_renderer`'s output, one of step (11)'s own named, not-yet-proposed ICU-formatting cluster — stays opaque, carried as a `TsExpr::Ident` wrapping already-formed JS, the established "call an unconverted sibling helper, carry its text opaquely" pattern this whole track uses. **Real printer gap found and closed**: `render_multiline_object_entry`'s own `Prop` arm rendered its value through the plain, depth-unaware `render_expr` — a `multiline: true` object nested as one `Prop`'s own value inside ANOTHER multiline object (this function's own real, doubly-nested `{ locale: { code: expr, ... }, ... }` table) silently fell back to single-line, since only `render_stmt_level_expr` (a statement/declaration-level entry point) intercepted `multiline` correctly before this slice. Closed by routing the `Prop` arm's own value through the same interception, one level deeper (`depth + 1`, matching the entry's own indent) — `TsExpr::Object`'s and `Array`'s own module docs updated to name this second reachable boundary, and a direct `bynk-ts` unit test (`prints_a_nested_multiline_object_as_a_props_own_value`) pins it. `verbatim_sites` unchanged (5) — this function never owned a construction site. `ts_writes` drops by **14** (1260 → 1246), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff: every `80[8-9]`/`81[0-9]` message-bundle fixture (`808_messages_basic` through `812_messages_icu_select_gender` and beyond) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged, confirmed directly — real, substantial existing coverage, no new fixture needed. | R7.1 | #1331, #1323, #1315, #1337 |
| **Arc C, slice 15 — `emit_capability`** (#1357, landed) | Splits step (6) of the design pass's (#1331) own decomposition order the same shape #1351 split step (4): `emit_capability`'s own full construction converts to real `bynk_ts` nodes; `emit_provider`'s own conversion is a real, separate remainder, deferred — a genuinely harder shape (a full class with N real method bodies, each needing the #1352/#1353 sub-builder/`merge` treatment individually, plus a new `TsDecl::Class.implements` field neither exists yet), not silently folded into "step (6) done." `export interface {Name} { op<T>(params): ret; ... }` is a real `TsDecl::Interface` over real, per-op `TsTypeMember::Method` entries — `generics`/`doc` are this slice's own real gap (bare names for `generics`, matching every other real generics-list precedent in this crate; `doc` mirrors `TsObjectEntry::Method.doc`'s own identical field, #1337, rendered the same "doc lives at the call site that carries `depth`" way — `render_type_member` itself has no `depth` to give `render_doc_comment`, so `TsDecl::Interface`'s own render arm renders a documented member's own doc directly, mirroring `render_multiline_object_entry`'s own identical split) — params route through the already-real `ts_ty_to_ts_type` (P7.9, #1315) instead of the opaque pre-printed `String` `ts_ty` returns. `TsTypeMember::method`'s own existing constructor keeps its exact signature (both new fields default), so all 6 of its real pre-existing callers are unaffected. The injection token (`export const {Name}Token: unique symbol = Symbol("{Name}");`) is a real `TsDecl::ConstDecl` — `unique symbol` stays one opaque `TsType::named` string, the same "an odd, one-off type shape stays opaque text" precedent P7.9 already used for `Query[T]`'s own extra-paren-wrapped shape (nothing else in this crate builds a `unique symbol` type). `emit_capability` keeps its exact existing `out: &mut String` signature — it never owned a `Verbatim` construction site. `verbatim_sites` unchanged (5). `ts_writes` drops by **4** (1246 → 1242), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected; `keep_in_sync` (a trend probe, not a gate) moved 227→229, noted plainly. Zero diff: every fixture containing a real `capability` declaration and `tsc_verify`'s full strict-`tsc` corpus pass unchanged. Op-level doc comments are reachable only through `TsTypeMember::Method.doc` — confirmed no positive fixture exercises one, a real, named fixture-coverage gap — pinned instead by a direct `bynk-ts` unit test (`prints_a_generic_documented_interface_method`), the same per-field-test precedent #1337/#1339 already established for identically-untested-by-fixture gaps. | R7.1 | #1331, #1337, #1315 |
| **Arc C, slice 16 — `emit_provider`** (#1359, landed) | Closes step (6) of the design pass's (#1331) own decomposition order, fully: each provider op converts to a real `bynk_ts::TsClassMethod` fragment (name/`is_async`/real `TsParam`s via `ts_type_ref_to_ts_type`, P7.9 #1315/real return type), printed through a new fragment entry point, `bynk_ts::print_class_method` (mirroring `print_object_entry`'s own "one fragment, not a whole document" scope, #1337). **The class's own wrapper (header/`implements`/deps field+constructor/closing brace) and the trailing factory `const` stay hand-written text, Decision C, a deliberate boundary, not a remaining gap**: building the whole class as one real `TsDecl::Class` tree would need every method's own body captured into a local buffer for `Raw`-embedding, and this class's own real spacing (no blank line between methods) genuinely differs from `TsDecl::Class`'s own established "one blank line before each method" policy (`events_fanout.rs`'s real convention, #1317) — the same "two real files disagree on a formatting convention" tension this track has hit before. `print_class_method` deliberately adds no automatic blank-line insertion of its own, the same "caller controls spacing" contract `print_object_entry` already established. Each op's own body still lowers through `emit_block_as_function_body_with_return` (unconverted, out of scope) but can no longer write directly into `out` — captured into a local `body_text` buffer instead, using the same local-sub-builder-then-`merge` pattern #1352/#1353 already established, applied once per method instead of once per function: `body_smb` collects checkpoints relative to `body_text`, then `merge`s into the real `source_map`, rebased at `body_text`'s own exact splice offset within `print_class_method`'s own printed fragment (computed by arithmetic against the fragment's own known length, not a text search — two ops could plausibly lower to identical body text, which a search could match ambiguously). `emit_provider` keeps its exact existing `out: &mut String` signature — it never owned a `Verbatim` construction site. `verbatim_sites` unchanged (5). `ts_writes` drops by **2** (1242 → 1240), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff: every fixture containing a real `provides`/provider declaration and `tsc_verify`'s full strict-`tsc` corpus pass unchanged. Per-method source-map correctness verified directly with a new project-form test (`bynkc/tests/source_map.rs`'s `provider_op_body_keeps_its_own_statement_lines`), itself proven to catch the bug the same way #1352/#1353's own regression tests were (temporarily reverted, confirmed the failure — a multi-statement provider-op body's own interior statements collapsed toward the class header — restored); a direct `bynk-ts` unit test (`prints_a_single_class_method_fragment`) pins `print_class_method`'s own output byte-for-byte. | R7.1 | #1331, #1337, #1315, #1352, #1353 |

**Arc D — settling (~8 slices)**

Provisionally lettered, not numbered — Arc C's own slice count is an estimate (~23-27, revised by
#1331, then #1333, then #1335, then #1337, then #1339, then #1351, then #1353, then #1355, then #1357, then #1359 — see §6's own "Revised estimate" paragraph above), so fixed `P7.N` numbers here would silently claim
a range Arc C's real slices will actually occupy. Real
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

Four probes, split across two slices (§5). `ts_writes` and `ts_any` **landed in P7.0** (#1296),
scoped to `as any` **and** bare `: any` (widened further after landing — review of #1297 — to also
catch generic type-argument position) per §3.3's own correction — mirroring `the-ir.md`'s own P6.0
being real infrastructure, not ceremony. `verbatim_origins` and `verbatim_sites` still need
building, in **P7.5**, alongside the `Verbatim`/`VerbatimOrigin` types they measure, since a probe
can't be fixture-tested before its target type exists. `design/bynk-greenfield-compiler.md`'s
Appendix D
carries no R7/R8 rows yet (its own note: "most of phases 6–8... have no live probe yet"), so this
settling pass adds none there — a future pass, once these probes exist and phase 7 is further
along, is the natural point to add them, not this one.

---

## 9. Risks

**Arc C's own originally-named first-slice trio (`contracts.rs` + `secrets.rs` +
`runtime_use.rs`) turned out not to need `bynk-ts` conversion at all — found and corrected by
P7.8 (#1313).** P7.5 seeded `VerbatimOrigin::Contracts`/`Secrets`/`RuntimeUse` around exactly
those three names, matching the schedule as written at the time. Reading all three in full
(P7.8's own grounding work): `contracts.rs`/`secrets.rs` build `bynk-contracts.json`/
`bynk-secrets.json` — hand-rendered JSON, never a `.ts` file, and `Document::Json` (not
`Document::Ts`) at their P7.6 construction sites; `runtime_use.rs` isn't emission code at all, a
`Cell`/`RefCell` accumulator recording which runtime helpers a module referenced. None of the
three can be "converted to real `bynk-ts` nodes" in any sense R7.1 means, because none of them
build `TsProgram`-shaped output today. **This was invisible before P7.6** (#1309): before
`Artefacts`/`Document` existed, every one of these three files' output sat in the same
undifferentiated `CompiledFile.typescript: String` field as genuine TypeScript, so nothing forced
the JSON/TS distinction to become visible. Confirmed by grep: `VerbatimOrigin::Contracts`/
`Secrets`/`RuntimeUse` have zero real `bynk-emit` callers, only test fixtures. This is a
correction, not a claim P7.5 erred — the seeding was reasonable given what was knowable before
P7.6 shipped. The three `VerbatimOrigin` variants stay (removing them is separate, later
cleanup, out of scope here); Arc C's own schedule (above) now names `events_fanout.rs` as the
real first file, which P7.8 (#1313) grounded its own new node algebra against directly.

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
- **ADR-B** — the migration escape hatch is `TsStmt::Verbatim{origin: VerbatimOrigin, text: String}`,
  statement granularity, sealed constructor, with a companion textual lint over its
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
