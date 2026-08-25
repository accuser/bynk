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
  confirmed), R8.17 (sorted route table, `workers_entry.rs:410/470/503` — three `.sort_by` call
  sites, http/cron/queue routes each), R8.18 (contract-hash ordering, `workers_entry.rs:298` —
  `own_contracts: &BTreeMap<String, String>`, deterministic by construction), R8.19 (typed TOML
  escaping, `escape_toml_basic_string`, `emitter/toml_doc.rs:215`, with a dedicated injection test
  at `toml_doc.rs:240-249`), R8.21 (`is_effectful_return` as the one shared predicate), R8.22
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

**Arc C — conversion, smallest blast radius first (revised nineteen times since, most recently to
~30-33 slices by #1377's own step (9) floor correction — see the "Revised estimate" paragraph below
for the live number and the full correction history; this heading's own original ~23-27 figure
is kept below only as the historical starting point, not a current estimate — review of #1378
caught it standing uncorrected here for several revisions running, with the live number always one
paragraph away in §6; was
~19 pre-Arc-C, ~12-18-more-from-here right after the design pass settled the `lower.rs` question,
honestly no longer close to the original ~19, and further still from the current live figure)**

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
ICU-formatting cluster (**landed**, slice 28, #1388, step (11) — see the table row below) →
`emit.rs`'s own remaining tree — the cross-context lowering cluster (**landed**, slice 29, #1390,
step (8) — see the table row below) — → `emit_project` itself (**landed**, slice 30, #1392, step
(10) — see the table row below; landed before `emit_integration_module`/`emit_test_module` below,
a real deviation from this ordering's own originally-guessed sequence, not an error — nothing about
`emit_project`'s own conversion depended on `tests_emit.rs` converting first) →
`project/tests_emit.rs`'s own remaining scope, unblocked by #1331 and now decomposed into 7
proposed slices by its own dedicated grounding pass (post-#1392, see below), **last of all, after
everything above**: the property-generator expression cluster (**landed**, slice 31, #1395, tests_emit.rs's own
slice A — `refined_gen_ts`/`gen_ts_for_ty`/`canon_ts_for_ty`/`binding_gen`), the small independent leaves
(**landed**, slice 32, #1399, tests_emit.rs's own slice B — `emit_integration_harness`/
`emit_test_deps`/`emit_ns_destructure`/`observation_call_record_types`), the stub cluster
(**landed**, slice 33, #1401, tests_emit.rs's own slice C — `emit_stub_class`/`emit_stub_rhs`), the case/scope-setup cluster
(**landed**, slice 34, #1403, tests_emit.rs's own slice D — `emit_test_scope_setup`/
`emit_test_case_function`), the property/history/attack runner cluster
(**landed**, slice 35, #1405, tests_emit.rs's own slice E — `emit_test_property_function`/
`emit_test_history_property_function`/`emit_contract_attack_function`), the HTTP driver cluster
(**landed**, slice 36, #1407, tests_emit.rs's own slice F — `emit_system_http_support`), and finally
`emit_integration_module` + `emit_test_module` themselves
(**landed**, slice 37, #1409, tests_emit.rs's own slice G — Arc C's own final slice), since both call
nearly everything named above and only make sense to convert once their own delegates already return
real nodes. Each slice
is checked against the P7.5 textual lint, not golden fixtures alone (§3.2). **`emitter/lower.rs`
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
can't name its own parameter for) and each message-entry's own renderer expression, then the
ICU-formatting cluster's own future scope (step (11)) — **now landed by #1388 (Arc C, slice 28),
closing step (11) too: see the table row below.**
**Step (6) is fully closed, in two slices: #1357 landed `emit_capability`'s own full conversion
(real generic, doc-commented interface methods — `bynk_ts::TsTypeMember::Method` gained
`generics`/`doc` — plus the real injection-token `const`); #1359 closed the named remainder,
`emit_provider`'s own per-op methods (each a real `bynk_ts::TsClassMethod` fragment, printed
through a new `bynk_ts::print_class_method` entry point, its own body source-mapped via a
per-method sub-builder/`merge`) — the class's own wrapper (header/`implements`/deps field+
constructor/closing brace) and the trailing factory `const` stay hand-written text, a deliberate
boundary, not a remaining gap (see the table row below for why).** **Step (7) is closed by #1361:
each `emit_service` handler converts to a real `bynk_ts::TsObjectEntry::Method`, printed through
the already-real `print_object_entry` — the enclosing `export const {name} = {`/`};` wrapper stays
hand-written, `deps`'s own dynamically-built type stays one opaque `TsType::named` string, and the
whole handler body (prologues, the optional events-IIFE wrapper, and the already-source-mapped
`body_out`) stays one opaque `TsStmtKind::Raw` blob — all three deliberate boundaries, not
remaining gaps (see the table row below for why). The two helpers `emit_service` directly calls to
build `deps_ty` (`cap_ref_ty`/`build_deps_object_ty_with_surface`) stay unchanged, `String`-
returning, confirmed directly — Decision B means nothing calls them differently.** **Step (8) splits,
the same "outer wrapper first, harder remainder later" shape steps (4)/(6) each already took, not
closes: #1364 lands `emit_make_surface`'s own full conversion — a real `bynk_ts::TsDecl::Function`
(wrapped in `TsDecl::Export`, printed through `bynk_ts::print_stmt`) whose body is a single `return`
of a real `TsExpr::multiline_object_entries`, one `TsObjectEntry::Method` per service, no opaque
carve-out at all (every shape it needs already existed in `bynk_ts`, and the function never lowers
an expression through `LowerCtx` — no `source_map` parameter even reaches it, so there is no
sub-builder/merge arithmetic to get right or wrong here, unlike almost every prior slice).
`emit_context_deps_interface` (the separate `export interface {Name}Deps {...}` builder
`emit_make_surface` calls first) stays exactly as it is, confirmed unaffected, `String`-returning —
not touched by this slice.** The cross-context lowering cluster (`lower_workers_cross_context_call`
and its own serialisation/codec-dispatch siblings, named by function, not by stale line range) is a
real, separate, harder remainder — genuine cross-module codec lowering, not top-level structure
construction — deferred to its own future slice(s), not yet grounded in detail — **now landed by
#1390 (Arc C, slice 29), closing step (8) entirely: see the table row below; (9) `emit_agent`
and its websocket-dispatch cluster — grounded in full after slice 18 landed (see the dedicated
grounding-pass paragraph below); real scope is smaller than the stale citation claimed
(`emit_agent` itself is `emit.rs:3642-4917`, 1276 lines, not ~2139; the real cluster is
`ws_open_do_method_name` through `emit_ws_dispatch_handlers`, `emit.rs:4954-5338`, ~385 lines, not
~1047 — the file's own trailing `#[cfg(test)]` modules for already-converted functions from
earlier slices are not part of this step at all, a stale-citation correction in the same family as
#1361's own `emit.rs:2638-4776` fix). Total real scope ~1661 lines. Proposed as 5-6 slices, landed
in dependency order (each assumes the previous one's real-node scaffolding exists), not the 3-5
originally guessed** — see the grounding-pass paragraph below for the full breakdown; (10) `emit_project` itself, last, once every function
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

**`tests_emit.rs` grounding pass (post-#1392), before any slice against
`emit_integration_module`/`emit_test_module` or their own delegates is proposed.** With every other
step in `emit.rs`'s own decomposition order landed or decided (#1392), this file
(`bynk-emit/src/project/tests_emit.rs`, 4,411 lines) is Arc C's own last remaining target — read
directly rather than trusted from this section's own pre-Arc-C "130 byte-golden assertions" citation
(see the confidence-caveats item below on what that number actually refers to). **Already converted,
confirmed by direct read, not remaining work**: `emit_test_main` (slice 5, #1325); `emit_commons_
barrel` (1853-1899) — missed by an earlier grep for `push_str`/`format!` sites, since it already
builds `TsStmt::comment`/`TsDecl::ReExportAll`/`TsProgram` directly, not text — a re-grounding
correction the same shape as #1333's own step-1 re-read, recorded here rather than silently
re-slicing already-real code; and the builder-helper block (3718-3804: `ident`/`str_lit`/`call`/etc.),
real reusable `TsExpr`/`TsStmt` shorthands. **Confirmed pure Rust logic, zero TS-emission, never an
Arc C target** (the same "9 of 13 aren't emission code at all" finding #1333's own step-1 re-grounding
made, applied to this file): `process_tests`/`process_integration_tests`/`driver_param_ty`/
`strip_effect_httpresult`/`integration_typed_commons`/`sanitise_suite`/`discovered_location`/
`discovery_manifest`/`block_uses_observation`/`target_service_handler_kinds`/`prop_history_binding`/
`prop_is_history`/`is_attackable_contract`/`numeric_or_scalar_base`/`attackable_contracts`/`json_codec_
qual_for_target`/`relative_import_for_test`/`lower_stub_value_block` (delegates to `emitter::lower_
block_to_async_body`, already-lowered opaque text, not direct emission)/`synthetic_typed_commons_for_
target`/`int_bounds`/`float_bounds`/`str_min`/`base_canon`/`sanitise_case_name`. **Five further
functions are verbatim static-file inclusion** (`include_str!` of `.ts` runtime files, not per-input
construction) — `expectation_runtime_helpers`/`stub_runtime_helpers`/`observation_runtime_helpers`/
`property_runtime_helpers`/`history_runtime_helpers` — the same permanent-opacity class
`emitter/lower.rs` itself occupies (Third correction, above), not a slice target either.

**Real TS-emission needing conversion, ~1,180 real lines across 18 functions** (2 module builders +
12 sub-emitters + 4 generator-cluster functions: 270 + 640 + 270 — reconciled against the per-
function estimates below, correcting a first-pass miscount that dropped the module builders and the
generator cluster from its own headline, leaving only the 640-line sub-emitter subtotal) — comparable
in kind, if not raw line count, to `emit_agent`'s own step (9) sub-tree (9 slices): the two top-level
module builders (`emit_integration_module`, 324-545, ~120 real lines; `emit_test_module`, 1329-1831,
~150 real lines — both the same header/import/per-case-wrapper/`run()`-runner shape `emit_project`'s
own already-converted `run()` established); a family of sub-emitters (`emit_integration_harness`,
~50 real lines; `emit_system_http_support`, ~230 real lines, the largest single unit — several
near-identical `async function __sysdrive_*` templates per route/service; `emit_stub_class`, ~90 real
lines — `class __Stub_<Cap> { fields; async methods with if-chains }`; `emit_stub_rhs`, ~55 real
lines — builds `switch`/`case` text; `emit_test_deps`, ~15 real lines; `emit_ns_destructure`, ~10
real lines, called pervasively by the others; `emit_test_scope_setup`, ~25 real emission lines amid
much Rust-side name-collection logic; `emit_test_case_function`, ~25 real lines;
`observation_call_record_types`, ~15 real lines; `emit_test_property_function`, ~40 real lines;
`emit_test_history_property_function`, ~40 real lines, mirrors the property function;
`emit_contract_attack_function`, doc-confirmed (not yet fully read) to mirror the property function
similarly, ~40-50 real lines); and a distinct, mutually-recursive property-generator expression
cluster (`refined_gen_ts`/`gen_ts_for_ty`/`canon_ts_for_ty`/`binding_gen`, ~270 real lines total) —
these build TS *expression* source recursively via `format!`/match returning `String`, not
`writeln!`/`push_str`, but are genuine per-input TS-emission, directly analogous to the already-
converted ICU cluster's `-> TsExpr` treatment (slice 28, #1388).

**Real `bynk_ts` algebra gaps, checked fresh against the current `program.rs`**: none found —
`TsStmtKind::TryCatch`, `TsStmtKind::Switch`/`TsSwitchCase`, `TsClassField`/`TsClassCtor`/
`TsClassMethod`, `TsDecl::Class` already cover every shape this file needs (class-with-fields-and-
methods, switch/case, if-chains, try/catch, async functions, array/object literals). One thing to
verify empirically during implementation, not assumed: whether `TsExpr::Object`'s multiline-entries
rendering matches this file's own dense, no-trailing-comma-on-last-entry inline-object style (e.g.
`emit_test_deps`'s `return { ... };`, the `__gens`/`__handlers` array-of-objects) — `TsExpr::Object`
already supports both single-line and multiline modes per existing precedent, likely fine, but not
directly confirmed against this file's own exact fixture bytes yet.

**Proposed decomposition, 7 slices, dependency order named explicitly**: (A) the property-generator
expression cluster (**landed**, slice 31, #1395 — `refined_gen_ts`/`gen_ts_for_ty`/`canon_ts_for_ty`/`binding_gen`) — self-
contained, no dependency on any other slice here, first since slices D/E consume its output as
strings today and would want it converted before or alongside their own conversion (confirmed by
landing: the three call sites inside D's/E's own `binding_gen` callers print the new real `TsExpr`
fields back to text via `bynk_ts::print_expr`, the established "print a fragment, splice into a
still-textual caller" seam); (B) the small
independent leaves (**landed**, slice 32, #1399 — `emit_integration_harness`/`emit_test_deps`/
`emit_ns_destructure`/`observation_call_record_types`) — converted `emit_ns_destructure` here since
C (2046), D (2543/2581/2606), and G (455) call it, confirmed by landing: `emit_ns_destructure` keeps
its exact `&mut String`-accumulator signature (its own callers, C/D/G, are all still unconverted),
building real `TsStmt` nodes internally and printing each one via `bynk_ts::print_stmt`, the identical
seam A's own `binding_gen` splice already established; (C)
the stub cluster (**landed**, slice 33, #1401 — `emit_stub_class`/`emit_stub_rhs`) — depended on B,
confirmed by landing (`emit_stub_class` calls `emit_ns_destructure` directly); each of `TsDecl::Class`/
`TsStmt::Switch` did have a direct `bynk_ts` equivalent as guessed, but `emit_stub_class`'s own class
wrapper (header/fields) stayed hand-written text (Decision C, `emit_provider`'s own precedent) rather
than a real `TsDecl::Class` — each method is a real `TsClassMethod` fragment instead, since its own
body mixes real structure with an opaque `async () => { ... }` IIFE (`TsExpr::Arrow`'s own
expression-body-only design, the same boundary `emit_composition_root`'s `__eventsDispatch` closure
already established) that a whole-node `TsDecl::Class.methods` list can't host correctly at a
non-zero depth; (D) the case/scope-setup cluster (**landed**, slice 34, #1403 —
`emit_test_scope_setup`/`emit_test_case_function`) — depended on B, confirmed by landing
(`emit_test_scope_setup` calls the now-real `emit_ns_destructure` directly); (E) the property/history/attack
runner cluster (**landed**, slice 35, #1405 — `emit_test_property_function`/`emit_test_history_property_function`/`emit_contract_
attack_function`) — depended on A alone (consumes `BindingGen`), confirmed by landing: each function's
own `__gens`/`__handlers` construction now consumes `binding_gen`'s real `TsExpr` fields directly,
closing the print-then-splice seam A's own doc comments named as temporary; (F) the HTTP driver cluster
(**landed**, slice 36, #1407 — `emit_system_http_support`) — structurally self-contained, no
dependency on any slice above, confirmed by landing; landed as **one** slice, not split by
driver-kind as this paragraph's own proposal-time caveat considered — once drafted, the 4 per-route
driver kinds shared one real structural shape completely (differing only in name/params/options/decode
function, all plain parameters to one shared `sysdrive_driver` helper), so splitting them across
separate PRs would have meant reviewing the identical helper four times rather than once;
(G) the two top-level module assemblers
(**landed**, slice 37, #1409 — `emit_integration_module`/`emit_test_module`), last, depended on B
(`emit_integration_module`'s own direct `emit_ns_destructure` call), confirmed by landing, in
addition to calling nearly everything above and only making sense to convert once their delegates
already returned real nodes rather than raw strings — every header/import line and each module's own
`run(only)` runner converted fully; the splice-sensitive per-case/per-property/per-attack loop
bodies stayed exactly as they were, unaffected.

**A pattern already established elsewhere in this step's remaining work applies here too, named
explicitly rather than left implicit**: `emit_test_scope_setup`/`emit_test_case_function`/
`emit_test_property_function`/`emit_test_history_property_function`/`emit_contract_attack_function`
all delegate an opaque lowered body from `emitter::lower_test_case_body`/`lower_block_to_async_body`
(`String`-returning, unchanged) — spliced in exactly like every prior slice's `Raw`-body opaque-splice
pattern, not built as real nodes themselves; each slice's own proposal should say so explicitly so it
isn't mistaken for missed scope, the same "two-level offset" naming discipline #1361/#1375 already
established for `emit_service`/`emit_agent`'s own handler bodies.

**Confidence caveats, honestly recorded**: the "130 byte-golden assertions" citation earlier in this
section does not refer to a literal `assert_eq!`/`assert!` count inside `tests_emit.rs` itself (only
36 exist there) — it is almost certainly the count of fixture files across `bless_positive_fixtures`/
`positive_fixtures` whose `tests/*.test.ts`/`tests/integration_*.test.ts` output this file's two
target functions produce, the byte-golden corpus this conversion must stay zero-diff against, not an
in-file literal; worth confirming the exact fixture count during slice G's own zero-diff run rather
than deriving it now — doesn't block slices A-F, which are covered by the same corpus indirectly
through G's own callers. `emit_contract_attack_function`'s own body (154 lines) was not fully read
this pass — relied on its own doc comment's explicit "mirrors `emit_test_property_function`" claim;
worth a full read at slice E's own proposal time, since that same doc comment names at least one
extra real transformation (an `Int`-to-`number` coercion) beyond the plain mirror.

**Step (9) grounding pass (post-#1365), before any slice against `emit_agent` is proposed.**
`emit_agent` (`emit.rs:3642-4917`) decomposes into real, separable phases, each marked by the
function's own `// N)` comments — not one tightly coupled blob: a data-prep section (3650-3899,
pure Rust, no TS text — reads `store_fields` into 5 typed collections across `Cell`/`Map`/`Set`/
`Cache`/`Log` kinds); **1) the state record type** (3900-3991, `export interface {Name}State {
... }`); a zero-factory pair (3992-4074, `const {registry} = new StateRegistry();` +
`function __zeroOf{Name}State(): {Name}State { ... }`); a rehydrate function (4075-4192); **2) the
Durable Object class** (4193-4744, one `export class {Name} { ... }` containing fields+constructor,
`private async loadState()`/`commitState()`, **3) per-handler methods** (4349-4587, the real bulk —
structurally near-identical to `emit_service`'s pre-#1361 shape), and conditionally WS-hosted
lifecycle methods plus a Workers-mode `fetch` dispatcher); a factory function (4746-4766); and a
conditionally-emitted history-driver (4768-4916, test-support only, gated on
`ctx.history_target_agents`, no `LowerCtx` involvement, self-contained). The websocket-dispatch
cluster is `ws_open_do_method_name`/`ws_message_do_method_name`/`ws_close_do_method_name`/
`ws_open_hosts_for`/`emit_ws_do_method`/`emit_ws_open_fetch_branch`/`ws_attachment_deps_arg`/
`emit_ws_dispatch_handlers` (`emit.rs:4954-5338`, ~385 lines — everything after 5338 is pre-existing
`#[cfg(test)]` modules for functions already converted by earlier slices, not this step's own
scope).

**Real `bynk_ts` algebra gaps, checked fresh against the current `program.rs`**: only two, both
small, mechanical struct fields — `TsDecl::Interface` and `TsDecl::Class`/`TsClassMethod`/
`TsClassField`/`TsClassCtor` already exist and need no new variant. (1) `TsClassMethod` has no
`private: bool` (only `TsClassField` does) — `loadState`/`commitState` are both `private async`,
the first real `private` class method this track has hit (every method landed so far, `emit_provider`'s
ops #1359, was public-only). (2) `TsClassMethod` has no `doc: Option<String>` — each `on call`
handler carries its own doc comment immediately before its method, the same need
`TsObjectEntry::Method.doc` (#1337) and `TsTypeMember::Method.doc` (#1357) already solved for their
own node kinds; `TsClassMethod` is the one remaining method-shaped node without it — **worth naming
explicitly given #1361's own doc-duplication bug was exactly this class of mistake** (a standalone
`emit_doc_block` call left in place after the real `doc` field started carrying it), so a future
slice's own implementation should check for that specific regression shape proactively, not
rediscover it via a fixture diff a second time. No other new expression/statement/type gap found:
the events-dispatch IIFE prologue/epilogue wrapping a handler body is byte-identical in shape to
`emit_service`'s own (#1361) — the same "whole prologue+body+epilogue as one opaque `TsStmt::Raw`,
printed via `print_class_method`, two-level offset" pattern applies directly, not a new design; the
WS-cluster functions are single-method bodies with a normal single-level splice, the same shape
`emit_provider`'s own per-op methods (#1359) already established.

**Source-map complexity**: `emit_agent` lowers expressions through `LowerCtx` in three places — the
zero-factory's per-`Cell`-field static initializer (small, direct; whether it needs a sub-builder
at all is unconfirmed, not assumed, and should be checked at the implementing slice's own time, not
guessed here); `commitState`'s invariant/transition predicates (writes directly into `out` today,
no local buffer — the same "direct write, real position" shape `emit_free_fn`/
`emit_contract_guarded_body` had *before* their own conversions introduced sub-builders for
`Raw`-embedding); and per-handler bodies (**already** uses a `body_smb`/splice-into-`out` local
buffer, the identical mechanism #1352 established, just not yet wrapped in a `TsStmt::Raw`/
`print_class_method` fragment). None of this is source-map-risky research — it is the same
mechanics already proven correct four times over (slices 12/13/16/17); the implementing slices need
care applying it, not new design.

**Fixture coverage confirmed real and substantial, no new fixture expected for any of the slices
below — corrected by review of #1366, which checked each fixture's real content rather than
trusting the marker-name match**: 31 fixtures use `store ... Cell/Map/Set/Cache/Log`.
`238_websocket_inbound_workers` (Workers-mode, all three of `on open`/`on message`/`on close`) is
the **only** fixture covering the full DO-hosted WS-dispatch cluster — `ws_message_do_method_name`,
`ws_close_do_method_name`, `emit_ws_do_method`'s message/close arms, and
`WsOpenHost::has_inbound`'s true branch are each pinned by exactly this one fixture, not three.
`237_websocket_chatroom_workers` (Workers-mode, `on open` only) separately pins the
`has_inbound == false` branch. `236_websocket_chatroom` is single-file form (no Workers target),
so it exercises `on message`/`on close` only at the language level, not the DO-dispatch cluster
this step actually scopes — dropped from this list rather than left implying dispatch-cluster
coverage it doesn't provide. `235_held_connection`/`1197_agent_held_map_only_write_still_commits`
cover held-connection paths; `248_history_property`/`249_history_provides` cover the
history-driver. A zero-diff conversion slice is legitimately verifiable by one fixture per real
shape, so "no new fixture expected" still holds — resting now on the real count, not an inflated
one.

**Proposed decomposition, 5-6 slices — a genuine widening past the original "3-5" guess on both
ends (floor 3→5, ceiling 5→6, corrected by review of #1366, which caught the first draft's own
"kept the top of the range" framing as wrong against its own arithmetic), not a narrowing: the
class-body method count pushes this step's real risk past its own original range entirely, even
though its real line count is smaller than the stale citation claimed), landed in dependency
order**: **(1) the state interface alone — landed by #1367 (Arc C, slice 19), fully, no opaque
carve-out, confirming this sub-slice's own "no algebra gap, no source-map exposure" prediction
exactly** (cleanest, smallest, the same risk/shape as slice 15, `emit_capability` — see the table
row below); **(2) the zero-factory + rehydrate function pair — landed by #1369 (Arc C, slice 20)**:
two free `TsDecl::Function`s, needs neither of the two new `TsClassMethod` fields (these aren't
methods) — correctly predicted — but DID need one real, unanticipated gap: `TsDecl::Function`
itself had no way to render as a single physical line (the zero-factory's own real shape), closed
with a new `inline: bool` field mirroring `TsObjectEntry::Method.inline`'s own identical precedent
(#1337) at a different node kind (see the table row below for the full account); **(3) the class
scaffold (header/fields/constructor stays hand-written, the same Decision-C-style boundary #1359
already used) plus `loadState`/`commitState` as real `TsClassMethod` fragments — now fully landed,
in two slices: #1371 (Arc C, slice 21) split off and landed `loadState` first (`TsClassMethod.
private` lands here, its own zero-source-map-risk body converting cleanly; `this.state.storage.
get<T>(...)`'s own generic-call syntax stays one opaque `TsExpr::Ident` callee, not a new
`TsExpr::Call.type_args` field — 41 real construction sites across the workspace, far more than
this one narrow need justifies touching); #1373 (Arc C, slice 22) then closed the deferred
remainder, `commitState` — and, unlike the caution #1371 itself raised, actually needed NO
sub-builder/merge work at all: `record_span` is a documented no-op when a `LowerCtx` has no
attached source-map builder, and neither invariant nor transition predicate lowering here is ever
given one (`emit_agent`'s own `source_map` parameter isn't threaded into either `LowerCtx::new(...)`
call) — confirmed by reading the code directly, not assumed from the general "predicate lowering
needs care" shape #1352/#1353 established elsewhere. A real correction to #1371's own stated
expectation, honestly recorded rather than silently landing "as predicted."**;
**(4) the per-handler methods themselves — landed by #1375 (Arc C, slice 23), exactly the "single
hardest slice in this whole step" this doc predicted**: each handler converts to a real
`bynk_ts::TsClassMethod` fragment via `print_class_method`, closing `TsClassMethod.doc` (the
grounding pass's own second predicted gap, after `private`) — the same two-level offset pattern
#1361 (`emit_service`) established, but with a genuinely new THIRD wrapper dimension `emit_service`
never needed: `writes_state` (the implicit-commit closure), which can combine with
`body_emits_directly` (the events-IIFE) at once — both wrappers, and their combination, preserved
exactly, including the currently dead-by-construction `is_store_agent == false` branches
(`is_store_agent` is a hardcoded `true` local constant, never reassigned — the conversion preserves
its own `else` branches faithfully rather than opportunistically removing provably-dead code out of
scope). See the table row below for the full account, including the bug-injection-proven regression
test covering both wrapper dimensions at once, not just one in isolation.**; **(5) the
WS-hosted DO methods, the `fetch` dispatcher, and `emit_ws_dispatch_handlers` — its own dedicated
grounding pass (post-#1378, no issue number of its own) found this cluster genuinely LOWER risk
than its size suggested, despite #1377's own caution**: ~430 real lines across 5 emission sites
(`emit_ws_do_method`, `emit_ws_open_fetch_branch`, `ws_attachment_deps_arg`, `emit_ws_dispatch_handlers`,
and the `fetch` method inside `emit_agent` itself — `ws_open_hosts_for` builds no TS
text at all, pure Rust-side `Vec<WsOpenHost>` construction, not an emission unit). Decomposes
cleanly into 3 independent slices, no ordering constraint between them: **`emit_ws_do_method` —
landed by #1380 (Arc C, slice 25), exactly as this grounding pass predicted**: single-level merge,
the identical shape `emit_provider`'s own already-landed ops (#1359) established, the least risky
of the three; `doc: None`, confirmed rather than assumed — this function never emitted a doc
comment (no `doc` parameter, no local, no `emit_doc_block` call), so the conversion correctly did
NOT mirror the sibling handler-method conversion's own `doc: h.documentation.clone()` idiom, the
exact trap review of #1379 flagged in advance. `connection: Connection<T>`'s own generic type
stays one opaque `TsType::named` string, the same "an odd, one-off shape stays opaque text"
precedent `this.state.storage.get<T>` already set twice (#1371/#1373) — see the table row below
for the full account. `emit_ws_dispatch_handlers` — landed by #1382 (Arc C, slice 26),
**which `ws_attachment_deps_arg` rides along with** (its only call site is inside
`emit_ws_dispatch_handlers`) — **zero `LowerCtx` involvement at all, confirmed by direct read**:
this function lowers no `.bynk` body, carries no `source_map` parameter at all, so there was
nothing to merge — both methods (`webSocketMessage`/`webSocketClose`) convert fully, no opaque
carve-out for the class-method-level structure, the same shape #1364/#1377 already achieved,
building real `TsStmt`/`TsExpr` node trees (`Const`/`Let`/`If`+`InlineBlock`/`As`/`New`/`Await`+
`Call`, plus the real `TsType::Union` — already-landed #1315, a genuine fit here, not an escape
hatch) rather than one opaque `Raw` blob, the same "pure boilerplate converts fully" precedent
`loadState` (#1371) established. **A real, previously-unpredicted correction to the grounding
pass's own account**: the grounding pass predicted only two opaque-escape-hatch shapes for this
whole 3-slice cluster (a spread call argument and `new WorkersConnection<T>(...)`) — the spread
argument turned out to belong to the still-unlanded `fetch` method, not this slice, while this
slice needed two DIFFERENT opaque shapes the grounding pass never named: the two
`try { ... } catch { ... }` lines (`TsStmtKind::TryCatch`'s own renderer has no single-line form —
every other real `TryCatch` site in this tree prints multi-line) and the
`void code; void reason; void wasClean;` suppression line (no `TsUnaryOp::Void` exists — this
function's own only real site). Neither is a new `bynk_ts` variant — both stay one `TsStmt::raw`
line each, the same "an odd, one-off shape stays opaque text" posture, just for two shapes the
grounding pass's own read did not surface. `emit_ws_open_fetch_branch` plus `emit_agent`'s own
`fetch` method — landed by #1384 (Arc C, slice 27), the third and final of sub-slice (5)'s own 3
independent slices, **closing sub-slice (5) entirely**. Same zero-`LowerCtx` posture: `fetch`
converts to a real `bynk_ts::TsClassMethod`; `emit_ws_open_fetch_branch` converts from a
`&mut String`-writing function into one returning a real `TsStmt::If` node, composed directly into
`fetch`'s own body once per WS-open host. **A real, previously-unpredicted algebra gap, beyond the
grounding pass's own correctly-predicted spread-call-argument shape** (`...args` stays one opaque
`TsExpr::Ident` call argument, exactly as predicted — no `TsExpr` variant represents a call-argument
spread, only `TsObjectEntry::Spread` for an object-literal spread already exists): both
`(this as unknown as Record<string, (...bynkArgs: unknown[]) => unknown>)` and `this.__env as
unknown as Record<string, unknown>` are a nested `As`-under-`As` chain (`x as A as B`) —
`TsExpr::As`'s own operand-parenthesization rule always wraps a nested `As`, which is correct for
the real cases that motivated it but would turn this real, parenless chain into a byte-diff-wrong
`(x as A) as B`. Rather than special-case "no parens for a nested `As` under an outer `As`" in the
printer (unclear how many other real sites, if any, want the opposite), both stay one opaque
`TsExpr::Ident` string each — the same "odd, one-off shape stays opaque text" posture. **Fixture
coverage confirmed sufficient for the first two slices, and the gap review of #1379 found for the
third now closed**: `238_websocket_inbound_workers`/`237_websocket_chatroom_workers` needed no new
fixture (confirmed by #1382's own zero-diff-on-first-attempt landing, matching slice 20's own prior
finding); for `fetch` specifically, `238` only reached its plain `else` arm, `328_agent_given_workers`
covered the `given_deps_expr` arm but isn't a WS fixture, and the `agent_uses_emit` arm (the
`__eventsDeps`/`dispatchToEventsFanout` rebuild) had **zero fixture coverage anywhere** —
`events_workers_wiring.rs`'s own `Ledger` agent exercises the Rust-side flag when compiled, but
that test only asserts on `wrangler.toml`/`events_fanout.ts`/`index.ts` substrings, never on
`handlers.ts` where `fetch` actually lives, so it gave no byte-level proof. Closed with a new
positive fixture, `1384_agent_emits_workers` — a workers-target agent whose own handler calls
`Events.emit` directly (no service in between, no `given` capability of its own either, pinning
the `agent_uses_emit` branch in isolation from `given_deps_expr`) — rather than the
deliberate-named-exclusion alternative item (6) below uses for the history-driver: unlike that
one, this branch is real, reachable production code (any agent handler that emits), not
test-support-only, so a fixture closing the real gap was the right call, not a documented
skip.**; **(6) the factory function plus the history-driver — split by #1377 (Arc C,
slice 24), the same "split, don't force one slice to cover more than it needs to" discipline this
track has used repeatedly (steps (4)/(6)/(8), and step (9)'s own sub-slice (3)): the factory
function lands fully, no opaque carve-out at all (every shape needed — `TsDecl::Function`,
`TsExpr::OptionalMember`/`Arrow`/`New` — already existed). **The history-driver's own priority,
left undecided by #1377, is now decided by #1386: a deliberate, standing exclusion, not a
conversion slice — closing sub-slice (6), and with it step (9), entirely.** Three real, converging
reasons: (1) test-support-only, stripped from deploy builds — `history_target_agents` gates on a
`for all run: History[Agent]` property (ADR 0155), never part of a deployed Worker/bundle, the
first (and only remaining) function in `emit_agent` that doesn't ship to production; (2) only 2
fixtures repo-wide exercise it (`248_history_property`/`249_history_provides` — one agent name in
one target configuration; the other real `History`-property fixtures stop at the checker or assert
only on runtime output, pinning no bytes here), among the thinnest coverage of any item this track
has converted or excluded; (3) the
pre-conversion code itself already defers the one real algebra question a conversion would face —
its own comments say `call`'s real shape needs "a single driver-wide type... the
intersection/union across every handler this agent's history targets, not a same-line text
change," deferred to `any` today BY DESIGN — a faithful `bynk_ts` conversion of its two `switch`
statements plus the composed `step_ty`/`Array<{...}>` return type would mostly relocate `writeln!`
calls into `TsStmt`/`TsExpr` builders without closing that one interesting gap, since the gap stays
`any` either way. Not claimed permanent in ADR 0391's own whole-subsystem sense — revisiting is
legitimate if a real per-handler-union need ever makes the payoff clearly worth it — and not given
a separate ADR of its own, a narrower single-function scope decision the track doc's own narrative
+ estimate paragraph records at the right weight (no table row — this is a decision, not a landed
slice, the same shape the grounding passes before #1367/#1380 also got no row for), matching how
`emit_agent`'s own `fetch` method's
`agent_uses_emit` branch was ALSO a live deliberate-exclusion candidate right up until #1384 closed
it with a fixture instead — this is the same decision point, resolved the other way, for a
genuinely different (test-support, not production) function. No code changes: `bynk-emit/src/
emitter/emit.rs` is untouched by this decision.**

**Revised estimate, corrected thirty-five times now, the last — by review of #1332 (the arithmetic), by #1333's
own step (1) closure (the real per-step sizing), by #1335's own step (2) split, by #1337's
own step (3) closure, by #1339's own step (2) closure, by #1351's own step (4) split, by #1353's
own step (4) closure, by #1355's own step (5) closure, by #1357's own step (6) split, by
#1359's own step (6) closure, by #1361's own step (7) closure, by #1364's own step (8) split, by
step (9)'s own dedicated grounding pass (post-#1365, no issue number of its own — a research pass,
not a slice), by #1367's own landing of step (9)'s first sub-slice, by #1369's own landing of step
(9)'s second sub-slice, by #1371's own split-and-partial-landing of step (9)'s third sub-slice, by
#1373's own landing of that sub-slice's deferred remainder, by #1375's own landing of step (9)'s
fourth sub-slice, by #1377's own split-and-partial-landing of step (9)'s sixth sub-slice, by
sub-slice (5)'s own dedicated grounding pass (post-#1378, no issue number of its own — a research
pass, not a slice, the same shape as step (9)'s own original grounding pass), by #1380's
own landing of the first of sub-slice (5)'s own 3 independent slices, by #1382's own
landing of the second, by #1384's own landing of the third, closing sub-slice (5)
entirely, by #1386's own decision to exclude the history-driver, closing sub-slice (6)
and step (9) itself entirely, by #1388's own landing of the ICU-formatting cluster,
closing step (11), by #1390's own landing of the cross-context lowering cluster,
closing step (8) entirely, by #1392's own landing of `emit_project`'s own
header/import/rebrand cluster, closing step (10) — every step in `emit.rs`'s own
decomposition order now landed or decided — and now by `tests_emit.rs`'s own dedicated grounding
pass (post-#1392, no issue number of its own — a research pass, not a slice, the same shape as
step (9)'s own original grounding pass and sub-slice (5)'s own), decomposing Arc C's own final
remaining piece into 7 proposed slices, by #1395's own landing of `tests_emit.rs`'s own
slice A (the property-generator expression cluster), the first of the 7, and now by #1399's own
landing of slice B (the small independent leaves), the second, and now by #1401's own landing of
slice C (the stub cluster), the third, and now by #1403's own landing of slice D (the
case/scope-setup cluster), the fourth, and now by #1405's own landing of slice E (the
property/history/attack runner cluster), the fifth, and now by #1407's own landing of slice F
(the system-http driver cluster), the sixth, and now by #1409's own landing of slice G (the two
top-level module assemblers), the seventh and last — closing `tests_emit.rs`'s own 7-slice
decomposition entirely, and with it, Arc C itself.**
Steps (2)-(7) are all
**fully landed** and entirely out of the remaining-work sum. Step (8)'s own remainder narrows
from "1-2" to **fixed at 0** — landed as ONE slice (its real scope, once actually read, was 2
functions, not the "genuine cross-module codec lowering... harder remainder" the original citation
implied), confirming the range's own floor, the same "a range resolved to its own floor" shape
#1361/#1386/#1388 already showed three times. Summing the list
directly: step (8)'s remainder is 0, fixed; step (9)'s remainder is 0, fixed; step
(10) is one slice, fixed; step (11)'s remainder is 0, fixed — **every term in the sum is now fixed,
not a range.**
`emit.rs`'s own remaining tree is now a fixed **0 slices** — `emit_project` itself (step (10))
landed by #1392, the last item still carrying the "never independently re-grounded" caveat now
resolved by direct read: real, but genuinely smaller than the raw ~1,100-line citation across its 7
direct helper functions implied — most of that is Rust-side decision logic that never changes, the
real conversion surface a modest ~29 `writeln!`/`write!` call sites, comparable to several
already-landed slices, not a multi-slice undertaking. `ast_importers` stayed unchanged at 5,
confirming the design pass's own original prediction directly rather than assuming it.
`tests_emit.rs`'s own citation, "the pair" (2), is now **superseded by direct read**: its own
dedicated grounding pass (post-#1392, see above) found ~1,180 real TS-emission lines across 18
functions, not 2 — a grounding-driven widening, the same "an unread range turns out larger than
guessed" shape the pass before #1367 and the pass before #1380 already showed, decomposed into 7
proposed slices (A-G, dependency order named in the grounding-pass paragraph above), not the 2 this
citation originally implied (`emit_integration_module`/`emit_test_module` alone, without their own
delegate sub-emitters and the property-generator expression cluster, none of which this track had
separately named before now). `tests_emit.rs`'s own slice A landed by #1395 immediately after —
the property-generator expression cluster (`refined_gen_ts`/`gen_ts_for_ty`/`canon_ts_for_ty`/
`binding_gen`) converts **fully, no opaque carve-out at the node-structure level**: every real
recursive shape (`Call`, `Member`, `Arrow` with an expression body, `Array`, `Object`, `Conditional`,
`Paren`) is a real node; the one leaf that stays opaque is `emitter::unchecked_construct_test`'s own
already-formed text (also called from `lower.rs`'s own permanently-excluded lowering, so its exact
`-> String` signature is untouched, per ADR 0391's own established boundary), carried as an `Ident`
the same way P7.9's `ts_base` and #1369's `Cell` zero value already carry an unconverted shared
sibling's own text. The `"{  }"` double-space quirk recurs a fourth time (after `workers.rs`/#1321,
`project.rs`/#1327, `emit.rs`/#1390), guarded explicitly rather than assumed unreachable for a
zero-field record. No new `bynk_ts` algebra gap. Slice E's own three functions (not yet converted)
now print `binding_gen`'s real `TsExpr` fields back to text via `bynk_ts::print_expr` at their own
three call sites, confirming the grounding pass's own predicted seam exactly. `ts_writes` drops by
**12** (1114 → 1102), verified via a fresh `cargo xtask greenfield-status --apply`.
`verbatim_sites`/`ast_importers`/`ts_any` unaffected. Zero diff, first attempt, no iteration:
`positive_fixtures`/`bless_positive_fixtures` and `tsc_verify`'s full strict-`tsc` corpus pass
unchanged. `tests_emit.rs`'s own slice B landed by #1399 immediately after — the 4 small independent
leaves (`emit_integration_harness`/`emit_test_deps`/`emit_ns_destructure`/
`observation_call_record_types`) each convert **fully**: `emit_integration_harness`/`emit_test_deps`
keep their exact `-> String` signatures (`emit_test_module`, slice G, is still unconverted) and
return `bynk_ts::print_stmt`'s own output; `emit_ns_destructure` keeps its exact `&mut String`
signature (its own callers, slices C/D/G, are all still unconverted) and appends each printed
statement instead; `observation_call_record_types` converts its `TsDecl::TypeAlias`/`TsType::Object`
construction the same way. The `"{  }"` double-space quirk recurs a fifth and sixth time (`emit_test_deps`'s
own zero-entries `return`, `observation_call_record_types`'s own zero-param capability op), both
guarded explicitly, the established precedent (#1321/#1327/#1390/#1395) applied twice more rather
than consolidated. **A real cleanup, not new scope**: `emitter::ts_type_ref_qualified` (a
`String`-returning function `observation_call_record_types` used to call) had no other production
caller once converted — deleted rather than left dead code, the same call slice 6 made for
`ts_string_literal`, with its own 2 direct unit tests rerouted through its already-existing
`TsType`-returning twin (`ts_type_ref_qualified_ts_type`, added alongside it in slice 3/#1321) plus
`bynk_ts::print_type`, keeping the identical behaviour pinned. No new `bynk_ts` algebra gap.
`ts_writes` drops by **5** (1102 → 1097), verified via a fresh `cargo xtask greenfield-status
--apply` — smaller than the structural scope might suggest, matching the grounding pass's own "small"
naming for this cluster. `verbatim_sites`/`ast_importers`/`ts_any` unaffected. Zero diff, first
attempt, no iteration: `positive_fixtures`/`bless_positive_fixtures` and `tsc_verify`'s full
strict-`tsc` corpus pass unchanged. `tests_emit.rs`'s own slice C landed by #1401 immediately after —
the stub cluster (`emit_stub_class`/`emit_stub_rhs`) confirms the grounding pass's own citation of
"both with direct `bynk_ts` equivalents already" was right about the shapes existing but not about
how they'd compose: `emit_stub_class`'s own class wrapper (header/fields) stays hand-written text,
Decision C (`emit_provider`'s own precedent, #1359) — each method is a real `TsClassMethod` fragment,
printed via `bynk_ts::print_class_method` at depth 0 (the only depth a `Raw`-bodied method's own
`debug_assert!` allows), since the method body mixes real structure with an opaque `async () => {
... }` IIFE that `TsExpr::Arrow`'s own expression-body-only design can't host (the identical boundary
`emit_composition_root`'s `__eventsDispatch` closure already established). `emit_stub_rhs` converts
**fully** — every real statement (`const __k = ...`, the sequence-cursor `if`/`++` guard, the
dispatch `switch`) is a real node, printed via `bynk_ts::print_stmt` per top-level statement at depth
0, matching its own caller's (`emit_stub_class`) pre-existing manual re-indentation exactly; each
switch case's own body stays one opaque `TsStmt::raw` line (the already-lowered value/fault text),
the same "print a real fragment, splice into a still-textual position" pattern. **Two real, grounded
`bynk_ts` algebra gaps found and closed**: `TsBinaryOp::LessThan` (`emit_stub_class`'s own
`ReturnsEach` sequence-cursor guard, `this.__seq_N < <bound>`, is the first real `<` comparison
anywhere in `bynk-emit`'s own content — added at the same precedence tier as the existing
`GreaterThan`); `TsSwitchCase.default_braced` (`emit_stub_rhs`'s own `ReturnsEach` dispatch braces
its `default` case, a genuinely different convention from `workers_entry.rs`'s own unbraced one that
established `Switch`'s original rendering — added as a per-case flag rather than changing the
existing unbraced-default rendering, to avoid touching `workers_entry.rs`'s own working, unrelated,
already-zero-diff content for no benefit). **A real cleanup, the same kind slice B/#1399 already
made for its own sibling**: `emitter::ts_type_ref_qualified_multi` (a `String`-returning function
`emit_stub_class` used to call twice) had no other production caller once converted — deleted rather
than left as dead code, with its own 1 direct unit test rerouted through a new `TsType`-returning
twin, `ts_type_ref_qualified_multi_ts_type` (mirroring `ts_type_ref_qualified`'s own twin, #1321),
plus `bynk_ts::print_type`. `verbatim_sites` unchanged (5). `ts_writes` drops by **4** (1097 →
1093), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any`
unaffected. Zero diff, first attempt, no iteration: `positive_fixtures`/`bless_positive_fixtures` and
`tsc_verify`'s full strict-`tsc` corpus pass unchanged; 138 `bynk-ts` unit tests (2 new, pinning
`LessThan` and `default_braced` directly) and all 300 `bynk-emit` unit tests (including the rerouted
`ts_type_ref_qualified_multi` test) pass. `tests_emit.rs`'s own slice D landed by #1403 immediately
after — `emit_test_scope_setup` converts **fully**, keeping its exact `&mut String`-accumulator
signature (its own callers are a mix of converted and unconverted); its own `obs_spec` object
(capability → sorted op-name array) moves from a `format!`-built literal relying on Rust's `Debug`
string-escaping happening to match JS to a real `TsExpr::Object`/`TsExpr::Array` of real string
literals, routing through `bynk_ts`'s own escaper properly for the first time.
`emit_test_case_function` converts its **tail** (the `return { pass: true }`, the catch clause's
`instanceof` check, and the two failure returns) to real nodes, printed via `bynk_ts::print_stmt` at
depth 2 and appended — its own source-map-sensitive splice arithmetic (`body_base = out.len()`,
`case_smb.merge(...)`, around the opaque `lower_test_case_body` output, a `lower.rs`-family function,
ADR 0391's permanent exclusion) stays **completely unchanged**, confirmed safe by the full
`bynkc/tests/source_map.rs` suite passing unchanged — the incremental `out.push_str` sequencing
doesn't care whether the surrounding text comes from `format!` or `print_stmt`, only the final bytes,
which are identical. The function/try header stays hand-written text (2 lines), not worth a fragile
partial-node representation given the offset-tracking constraint. **One real, grounded `bynk_ts`
algebra gap found and closed**: `TsBinaryOp::InstanceOf` — `e instanceof ExpectationError` (the catch
clause's own error-type check) is the first real `instanceof` anywhere in `bynk-emit`'s own content,
added at the same precedence tier as the existing `LessThan`/`GreaterThan` (real JS/TS precedence
puts all three relational operators at one level). **Review of #1404, finding 1: the "converts
fully" claim above was not actually true** — the cross-context surface alias site (`const {key} =
(deps as any).surface?.{key};`) was still built via `format!` at merge time, a real miss (not a
documented carve-out — `TsExpr::OptionalMember` already existed), fixed before merge; **finding 2:
the original single `InstanceOf` test pinned only the operator text, not the precedence tier the
same diff added to `binary_precedence`** — closed with two more direct tests (`Add` binding tighter,
and the more valuable `e instanceof A || e instanceof B` staying flat rather than silently
over-parenthesizing). `verbatim_sites` unchanged (5). `ts_writes` drops
by **6** (1093 → 1087), verified via a fresh `cargo xtask greenfield-status --apply`.
`ast_importers`/`ts_any` unaffected. Zero diff, first attempt, no iteration: `positive_fixtures`/
`bless_positive_fixtures`, the full `bynkc/tests/source_map.rs` suite (10 tests, all source-map-splice
regression coverage), and `tsc_verify`'s full strict-`tsc` corpus pass unchanged; 139 `bynk-ts` unit
tests (3 new) and all 300 `bynk-emit` unit tests pass. `tests_emit.rs`'s own slice E landed by #1405
immediately after — `emit_test_property_function`/`emit_test_history_property_function`/
`emit_contract_attack_function`'s own `__gens`/`__handlers` construction converts **fully**, closing
the print-then-splice seam slice A's own doc comments explicitly named as temporary: `binding_gen`'s
real `TsExpr` fields (`boundaries`/`gen_ts`/`shrink`) now build a real `TsExpr::multiline_array` of
real `TsExpr::object` entries directly, instead of being printed back to text via `bynk_ts::print_
expr` and re-embedded via `format!`. A new shared helper, `gen_descriptor_entry(name: Option<TsExpr>,
bg: &BindingGen) -> TsExpr`, factors the identical descriptor-object shape all three call sites build
(the history-handler shape has no `name` key at all). The `const __where = null;` fallback (all
three), `emit_test_history_property_function`'s own defensive early-return, and its own
expression-bodied `__drive` closure all convert fully too; each function's own final `return await
__bynkRunProperty(...)`/`return await __bynkRunHistory(...)` converts to a real `TsStmt::return_stmt`
wrapping a real `await`+`Call`+object-literal. The block-bodied `__where`/`__body` arrows (wrapping
opaque already-lowered text from `lower_block_to_async_body`/`lower_test_case_body`, both
`lower.rs`-family functions, ADR 0391's permanent exclusion) and the history function's own triple
type-alias line (three declarations crammed onto one physical line — a genuinely new shape with no
existing "N statements on one line" algebra representation) stay hand-written text, each for its own
named reason. No new `bynk_ts` algebra gap. `verbatim_sites` unchanged (5). `ts_writes` drops by
**7** (1087 → 1080), verified via a fresh `cargo xtask greenfield-status --apply`.
`ast_importers`/`ts_any` unaffected. Zero diff, first attempt, no iteration: `positive_fixtures`/
`bless_positive_fixtures`, `source_map`, `property_behaviour`, `history_behaviour`,
`contract_behaviour`, and `tsc_verify`'s full strict-`tsc` corpus all pass unchanged; all 300
`bynk-emit` unit tests pass. `tests_emit.rs`'s own slice F landed by #1407 immediately after —
`emit_system_http_support`'s own per-route driver loop converts **fully at the structural level** via
a new shared helper, `sysdrive_driver(kind_prefix, sname, key, params, body_stmt, url, options,
binding, decode_fn, payload)`, factoring the one real shape all four `__sysdrive_{,raw_,noauth_,
rawnoauth_}*` drivers share (async function decl, optional `const __body = JSON.stringify(...)` lead
statement, `const __h = makeHarness();`, `const __req = new Request(<url>, <options>);`, `const __res
= await __h.env.<binding>.fetch(__req);`, `return <decode>(__res, <payload>);`) instead of four
near-identical hand-written `format!` blocks. The wrong-method driver (`__sysdrive_wrongmethod_
{sname}`) converts **fully, no carve-out at all** — its own `new Request(\`https://test${path}\`, {
method })` is this file's first real dynamic (non-baked) `TsExpr::TemplateLit` substitution and its
first real `TsObjectEntry::Shorthand` object literal, both byte-matching the existing hand-formatted
text exactly (the shorthand object has exactly one entry, never conditionally empty, so it matches
`TsExpr::Object`'s own tight single-line rendering). **Decision A: the per-route options object
(`{ method, headers: {...}, body, }`) stays opaque hand-formatted text**, passed as a
`TsExpr::Ident`-wrapped fragment argument to a real `TsExpr::New` call — every branch's own
hand-written template bakes in an unconditional trailing `, ` before its closing brace, and prints
`{ }` (one space) rather than `TsExpr::Object`'s own tight `{}` when a sub-object has zero entries, a
shape `TsExpr::Object`'s general single-line algebra (comma-separated, no trailing comma, tight `{}`
for zero entries) cannot reproduce byte-for-byte — the same "odd, one-off shape stays text" call this
track has made before (Decision B, #1327; Decision C, #1359). The secrets-bootstrap loop's own
`format!` call converts fully too, to real `As`/`Member`/`Binary(NullishCoalescing)`/`Index`/`Assign`
nodes. **The static signer preamble (`__bynkNow`/`__b64url`/`__bytesB64url`/`__bynkSignHs256`) stays
out of scope, honestly recorded, not silently skipped**: it's built via a plain `push_str` literal
with zero per-target dynamism, already excluded from the `ts_writes` probe (no `write!`/`writeln!`/
`format!` on those lines) — converting it moves no metric and adds no real coverage. No new `bynk_ts`
algebra gap — every shape needed already existed. **A real, non-metric-driven simplification found
while implementing**: the first draft built each driver's own name via a separate `format!` call per
call site, which (surprisingly) *increased* `ts_writes` by 1 despite eliminating four hand-formatted
functions — every one of the 5 driver kinds' own name interpolation, previously merged into one big
`format!` call each, became its own newly-countable line. Consolidated into `sysdrive_driver`'s own
single internal `format!("__sysdrive_{kind_prefix}{sname}_{key}")` — one shared line instead of four
call-site ones — turning the accidental increase into a genuine, larger net decrease; recorded here
as an honest account of a probe artifact caught and fixed before merge, the same discipline #1325's
own "an increase despite eliminating ~90 lines" finding already established. **One real transcription
bug, caught by the fixture corpus, fixed before the CI pass**: the wrong-method driver's own decode
call was first built as `responseToHttpResult(__res, ...)` — the existing hand-written text actually
calls `responseToHttpOutcome` there (the router's own `405` fall-through decodes to an `HttpOutcome`,
not an `HttpResult`, the same distinction the raw/no-auth drivers already draw) — `positive_fixtures`
failed immediately with a one-line diff naming the exact mismatch; fixed by correcting the decode-call
identifier, no other change needed. `verbatim_sites`
unchanged (5). `ts_writes` drops by **3** (1080 → 1077), verified via a fresh `cargo xtask
greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff, confirmed after the one
fix above: `positive_fixtures`/`bless_positive_fixtures`,
`source_map`, and `tsc_verify`'s full strict-`tsc` corpus all pass unchanged; all `bynk-ts`/`bynk-emit`
unit tests pass. `tests_emit.rs`'s own slice G landed by #1409 immediately after — Arc C's own
final slice, closing the entire track. Converts `emit_integration_module` and `emit_test_module`,
the two top-level module assemblers, which call nearly everything slices A-F already converted and
only made sense to convert once their own delegates already returned real nodes. **Every header
comment line, every `import` line, and each module's own `export async function run(only?: string)
{ ... }` runner convert fully, no carve-out at all.** The two functions' own per-case/per-property/
per-attack loop BODIES stay exactly as they are — each already delegates to an already-converted
(A-F) or intentionally-opaque (`lower.rs`-family, ADR 0391) builder returning pre-formatted text with
its own source-map splice arithmetic (`module_smb.merge(...)`), so splicing that text via
`out.push_str` is unchanged; only the bookend content AROUND those splices (headers, imports, the
run-loop) is new real structure. **One real, grounded `bynk_ts` algebra gap found and closed**:
`TsDecl::ImportDefault { alias, from }` — `emit_integration_module`'s own per-participant `import
worker_{ns} from "../workers/{dir}/index.js";` (the participant's own Worker entry module's default
export) is the first real default import anywhere in `bynk-emit`'s own converted content; every
prior import site is either named (`TsDecl::Import`) or namespace (`TsDecl::ImportNamespace`). Added
alongside a direct printer unit test and folded into the existing "no blank line between adjacent
imports" `TsProgram`-level grouping rule (this file's own two functions never reach that path
themselves — they build a plain `String` via individual `print_stmt` calls, not a `TsProgram` — but
the rule is now complete for a future whole-program caller). **A new shared pair of helpers,
`run_dispatch_stmt`/`build_run_function`, factors the identical `run(only)` shape both functions
build** — one `if (want("name")) results.push({ name: "...", ...(await runner()) });` dispatch line
per case/property/attack (a real `TsObjectEntry::Spread` wrapping a real `TsExpr::Paren(await_expr(
...))`, matching the existing text's own explicit parens around the spread — `TsObjectEntry::Spread`
renders its inner expression through the depth-unaware `render_expr`, with no `render_operand`
parenthesisation, since `Await` isn't in `needs_parens_as_operand`'s own set), wrapped in one real
`TsDecl::Function` (`is_async: true`, `only?: string`, `const results = []`, the `want` arrow, every
dispatch line, `return results;`) — this function carries no opaque lowered content and no
source-map splice sensitivity at all (unlike the per-case bodies each dispatch line calls out to),
so it converts fully rather than needing `emit_test_case_function`'s own header/tail split. No other
new `bynk_ts` algebra gap — every other shape needed (`TsStmt::comment`, `TsDecl::Import`/
`ImportNamespace`, `TsDecl::Export`, `TsStmt::if_stmt` unbraced, `TsExpr::Arrow`) already existed.
`verbatim_sites` unchanged (5). `ts_writes` drops by **5** (1077 → 1072), verified via a fresh
`cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff, first
attempt, no iteration: `positive_fixtures`/`bless_positive_fixtures`, `source_map`/`source_map_
bodies`, and `tsc_verify`'s full strict-`tsc` corpus all pass unchanged; 141 `bynk-ts` unit tests (1
new, pinning `ImportDefault` directly) and all `bynk-emit` unit tests pass. **This closes Arc C
entirely — all 37 slices landed, `emit_project`'s own call graph and every function it calls is now
real-node-based**, `format!`-built or opaque-and-documented by name, with no undecided remainder
anywhere in the decomposition this track's own `emit.rs`/`tests_emit.rs` split named. Total
remaining from here: `emit.rs` (0) + `tests_emit.rs` (0) —
#1331/#1332, slice 8 (#1333),
slice 9 (#1335), slice 10 (#1337), slice 11 (#1339), slice 12 (#1351), slice 13 (#1353), slice 14
(#1355), slice 15 (#1357), slice 16 (#1359), slice 17 (#1361), slice 18 (#1364), slice 19 (#1367),
slice 20 (#1369), slice 21 (#1371), slice 22 (#1373), slice 23 (#1375), slice 24 (#1377), slice
25 (#1380), slice 26 (#1382), slice 27 (#1384), slice 28 (#1388), slice 29 (#1390), slice 30
(#1392), slice 31 (#1395), slice 32 (#1399), slice 33 (#1401), slice 34 (#1403), slice 35
(#1405), slice 36 (#1407), and slice 37 (#1409) are
all already landed, no longer "remaining" — a fixed **0 slices from here**, down from "1", a
flat relabeling (landing inside the already-set 7, no total change) that finally empties the sum.
Arc C's own real total (the **37** slices already landed — slice 1, the schedule-correction, slices
3/4/5/6/7/8/9/10/11/12/13/14/15/16/17/18/19/20/21/22/23/24/25/26/27/28/29/30/31/32/33/34/35/36/37):
a fixed **37**, unchanged from the post-#1394 widening, now with the sum itself at **0 remaining** —
Arc C is closed. The honest reason the total never moved again after that one widening: unlike every
prior "landing within an already-fixed value" correction in this section, `tests_emit.rs`'s own
citation was never itself grounded before then (its "2" was inherited unread from this track's own
pre-Arc-C opening, the same kind of stale citation P7.9's own review and #1333's own step-1 re-read
already found and corrected elsewhere) — grounding it surfaced 5 previously-uncounted slices, not a
range resolving to a floor or a slice landing inside an already-set bound. Five
distinct kinds of correction occurred
across the twenty-five updates from #1332 through this one — a narrowing (#1361, a range resolved to
its own floor; #1386's own history-driver exclusion decision, a second; #1388's own landing of the ICU
cluster as one slice, a third; and #1390's own landing of the cross-context lowering cluster,
a fourth), a split-driven widening (#1364, a silently-absorbed remainder surfaced at the STEP level), a
grounding-driven widening (the pass before #1367, an unread range turning out larger than guessed
on both ends; the pass before #1380, the same kind recurring a second time at a deeper sub-level;
and `tests_emit.rs`'s own grounding pass, post-#1392, the same kind a third time, at the whole-
file level — the citation itself, "2," widened to "7"), a
flat relabeling (#1367, then #1369, then #1373, then #1375, then #1380 again, then #1382 again,
then #1384 again, then #1392 again, then #1395 again, then #1399 again, then #1401 again,
then #1403 again, then #1405 again, then #1407 again, then #1409 again — the last of these finally
landing inside an
already-set range with no total change, the sum itself reaching 0), and a split-driven widening one
level deeper (#1371's own
sub-slice-3 split, and #1377's own sub-slice-6 split, a genuinely
uncounted floor surfacing both times, in two different sub-slices)
— each was a different failure mode or non-failure of estimation, not the same mistake repeating,
and each got the same "state the real reason plainly" treatment rather than a flat "corrected
again." `emit_agent`'s own
sub-decomposition (step 9), the ICU-formatting cluster (step 11), the cross-context lowering
cluster (step 8), and `emit_project` itself (step 10) were **all fully resolved and closed** —
**every step in `emit.rs`'s own decomposition order landed or was decided.** All 7 of
`tests_emit.rs`'s own proposed slices (A-G) have now landed (#1395, #1399, #1401, #1403, #1405,
#1407, #1409). **Arc C is closed: all 37 slices landed, nothing remaining anywhere in either
decomposition.**

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
| **Arc C, slice 17 — `emit_service`** (#1361, landed) | Closes step (7) of the design pass's (#1331) own decomposition order, fully, in one slice rather than the "1-2" the estimate reserved: each handler converts to a real `bynk_ts::TsObjectEntry::Method` fragment, printed through the same `bynk_ts::print_object_entry` entry point slice 14's own `messagesByLocale` per-locale entries already use (#1337) — chosen over slice 16's `TsClassMethod`/`print_class_method` because a service's own top-level shape is a plain `export const {name} = { ... }` object literal, not a class. Every param, including the two synthetic ones (the WebSocket `connection` param, the `__bynkSchemaEnv` param), converts to a real `bynk_ts::TsParam` via `ts_ty_to_ts_type` — no param stays a hand-built string. **Three deliberate boundaries, not remaining gaps: Decision A**, the enclosing `export const {name} = { ... }` object wrapper stays hand-written text, the same "fragment into a still-hand-written literal" shape #1337/#1359 already established; **Decision B**, `deps`'s own dynamically-built type string (`deps_ty` — capability refs, actor-seam widening, `__exec`, `__eventsDispatch`, all via the pre-existing `append_deps_field`) stays entirely unconverted, wrapped only at its outer edge as one opaque `bynk_ts::TsType::named(deps_ty)`, since none of its own internal construction is this slice's concern; **Decision C**, a handler's whole body — including the conditional events-dispatch IIFE prologue/epilogue (`const __events: Array<...> = []; const __result = await (async () => { ... })(); if (__events.length > 0) { await deps.__eventsDispatch(__events); } return __result;`) wired around it when `block_uses_emit` is true — stays one single opaque `TsStmt::Raw` blob, deliberately not split into a prologue/body/epilogue trio of real statements, since the prologue/epilogue text is itself still hand-`writeln!`-built and splitting it further belongs to a future, narrower slice, not this one. **Real bug found and fixed, caught by the zero-diff fixture check itself, not by review**: the old standalone `emit_doc_block(out, handler.documentation.as_deref(), INDENT_STEP)` call duplicated the handler's own JSDoc block once `TsObjectEntry::Method.doc` also started carrying it (`102_full_orders_payment_integration`'s own fixture diff caught it directly) — fixed by deleting the old direct call, letting the one real `doc` field own it. **Source-map correctness needed a genuinely new, two-level offset computation**, not slice 16's single-level one: here the source-mapped sub-content (`body_out`, the handler's own lowered body) is captured into a further, larger opaque blob (`raw_body`, the whole prologue+body+epilogue text) that is itself only later spliced into `print_object_entry`'s own printed fragment — so the real merge `base` is `out.len() - printed.len() + <raw_body's own offset within printed> + <body_out's own offset within raw_body>`, the second term tracked by construction (`raw_body.len()` recorded right before `body_out` is pushed in), never searched for. Both offsets are checked at runtime via an `if`-with-let-chain, not a `debug_assert!` (the same "skip a wrong merge rather than risk performing one" precedent slice 16's own review, finding 3, established) — reused proactively here, not review-prompted this time. `emit_service` keeps its exact existing signature — it never owned a `Verbatim` construction site. `verbatim_sites` unchanged (5). `ts_writes` drops by **4** (1240 → 1236), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected (5/30). Zero diff: every fixture containing a real `service` declaration and `tsc_verify`'s full strict-`tsc` corpus pass unchanged. The two-level offset arithmetic is pinned directly by a new project-form test (`bynkc/tests/source_map.rs`'s `service_handler_body_keeps_its_own_statement_lines_inside_the_events_iife`, against a fixture whose handler both emits an event and computes a local `let`), proven to catch the bug the same way every prior slice's own regression test was — temporarily hardcoding the inner offset term to 0, confirming the expected/actual line assertion failed (9 vs. 10), then reverting. **Named, deliberately out of scope (Decision D)**: a pre-existing source-map bug in the ALREADY-EXISTING prologue-insertion-then-merge ordering (`body_out.insert_str(0, ...)` happening after `body_smb`'s own checkpoints were already recorded, shifting every statement's own mapping one generated line too early per inserted subscriber-filter/schema-gate prologue) was noticed while reading this function but not investigated or fixed here, so a future slice does not rediscover it and misattribute it to this conversion's own work — review of #1362 confirmed the mechanism directly (not merely plausible), so it is tracked as #1363 rather than left as only a track-doc footnote. **Review of #1362 also found and fixed two smaller issues in this same PR**: the "degrade rather than mis-record" offset guard indexed `printed[..]` by a byte range instead of using `.get(..)`, which could panic instead of gracefully skipping the merge if the tail-layout assumption ever broke on non-ASCII content — fixed here in both this slice's own copy and slice 16's pre-existing `emit_provider` copy (#1360) in the same pass; and the new regression test only covered the `body_emits_directly == true` branch — extended with a second `service` (`tripler`, a plain non-emitting handler) and a third assertion, proven via the same bug-injection discipline (temporarily miscomputing `body_out_offset_in_raw` for only the non-emitting branch, confirming the new assertion specifically failed, then reverting). | R7.1 | #1331, #1337, #1315, #1352, #1353, #1359, #1363 |
| **Arc C, slice 18 — `emit_make_surface`** (#1364, landed) | Splits step (8) of the design pass's (#1331) own decomposition order, the same "outer wrapper first, harder remainder later" shape steps (4)/(6) each already took: `emit_make_surface` converts **fully**, no opaque carve-out at all — the first slice in this whole track to close that cleanly. `export function makeSurface(deps: {Name}Deps[, __caller: string]) { return { ...one method per service... }; }` is a real `bynk_ts::TsDecl::Function` (wrapped in `TsDecl::Export`, printed through `bynk_ts::print_stmt` at depth 0, the same `TsStmt::decl(...)` wrapping precedent #1352 established), whose single body statement is a real `TsStmt::Return` of a real `bynk_ts::TsExpr::multiline_object_entries` — the same "one entry per line" shape `workers.rs`'s own `compose`-returned surface object already uses (#1321), confirming the precedent transfers cleanly to a second real call site rather than needing its own new variant. Each service's own method entry is a real `bynk_ts::TsObjectEntry::Method` (`params`/`return_type` routed through `ts_type_ref_to_ts_type`, the same real-node sibling `emit_free_fn`/`emit_service` already use, replacing the plain-`String` `ts_type_ref` this function called before); a caller-binding handler's `{ ...deps, identity: __caller }` argument is a real `TsExpr::object_entries` mixing a `TsObjectEntry::Spread` (already existed, #1321, never previously exercised by any function in `emit.rs` itself) with a `TsObjectEntry::Prop`. **No source-map work was needed or done**: `emit_make_surface` takes no `source_map` parameter and never lowers an expression through `LowerCtx` — every value it builds is a literal, a param/service name, or a type reference, so there is no sub-builder/merge arithmetic for this slice to get right or wrong, unlike nearly every prior Arc C slice. `emit_context_deps_interface` (the separate `export interface {Name}Deps {...}` builder this function calls first) stays exactly as it is, confirmed unaffected, `String`-returning — not touched here. **The cross-context lowering cluster (`lower_workers_cross_context_call` and its own serialisation/codec-dispatch siblings), step (8)'s other named half, is a real, separate, harder remainder — deferred, not silently folded into "step (8) done"** — genuine cross-module codec lowering (serialise/deserialise helper dispatch against a consumed context's own service signature), a different kind of complexity from top-level structure construction, not yet grounded in detail (see §6's own "Revised estimate" paragraph for the honest ceiling-widening this split causes, the same direction steps (4)/(6)'s own splits already took). `emit_make_surface` keeps its exact existing signature — it never owned a `Verbatim` construction site. `verbatim_sites` unchanged (5). `ts_writes` drops by **10** (1236 → 1226), verified via a fresh `cargo xtask greenfield-status --apply` — the largest single-slice drop since #1355, matching this function's own unusually complete conversion (every `writeln!` site in it is gone, not just some). `ast_importers`/`ts_any` unaffected. Zero diff: every fixture containing a `service` declaration with an `on call` handler (both the plain and the caller-binding shape) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged — no new project-form regression test was needed, since this slice carries no source-map risk to pin. | R7.1 | #1331, #1352, #1321, #1359, #1361 |
| **Arc C, slice 19 — `emit_agent`'s state interface** (#1367, landed) | The first of step (9)'s own proposed 5-6 sub-slices per the dedicated grounding pass (#1366): converts `emit_agent`'s own state-record interface block (`export interface {Name}State { ... }`, marked `// 1) State record type.` by the function's own existing comments) to a real `bynk_ts::TsDecl::Interface`, **fully — no opaque carve-out at all**, the second slice in this whole track to close that cleanly (after #1364). Every field across all 5 store kinds (`Cell`/`Map`/`Set`/`Cache`/`Log`) plus `@indexed(by: f)` posting-lists becomes a real `TsTypeMember::Prop`, `ty` routed through `ts_ty_to_ts_type` (the real-node sibling of the plain-`String` `ts_ty` this block called before, already used by `emit_service`/`emit_make_surface`) — no new algebra gap, confirming the grounding pass's own prediction exactly. Two compound member types needed care, not a new gap: `Cache`'s `Record<string, { v: T; exp: number }>` and `Log`'s `Array<{ t: number; v: T }>` are each a real `TsType::named_with_args` wrapping a real inline `TsType::Object`; the posting-list's own `Record<string, string[]>` deliberately uses `TsType::Array{element, readonly: false}` (postfix `[]` syntax), NOT `TsType::named_with_args("Array", ..)` (the generic `Array<T>` syntax `Cache`/`Log` themselves use) — the two are semantically identical but textually distinct, and the original text is postfix, so using the wrong variant would have been a real, easy-to-miss zero-diff break caught before it happened, not after. **No source-map work needed or done**: this block never lowers an expression through `LowerCtx` — every value is a field name, a resolved `TyId`, or a literal — the same posture #1364 had. `emit_agent` keeps its exact existing signature; only this one block converts, the surrounding data-prep section and everything after (zero-factory, rehydrate, class scaffold, handlers, WS cluster) stay exactly as they are, each its own later sub-slice per the grounding pass's own proposed order. `verbatim_sites` unchanged (5). `ts_writes` drops by **8** (1226 → 1218), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff: every fixture with a `store` field (31 fixtures across all 5 kinds) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged. | R7.1 | #1331, #1339, #1315, #1364, #1366 |
| **Arc C, slice 20 — `emit_agent`'s zero-factory + rehydrate function** (#1369, landed) | The second of step (9)'s own proposed 5-6 sub-slices per the dedicated grounding pass (#1366), following slice 19's own state interface (#1367). Converts the per-agent state registry `const`, the zero-value factory function, and (when `agent_needs_rehydrate`) the rehydration-validation function. **One genuinely new algebra gap, not anticipated by the grounding pass**: `TsDecl::Function` had no way to render as a single physical line — the zero-factory's own real shape (`function __zeroOf{Name}State(): {Name}State { return {...}; }`, braces and body sharing the header's own line) — `render_block_stmts`, its only body-rendering path, always produces `{\n  ...\n}\n`. Closed with a new `inline: bool` field, mirroring `TsObjectEntry::Method.inline`'s own identical single-line-vs-multi-line precedent (#1337) at a different node kind — reusing `render_inline_block`'s own already-existing compact `{ stmt; stmt; }` renderer directly at the declaration's own header line, not a new rendering mechanism. 9 existing `TsDecl::Function` construction sites across `bynk-emit`/`bynk-ts` (both real code and test helpers) needed a mechanical `inline: false` added; none change behavior — the compiler's own missing-field errors found every one of them, not a manual audit. Pinned by a new direct `bynk-ts` unit test, `prints_an_inline_top_level_function`, since nothing existing covered this shape. **Real, deliberate opacity, named explicitly, not silently dropped**: a `Cell` field's own initialiser-derived zero value lowers through `LowerCtx` — `lower.rs`'s own permanently-excluded general expression lowering — carried as `TsExpr::Ident(val)`, the same "carry an unconverted sibling's already-formed JS text as an opaque identifier" pattern this track already uses (#1355's own `emit_message_entry_renderer` call); every EMPTY-container field value (`Map`/`Set`/`Cache`/held-map/posting-list `{}`, `Log`'s `[]`), by contrast, is fully REAL — `TsExpr::object(vec![])`/`TsExpr::array(vec![])` — no opacity needed there at all. The rehydrate function's own per-check text (`serialisation::deserialise_expr`'s output, a confirmed unaffected `String`-returning sibling) stays opaque, but each check becomes its OWN independent `TsStmt::Raw` rather than one merged blob, since each is already a self-contained statement — correct because this declaration is always printed at depth 0, giving the body depth 1, matching every check's own hardcoded two-space indent exactly. **No source-map work needed or done**: neither block ever calls `record_span` on the real module `source_map` — the zero-record's own `LowerCtx` is a fresh, local, `BodyMode::StaticInit` context whose lowering only ever returns text, and the rehydrate checks are pre-formatted text from an unconverted sibling helper. `emit_agent` keeps its exact existing signature; only these three constructs convert — the state interface (already landed), the surrounding data-prep section, and everything after (class scaffold, handlers, WS cluster) stay exactly as they are, each its own later sub-slice. `verbatim_sites` unchanged (5). `ts_writes` drops by **11** (1218 → 1207), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff: every fixture with a `store` field (31 fixtures across all 5 kinds) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged. | R7.1 | #1331, #1337, #1352, #1355, #1366, #1367 |
| **Arc C, slice 21 — `emit_agent`'s `loadState`** (#1371, landed) | Splits step (9)'s own third sub-slice ("class scaffold + `loadState`/`commitState`," per the grounding pass, #1366) rather than closing it whole, the same "outer wrapper first, harder remainder later" shape steps (4)/(6)/(8) each already took, applied one level deeper: `loadState` converts fully to a real `bynk_ts::TsClassMethod` fragment, printed through `print_class_method` (#1359's own fragment entry point); the class's own wrapper (header/fields/constructor) stays hand-written text, Decision C, the same boundary #1359's own `emit_provider` already used; `commitState` stays deferred as its own separate, later sub-slice, found genuinely harder only once actually read: its own `@invariant`/`transition` predicate lowering writes directly into `out` today, the same "direct write, real position" shape `emit_free_fn`/`emit_contract_guarded_body` had *before* their own conversions needed a sub-builder for `Raw`-embedding (#1352/#1353), not silently folded into "sub-slice (3) done." **One real, grounding-pass-predicted gap closed**: `TsClassMethod.private: bool` — `loadState`/`commitState` are the first `private` method sites this whole track has hit (`emit_provider`'s own ops, #1359, were all public), rendered before `async` matching the one real site's own modifier order; 4 existing `TsClassMethod` construction sites across `bynk-emit`/`bynk-ts` (real code and test helpers) needed a mechanical `private: false` added, found via the compiler's own missing-field errors. **One real gap deliberately NOT closed, named explicitly**: `this.state.storage.get<{state_ty}>("state")`'s own generic method call has no representation in `TsExpr::Call` — adding `type_args` would touch 41 real construction sites across the workspace for one narrow need, so it stays one opaque `TsExpr::Ident` callee text instead, the same "an odd, one-off shape stays opaque text" precedent P7.9's own `Query[T]` and #1357's own `unique symbol` already established. **No source-map work needed or done**: `loadState`'s own body never lowers an expression through `LowerCtx` — every statement is a literal, a field name, or a call to `zero_fn`/`rehydrate_fn` by name. `emit_agent` keeps its exact existing signature; only `loadState` converts here. `verbatim_sites` unchanged (5). `ts_writes` drops by **6** (1207 → 1201), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff: every fixture with an `agent` declaration (all agent fixtures reach `loadState`) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged. | R7.1 | #1331, #1359, #1352, #1353, #1315, #1357, #1366 |
| **Arc C, slice 22 — `emit_agent`'s `commitState`** (#1373, landed) | Closes step (9)'s own third sub-slice fully, landing the deferred remainder #1371 split off: `commitState` converts to a real `bynk_ts::TsClassMethod` fragment, the same `print_class_method` entry point `loadState` (#1371) already used. **A real correction to #1371's own stated expectation, found by reading the code rather than assumed**: #1371 predicted `commitState` would need the same source-map sub-builder/merge care `emit_free_fn`/`emit_contract_guarded_body` established (#1352/#1353) — this slice found that prediction genuinely wrong, not merely cautious: `record_span` (`emitter.rs:3776`) is a documented no-op when a `LowerCtx` has no attached source-map builder, and neither the invariant nor the transition `LowerCtx` `commitState` constructs is ever given one — both are built via plain `LowerCtx::new(...)`, `emit_agent`'s own `source_map` parameter isn't threaded into either. No sub-builder/merge work exists in this diff at all, confirmed directly, not assumed from the general "predicate lowering needs care" shape. Each invariant's/transition's own hoisted pre-statements and predicate expression stay opaque (`lower.rs`'s own permanently-excluded general expression lowering) — the hoisted lines as their own `TsStmt::Raw` with indent baked in manually (`Raw` prints verbatim with none of its own — "    " for an invariant's own top-level check at method-body depth 2, "      " for a transition's own check nested one level deeper inside the `if (__prior !== undefined)` block), the predicate as one opaque `TsExpr::Ident(format!("!({pred})"))` condition. Everything else is real, built via one shared local closure (`build_violation_check`, reused for both invariants and transitions): the `if (!(pred)) { console.error(...); throw invariantViolation(...); }` wrapper, the transition prologue (`const __prior = await this.state.storage.get<T>("state")` — the same opaque-callee-text pattern #1371 already established for the identical generic-call shape; `if (__prior !== undefined) { const __old = { ...zero(), ...__prior }; const __new = s; ... }`), and the final `await this.state.storage.put("state", s)`. No new `bynk_ts` algebra gap — every shape needed (`TsStmt::If`/`Block`/`Throw`/`ExprStmt`, `TsExpr::Call`/`Binary`/`object`) already existed from prior slices. `emit_agent` keeps its exact existing signature; only `commitState` converts here — the class scaffold, `loadState`, and everything after (handlers, WS cluster) stay exactly as they are. `verbatim_sites` unchanged (5). `ts_writes` drops by **14** (1201 → 1187), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff: every fixture with an `agent` declaration carrying `@invariant`/`transition` (e.g. `222_agent_invariant`) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged. | R7.1 | #1331, #1359, #1371, #1352, #1353 |
| **Arc C, slice 23 — `emit_agent`'s per-handler methods** (#1375, landed) | Closes step (9)'s own fourth sub-slice — the sub-slice the grounding pass (#1366) itself named "the largest slice, closest in shape/risk to #1361 (`emit_service`) itself... likely the single hardest slice in this whole step," confirmed accurate. Each handler converts to a real `bynk_ts::TsClassMethod` fragment, printed through `print_class_method` (#1359) — a class method here, not an object-literal entry (`emit_service`'s own shape), since agent handlers are methods on the Durable Object class. Params/return-type build through `ts_type_ref_to_ts_type`; the old standalone `emit_doc_block` call is removed in favour of the method's own real `doc` field, proactively avoiding the exact doc-duplication bug #1361's own review caught rather than rediscovering it. **Closes `TsClassMethod.doc: Option<String>`, the grounding pass's own second predicted gap (#1366)** — the third and last real method-shaped node in this crate to gain `doc`, after `TsObjectEntry::Method` (#1337) and `TsTypeMember::Method` (#1357); 6 existing `TsClassMethod` construction sites needed a mechanical `doc: None` added, found via the compiler's own missing-field errors. **The whole handler body stays one opaque `TsStmt::Raw`, `emit_service`'s own two-level-offset pattern (#1361) applying directly, but with a genuinely new THIRD wrapper dimension `emit_service` never needed**: `writes_state` (the implicit-commit closure, `const __state = { ... }; ... await this.commitState(__state);`), which can combine with `body_emits_directly` (the events-IIFE) at the same time. Both wrappers, and their combination, are preserved exactly as the pre-conversion code built them — **including the currently dead-by-construction `is_store_agent == false` branches**: `is_store_agent` is a hardcoded `true` local constant, never reassigned anywhere in `emit_agent`, so those branches never run today — the conversion preserves them faithfully rather than opportunistically removing provably-dead code, out of this slice's own scope. `deps_ty`'s own dynamic construction stays entirely opaque, wrapped only at its outer edge as `TsType::named(deps_ty)` — the same Decision B `emit_service` already established. `emit_agent` keeps its exact existing signature; only the handler loop converts here — the class scaffold, `loadState`/`commitState`, and everything after (WS cluster) stay exactly as they are. `verbatim_sites` unchanged (5). `ts_writes` drops by **3** (1187 → 1184) — smaller than the structural scope of this conversion might suggest, since the prologue/epilogue's own `writeln!`-based construction into the new local `raw_body` buffer is unchanged, still counted by the probe (the same shape #1361's own modest delta had). `ast_importers`/`ts_any` unaffected. Zero diff: every fixture with an `agent` declaration and at least one handler and `tsc_verify`'s full strict-`tsc` corpus pass unchanged — first-attempt zero diff, no iteration needed. Per-handler source-map correctness verified directly with a new project-form test (`bynkc/tests/source_map.rs`'s `agent_handler_body_keeps_its_own_statement_lines_inside_the_commit_and_events_wrappers`), exercising the deepest real nesting this conversion reaches — `writes_state` AND `body_emits_directly` at once, not just one wrapper in isolation the way `emit_service`'s own test only needed to — proven via bug injection (temporarily zeroing `body_out_offset_in_raw` in that exact branch, confirming the expected/actual line assertions failed with the predicted numbers, then reverting). | R7.1 | #1331, #1361, #1359, #1337, #1357, #1366, #1373 |
| **Arc C, slice 24 — `emit_agent`'s factory function** (#1377, landed) | Splits step (9)'s own sixth sub-slice ("the factory function plus the history-driver," per the grounding pass, #1366) rather than treating it as one optional unit — the same "split, don't force one slice to cover more than it needs to" discipline this track has used repeatedly (steps (4)/(6)/(8), and step (9)'s own sub-slice (3)): the factory function (`export function {factory}(key: {KeyTy}, env?: { {bind}?: DurableObjectNamespace }): {Agent} { return makeAgent({registry}, env?.{bind}, key, (state) => new {Agent}(state)); }`) converts **fully — no opaque carve-out at all**, the third slice in this whole track to close that cleanly (after #1364/#1369). Every shape needed — `TsDecl::Function`, `TsExpr::OptionalMember` (the `env?.{bind}` access), `TsExpr::Arrow` (the `(state) => new {Agent}(state)` factory closure), `TsExpr::New` — already existed from prior slices; no new `bynk_ts` algebra gap. **No source-map work needed or done**: this function never lowers an expression through `LowerCtx` — every value is a field/param name, a type reference, or a literal, the same posture #1364/#1371 had. The history-driver, sub-slice (6)'s other half, stays deferred, its own priority still genuinely undecided (test-support-only, stripped from deploy builds, only 2 fixtures exercise it — a deliberate exclusion remains a legitimate outcome, not assumed here). **A real correction to the original estimate, honestly recorded**: sub-slice (6) had been guessed as "0-1" (possibly zero real slices, if the factory folded elsewhere) — this slice confirms the factory alone needed its own real slice regardless, a genuine floor correction (0 → 1) for that item, the same "a split reveals a previously-uncounted mandatory floor" shape #1371's own sub-slice-3 split already established, applied to a different sub-slice — see §6's own "Revised estimate" paragraph above for the resulting total widening. **Also names, while scoping this slice, that sub-slice (5) (the WS-hosted DO methods/`fetch` dispatcher/`emit_ws_dispatch_handlers` cluster) is genuinely harder than "single-level merges throughout" suggested** — a large, workers-mode-specific dispatch method plus three external helper functions with their own significant bodies, needing its own dedicated grounding pass before a first slice is proposed against it, not touched by this slice. `emit_agent` keeps its exact existing signature; only the factory function converts here. `verbatim_sites` unchanged (5). `ts_writes` drops by **3** (1184 → 1181), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff: every fixture with an `agent` declaration (all agent fixtures reach the factory function) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged — first-attempt zero diff, no iteration needed. | R7.1 | #1331, #1364, #1369, #1371, #1366 |
| **Arc C, slice 25 — `emit_ws_do_method`** (#1380, landed) | The first of step (9) sub-slice (5)'s own 3 independent slices per the dedicated grounding pass (post-#1378) — the WS-hosted `on open`/`on message`/`on close` DO methods, up to 3 per host, called from `emit_agent`'s own host loop. Converts to a real `bynk_ts::TsClassMethod` fragment, printed through `print_class_method` — the identical shape `emit_provider`'s own already-landed ops (#1359) established: params/return-type real, the whole body one opaque `TsStmt::Raw`, a single-level offset (no nesting — this method wraps no events-IIFE, no implicit-commit closure, unlike `emit_service`/`emit_agent`'s own handler loop). Confirmed, by implementation, as the least risky of the three exactly as the grounding pass predicted: zero-diff on the first attempt, no new `bynk_ts` algebra gap. `connection: Connection<T>`'s own generic type stays one opaque `TsType::named` string, the same "an odd, one-off shape stays opaque text" precedent `this.state.storage.get<T>` already set twice (#1371/#1373) — `Connection`'s own type argument is this function's own real, single grounded need, not a general case for a new `TsType` type-argument mechanism. `doc: None`, confirmed rather than assumed: this function never emitted a doc comment (no `doc` parameter, no local, no `emit_doc_block` call), so the conversion correctly did NOT mirror the sibling handler-method conversion's own `doc: h.documentation.clone()` idiom — the exact trap review of #1379 flagged in advance, avoided here by design, not luck. `emit_agent` and the class scaffold stay exactly as they are; only `emit_ws_do_method` converts here. `verbatim_sites` unchanged (5). `ts_writes` drops by **3** (1181 → 1178), verified via a fresh `cargo xtask greenfield-status --apply` — review of #1381 later found `emit_provider`'s/`emit_agent`'s own sibling merge sites shared one copy-pasted print-then-guarded-merge block; extracting it into one shared helper (`emit_class_method_and_merge_source_map`) dropped `ts_writes` a further 2 (1178 → 1176), a dedup correction not attributable to this slice's own conversion. `ast_importers`/`ts_any` unaffected. Zero diff: `238_websocket_inbound_workers` (the only fixture reaching the full open/message/close cluster) and `237_websocket_chatroom_workers` (open-only) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged. | R7.1 | #1331, #1359, #1371, #1373, #1378, #1379 |
| **Arc C, slice 26 — `emit_ws_dispatch_handlers`** (#1382, landed) | The second of step (9) sub-slice (5)'s own 3 independent slices per the dedicated grounding pass (post-#1378) — the hibernatable-WebSocket dispatch handlers Cloudflare invokes on an accepted socket (`webSocketMessage`/`webSocketClose`), called from `emit_agent`'s own host loop; `ws_attachment_deps_arg` rides along (its only call site is inside this function). Unlike every prior Arc C slice, this function lowers no `.bynk` body at all — no `LowerCtx`, no `body_smb`, no `source_map` parameter — so there is no merge arithmetic here, only real `TsStmt`/`TsExpr`/`TsType` construction: both methods' own class-method/statement SHAPE converts **fully**, no opaque carve-out at that level, the same "pure boilerplate converts fully" precedent `loadState` (#1371) established, not the "one opaque `Raw` body, real header only" shape most prior slices needed — **review of #1383, finding 2: this is narrower than "converts fully" reads on its own** — several LEAF expressions still carry opaque text (`deps_arg`/each `call_args` entry, the decode call, the generic `WorkersConnection<T>` constructor callee, the structural `ws`/`att` parameter types), routed through `TsExpr::Ident`/`TsType::named` rather than `writeln!`, the same established escape hatch `emit.rs:388`/`:4627`/`:4761` and slice 25's own `Connection<T>` carve-out already use — not a new carve-out, but real opaque text nonetheless, worth naming precisely rather than implying zero. `Const`/`Let`/`If`+`InlineBlock`/`As`/`New`/`Await`+`Call`, plus the real `TsType::Union` (already-landed #1315 — a genuine fit for `string \| ArrayBuffer` and `{att_ty} \| null`, not an escape hatch) cover every real statement except two shapes: the two `try { ... } catch { ... }` lines (`TsStmtKind::TryCatch`'s own renderer always prints multi-line — no single-line form exists, and every other real `TryCatch` site in this tree IS multi-line, so extending it for one function's own single-line shape was rejected in favour of the established "odd, one-off shape stays opaque text" posture) and the `void code; void reason; void wasClean;` suppression line (no `TsUnaryOp::Void` exists — this function's own only real site) — both stay one `TsStmt::raw` line each. **A real, previously-unpredicted correction to the grounding pass's own account**: it predicted two opaque-escape-hatch shapes for this whole 3-slice cluster (a spread call argument, `new WorkersConnection<T>(...)`) — the spread argument belongs to the still-unlanded `fetch` method, not this slice, while this slice needed the two DIFFERENT shapes above the grounding pass never named; neither is a new `bynk_ts` variant. `emit_agent` and the class scaffold stay exactly as they are; only `emit_ws_dispatch_handlers` converts here. `verbatim_sites` unchanged (5). `ts_writes` drops by **14** (1176 → 1162), verified via a fresh `cargo xtask greenfield-status --apply` — the largest single-slice drop since #1364, matching this function's own unusually complete conversion (both methods entirely `writeln!`-free). `ast_importers`/`ts_any` unaffected. Zero diff, first attempt, no iteration needed: `238_websocket_inbound_workers` (the only fixture reaching both `webSocketMessage`/`webSocketClose`), `237_websocket_chatroom_workers`, and `tsc_verify`'s full strict-`tsc` corpus pass unchanged. **Review of #1383, finding 1**: those two fixtures alone left the no-binder (`deps_arg == "{}"`), single-handler (message-only/close-only), and route-param-count branches unpinned — none a regression (the pre-conversion code had the identical gap), but this slice is where the codegen got rebuilt from scratch. Added `1382_ws_dispatch_no_binder_zero_route` — a bare `by Participant` (no binder) host with `on message` and no `on close`, the message handler taking zero extra route params — pinning `deps_arg == "{}"`, the message-only shape, and the zero-route-param `call_args` path in one cheap fixture; the close-only shape and a multi-route-param count stay unpinned, a smaller residual gap, deliberately not chased further here. | R7.1 | #1331, #1315, #1364, #1371, #1378, #1380 |
| **Arc C, slice 27 — `emit_agent`'s `fetch` method + `emit_ws_open_fetch_branch`** (#1384, landed) | The third and final of step (9) sub-slice (5)'s own 3 independent slices per the dedicated grounding pass (post-#1378) — **closing sub-slice (5) entirely**. Same zero-`LowerCtx` posture #1382 established for this cluster: no `.bynk` body lowering, no `source_map` parameter, no merge arithmetic. `emit_ws_open_fetch_branch` converts from a `&mut String`-writing function into `ws_open_fetch_branch_stmt`, returning a real `TsStmt::If` node (a `Block` then-branch) composed directly into `fetch`'s own body once per WS-open host; `fetch` itself converts to a real `bynk_ts::TsClassMethod`, printed through `print_class_method`. **A real, previously-unpredicted algebra gap, beyond the grounding pass's own correctly-predicted one**: `...args` stays one opaque `TsExpr::Ident` call argument exactly as the grounding pass predicted (no `TsExpr` variant represents a call-argument spread — only `TsObjectEntry::Spread`, for an object-literal spread, already exists). Beyond that, real implementation found `(this as unknown as Record<string, (...bynkArgs: unknown[]) => unknown>)` and `this.__env as unknown as Record<string, unknown>` are both a nested `As`-under-`As` chain (`x as A as B`) — `TsExpr::As`'s own operand-parenthesization rule (`needs_parens_as_operand`) always wraps a nested `As`, correct for the real cases that motivated it but would turn this real, parenless chain into a byte-diff-wrong `(x as A) as B`; extending the printer to special-case "no parens for a nested `As` under an outer `As`" was rejected as broader than this one real site needs (unclear how many other real sites, if any, want the opposite) — both stay one opaque `TsExpr::Ident` string each, the same "odd, one-off shape stays opaque text" posture #1382 already used twice. `Const`/`Let`/`If`+`Block`/`As`/`New`/`Await`+`Call`/`Index`/`Arrow`, plus the real `TsObjectEntry::Spread` (for the rebuilt-deps object) cover every real statement beyond those two leaves — no `Raw` statement anywhere in either function, a fuller conversion than #1382's own two `TsStmt::raw` lines needed. **Fixture coverage**: `238`/`237` needed no new fixture (unchanged from #1382's own finding); `328_agent_given_workers` already covers the `given_deps_expr` arm; the `agent_uses_emit` arm (review of #1379's own finding) had zero fixture coverage anywhere — closed with a new fixture, `1384_agent_emits_workers` (a workers-target agent whose own handler calls `Events.emit` directly, no service and no `given` capability of its own, pinning `agent_uses_emit` in isolation from `given_deps_expr`) rather than a deliberate exclusion, since this branch is real, reachable production code, not test-support-only. `emit_agent`'s own class scaffold and constructor stay exactly as they are (the `needs_env_ctor`-gated constructor is a separate, still-unconverted remainder, not part of this cluster's own named 5 sites); only `fetch`/`emit_ws_open_fetch_branch` convert here. `verbatim_sites` unchanged (5). `ts_writes` drops by **20** (1162 → 1142), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff against the three existing fixtures (`238_websocket_inbound_workers`, `237_websocket_chatroom_workers`, `328_agent_given_workers`) and `tsc_verify`'s full strict-`tsc` corpus, both first attempt. **Review of #1385, finding 1: the new `agent_uses_emit` fixture was NOT actually zero-diff — it just looked that way**, because the fixture itself was blessed from the post-conversion compiler, so it pinned whatever this slice emitted rather than proving the old bytes were preserved. The real bug it hid: `__eventsDeps`'s own `events` parameter used `TsType::array(TsType::named(EVENTS_WIRE_EVENT_TS_TYPE))`, which prints through the POSTFIX `T[]` form — the pre-conversion text used the generic `Array<T>` spelling (matching the two OTHER real call sites building this identical `__eventsDispatch` arrow, `workers.rs:546`/`project.rs:3399`, both `TsType::named_with_args("Array", ...)`), so the same wire type was spelled two different ways in one generated file. `tsc` treats them identically, so nothing miscompiled and no other check caught it — the fixture this slice added to pin the untested branch pinned the WRONG bytes for that one line, exactly the failure mode a reviewer, not a test, was needed to catch. Fixed by switching to `TsType::named_with_args("Array", ...)`, re-blessed (the fixture's own `events` parameter line is the only byte that changed). A real, if narrow, correction — worth recording as a genuine finding, not folded silently into "zero diff, first attempt." | R7.1 | #1331, #1364, #1371, #1378, #1380, #1382, #1379 |
| **Arc C, slice 28 — the ICU-formatting cluster** (#1388, landed) | Closes step (11), re-scoped out of step (1) by #1355 ("too substantial to be a leaf helper... its own future dedicated slice") and never separately proposed until now — the last real, unconverted TS-emission code inside `emit_messages_bundle`'s own sub-tree. `emit_message_entry_renderer`/`emit_icu_placeholder`/`emit_sub_message` (`bynk-emit/src/emitter/emit.rs`) all change signature from `out: &mut String`/`-> String` to `-> bynk_ts::TsExpr`, the same precedent `emit_attached_methods` (#1337) established — unlike almost every prior Arc C slice, these three build one EXPRESSION each, never a statement; `emit_messages_bundle`'s own call site (#1355) drops its own opaque `TsExpr::Ident(renderer_text)` wrap and uses the real returned expression directly as the object entry's value. **Two real, previously-nonexistent algebra additions, landed in the same PR as a deliberate, coordinated pair, not discovered piecemeal:** (1) `TsBinaryOp::Add` (`+`, string concatenation) — the cluster's own dominant structural pattern (every literal/placeholder segment in a message template joins this way), needing three coordinated printer changes: `binary_precedence` (a new, higher-than-`GreaterThan` entry, matching real JS/TS precedence), the render-arm's `" + "` text, and — the easy-to-miss piece — `render_binary_operand`'s own same-operator associativity allowlist (`matches!(outer_op, Or \| And)` widened to include `Add`), since a left-folded `Binary{Add, Binary{Add, a, b}, c}` needs it to print flat (`"a" + "b" + "c"`) rather than the wrongly-parenthesised `("a" + "b") + "c"` the pre-existing conservative default would have produced; pinned by a new direct `bynk-ts` unit test, `a_three_term_add_chain_of_the_same_operator_prints_flat`, mirroring the existing `||`-chain test exactly. **Review of #1389, finding 1: the first version of this exemption was a real correctness bug, not caught by the zero-diff fixture run** — it dropped parens for `Add` on BOTH sides of a same-operator chain, matching `Or`/`And`'s own treatment, but `||`/`&&` are safe to flatten because they're SEMANTICALLY associative (`(a || b) || c` always agrees with `a || (b || c)`); `+` is only GRAMMATICALLY left-associative — `1 + (2 + "3")` (`"123"`) and `(1 + 2) + "3"` (`"33"`) disagree once a number joins the chain, so flattening a RIGHT-nested `Add` would silently change the value for any future caller (`bynk-emit`'s own `join_plus` only ever left-folds, so the fixture corpus never exercised the unsafe direction). Fixed by threading which side `render_binary_operand` is rendering into the function itself (`is_left: bool`) and exempting `Add` only when `is_left` — `Or`/`And` still exempt on both sides. Two more direct tests pin what the fixture corpus can't: `a_right_nested_add_chain_keeps_its_parens` (the bug's own exact shape) and a `GreaterThan`-nesting pair confirming `Add`'s new precedence entry works both directions. (2) `bynk_ts::print_expr(expr) -> String`, a new fragment entry point mirroring `print_type`'s own "one fragment, not a whole document" scope — needed for exactly one site (see below). **One deliberate opaque carve-out, named explicitly, exactly as the proposal predicted**: `emit_icu_placeholder`'s `Select` arm builds a BLOCK-bodied IIFE (`(__arg) => { if (...) {...} const __arms = {...}; return ...; }`) — the only one of the cluster's 4 real shapes (`Plural`/`Select`/`Number`/`Date`) that isn't a single expression; `TsExpr::Arrow` deliberately has no block-body variant (its own doc: "nothing else in that slice's own grounding needed one"), and extending it for this one real site was rejected as disproportionate, the same "odd, one-off shape stays opaque text" posture this track applies repeatedly. The `Select` arm's own whole IIFE stays one opaque `TsExpr::Ident`; its own arm VALUES (`emit_sub_message`'s now-real `TsExpr` results) get stringified via `print_expr` before splicing into that opaque host text — `print_expr`'s own one real use in this slice. A second, smaller real finding caught only by careful precedent-reading, not by the zero-diff check (which passed either way, since both forms are grammatically equivalent): the `Plural`/`Number` arms' own `&&`/`||` sub-expressions are explicitly parenthesised in the pre-conversion text even where the existing precedence table would omit the parens as grammatically unnecessary (`&&` already binds tighter than `||`) — reproduced via an explicit `TsExpr::Paren` wrap rather than relying on the automatic (and, here, insufficient) precedence machinery, matching the real bytes rather than the grammatically-equivalent-but-different-looking alternative. `TsLit::Str`'s own renderer applies the identical escaping `escape_ts_string` already did (documented byte-for-byte match) — every string literal that converts to a real `TsLit::Str` now carries its RAW, unescaped text, not `escape_ts_string`'s own pre-escaped output, avoiding a double-escape hazard; every string that stays inside the `Select` arm's own still-opaque text keeps calling `escape_ts_string` directly, since no printer-level escaping applies there. `verbatim_sites` unchanged (5) — none of the three functions owned a construction site. `ts_writes` drops by **13** (1142 → 1129), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff, first attempt, no iteration needed: `808_messages_basic` (literal + plain-placeholder), `811_messages_icu_plural_polish`, `812_messages_icu_select_gender`, `813_messages_icu_number_styles`, `814_messages_icu_date_styles`, `815_messages_icu_reordered_arms_non_regression`, every other message-bundle fixture, and `tsc_verify`'s full strict-`tsc` corpus pass unchanged — real, substantial existing coverage across all 4 ICU kinds plus both non-ICU shapes, no new fixture needed. | R7.1 | #1331, #1355, #1337, #1315 |
| **Arc C, slice 29 — the cross-context lowering cluster** (#1390, landed) | Closes step (8) entirely — its own other half, the outer wrapper `emit_make_surface`, landed by #1364; this closes the "real, separate, harder remainder... genuine cross-module codec lowering... not yet grounded in detail" that slice left deferred. **Re-grounding on read finds the real scope is 2 functions, not the 4 the original citation named** — the same "re-grounding on read finds it smaller than the citation implied" shape step (1) already showed (#1333: 13 named, 9 not TS-emission code at all). Of the cited cluster's 4 functions (`bynk-emit/src/emitter/emit.rs`, ~3473-3667): `cross_context_lowering_prefix` and `flatten_emit_ident_chain` are pure Rust logic (string/`HashMap` matching, AST-chain-flattening) that never builds TS text — not Arc C targets, named here explicitly rather than silently dropped, the same discipline #1333 used for its own 9. The real remaining scope is exactly `lower_workers_cross_context_call` and `param_cast`, ~140 lines together. **Not part of `lower.rs`'s own ADR 0391 exclusion, worth naming explicitly since it's the natural next question**: both real functions are called exclusively from `lower.rs`'s own general expression-lowering dispatch and return the same `String`-based `Lowered`/`String` shape that machinery expects — but ADR 0391 excludes `lower.rs` **itself** (a 90-function lowerer covering the entire Bynk expression grammar at once), not every function it happens to call into; each of these two is a single, bounded, well-scoped codegen unit (one call expression, one cast expression) living in `emit.rs`, exactly the "bounded, groundable, file-specific unit of work" shape ADR 0391 says converts. Both keep their exact existing signatures (`-> Lowered` / `-> String`, the P7.9 pattern) — build a real `bynk_ts::TsExpr` internally, print it via `bynk_ts::print_expr` (#1388), splice the printed text into `pre.finish(...)`/the returned `String` exactly as before. `lower_workers_cross_context_call`'s own `callService(...)` becomes a real `TsExpr::Call`; the multi-arg case's own hand-built object literal becomes a real `TsExpr::object_entries`, one `TsObjectEntry::Prop` per param (each still carrying its own opaque, pre-serialised value from the unconverted `emitter::serialisation` siblings `serialise_expr_via`/`deserialise_ref_via`). `param_cast`'s own double cast (`arg as unknown as Ns.Type`) is the SAME nested-`As`-under-`As` shape #1385 (slice 27) already found and named — stays one opaque `TsExpr::Ident`, wrapped in a real `TsExpr::Paren` for the real outer parens, not a third printer special-case for the same shape. **One real diff caught only by the zero-diff fixture run, not by reasoning about the algebra**: the zero-arg case fell into the same-shaped `else` branch as the multi-arg case, and `TsExpr::Object`'s own renderer always collapses an empty object to the tight `{}` — but the pre-conversion text's own empty case printed `{  }` (double space), the SAME quirk #1327 (slice 6) already found and carried for `{ns}Deps`'s own empty case. `172_integration_with_capability`'s own zero-arg `tick` call caught the mismatch on the first real test run; fixed by special-casing the zero-arg case to the same opaque `{  }` text before the multi-arg branch is ever reached. `verbatim_sites` unchanged (5) — neither function owned a construction site. `ts_writes` drops by **3** (1129 → 1126), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff (after the one fix above): every fixture with a cross-context `consumes` call at Workers target (`98_cross_context_call_with_alias` and siblings, plus `172_integration_with_capability`'s own zero-arg shape), `cross_context_caller.rs`'s own 3 dedicated tests, `integration_contracts_behaviour.rs`, and `tsc_verify`'s full strict-`tsc` corpus pass unchanged. | R7.1 | #1331, #1364, #1385, #1388 |
| **Arc C, slice 30 — `emit_project`'s own header/import/rebrand cluster** (#1392, landed) | Closes step (10), `emit_project` itself — carried as a **fixed 1 slice** throughout every revision of the estimate, unverified by direct read. **Re-grounding on read: real, but genuinely bigger than "1 slice, unread" implied at first — then genuinely smaller once actually read.** `emit_project` (`bynk-emit/src/emitter.rs:319-559`, ~240 lines) calls 7 direct helper functions that still built raw text (`write_header`/`write_header_single`/`write_commons_doc`/`emit_project_imports`/`emit_cross_context_namespace_imports`/`emit_context_rebrands`/`emit_boundary_helpers`) — combined, the raw line count (~1,100 lines) rivals `emit_agent`'s own sub-tree, the first signal this needed grounding. But most of that count is Rust-side decision logic that never changes (which import names to include, alias/rebrand resolution, boundary-type collection) — the real `writeln!`/`write!` sites needing conversion are a modest ~29 across all 7 functions, comparable to several already-landed slices, not a multi-slice undertaking; `emit_json_codec_helpers` (0 direct write calls, pure delegation to the unconverted `emitter::serialisation` module) needed no changes at all. **One real, previously-nonexistent algebra gap**: `emit_cross_context_namespace_imports`'s own `import type * as ns from "...";` form (#661, a Workers-mode consumed-context import reaching the callee's types only) had no representation — `TsDecl::ImportNamespace` had `alias`/`from` but no `type_only`, unlike its sibling `TsDecl::Import`, which already had the identical field; a parallel gap by omission, not deliberate design. Closed by adding `type_only: bool` to `ImportNamespace` (mechanical; 7 existing construction sites across `workers.rs`/`workers_entry.rs`/`project.rs`/`tests_emit.rs` plus 3 direct `bynk-ts` unit tests all got `type_only: false`, no behaviour change for any of them), pinned by a new direct test, `prints_a_type_only_namespace_import`. **`emit_context_rebrands`, the most complex of the seven, converts fully**: `export type X = ... & { readonly __ctxBrand: "..." }` is the SAME `TsDecl::TypeAlias` over `TsType::Intersection` shape #1339's own `emit_refined_type` already established for its sibling `__brand` alias; the `export const X = { of(...) {...}[, unsafe(...) {...}] }` object's own `X.method(value) as unknown as T` bodies build as REAL nested `TsExpr::As` — **empirically verified before use, not assumed**: a quick throwaway check confirmed the printer's own `As` arm only guards `Binary`/`Arrow`/`Conditional` for its OWN inner-expr position (not a nested `As`), so `x as A as B` prints flat with no extra parens, and when the whole chain later sits as an operand elsewhere (an `Index`'s object, etc.) `render_operand`'s separate `needs_parens_as_operand` check correctly wraps the WHOLE chain, matching real hand-written text exactly either way — the same shape `emit_forwarded_methods` (also converted in this slice, `out: &mut String` → `-> Vec<TsObjectEntry>`, the `emit_attached_methods` precedent, #1337) had already proven correct, unnoticed until now. Slices 27/29's own opaque-`Ident` carve-out for this identical `X as unknown as Y` shape was the safer, lower-risk choice at the time and is NOT revisited here (working, reviewed, merged code, no functional benefit to touching it) — but this slice's own sites use the more-real form now that it's been directly confirmed to work. One further real gap, named rather than force-fit: `emit_boundary_helpers`'s own bare `export { ... };` re-export (already-bound local names, no `from` clause) has no `TsDecl` representation — `ReExport` always carries one — stays one opaque `TsStmt::raw` line, the established "one-off shape stays opaque text" posture for a single call site. `emit_project`'s own inline `__resetAgents` function converts to a real `TsDecl::Function`; its own remaining body is orchestration (calling the above, now-real-node-returning helpers) plus the final source-map/runtime-import post-pass, unchanged — it operates on the whole module's own already-printed text, not original construction. `verbatim_sites` unchanged (5). `ts_writes` drops by **12** (1126 → 1114), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers` **unchanged at 5, confirming the design pass's own original prediction directly** ("will NOT move `ast_importers`") rather than assuming it silently. `ts_any` unaffected. Zero diff, first attempt on every conversion including `emit_context_rebrands`'s own real nested-`As` bodies: the full `positive_fixtures`/`bless_positive_fixtures` corpus (every fixture reaches `emit_project`) and `tsc_verify`'s full strict-`tsc` corpus pass unchanged — the broadest real-content coverage any Arc C slice has had, by construction. Closes step (10) — every step in `emit.rs`'s own decomposition order is now landed or decided; only the `tests_emit.rs` pair remains. | R7.1 | #1331, #1339, #1337, #1390 |
| **Arc C, slice 31 — the property-generator expression cluster (`tests_emit.rs` slice A)** (#1395, landed) | The first of `tests_emit.rs`'s own 7 proposed slices per its dedicated grounding pass (post-#1392) — Arc C's own final remaining piece. Converts the mutually-recursive property-generator expression cluster in `bynk-emit/src/project/tests_emit.rs`: `refined_gen_ts`/`gen_ts_for_ty`/`canon_ts_for_ty` (`-> String` become `-> bynk_ts::TsExpr`) and `binding_gen`/`BindingGen` (`boundaries: Vec<String>, gen_ts: String, shrink: String` become `Vec<TsExpr>, TsExpr, TsExpr`) — the same "real per-input TS-emission via `format!`/match, no `writeln!`/`push_str`" shape the already-converted ICU cluster (#1388) established for a `-> TsExpr` treatment, confirmed directly rather than assumed. **Converts fully at the node-structure level — no opaque carve-out for any recursive shape**: `Call`, `Member`, `Arrow` (expression-bodied, for sum-variant thunks and shrink closures), `Array`, `Object`, `Conditional`, `Paren` all already existed; no new `bynk_ts` algebra gap. The one leaf that stays opaque, by design, not oversight: `emitter::unchecked_construct_test(name, value: &str, is_opaque) -> String` is also called from `lower.rs`'s own permanently-excluded general expression lowering (ADR 0391) and keeps its exact `-> String` signature untouched — its own branching (a real `.unsafe(...)` call vs a real `(... as any)` cast) is not duplicated as a second node-building path; each call site prints the just-built draw/literal `TsExpr` to text first (`bynk_ts::print_expr`), then wraps the function's own already-formed output as an opaque `Ident`, the same "an unconverted shared sibling's own text stays opaque" posture P7.9's `ts_base` and #1369's `Cell` zero value already use. **The `"{  }"` double-space quirk recurs a fourth time** (after `workers.rs`/#1321, `project.rs`/#1327, `emit.rs`/#1390): `gen_ts_for_ty`/`canon_ts_for_ty`'s own Record-field object-literal construction used the identical `format!("{{ {} }}", fields.join(", "))` template, which produces a literal double space for a zero-field record — a zero-field record is exotic but not provably unreachable here, so both sites guard for it explicitly (`ident("{  }")`) rather than assuming it can't happen, not consolidated into a shared constant, matching the established "each site stays its own opaque text" precedent. **Three call sites in `emit_test_property_function`/`emit_test_history_property_function`/`emit_contract_attack_function`** (`tests_emit.rs` slice E, not yet converted, still `String`/`format!`-based) call `binding_gen` and interpolate its fields directly into their own template — each now prints the returned `TsExpr` fields back to text via `bynk_ts::print_expr` before interpolating, confirming the grounding pass's own predicted seam exactly, the established "print a real fragment, splice into a still-textual caller" pattern every prior slice's own not-yet-converted callers use. `verbatim_sites` unchanged (5). `ts_writes` drops by **12** (1114 → 1102), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff, first attempt, no iteration: `cargo test -p bynkc --test e2e positive_fixtures`/`bless_positive_fixtures`, `--test property_behaviour`, `--test history_behaviour`, `--test contract_behaviour`, and `--test tsc_verify`'s full strict-`tsc` corpus all pass unchanged. | R7.1 | #1331, #1388, #1394, #1369 |
| **Arc C, slice 32 — the small independent leaves (`tests_emit.rs` slice B)** (#1399, landed) | The second of `tests_emit.rs`'s own 7 proposed slices per its dedicated grounding pass (post-#1392), following slice A/#1395. Converts `emit_integration_harness`, `emit_test_deps`, `emit_ns_destructure`, and `observation_call_record_types` in `bynk-emit/src/project/tests_emit.rs`, all **converting fully — no opaque carve-out at the node-structure level**. `emit_integration_harness`/`emit_test_deps` keep their exact `-> String` signatures (`emit_test_module`, slice G, is still unconverted) and return `bynk_ts::print_stmt`'s own printed text of a real `TsStmt::decl(TsDecl::Function{..})`; a small shared helper, `service_binding_forward`, factors the identical `{ fetch: (req: Request) => worker_X.fetch(req, env_X) } as ServiceBinding` shape both the per-participant and root-env sites in `emit_integration_harness` build, and `undefined_as_unknown_as` factors the identical placeholder shape 3 sites in `emit_test_deps` build. `emit_ns_destructure` keeps its exact `&mut String`-accumulator signature (its own callers, slices C/D/G, are all still unconverted) and appends each real `TsStmt` (a `Const` over `TsBindingName::ObjectPattern`, already existing since P7.5, and a `TypeAlias` per type name) printed via `bynk_ts::print_stmt` at depth 2, matching its own callers' hardcoded 4-space indent. `observation_call_record_types` converts its own `type Name = { field: T; ... };` construction to a real `TsDecl::TypeAlias` over `TsType::Object(Vec<TsTypeMember::Prop>)`, using the already-existing `emitter::ts_type_ref_qualified_ts_type` (P7.9-era, added alongside its `String`-returning sibling in Arc C slice 3/#1321 for `workers.rs`'s own use, never previously called from `tests_emit.rs`). **The `"{  }"` double-space quirk recurs a fifth and sixth time** (after `workers.rs`/#1321, `project.rs`/#1327, `emit.rs`/#1390, slice 31/#1395's own two sites): `emit_test_deps`'s own zero-entries `return` (a non-`Context` target, or a `Context` with no capabilities/flattened caps/consumed surface) and `observation_call_record_types`'s own zero-param capability op, both guarded explicitly with the established `ident("{  }")`/`TsType::named("{  }")` idiom, not consolidated into a shared constant. **A real cleanup, found while converting, not new scope**: `emitter::ts_type_ref_qualified` — the `String`-returning function `observation_call_record_types` used to call — had no other production caller once this slice's own conversion landed (its own doc comment's "still needed by `emit.rs`/`lower.rs`" claim never materialised as real call sites in either file); deleted rather than left as dead code once `cargo build`'s own `dead_code` lint caught it, the same call slice 6 made for `ts_string_literal`, with its own 2 direct unit tests (`bynk-emit/src/emitter.rs`, `bynk-emit/src/project/tests_emit.rs`) rerouted through `bynk_ts::print_type(&ts_type_ref_qualified_ts_type(...))` instead, keeping the identical assertions and behaviour pinned. No new `bynk_ts` algebra gap — `TsBindingName::ObjectPattern`, `TsDecl::TypeAlias`, `TsType::Object`/`TsTypeMember::Prop`, `TsStmt::Assign`, and nested `TsExpr::As` all already existed. `verbatim_sites` unchanged (5). `ts_writes` drops by **5** (1102 → 1097), verified via a fresh `cargo xtask greenfield-status --apply` — smaller than the structural line count might suggest, matching the grounding pass's own "small" naming for this cluster: most of these four functions' own bulk is Rust-side collection/sorting logic, not `writeln!`/`format!` sites. `ast_importers`/`ts_any` unaffected. Zero diff, first attempt, no iteration: `cargo test -p bynkc --test e2e positive_fixtures`/`bless_positive_fixtures` and `--test tsc_verify`'s full strict-`tsc` corpus pass unchanged; all 300 `bynk-emit` unit tests (including the 2 rerouted `ts_type_ref_qualified` tests) pass. | R7.1 | #1331, #1394, #1395, #1321 |
| **Arc C, slice 33 — the stub cluster (`tests_emit.rs` slice C)** (#1401, landed) | The third of `tests_emit.rs`'s own 7 proposed slices per its dedicated grounding pass (post-#1392), following slice B/#1399, which it depends on (`emit_stub_class` calls the now-real `emit_ns_destructure` directly). Converts `emit_stub_class` and `emit_stub_rhs` in `bynk-emit/src/project/tests_emit.rs`. `emit_stub_class`'s own class wrapper (header/fields) stays hand-written text — **Decision C** (`emit_provider`'s own precedent, #1359): each method is a real `bynk_ts::TsClassMethod` fragment, printed via `bynk_ts::print_class_method` at depth 0 — the only depth `render_class_method`'s own `debug_assert!` allows a `Raw`-bodied method to print correctly at (nesting it inside a real `TsDecl::Class.methods` list would print it at depth 1, since the class itself sits at depth 0). The method body mixes real structure (per-clause pattern-match consts, an `if`/`__bynkDeepEqual` condition, a `throw` for no match) with an opaque `async () => { ... }` IIFE the pattern-value consts wrap around each clause's own lowered check — a block-bodied arrow `TsExpr::Arrow`'s own expression-body-only design can't host, the identical boundary `emit_composition_root`'s own `__eventsDispatch` closure already established (Decision B, #1327) — so the whole body stays one opaque `TsStmt::raw`, unchanged internal construction logic, just redirected into a local buffer instead of the top-level accumulator. `emit_stub_rhs` converts **fully**: every real statement (`const __k = this.__seq_N;`, the sequence-cursor `if (...) this.__seq_N++;` guard, the `switch (__k) { ... }` dispatch) is a real node, each top-level statement printed independently via `bynk_ts::print_stmt` at depth 0 and concatenated — matching `emit_stub_class`'s own pre-existing manual line-by-line re-indentation exactly, the same "print real fragments, splice into a still-textual position" seam slice A's own `binding_gen` already established. Each switch case's own body stays one opaque `TsStmt::raw` line carrying `lower_stub_value_block`'s already-lowered value/fault text, reindented by a new small helper (`reindent_by`) mirroring `TsStmtKind::Raw`'s own "pre-indented at a fixed absolute depth by the caller" contract. **Two real, grounded `bynk_ts` algebra gaps found and closed**: `TsBinaryOp::LessThan` — `emit_stub_class`'s own `ReturnsEach` sequence-cursor guard (`this.__seq_N < <bound>`) is the first real `<` comparison anywhere in `bynk-emit`'s own content, added at the same precedence tier as the existing `GreaterThan` (real JS/TS relational operators all share one level); `TsSwitchCase.default_braced: bool` — `emit_stub_rhs`'s own `ReturnsEach` dispatch braces its `default` case (`default: { ... }`), a genuinely different convention from `workers_entry.rs`'s own unbraced `default:` that established `TsStmtKind::Switch`'s original rendering (#1323) — added as a per-case flag (only meaningful when `test` is `None`) rather than changing the existing unbraced-default behaviour, to avoid risking `workers_entry.rs`'s own real, already-zero-diff content for no benefit; the 3 existing `TsSwitchCase` construction sites (`workers_entry.rs`'s own `case_`/`default_case` helpers, one `bynk-ts` grounding test) needed a mechanical `default_braced: false` added. A first implementation pass also added `TsClassField.ty: Option<TsType>`/`init: Option<TsExpr>` to represent `emit_stub_class`'s own per-call sequence-cursor field (`__seq_N = 0;`, an initializer with no type annotation, no prior real site) as a genuine `TsDecl::Class` field — reverted before merge once `render_class_method`'s own depth-0-only `debug_assert!` (above) ruled out building the WHOLE class as one real node tree in the first place (Decision C applies to the whole wrapper, not just the header), leaving the field extension with no real call site; caught by re-reading the constraint before committing, not by review. **A real cleanup, the same kind slice B/#1399 already made for its own sibling**: `emitter::ts_type_ref_qualified_multi` (a `String`-returning function `emit_stub_class` used to call twice, for its own method params/return-type) had no other production caller once converted — deleted rather than left as dead code, with its own 1 direct unit test rerouted through a new `TsType`-returning twin, `ts_type_ref_qualified_multi_ts_type` (mirroring `ts_type_ref_qualified`'s own twin, #1321), plus `bynk_ts::print_type`. `verbatim_sites` unchanged (5). `ts_writes` drops by **4** (1097 → 1093), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff, first attempt, no iteration: `positive_fixtures`/`bless_positive_fixtures` and `tsc_verify`'s full strict-`tsc` corpus pass unchanged; 138 `bynk-ts` unit tests (2 new, pinning `LessThan` and `default_braced` directly) and all 300 `bynk-emit` unit tests (including the rerouted `ts_type_ref_qualified_multi` test) pass. | R7.1 | #1331, #1399, #1359, #1327, #1323, #1321 |
| **Arc C, slice 34 — the case/scope-setup cluster (`tests_emit.rs` slice D)** (#1403, landed) | The fourth of `tests_emit.rs`'s own 7 proposed slices per its dedicated grounding pass (post-#1392), following slice C/#1401. Converts `emit_test_scope_setup` and `emit_test_case_function` in `bynk-emit/src/project/tests_emit.rs`. `emit_test_scope_setup` converts **fully**, keeping its exact `&mut String`-accumulator signature (its own callers — `emit_test_case_function` here, plus the still-unconverted `emit_test_property_function`/`emit_test_history_property_function`/`emit_contract_attack_function`, slice E — are a mix): real `TsStmt`s for the agent-reset call, the `__obs` recording-proxy setup (`const __obs = { log: {} as Record<string, { args: unknown[]; order: number }[]>, n: 0 }`), all three `deps` factory shapes (recorded/plain/empty), and the cross-context surface alias (`const {key} = (deps as any).surface?.{key};`), each printed via `bynk_ts::print_stmt` at depth 2. Its own `obs_spec` object (capability → sorted op-name array) moves from a `format!`-built literal that relied on Rust's `Debug` string-escaping happening to coincide with JS string-literal escaping to a real `TsExpr::Object`/`TsExpr::Array` of real string literals, routing through `bynk_ts`'s own escaper for the first time. `emit_test_case_function` — a `-> (String, SourceMapBuilder)` function with its own real source-map-sensitive splice arithmetic (`body_base = out.len()`; `case_smb.merge(&body_smb, &body_src, &out, body_base, 0)`, anchoring the opaque `emitter::lower_test_case_body` output — a `lower.rs`-family function, ADR 0391's permanent exclusion) — converts its own **tail** (`return { pass: true }`, the catch clause's `instanceof` guard, and the two failure-path returns) to real `TsStmt`s, each printed via `bynk_ts::print_stmt` at depth 2 and appended; the splice arithmetic itself stays **completely unchanged**, since the incremental `out.push_str` sequencing doesn't care whether the surrounding text comes from `format!` or `print_stmt` — only the final bytes, confirmed identical by the full `bynkc/tests/source_map.rs` suite (10 tests) passing unchanged. The `async function {runner_name}() { try {` header stays hand-written text (2 lines) — not worth a fragile partial-node representation given the offset-tracking constraint the splice itself needs. **One real, grounded `bynk_ts` algebra gap found and closed**: `TsBinaryOp::InstanceOf` — `e instanceof ExpectationError` is the first real `instanceof` anywhere in `bynk-emit`'s own content, added at the same precedence tier as the existing `LessThan`/`GreaterThan` (real JS/TS precedence puts all three relational operators — and `in` — at one shared level), rendered as the keyword `" instanceof "` rather than symbol punctuation. **Review of #1404 found two real issues, both fixed before merge**: (1) the surface-alias site named above was still `format!`-built, not actually a documented carve-out (`TsExpr::OptionalMember` already existed) — converted; (2) the original `InstanceOf` test pinned only the operator text, not the precedence tier the same diff added to `binary_precedence` — closed with two more direct tests (`Add` binding tighter, and `e instanceof A || e instanceof B` staying flat). `verbatim_sites` unchanged (5). `ts_writes` drops by **6** (1093 → 1087), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff, first attempt, no iteration: `positive_fixtures`/`bless_positive_fixtures`, the full `bynkc/tests/source_map.rs` suite, and `tsc_verify`'s full strict-`tsc` corpus pass unchanged; 139 `bynk-ts` unit tests (3 new) and all 300 `bynk-emit` unit tests pass. | R7.1 | #1331, #1401, #1391 |
| **Arc C, slice 35 — the property/history/attack runner cluster (`tests_emit.rs` slice E)** (#1405, landed) | The fifth of `tests_emit.rs`'s own 7 proposed slices per its dedicated grounding pass (post-#1392), following slice D/#1403; depended on slice A alone (`BindingGen`), confirmed by landing. Converts `emit_test_property_function`, `emit_test_history_property_function`, and `emit_contract_attack_function` in `bynk-emit/src/project/tests_emit.rs`. Each function's own `async function {runner_name}() { ... }` header/closing brace stay hand-written text, matching `emit_test_case_function`'s own established precedent (slice D). **`__gens`/`__handlers` construction converts fully**, closing the print-then-splice seam slice A's own doc comments explicitly named as temporary: `binding_gen`'s real `TsExpr` fields (`boundaries`/`gen_ts`/`shrink`) now build a real `TsExpr::multiline_array` of real `TsExpr::object` entries directly, instead of being printed back to text via `bynk_ts::print_expr` and re-embedded via `format!`. A new shared helper, `gen_descriptor_entry(name: Option<TsExpr>, bg: &BindingGen) -> TsExpr`, factors the identical `{ [name: "…",] boundaries: […], gen: (rng: any) => …, shrink: (v: any) => …, show: (v: any) => __bynkShow(v) }` shape all three call sites build (the history-handler shape has no `name` key at all). **`const __where = null;` converts fully** (all three functions, the no-predicate fallback); **`emit_test_history_property_function`'s own defensive early-return** (the malformed-history-property guard) **converts fully**; its own **expression-bodied `__drive` closure converts fully** too — `(seq: ...) => (target_ns as any).__bynkDriveHistory_X(seq, deps)`, unlike `__body`/`__where`'s own block-bodied siblings; its `Array<{ h: number, args: unknown[] }>` param type stays one opaque `TsType::named` string, since that inline object type's real member separator is a comma (`, `), not `TsType::Object`'s own established semicolon (`; `) convention — building it as a real `TsType::Object` would silently change the emitted bytes, the same "odd, one-off shape stays opaque text" posture already used for `Query[T]`/`this.state.storage.get<T>`/`ReturnType<typeof X>`. **Each function's own final `return await __bynkRunProperty(...)`/`return await __bynkRunHistory(...)` converts fully** to a real `TsStmt::return_stmt` wrapping a real `await`+`Call`+object-literal. **The block-bodied `__where`/`__body` arrows and the history function's own triple type-alias line stay hand-written text**, each for its own named reason: the arrows always wrap opaque already-lowered text (from `lower_block_to_async_body`/`lower_test_case_body`, both `lower.rs`-family functions, ADR 0391's permanent exclusion) and `TsExpr::Arrow` is expression-body-only by design (the same boundary `emit_composition_root`'s `__eventsDispatch` closure and `emit_stub_class`'s own pattern-value IIFEs already established) — the gap is in the arrow shape itself, not whether the wrapped content happens to be simple (the contract-attack function's own `__body` is just a destructure plus one bare call, still can't be hosted); the triple type-alias line (three declarations crammed onto one physical line, separated by `; `) is a genuinely new, odd shape with no existing "N statements on one physical line" algebra representation, not worth inventing for one site. No new `bynk_ts` algebra gap. `verbatim_sites` unchanged (5). `ts_writes` drops by **7** (1087 → 1080), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff, first attempt, no iteration: `positive_fixtures`/`bless_positive_fixtures`, `source_map`, `property_behaviour`, `history_behaviour`, `contract_behaviour`, and `tsc_verify`'s full strict-`tsc` corpus all pass unchanged; all 300 `bynk-emit` unit tests pass. | R7.1 | #1331, #1395, #1359, #1327 |
| **Arc C, slice 36 — the system-http driver cluster (`tests_emit.rs` slice F)** (#1407, landed) | The sixth of `tests_emit.rs`'s own 7 proposed slices per its dedicated grounding pass (post-#1392), following slice E/#1405; structurally self-contained, no dependency on any of A-E, confirmed by landing. Converts `emit_system_http_support` in `bynk-emit/src/project/tests_emit.rs`. **The 4 per-route drivers convert fully at the structural level** via a new shared helper, `sysdrive_driver(kind_prefix, sname, key, params, body_stmt, url, options, binding, decode_fn, payload)` — the one real shape all four `__sysdrive_{,raw_,noauth_,rawnoauth_}*` drivers share (async function decl, optional `const __body = JSON.stringify(...)` lead statement, `const __h = makeHarness();`, `const __req = new Request(<url>, <options>);`, `const __res = await __h.env.<binding>.fetch(__req);`, `return <decode>(__res, <payload>);`), replacing four near-identical hand-written `format!` blocks with one parameterised builder. **The wrong-method driver converts fully, no carve-out at all** — its own `new Request(\`https://test${path}\`, { method })` is this file's first real dynamic (non-baked) `TsExpr::TemplateLit` substitution and first real `TsObjectEntry::Shorthand` object literal, both byte-matching the existing text exactly (a single, never-conditionally-empty shorthand entry matches `TsExpr::Object`'s own tight single-line rendering). **Decision A: the per-route options object (`{ method, headers: {...}, body, }`) stays opaque hand-formatted text**, passed as a `TsExpr::Ident`-wrapped fragment argument to a real `TsExpr::New` call — every branch's own hand-written template bakes in an unconditional trailing `, ` before its closing brace and prints `{ }` (one space) rather than `TsExpr::Object`'s own tight `{}` when a sub-object has zero entries, a shape `TsExpr::Object`'s general single-line algebra cannot reproduce byte-for-byte, the same "odd, one-off shape stays text" call this track has made before (Decision B, #1327; Decision C, #1359). The secrets-bootstrap loop's own `format!` call converts fully too, to real `As`/`Member`/`Binary(NullishCoalescing)`/`Index`/`Assign` nodes. **The static signer preamble stays out of scope, honestly recorded**: built via a plain `push_str` literal with zero per-target dynamism, already excluded from the `ts_writes` probe (no `write!`/`writeln!`/`format!` on those lines) — converting it moves no metric and adds no real coverage. **Landed as one slice, not split by driver-kind**: this paragraph's own proposal-time caveat considered splitting by typed/raw/noauth/rawnoauth/wrongmethod, but once drafted the 4 per-route kinds shared one structural shape completely — every real difference became a plain parameter to `sysdrive_driver`, so a split would have meant reviewing the identical helper four times rather than once. **One real transcription bug, caught immediately by `positive_fixtures`, fixed before the CI pass**: the wrong-method driver's decode call was first built as `responseToHttpResult`, not the existing text's own `responseToHttpOutcome` (the router's `405` fall-through decodes to an `HttpOutcome`, not an `HttpResult`) — a one-line fixture diff named the exact mismatch. **A real, non-metric-driven simplification found while implementing**: the first draft's per-call-site `format!` for each driver's own name *increased* `ts_writes` by 1 despite eliminating four hand-formatted functions (each driver-kind's name interpolation became its own newly-countable line, previously merged into one big `format!` call); consolidated into `sysdrive_driver`'s own single internal `format!("__sysdrive_{kind_prefix}{sname}_{key}")`, turning the accidental increase into a genuine larger net decrease. No new `bynk_ts` algebra gap — every shape needed (`TsDecl::Function` `is_async`, `TsExpr::New`, `TsExpr::TemplateLit` both zero- and real-substitution, `TsExpr::Await`, `TsExpr::As`, `TsObjectEntry::Shorthand`, `TsExpr::Arrow` expression-bodied) already existed. `verbatim_sites` unchanged (5). `ts_writes` drops by **3** (1080 → 1077), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff, confirmed after the one fix above: `positive_fixtures`/`bless_positive_fixtures`, `source_map`, and `tsc_verify`'s full strict-`tsc` corpus all pass unchanged; all `bynk-ts`/`bynk-emit` unit tests pass. | R7.1 | #1331, #1394, #1327, #1359 |
| **Arc C, slice 37 — the two top-level module assemblers (`tests_emit.rs` slice G)** (#1409, landed) | The seventh and last of `tests_emit.rs`'s own 7 proposed slices per its dedicated grounding pass (post-#1392), following slice F/#1407 — closes Arc C entirely. Converts `emit_integration_module` and `emit_test_module` in `bynk-emit/src/project/tests_emit.rs`, the two top-level module assemblers; depended on slice B (`emit_ns_destructure`), confirmed by landing, in addition to calling nearly every other already-converted delegate. **Every header comment line, every `import` line, and each module's own `export async function run(only?: string) { ... }` runner convert fully, no carve-out at all.** The two functions' own per-case/per-property/per-attack loop bodies stay exactly as they are — each already delegates to an already-converted or intentionally-opaque (`lower.rs`-family, ADR 0391) builder returning pre-formatted text with its own source-map splice arithmetic (`module_smb.merge(...)`); only the bookend content around those splices is new real structure. **One real, grounded `bynk_ts` algebra gap found and closed**: `TsDecl::ImportDefault { alias, from }` — `emit_integration_module`'s own per-participant `import worker_{ns} from "../workers/{dir}/index.js";` (the participant's Worker entry module's default export) is the first real default import anywhere in `bynk-emit`'s own converted content; every prior import site is either named (`TsDecl::Import`) or namespace (`TsDecl::ImportNamespace`). Added with a direct printer unit test and folded into the existing "no blank line between adjacent imports" grouping rule. **A new shared pair of helpers, `run_dispatch_stmt`/`build_run_function`, factors the identical `run(only)` shape both functions build**: one `if (want("name")) results.push({ name: "...", ...(await runner()) });` dispatch line per case/property/attack (a real `TsObjectEntry::Spread` wrapping a real `TsExpr::Paren(await_expr(...))`, matching the existing text's own explicit parens around the spread), wrapped in one real `TsDecl::Function` — this function carries no opaque lowered content and no source-map splice sensitivity at all, so it converts fully rather than needing `emit_test_case_function`'s own header/tail split. No other new `bynk_ts` algebra gap. `verbatim_sites` unchanged (5). `ts_writes` drops by **5** (1077 → 1072), verified via a fresh `cargo xtask greenfield-status --apply`. `ast_importers`/`ts_any` unaffected. Zero diff, first attempt, no iteration: `positive_fixtures`/`bless_positive_fixtures`, `source_map`/`source_map_bodies`, and `tsc_verify`'s full strict-`tsc` corpus all pass unchanged; 141 `bynk-ts` unit tests (1 new, pinning `ImportDefault`) and all `bynk-emit` unit tests pass. **Closes Arc C entirely: all 37 slices landed.** | R7.1 | #1331, #1394, #1399, #1325 |

**Arc D — settling (~8 slices; 3 landed as P7.10/P7.11/P7.12, 5 remaining)**

Provisionally lettered, not numbered — Arc C's own slice count is an estimate (originally ~23-27,
now ~30-33, revised by
#1331, then #1333, then #1335, then #1337, then #1339, then #1351, then #1353, then #1355, then #1357, then #1359, then #1361, then #1364, then #1367, then #1369, then #1371, then #1373, then #1375, then #1377, then #1380 — see §6's own "Revised estimate" paragraph above for the live number, not the figure named here), so fixed `P7.N` numbers here would silently claim
a range Arc C's real slices will actually occupy. Real
`P7.N` numbers are assigned sequentially as each slice is actually cut, in landing order, the same
convention every prior track on this trajectory used.

| Slice | What lands | Rules | Gated on |
|---|---|---|---|
| **P7.12** (landed) | `bynk-ir`/`bynk-lower` carved as crates — see below | R10.3 | P7.5 |
| **P7.d2** | R8.2 — brand string recorded once (R4.10), read at emission rather than computed from `ctx.owning_context` | R8.2 | Arc C substantially landed |
| **P7.d3** | R8.14 — the JSON/boundary codec collector unified into one collector over `bynk-ts` tree nodes, revisiting P6.56's declined IR-based attempt now that a tree exists to collect over | R8.14 | P7.8 |
| **P7.10** (landed) | R8.4/d5/d8 settling sweep — see below | R8.4, R8.5,7,9,11–13,15,17–19,21,22 (verify) | Arc C landed |
| **P7.11** (landed) | R10.4 surface enumeration — see below | R10.4 | Arc C landed |
| **P7.d7** | R10.2 verification — `bynk-lsp` stops linking emission code it never executes (finding #39) | R10.2 | P7.12 (landed, unblocked) |

**P7.d1 re-grounding (this settling pass, not yet implemented) — `ir/lower.rs` is not one crate's
worth of live code, it's two.** `bynk-emit/src/ir.rs` (1,756 lines) + `bynk-emit/src/ir/lower.rs`
(10,095 lines) total ~11,850 lines, both `pub(crate)` at the crate root — no external crate
reaches `bynk_emit::ir` today, so the carve is a pure internal reorganisation with no external-API
compatibility surface to preserve. But `ir/lower.rs` itself is not uniformly live: it contains two
structurally distinct layers under one file.

**Corrected twice within this same pass, before any code changed — both corrections recorded
rather than silently folded away, since the methodology matters more than either number.**

*First correction:* the initial cut classified "dead" by grepping for callers *outside*
`ir/lower.rs` only, which missed real same-file call chains. `lower_event_subscriber_shapes_ir` (a
genuine root, called from `project.rs:1447`) calls `lower_service_item_ir` directly
(`ir/lower.rs:1767`), which — per its own doc comment — "unconditionally lowers every handler's
own *body* (not just its declared shape)", reaching `lower_service_handler_ir` →
`lower_service_handler_body_ir` → `lower_block_ir` → `lower_expr_ir`/`lower_stmt_ir` and the rest
of the recursive expression-lowering machinery. That doc comment records a `catch_unwind` safety
probe finding **zero panics across the entire e2e fixture corpus** — real, live, tested production
code, not dead. Root cause: a naive text scan without stripping `///`/`//` lines treats a rustdoc
cross-link like `` [`lower_service_item_ir`] `` in a neighbouring doc comment as a real call.

*Second correction (caught by an independent review of the first correction, then generalised):*
review of the first correction's own PR found the entry-point count self-inconsistent (18 vs. 15
named) and one root missing entirely — `lower_actor_seam_ir` (real callers at
`emitter/emit.rs:2656` and `emitter/workers.rs:632`), bringing the true root count to **17**.
Re-running the reachability analysis with a bare-word matcher (not requiring a trailing `(`, since
`.map(lower_event_pattern_ir)` and `.map(lower_cap_ref_ir)` pass a function *by reference*, no call
syntax at the reference site) found two more Layer-1 misses: `lower_event_pattern_ir` (called from
`lower_protocol_ir_from_commons`, `ir/lower.rs:1457`) and `lower_cap_ref_ir` (called from both
`lower_provider_given_ir` and `lower_handler_given_ir`, `ir/lower.rs:2088`/`2100`). The same
looser matcher also produced one new false positive the stricter one hadn't — `lower_fn_body_ir`
now looked live via a hit inside `lower_ident_ir`, but reading that line
(`ir/lower.rs:3524`) shows it is a `todo!()` panic message's own prose ("...structurally
unreachable through lower_fn_body_ir (see its own doc comment)..."), not a call — a string
literal, invisible to whole-line comment stripping. Confirmed dead by its one real (non-string)
caller being `lower_fn_item_ir`, itself dead.

**Layer 1 (live, confirmed by call-graph reachability from 17 real production entry points, 61
functions total):** every function this file needs to construct `IrItem::Type`/`Capability`/
`Service` (bodies included) and the shapes `emitter.rs`/`emitter/emit.rs`/`emitter/workers.rs`/
`emitter/workers_entry.rs`/`emitter/lower.rs`/`project.rs` read directly. The 17 roots:
`lower_handler_kind_ir`, `lower_protocol_ir`, `lower_protocol_ir_from_commons`,
`lower_handler_given_ir`, `lower_provider_given_ir`, `lower_route_cache_ir`,
`lower_route_limit_ir`, `lower_event_subscriber_shapes_ir`, `lower_actor_seam_ir`,
`is_effectful_return`, `body_writes_state`, `capability_op_sig_from_commons`,
`lower_attached_fn_sig_ir_from_types`, `lower_type_item_ir`, `lower_capability_item_ir`,
`lower_service_handler_signature_ir`, `lower_store_field_shape_ir`. Reached transitively (mostly
through `lower_event_subscriber_shapes_ir` → `lower_service_item_ir`, plus the two function-by-
reference cases above): `lower_service_handler_ir`, `lower_service_handler_body_ir`,
`lower_block_ir`, `lower_stmt_ir`, `lower_expr_ir`, `lower_pattern_ir`, `lower_arm_ir`,
`lower_exhaustive_ir`, `lower_call_ir`, `lower_lambda_ir`, `lower_ident_ir`,
`lower_record_spread_ir`, `lower_question_ir`, `lower_is_ir`, `lower_commit_shape_ir`,
`lower_op_sig_ir`, `lower_op_sig_ir_from_commons`, `lower_fn_sig_ir_from_types`,
`lower_http_method_ir`, `lower_policy_ir`, `lower_event_pattern_ir`, `lower_cap_ref_ir`, and a
dozen more small private helpers (full list in the proposal issue, not repeated here).

**Third correction — the "Layer 2 = delete" premise itself was wrong, caught before implementation
by widening the reachability question to include test callers, not just production ones.** The
above found 11 functions with zero *production* callers. Attempting the actual deletion on this
branch hit real compile errors from test helpers (`lower_fn`, `agent_store_cells`,
`handler_ir_of_with_predicates`, and others) that wrap exactly these 11 — `lower_fn` alone wraps
`lower_fn_body_ir` and is called from **51 real test sites**. Reading what those tests actually
assert settles the question: they are not redundant coverage of already-tested behaviour, and not
assertions about output nothing consumes. Per function, checked by reading real test bodies, not
inferred from caller counts:

- **`lower_fn_body_ir`/`lower_handler_body_ir`** — pure scope-seeding wrappers around
  `lower_block_ir` (live). `lower_fn`'s 51 call sites are this file's own primary harness for
  testing the *shared* expression/pattern/lambda-lowering machinery (`lower_expr_ir` and
  everything under it) via the cheapest available fixture (a bare free function) rather than
  constructing a full service+handler AST every time.
- **`lower_handler_ir`/`lower_provider_op_ir`** — named explicitly, together with
  `lower_fn_body_ir`/`lower_service_handler_body_ir`, as "the four real body-lowering entry
  points" in a regression test for a real historical bug (review of #1238: a `set_return_ty`
  panic on a resolve-miss path) — each of the four needs independent coverage precisely because
  the bug was entry-point-specific. Deleting either removes that regression guard for its own
  entry point.
- **`lower_fn_item_ir`** — its tests assert receiver detection (self vs. none), generic
  type-variable resolution, and effectful-flag detection: `IrItem::Fn`'s own assembly logic, not
  covered by `lower_fn_body_ir`'s tests (which test the body/expression side only).
- **`lower_agent_item_ir`** — its tests assert the "compute once per agent, thread through every
  handler" behaviour is real (a real, non-empty `invariants`/`transitions` pair reaching each
  handler's `commit`, "not the empty pair `lower_handler_ir` would produce on its own if this
  function forgot to pass them" — the test's own words), plus a named regression
  (`agent_invariant_with_a_real_comparison_lowers_without_panicking`, #1189's own breakage point).
  Both are unique to this function's own assembly step.
- **`lower_provider_item_ir`** — reached directly by the same #1238 regression test above
  (`question_reached_from_a_provider_op_body_sets_return_ty_without_panicking`) as the entry point
  needed to exercise `lower_provider_op_ir`'s own body-lowering call.
- **`lower_store_field_ir`** — tests assert `init`-expression lowering for a `Cell` field
  ([DECISION D]), which `lower_store_field_shape_ir` (the live shape-only sibling) deliberately
  does *not* do — genuinely non-overlapping coverage, not a duplicate of the live variant's tests.
- **`lower_invariant_ir`/`lower_transition_ir`/`lower_handler_signature_ir`** — each has its own
  direct test asserting real, non-redundant behaviour (named-predicate construction with real
  `old`/`new` state-typed binding; params/given/effectful extraction respectively).

**Conclusion: none of the 11 is dead in the sense that matters for a carve.** Zero *production*
callers, but every one has a real test caller exercising either genuinely-live shared logic (the
`_body_ir` family) or its own non-redundant assembly/regression-guarding logic. The corrected rule,
same for a function with 1 test caller as one with 51: does the test exercise real logic, or is it
itself dead weight? Here, every test earns its keep. **All 11 move into `bynk-lower` as-is** — no
deletion — each with a short doc-comment note added at the carve ("no production caller; exercises
`bynk-lower`'s shared lowering machinery / guards against issue #1238's/#1189's own regression /
&lt;its own real reason&gt;, per its own real test coverage — see that test for detail").

This means P7.d1 is, after all, close to a mechanical file-move (`ir.rs` + all of `ir/lower.rs`,
unchanged in content) into `bynk-ir`/`bynk-lower` — new manifests, ~99 call-site import-path fixes
across 7 consumer files, resolving `ir/lower.rs`'s own live reverse dependency — `use
crate::emitter::{ MUTATING_CELL_OPS, MUTATING_LOG_OPS, MUTATING_MAP_CACHE_OPS, MUTATING_SET_OPS,
block_uses_emit, match_needs_if_chain };` — before the new crate graph is acyclic (the four
`MUTATING_*_OPS` consts have no other `bynk-emit`-side use and can simply move; `block_uses_emit`/
`match_needs_if_chain` are each genuinely shared with real `bynk-emit`-side callers of their own
and need a real new home both sides can reach, not a relocation), and fixing `ir/lower.rs`'s own
module doc comment (`ir/lower.rs:6-8`), which still asserts "nothing in this module is called from
anywhere in `bynk-emit`'s existing emission path... it has no consumer yet" — false since the first
correction above.

**P7.12 (landed, #1414) — the `bynk-ir`/`bynk-lower` crate carve, exactly as scoped above: all
~11,850 lines move, nothing deleted.** New crates: `bynk-ir` (`ir.rs`'s types, depends on
`bynk-syntax`/`bynk-check` only) and `bynk-lower` (`ir/lower.rs`'s lowering pass, unchanged content,
depends on those two plus `bynk-ir`; `bynk-project` as a dev-dependency for its own test fixtures).
`bynk-emit` depends on both. The reverse dependency resolved by relocating `block_uses_emit`,
`walk_exprs`/`walk_block_exprs`, `match_needs_if_chain` + its private `pattern_has_nested_test`
helper, and the four `MUTATING_*_OPS` consts into `bynk-ir` itself — every `pub(crate)` item in
`ir.rs`/`ir/lower.rs` that needed reaching from outside its own file became `pub` (the crate
boundary is what encapsulates now, not the old intra-crate visibility). Every `crate::ir::`/
`crate::ir::lower::` call site across `bynk-emit`'s 7 consumer files repointed to `bynk_ir::`/
`bynk_lower::`; the stale module-doc-comment claims in both `ir/lower.rs` and `bynk-emit/src/lib.rs`
fixed in place, not left standing. `xtask/src/greenfield_status.rs`'s `AST_IMPORTER_EXCEPTIONS`/
`TS_WRITES_EXCLUDED_FILES` both dropped their `ir.rs`/`ir/lower.rs` entries — those files didn't
stop importing the AST or writing internal strings, they left `bynk-emit/src` entirely, outside
either probe's own scanned universe; three of that fix's own tests updated to match. Zero-diff:
full workspace `cargo check --all-targets` and `cargo test --workspace` both clean before touching
anything test-related, and clean again after; `bynk-lower`'s own 134 tests (including the #1238/
#1189 regressions this note's own "keep all 11" finding hinged on) moved verbatim and all pass.
`design/greenfield-status.md` regenerated: every gated probe reads identically to pre-carve — only
the trend-only `test_density` breakdown changed (two new crate rows, `bynk-emit`'s own percentage
shifting as expected once ~11,850 lines and their tests left it). P7.d7 is now unblocked.

**P7.10 (landed) — the R8.4/d5/d8 settling sweep, a single verify-only pass, no code changes.**
Re-audited against the tree as it stands after Arc C's full 37-slice landing (all citations below
are current, not the stale pre-Arc-C line numbers §3.4 originally recorded):

- **R8.4** (numeric-refinement guard ordering): checked all five real base-guard/predicate
  emission sites — `refined_check_as_bool` (`emitter/lower.rs`, the `is`/`.of`-check path),
  `emit_refined_checks`/`print_numeric_guard_stmt` (`emitter/emit.rs`, the `.of` constructor path),
  `emit_inline_refinement_checks` (`emitter/serialisation.rs`, the `Named`-with-predicates wire
  path), `WireRef::Base`'s handler (`emitter/serialisation.rs`, the guard-only bare-scalar wire
  path), and `deserialise_expr_via`'s `TypeRef::Base` arm (`emitter/serialisation.rs` ~1153-1180,
  its own `extra` guard). The last two carry no separate predicate list by construction — a
  refined named scalar routes through `WireRef::Named` instead — so neither can violate the
  ordering, but both are named here so the closure matches what a `Number.isInteger|isFinite` grep
  actually finds. Every site emits the base guard (`Number.isInteger`/`Number.isFinite`) before
  any predicate, with no exception. **Confirmed closed, no violation
  found.**
- **P7.d5** (severing `bynk-emit`'s `bynk-check` dependency): confirmed NOT severable —
  222 real `bynk_check::` use sites across 17 files in `bynk-emit/src`, and the dependency is
  named directly in `bynk-emit`'s own `Cargo.toml` description ("atop ... bynk-check (all semantic
  checking)"). This is `bynk-emit`'s primary semantic-model input, not conversion residue.
  **Resolved: nothing to sever, the tentative slice does not apply.**
- **R8.12's self-supersession**: confirmed NOT yet triggered. R8.12 self-supersedes "at the exact
  moment R7.1 lands" (§3.4), and R7.1 has not fully landed — `design/greenfield-status.md`'s own
  `ts_any` probe currently reads **30** (not 0), so `Any` has not been fully eliminated. R8.12
  remains correctly closed under its own current carve-out text (only `Call` wrappers get real
  param types by design); this is not a regression, since the triggering condition hasn't fired.
- **The other eleven already-closed R8 rules** (R8.5, R8.7, R8.9, R8.11, R8.13, R8.15, R8.17,
  R8.18, R8.19, R8.21, R8.22): spot-verified still present and behaviourally intact post-Arc-C —
  `emit_context_rebrands` (R8.5, `emitter.rs:1708`), the `loadState` validation gate (R8.7,
  `emitter/emit.rs` ~4581+), the three `.sort_by` route-ordering call sites (R8.17,
  `emitter/workers_entry.rs:410/470/503`). One stale citation caught and worth noting for a future
  reader: R8.19's `escape_toml_basic_string` now lives in `emitter/toml_doc.rs` (moved out of
  `wrangler.rs` before this pass, unrelated to Arc C), not at the location §3.4 originally cited —
  the function and its injection test are both present and unchanged. Arc C's own zero-diff
  discipline (every one of its 37 slices verified byte-identical fixture/tsc output) is why none of
  these show real behavioural drift — only citations move when the underlying code does.
- **Probe floors, as recorded in `design/greenfield-status.md` at this commit** (aggregate
  counts, gated there by `greenfield_status_table_is_current` — this copy will drift if that
  file changes and this paragraph isn't re-read alongside it): `ts_writes` = 1072, `ts_any` = 30,
  `ast_importers` = 5 (its one file-scoped floor — `bynk-emit/src/emitter{,/**}`, the phase-6
  floor, unchanged by Arc C), `verbatim_origins` = 1, `verbatim_sites` = 5.

No code changed; no `design/pending` entry (doc-only, matching the #1366/#1379/#1394 precedent
for a pure verification pass with nothing to fix).

**P7.11 (landed) — R10.4 surface enumeration over `bynk_emit::emitter`, verify-only, no code
changes.** Finding #42 (the July review) named "33 of 38 world-reachable items have no external
user" as the evidence for R10.4 against this module. Full census against the current tree:
`bynk_emit::emitter`'s only reachable surface is 4 `pub mod`s (`contracts`, `secrets`, `toml_doc`,
`wrangler`) plus 1 root-level re-export (`print_toml_document`) plus 3 root-level `pub fn`s
(`emit_runtime_module`, `emit_tsconfig`, `emit_tsconfig_with_source_maps`) — no nested `pub mod`s,
no crate-root re-export beyond `pub mod emitter;` itself. Enumerating every `pub` item inside those
4 submodules (12 total, `emitter.rs`'s own body is already `pub(crate)` almost everywhere) and
cross-referencing every real external reach point (`bynkc`, `bynk-driver`, `bynk`, `bynk-strip`)
by grep across the whole workspace: **all 12 have a confirmed real external user** —
`CONTRACTS_MANIFEST`/`SECRETS_MANIFEST` (deploy tooling), `emit_runtime_module`/`emit_tsconfig`/
`emit_tsconfig_with_source_maps` (bynkc), `KV_NAMESPACE_ID_PLACEHOLDER`/
`wrangler_needs_kv_materialisation`/`materialise_kv_namespace_id` (`bynk/src/deploy*.rs`),
`print_toml_document` (`bynk-driver`), `TomlDocument` + `set_main` (the type crosses the boundary
inside `Document::Toml(TomlDocument)`; `set_main` is called directly by `bynk-strip::
strip_project_to_js`, per that method's own doc comment). **Zero spurious `pub` items remain.**
Finding #42's own count (33-of-38) does not match current reality — most likely already closed as
an incidental byproduct of earlier work (`toml_doc.rs`'s own P7.3 module doc already argues from an
R10.4-conscious posture: "not a general TOML library... no more"), not something this pass had to
fix. No code changed; no `design/pending` entry, same doc-only precedent as P7.10.

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
