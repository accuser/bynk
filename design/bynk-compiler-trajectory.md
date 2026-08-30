# The compiler trajectory

**From v0.245.0 to the greenfield reference, in eight phases, without losing a feature.**

- **Status:** **Complete.** All eight phases retired — phase 8 (`incrementality.md`, the last) retired
  30 August 2026, closing spine [#1507](https://github.com/accuser/bynk/issues/1507) and, per this
  track's own §12, this document itself: §1's endpoint is reached. This is the **theme** document: it
  holds the endpoint and the order. It was not a track and not a build authorisation; each phase was
  realised by its own track (`design/tracks/`, now all retired), each track by its own slices. See the
  closing note at the end of §1 for what "reached" does and does not mean, and §7's per-phase
  retirement links for the full record.
- **Companion to:** [`bynk-greenfield-compiler.md`](bynk-greenfield-compiler.md) — the destination
  specification, 130 numbered rules. This document holds the *path*; that one holds the *place*.
- **Evidence:** [`reviews/2026-07-27-compiler-pipeline-review.md`](reviews/2026-07-27-compiler-pipeline-review.md).
- **Precedent for the layer:** the LSP theme already spanned three tracks — `lsp.md`,
  `lsp-foundations.md`, `editor-currency.md` — with the spec living on at `bynk-lsp-spec.md` after
  all three retired. A multi-track theme is normal here; what has been missing is a document that
  names one and holds its endpoint.

---

## 1. The endpoint

**The compiler Bynk ships today, feature for feature, rebuilt on the architecture in
`bynk-greenfield-compiler.md`.**

Not a smaller language. Not a subset. Every construct that compiles at v0.245.0 compiles at the end
of phase 8, with the same semantics and — except where a phase records a deliberate correction — the
same emitted behaviour.

**A note on scope, because the two documents differ deliberately.** The reference specifies the
**Foundations layer** to build-from depth, with named extension points, and walks four
post-Foundations features through those points in its Part 13. The trajectory's endpoint is the
**whole current surface** realised on that architecture: all six service protocols, events, messages
and locale, WebSockets and held resources, the query algebra and `@indexed`, the five storage kinds,
the test sublanguage, the boundary codecs, the deploy artefacts, and the LSP surface. Part 13 is the
evidence that the architecture carries them; phases 6 and 7 are where that evidence is tested for
real rather than argued.

**Closing note (30 August 2026): this endpoint is reached, and what "reached" does and does not
mean.** Phase 8, the last, retired the same day as this note — closing summary in
[`archive/retired-tracks.md`](archive/retired-tracks.md), §7's table below has every phase's own
retirement. No phase dropped a capability (§2's invariant held for all eight); every construct that
compiled at v0.245.0 compiles today, on the architecture `bynk-greenfield-compiler.md` specifies.
**What this endpoint does not claim:** R3.15's own scheduler decision (salsa, a hand-rolled memo
table, or nothing) was a separable decision phase 8 explicitly deferred whole, not silently dropped
— it stays *unopened, no trigger yet* (phase 8's own §7), the same standing as R10.5's `bynk-driver`
consolidation, a lossless CST (rowan), and an emit-side signature concept for `Artefacts`. None of
these four is a construct the shipped compiler loses; each is a forward reference with a named,
unfired trigger, per R15.1's own discipline against recording an intention as a refusal. This
document stays in place as the historical record of the path walked, per the LSP theme's own
precedent above — `bynk-greenfield-compiler.md` continues to hold the place.

---

## 2. The invariant that makes this a migration and not a rewrite

**No phase may drop a capability, and no phase boundary may leave the compiler incoherent.**

Two consequences, and they are the whole reason an eight-phase commitment is tolerable to start.

**Every phase boundary is a safe stopping point.** The trajectory can be abandoned after any phase
and what remains is a coherent compiler with a strictly better internal architecture than the one
before it. There is no phase whose value depends on the next phase landing. This is the property that
distinguishes a trajectory from a rewrite, and it is bought by the completion discipline in §9 rather
than by good intentions.

**Stopping mid-phase is not safe.** A phase that half-lands leaves two paths reachable, which is the
failure mode every regret in this corpus shares. So: phases may be abandoned; slices may not be
abandoned mid-phase without either finishing or reverting.

---

## 3. The eight phases

Each phase is stated as a **state of the compiler**, not as a list of work. The invariant is what a
review checks; the probe is what CI checks.

### 3.0 Measured baseline — 30 July 2026, v0.245.0

Every probe below was run against the working tree. Where a phase reads zero it is complete; where it
does not, the number is the distance.

| Phase | Probe | Reads | Target |
|---|---|---|---|
| 0 Seams | `std::fs` files below the driver | **10** (`bynk-emit` 4, `bynk-ide` 5, `bynk-fmt` 1) | 0 |
| 0 Seams | `CompileOptions.sources`, `testkit.rs` | present | present ✅ |
| 0 Seams | fixture assertion kinds in use | 3 / 2 / 1 vs **419** `expected_error` | broad adoption |
| 1 Paydown | `[workspace.lints]` | **absent** | present, `deny` |
| 2 Typed lowering | `stmts: &mut Vec<String>` | **32** | 0 |
| 2 Typed lowering | `contains("await` in `lower.rs` | **1** | 0 |
| 3 Identity | `HashMap<Span` | **27** | 0 |
| 3 Identity | `Ty::Error`, `certify`, `FileId` on `Span` | absent, 0, absent | present |
| 4 Project model | `bynk-project` crate; `bynk-ide`→`bynk-emit` | absent; present | present; absent |
| 5 Semantics | registered `bynk.*` codes in `bynk-emit` | **200** (`bynk-check` 206) | 0 |
| 6 The IR | `bynk_syntax::ast` importers in `bynk-emit` | **13 files** | 0 |
| 7 TS tree | TypeScript-producing `write!` outside a printer | not measured | 0 |
| 8 Incrementality | keystroke-to-diagnostic latency by query level | not measured | — |

**The phase-5 row is a snapshot, not a live reading — see the correction below.** This table records
what a probe read on 30 July 2026; it is not re-run in place as later work changes the number, the same
convention `bynk-greenfield-compiler.md`'s Appendix D uses. Phase 5's own settling review
(`design/tracks/semantics-in-the-checker.md` §1, 8 August 2026) found the live probe (`emit_diagnostics`)
had already dropped to 49/53 by the time that track opened — reflected in Appendix D's own corrected row,
not rewritten here.

**Two readings matter more than the rest, read together with that correction.** Phases 0 and 1 are
largely complete and *nobody planned them* — the paydown happened as ordinary work between v0.237.1 and
v0.245.0, including the `ResolvedCommons` constructor that re-enabled three security gates. Phase 5's
distance *did* grow between the July review and this table's own 30 July reading (190 counted codes to
200 registered ones) — but it has since reversed hard, for the same "ordinary churn" reason as phases 0–1,
not because phase 5 opened yet: phase 4's own P4.1 slice relocated most of the shared checking pipeline as
a side effect of closing a different rule (R10.2).

That pair is the trajectory's whole argument, measured rather than asserted: **small defects are fixed by
ordinary churn; structural ones drift the wrong way without a track — until a track for a *neighbouring*
phase exists to catch one by accident, which is what phase 4 did for phase 5 here, and still needed phase
5's own settling review to notice, name and finish the job.**

### Phase 0 — Seams

*The compiler is testable in memory, at crate granularity, and the fixture format can assert
something other than whole-file identity.*

Today ~17k lines are reachable only from `bynkc`'s on-disk fixtures, and the fixture format offers
two granularities — byte-identical emission and category strings — "and nothing between". Every phase
after this one is a structural change, and a structural change gated only by whole-file goldens one
crate up is what ADR 0198 calls "the weakest possible evidence".

**Invariant:** no crate below the driver reads the filesystem; every fixture granularity has at least
one user.
**Probe:** `std::fs` mentions below the driver = 0; `CompileOptions` has `sources`.
**Reference rules:** R2.3, R3.1, R11.2, R11.3, R11.7.

### Phase 1 — Paydown

*The known defects are closed, the registries are single-sourced, and ordering is deterministic.*

The July review's own verdict on this phase: "Nothing here suggests remediation. This is paydown, and
most of the highest-value items are small." It includes the security slice — the `ResolvedCommons`
constructor that re-enables `.raw`, `T.unsafe(…)` and owner-only event emission, three gates that are
off inside agent handler bodies today.

**Invariant:** no wildcard arm over a compiler-owned enum; `bynk check`'s exit code matches the
highest severity emitted; no fact exists in two hand-synced copies where one can be derived.
**Probe:** `clippy::wildcard_enum_match_arm` at `deny` workspace-wide with zero allowances.
**Reference rules:** R2.11, R2.12, R3.6, R6.11, R9.1, R9.4, R9.6, R11.6, R12.2.

### Phase 2 — Typed lowering

*Hoisting is a value the caller must consume, not a side channel it may ignore.*

`lower_expr` returns `Lowered { pre, expr }`. Closes the dropped-statements bug, the
spliced-statements bug, the `let x = match risky()? { … }` miscompile, and the short-circuit
violation, in one mechanical change across ~90 functions.

**Invariant:** no function in `bynk-emit` takes a statement sink; async-ness is never decided by
inspecting generated text.
**Probe:** `stmts: &mut Vec<String>` occurrences = 0.
**Reference rules:** R6.2, R6.3, R6.4, R6.6.

### Phase 3 — Identity and totality

*Nodes have identity independent of position; every side table is total; the editor consumes a
program that does not compile.*

`ExprId` at parse, `FileId` on `Span`, `Ty` interned, `Ty::Error` as a real variant, `expr_ty` as an
`IndexVec`, and `certify` as the sole constructor of `CheckedProgram`. This is the phase that makes
the batch path and the editor path one program.

**Invariant:** no compiler map is keyed by source position; no lookup that must succeed can fail; the
editor never calls `certify`.
**Probe:** `HashMap<Span` occurrences = 0.
**Reference rules:** R2.4, R2.5, R3.10, R4.1, R4.2, R4.3, R4.9.

### Phase 4 — The project model as a phase

*Discovery, the unit graph and the schema registry live below both check and emit.* Contract hashing
was named here too when this section was first drafted; `project-model.md` §3.4 settled it as phase 8's
`UnitSignature` concept instead, once the tree turned up no `ContractHash`-shaped type this phase's own
reference rules actually need.

`bynk-project` becomes a crate. `bynk-ide` repoints at it and drops its `bynk-emit` edge —
`bynk-ide/Cargo.toml` states the whole reason for the dependency, "`analyse_project` (the non-bailing
project analysis) lives in the project driver." That repoint turned out not to be one function's worth
of work as first estimated here: `project-model.md` §3.3(a) found `analyse_project` calls the same
checking-and-discovery orchestration function (`run_checks`) the CLI path does, with no seam at
project-model granularity — closing the edge needs a new `bynk-check`-side analysis entry point, not a
relocation.

Cheap, and it is the *cause* the next phase treats. Context semantics live in the emitter because the
project model does.

**Invariant:** `bynk-ide` does not link the emitter; cross-build state is threaded, not ambient.
**Probe:** `bynk-ide` → `bynk-emit` edge absent from the manifest.
**Reference rules:** R3.7, R3.8, R3.9, R3.11, R10.2.

### Phase 5 — Semantics in the checker

*Everything that makes Bynk Bynk is checked in one crate.*

The diagnostic codes in `validate.rs` and elsewhere in `bynk-emit` — 110 and 80 at the July review,
200 total and rising as of §3.0's measurement — move to `bynk-check`; `icu.rs` and
`websocket::analyse_open_shape` go with them; `Ctx` becomes `pub(crate)`; the accidental `pub` surface
is demoted.

This is the phase that makes the crate names true, and therefore the phase that most directly serves
the reviewability goal: a contributor asked to add a check goes to `bynk-check` and finds it.

**Invariant:** `bynk-emit` emits no diagnostic; no crate reaches back across a boundary to drive the
checker.
**Probe:** `bynk.*` codes originating in `bynk-emit` = 0.
**Reference rules:** R3.5, R4.6, R4.11, R10.1, R10.4.

### Phase 6 — The IR

*The emitter consumes a typed intermediate representation and never reads an AST node.*

`CheckedProgram → Ir → TsProgram`. Expressions and declarations both: `IrExpr` with a `TyId` by
construction, `Callee` as a closed enum, `IrPat` with a recorded lowering form, `IrItem` with
`CommitShape` computed from resolved store writes.

This is where the reference's Part 13 stops being an argument. Events, messages, WebSockets and
idempotency each get their variants; if the extension points hold, the cost is the 11 variants, 3
fields and 4 emission arms Part 13 predicts. If they do not, this phase is where that is discovered,
and the trajectory's estimate for phase 7 changes accordingly.

**Invariant:** `bynk-emit` names no AST type; every dispatch decision is a match on a resolved enum;
`CommitShape` is data, not control flow.
**Probe:** `bynk_syntax::ast` imports in `bynk-emit` = 0.
**Reference rules:** R6.1, R6.5, R6.7–R6.16, R5.1–R5.11.

### Phase 7 — The TypeScript tree and printer

*Emission produces nodes; one printer writes every character.*

`bynk-ts` holds the tree and the printer. The tree omits `enum`, `namespace`, decorators, parameter
properties and `TsType::Any` — which converts ADR 0136's strip-only erasability from a standing
invariant tested against six snippets into a property that cannot be typed, and stops `tsc --strict`
being disarmed at the two places it currently is. Source maps come from the printer, which owns the
buffer, so the wrong-offset class disappears with the right-offset one.

`Artefacts` becomes a keyed set of typed documents, so `wrangler.toml` couples to a node rather than
to a substring of emitted text.

**Invariant:** no crate but `bynk-ts` writes a character of output; `TsType::Any` does not exist.
**Probe:** `write!`/`format!` producing TypeScript outside `bynk-ts` = 0.
**Reference rules:** R7.1–R7.8, R8.1–R8.22.

**Correction, argued and accepted mid-phase (`the-typescript-tree.md` §6, #1331):** the `write!`/
`format!` probe will not reach a literal 0. `emitter/lower.rs` is the compiler's own second
code-generation pass — general expression lowering, match-to-IIFE compilation, per-builtin-type
"kernels," covering the entire Bynk expression/statement grammar — not a bounded, file-specific
conversion target the way every other phase 7 slice has been. Converting it to build `bynk-ts`
nodes natively would be re-architecting the compiler's whole lowering strategy, comprehensive
language-surface work this phase was never scoped to cover. Its output stays a `String`, carried
as opaque text at its one splice boundary into the tree, the same pattern already used for the
handful of one-off closures this phase couldn't structure narrowly. The same shape as phase 6's own
`ast_importers` floor (5, not 0, argued file by file at retirement): a probe reads a real,
non-zero, argued floor rather than an unmet 0, named explicitly rather than discovered as a
surprise at this phase's own retirement.

### Phase 8 — Incrementality

*Query granularity and the firewall; the editor path is memoised.*

`Tokens(FileId)`, `UnitSignature(UnitId)`, `Body(DefId)`, `ProjectGraph` — with `UnitSignature`
stable under any body edit. The firewall is unusually cheap here because design notes §15's
annotation policy ("visible boundaries, invisible internals") *is* a firewall specification in
ergonomic language, and ADR 0200's contract hash is already `UnitSignature`'s identity function
pointed at a different problem.

Last, because the preconditions are the previous seven phases: pure value-to-value phases (phase 0),
stable interned identity (phase 3), total side tables (phase 3), explicit cross-build state (phase
4). The scheduler — salsa, a hand-rolled memo table, or nothing — stays a separable decision.

**Invariant:** `UnitSignature` does not change when a body changes; the editor does not re-parse a
file it has not seen change.
**Probe:** keystroke-to-diagnostic latency attributed by query level.
**Reference rules:** R3.13, R3.14, R3.15.

---

## 4. Why this order

The dependency graph, stated so the order can be argued with rather than assumed.

```
0 Seams ─────────────────────────────────────────────► every later phase
   │                                                    (structural change needs a crate-local gate)
   ├─► 1 Paydown            (independent; only needs the gate)
   └─► 2 Typed lowering     (needs the gate; needs nothing else)
          │
          └─► 3 Identity & totality
                 │
                 ├─► 4 Project model as a phase
                 │      │
                 │      └─► 5 Semantics in the checker
                 │             │
                 │             └─► 6 The IR
                 │                    │
                 │                    └─► 7 TS tree & printer
                 │
                 └──────────────────────────────────► 8 Incrementality
                                                       (needs 0, 3, 4)
```

Four edges are load-bearing and worth stating:

**0 → everything.** A structural change gated only by whole-file goldens in another crate is how ADR
0198's identity bug survived 331 negative fixtures and sixty increments.

**2 before 3.** The typed hoist is mechanical and closes a live miscompile; identity is expensive and
gated. Doing the cheap correctness fix first means the compiler that carries the 1.0 promise does not
contain the miscompile, whatever happens to the rest of the trajectory.

**4 before 5.** The project model's location is the *cause* of the semantics' location. Moving
`validate.rs` before extracting `bynk-project` means moving it twice.

**5 before 6.** An IR built while hundreds of diagnostic codes still live in the emitter would have to
model the emitter's checking needs. Move the checks first and the IR only has to carry what emission
needs.

**3 and 4 before 8.** The firewall needs stable identity and a project model that is not ambient.

---

## 5. Honest sizing

Relative, not absolute — an absolute estimate here would be false precision.

| Phase | Relative size | Confidence |
|---|---|---|
| 0 Seams | 1 | high — the seam is 90% built |
| 1 Paydown | 2 | high — each slice under a day, ~9 slices |
| 2 Typed lowering | 3 | high — "mechanical across roughly ninety functions" |
| 3 Identity & totality | 8 | medium — the review killed a naive retrofit; §6's parallel-data technique is the mitigation |
| 4 Project model | 3 | medium |
| 5 Semantics | 12 → **revised, see note** | low → **medium**, per the settling review below |
| 6 The IR | 20 | low — Part 13 predicts the extension cost; phases 0–5 do not de-risk the declaration half |
| 7 TS tree & printer | 15 | low |
| 8 Incrementality | 5 | medium — the preconditions are all paid by then |

**Phase 5's row is revised, not just stale — see `design/tracks/semantics-in-the-checker.md`.** Its
settling review (§1, 8 August 2026) found phase 4's own P4.1 slice had already relocated most of what
this row's "12, low confidence, 200 diagnostic codes and a reverse edge dragging `icu.rs`" estimate
priced in — `icu.rs` and `websocket::analyse_open_shape` are already in `bynk-check`, and the live probe
reads 49/53, not 200. The track's own §1/§6 replace the estimate with a named, seven-category
decomposition (`bynk-check/src/analysis.rs`'s own accounting) across six accepted slices, most of them
small; only test/integration-suite processing (P5.4) is genuinely large, per that category's own flag
that porting it "out of proportion" as a single move. This is a routine update carried by phase 5's
settling pass, the same way `identity-and-totality.md`'s settling corrected phase 3's own sizing in
its day; the row above is left as a dash-through rather than silently overwritten so the original
estimate — and how wrong its premises turned out to be — stays legible.

**Phases 0–2 are roughly a tenth of the whole.** They are also where every high-confidence estimate
is. That is not an argument against the trajectory; it is the reason §2's stopping property matters,
and the reason phases 6 and 7 carry the reference's Part 15 cost section rather than a schedule.

---

## 6. The standing frame for reviews

The reason this document exists. Every design and implementation review in any phase's track asks
these, in order:

1. **Which phase is this in, and which reference rule does it close?** A slice that closes no rule is
   either paydown outside the trajectory — fine, say so — or scope creep.
2. **Does it move the phase invariant toward zero?** Not "is it an improvement" but "does the probe
   read closer to zero after this".
3. **Does it add a second path?** The single most reliable predictor of a regret in this codebase.
   If the answer is yes, the slice is not done, whatever else is true of it.
4. **Does it preserve the shipped surface?** §1's endpoint is the current compiler rebuilt. A slice
   that drops a construct, a protocol, a storage kind or an LSP query has left the trajectory.
5. **Does it foreclose a later phase?** A choice that is locally right and makes phase 6 harder needs
   to say so and be argued on those terms.
6. **What did the evidence say, and is it still true?** The review is v0.237.1 against a v0.245.0
   tree, and some findings have already been actioned. Re-check before assuming.

Questions 3 and 5 are the ones this document adds. Without a stated endpoint neither can be asked.

---

## 7. Tracks

One track per phase, opened in order, each with its own spine issue and slices. Phases 0–2 are small
and tightly coupled — 0 enables both, 1 is independent, 2 needs only 0 — so they share a track.

| Phase | Track | Status |
|---|---|---|
| 0, 1, 2 | *(retired)* | **Retired 3 August 2026** (spine [#996](https://github.com/accuser/bynk/issues/996), closing summary in [`archive/retired-tracks.md`](archive/retired-tracks.md)). Tier B's probe (`hoist_sinks`) reads 0; three Tier A slices carry named, separately-filed residue rather than blocking retirement |
| 3 | *(retired)* | **Retired 4 August 2026** (settled 3 August 2026; spine [#1046](https://github.com/accuser/bynk/issues/1046), closing summary in [`archive/retired-tracks.md`](archive/retired-tracks.md)). All nine slices shipped, closing R2.2, R2.4 (expressions), R3.10, R4.1, R4.2, and R4.3 in full; R2.5/R4.9 functionally but not structurally (`expr_types` stays a `HashMap`, not the `IndexVec` those rules literally name — a named open question, not silent residue). `span_keyed_maps` reads 3, down from 27 — the remainder (`Ctx::pattern_binding_types`) is a deliberate exclusion, not residue |
| 4 | *(retired)* | **Retired 8 August 2026** (settled 6 August 2026; spine [#1107](https://github.com/accuser/bynk/issues/1107), settling PR [#1108](https://github.com/accuser/bynk/pull/1108), closing summary in [`archive/retired-tracks.md`](archive/retired-tracks.md)). All three named slices shipped, closing R3.7, R3.8, R3.9 and R10.2 in full (R3.11 separately found already closed by prior paydown); contract hashing and the typed `ProjectGraph` deferred to phase 8; R10.2 closed via a new, narrow `bynk-check` analysis entry point rather than relocating `bynk-emit`'s shared discovery-and-checking orchestration, with the resulting duplication named as phase 5's to remove |
| 5 | *(retired)* | **Retired 9 August 2026** (settled 8 August 2026; spine [#1126](https://github.com/accuser/bynk/issues/1126), settling PR [#1127](https://github.com/accuser/bynk/pull/1127), closing summary in [`archive/retired-tracks.md`](archive/retired-tracks.md)). All six named slices shipped, closing R3.5 in full — the seven `bynk-check/src/analysis.rs`-named diagnostic categories plus two sites its own accounting didn't cover relocated out of `bynk-emit`, five categories closing a named, fixture-pinned editor regression, the rest for R3.5 compliance alone with no observable change; `check_function_type_boundaries`'s reach-back hook closed; R10.1 closed with a crate-doc correction, not a `bynk-driver` split; R4.6/R4.11/R10.4 reverified still closed, not reopened. `emit_diagnostics` reads 4/6 (true/naive), true=4 the named floor |
| 6 | *(retired)* | **Retired 19 August 2026** (settled 17 August 2026; spine [#1137](https://github.com/accuser/bynk/issues/1137), closing summary in [`archive/retired-tracks.md`](archive/retired-tracks.md)). `bynk-emit` gained a typed IR (`ir.rs`/`ir/lower.rs`) and every genuinely convertible declaration-read site the track's own research found moved onto it; `ast_importers` reads its re-settled floor, **5**, not the 0 first named — `bynk-emit/src/emitter{,/**}`, the TypeScript-rendering subtree phase 7's own printer inherits, argued file by file in the closing summary rather than left as an unmet criterion. `bynk-ir`/`bynk-lower` stay inside `bynk-emit` (Part 10's own crate split deferred to phase 7, per this track's own settling); R6.5's store-write data-loss defect (§1's "strongest single argument") closed structurally, not patched |
| 7 | *(retired)* | **Retired 29 August 2026** (settled 20 August 2026; spine [#1293](https://github.com/accuser/bynk/issues/1293), closing summary in [`archive/retired-tracks.md`](archive/retired-tracks.md)). A new `bynk-ts` crate (tree, printer, source map) became the single writer of every generated TypeScript character; `TsType::Any` eliminated in full bar a small, named residual on R7.7's runtime-typing work; R8.1–R8.22 closed or relocated rule by rule. Run as six arcs, far beyond the 37 slices first estimated at settling. All four gated probes retired at an argued floor, not the flat zero first proposed, the same honesty `ast_importers` (floor 5) already modelled: `ts_any` **26**, `verbatim_sites` **2**, `ts_writes` **809**, `verbatim_origins` **1** — each argued file by file in the closing summary rather than left as an unmet criterion |
| 8 | *(retired)* | **Retired 30 August 2026** (settled 29 August 2026; spine [#1507](https://github.com/accuser/bynk/issues/1507), closing summary in [`archive/retired-tracks.md`](archive/retired-tracks.md)). All six slices shipped (P8.0–P8.5); `UnitSignature` built as a new type wrapping ADR 0200's `combined_types_for` unchanged, plus fresh projections from `UnitTable`, since no existing type was already body-free enough to widen; a new shared `Tokens(FileId)`/`Ast(FileId)` cache landed in `bynk-project` since `bynk-check` cannot depend on `bynk-ide`, where completion's own cache lives; `ProjectGraph`/`Body`/`TypeOf` landed in `bynk-check`; no memo table shipped, deferring R3.15's scheduler decision whole. `incremental_query_types` retires at **satisfied**, not a floor — the first probe on this trajectory shaped as a proof rather than a shrinking count; `keystroke_latency` stays trend-only, "not measured," per this phase's own settled scope. **This is the trajectory's last phase — its retirement reaches this document's own §1 endpoint; see the status note above** |

A phase's track opens when the previous phase's probe reads zero. Not before — an open track for a
phase whose prerequisite is unmet is how a trajectory becomes a wish list. Phase 6 was the first
exception on record, and only because it argued one, in the open, before phase 7 opened rather than
after: `the-ir.md`'s own retirement re-settled `ast_importers` at a named floor of 5, not 0, tracing
each surviving file's own structural reason (its closing summary in `archive/retired-tracks.md` has
the argument in full) — a literal zero was never reachable within phase 6's own scope, per that
argument, so gating phase 7's own opening on it would have been the "refusal with no trigger" the
next paragraph's own discipline exists to prevent, not caution. **Phase 7 is the second** —
`the-typescript-tree.md`'s own retirement named argued floors for all four of its gated probes
(`ts_any` 26, `verbatim_sites` 2, `ts_writes` 809, `verbatim_origins` 1), each traced file by file
in its own closing summary, the identical discipline applied a second time rather than assumed to
transfer automatically.

**The track already reflects this.** Its own §7 records the IR, the printer and the crate re-graph as
forward references with entry conditions, not as *refusals with triggers* — recording an intention as
a refusal is the inverse of the failure R15.1 names. The entry conditions live in that same §7,
alongside the forward references they gate, rather than in a separate section.

---

## 8. What could stop it, and what that would mean

**The language starts moving again.** The strongest argument against the trajectory has always been
that the shipped architecture is what let the language cover 304 decision records in a year, because
text is cheap to change and an IR node is a five-place edit. That argument weakens as the language
settles and strengthens the moment it does not. *If it happens:* stop at the current phase boundary.
Phases 0–5 impose almost no representational tax on a moving language; phase 6 imposes the most.

**A phase's estimate is wrong by a large factor.** Phases 5–7 carry low confidence and are each
larger than everything before them. *If it happens:* the phase boundary is the stopping point, and
the trajectory's value is what has already landed, not what remains.

**Reviewer bandwidth does not materialise.** This is one person's attention, and the collaboration
payoff that motivates the trajectory arrives mostly at phase 5. *If it happens:* phases 0–2 still
close a live miscompile and three security gates, which is a good outcome for a small investment.

In all three cases the answer is the same, and it is §2: **stop at a boundary.** The trajectory is
designed so that is always available.

---

## 9. Keeping it honest

**Probes, not prose.** Each phase's invariant is a number that CI can compute. A phase is complete
when its number is zero, not when it feels done.

**`cargo xtask greenfield-status`** — built (track slice T0.0, `xtask/src/greenfield_status.rs`),
regenerating the committed `design/greenfield-status.md` from thirteen gated probes plus four
trend-only ones, and failing CI (`greenfield_status_table_is_current`) when that table goes stale.
It does not yet report the current phase or the distance to its own invariant the way this
paragraph originally specified — that reporting is still open, not the command itself.

**The evidence ages.** The review is v0.237.1; the workspace is v0.245.0; `bynk-ide` has already been
demoted to a dev-dependency of `bynkc` citing finding #41. Every slice re-checks its finding. A
trajectory built on a snapshot is a trajectory toward where the codebase used to be.

**This document is re-settled, not edited in passing.** A change to the endpoint, the phase order, or
a phase's invariant is a reviewed PR against this file, the way a track's re-settling pass works. A
phase completing is a routine update that rides the implementing PR.
