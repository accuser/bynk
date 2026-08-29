# Incrementality — query granularity and the firewall (phase 8)

- **Status:** **Settled — Slicing on merge.** §3's five questions were argued under a settling
  review on this branch. None reversed the draft's own working candidates, but three (Q1, Q2, Q4)
  each turned on a concrete fact the draft hadn't yet checked, tightening the decision rather than
  complicating it — see the provenance note at the head of §3. Merging settles **direction**; it
  is not a build authorisation. Each slice is still an ordinary increment proposal, and `accepted`
  on that sub-issue is the approval to build.
- **Spine:** [#1507](https://github.com/accuser/bynk/issues/1507).
- **Theme:** **Phase 8** of [`../bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md) —
  the last phase. Every compiler output is decomposed to the granularity at which it is
  *invalidated* — `Tokens(FileId)`, `Ast(FileId)` at file level; `UnitSignature(UnitId)` at unit
  level, declarations only, no bodies; `Body(DefId)`/`TypeOf(DefId)` at definition level;
  `ProjectGraph` at project level — with `UnitSignature` proved stable under any edit to a body
  inside that unit (the firewall). The scheduler that would actually memoise these queries — salsa,
  a hand-rolled table, or nothing at all — is a separable decision this phase commits the
  *granularity* for and explicitly defers (settled: **nothing at all**, this phase — Q3). The
  trajectory's endpoint is the current compiler rebuilt on
  [`../bynk-greenfield-compiler.md`](../bynk-greenfield-compiler.md); this track walks the last
  phase of it, and its retirement is the trajectory's own.
- **Phase boundaries are safe stopping points** (trajectory §2). Phase 7 is retired
  ([#1293](https://github.com/accuser/bynk/issues/1293), retired 29 August 2026) and leaves a
  coherent compiler regardless of whether this phase lands.
- **Relates:** reference rules R3.13, R3.14, R3.15 in full (`bynk-greenfield-compiler.md` Part 3,
  lines 454–495), and the precondition table immediately above them (lines 440–449) — six
  preconditions for demand-driven recomputation, each already claimed closed by a named prior
  phase (P1 purity — phase 2; P3/R2.3 no ambient state — phase 0; R2.4 stable interned identity —
  phase 3; R2.5 total side tables — phase 3; R3.11 explicit cross-build state — phase 4; R3.3
  serialisable/comparable outputs — assumed, not separately tracked by any phase). **Q2 found R2.4
  more partial than this list assumed** — see §3.2. Part 15.1's "demand-driven query framework"
  entry (lines 2195–2200) is this phase's own standing frame: "the **architecture is adopted**...
  the **framework is deferred**," triggered separately by "a hand-rolled memo table... measurably
  the bottleneck" — a trigger this phase's own Q3 decision confirms cannot fire yet.
- **Precedent:** `the-typescript-tree.md` (retired) is this track's direct predecessor on the same
  trajectory. Its own §10 ("What this phase causes") named this phase's entry condition explicitly
  — "needs phases 3 and 4 **together with** this one, since a query firewall over an emitter that
  still writes strings ad hoc has no stable unit to memoize against" — and flagged, without
  deciding, that its own `Artefacts` type (R7.8) is "plausibly phase 8's `UnitSignature` target for
  the emit side." Its closing summary (`../archive/retired-tracks.md`) repeats the same open note
  and adds a second one: R8.16's own PARTIAL finding ([ADR 0388](../decisions/0388-typescript-tree-r8-scope.md))
  left "R8.16's data-model half" — a typed `ProjectGraph` — as phase 8's, matching
  [ADR 0326](../decisions/0326-project-model-phase4-scope.md)'s own deferral from phase 4: "extract
  today's name-keyed shape; the typed `ProjectGraph`/`UnitId`/`ContractHash` defer to phase 8."
  **Both notes are settled by Q1, below: this phase's `UnitSignature` covers the check side only;
  `Artefacts` gets no signature concept of its own unless an emit-side query is actually proposed
  later.** `incrementality.md` is the thirteenth track to run the ADR 0167 flow from the start,
  after `compiler-architecture.md` (sixth), `identity-and-totality.md` (seventh),
  `content-ownership.md` (eighth), `project-model.md` (ninth), `semantics-in-the-checker.md`
  (tenth), `the-ir.md` (eleventh) and `the-typescript-tree.md` (twelfth).

### ADR 0076 trigger check

| Trigger | Met? |
|---|---|
| Spans several increments | **Yes, on the smallest margin on this trajectory, and settled at that margin.** §6's settled slice count is **6** (P8.0–P8.5) against the trajectory's own row-5, medium-confidence sizing — the first phase on this trajectory whose real surface does not need revising upward once actually measured (phase 5, 6 and 7 each did) |
| Surface not yet settled | **No, and now settled here.** R3.13–R3.15 fully specify the destination; §3 closes the migration method for all five questions — whether `UnitSignature` extends or wraps `combined_types_for` (Q1), whether the file level shares one cache or two (Q2), whether a memo table ships here (Q3, decided: no), what `UnitSignature` actually contains and how its stability is proved (Q4), and what the gated probe measures (Q5) |
| Security/safety boundary | **No** — this phase's invariant (`UnitSignature` stability, no stale re-parse) is a performance and architecture property. A defect here produces wrong or slow analysis, not an authorization or capability failure |

**One of three**, the same count as every internal-architecture track on this trajectory before it.

---

## 1. Why now

**Phase 7's retirement is the procedural trigger, and its own closing note is the substantive
one.** The trajectory's own rule — "a phase's track opens when the previous phase's probe reads
zero" — is satisfied: all four of phase 7's gated probes (`ts_writes` 809, `ts_any` 26,
`verbatim_origins` 1, `verbatim_sites` 2) retired at argued floors on 29 August 2026. But
`the-typescript-tree.md`'s own §10 went further, naming this phase's real precondition as phases 3
**and** 4 *together with* phase 7 — not the trajectory's own §4 diagram, which shows only "3 and 4
before 8" and no edge from 7. That gap is real, not decorative: `Artefacts` (R7.8, phase 7's typed
document set) was the only candidate for what an incremental **emit** side would key on — **Q1
settles this phase does not need it**, since this phase's own scope is the check side, per design
notes §15's annotation policy being a check-side (not emit-side) contract.

**The rationale bugs R3.13 cites are half-closed, and the surviving half is the one the probe is
literally named for.** R3.13's own rationale cites two bugs: #65 ("every IDE query re-lexes and
re-parses from `&str`") and #62 ("`for_each_unit` never filters the cursor's own path out of
`files`, so the current file is parsed twice per keystroke"). Both are closed for **completion**
— issue [#733](https://github.com/accuser/bynk/issues/733) is closed, and
`bynk-ide/src/completion.rs`'s `for_each_unit` (`:1578`) now takes a caller-built `files` overlay
rather than re-sweeping disk, backed by a content-keyed cache, `PROJECT_UNIT_CACHE` (`:1520`, cap
4096 at `:1530`), through `cached_project_unit` (`:1537–1561`). **But the diagnostics path is
untouched.** `bynk-ide::diagnose_project_with` (`bynk-ide/src/lib.rs:299`) calls
`bynk_check::analysis::analyse_project` (`bynk-check/src/analysis.rs:262`) unconditionally on
every invocation — full discovery, parse, resolve and check over the whole project, with no cache
of any kind below the file-overlay level. **Settling found this is not just a missing patch —
it's structurally unreachable as one:** `bynk-check`'s own `Cargo.toml` cannot depend on
`bynk-ide` (the crate graph runs the other way — `bynk-project` ← `bynk-check` ← `bynk-ide`,
confirmed fresh against both crates' manifests), so `PROJECT_UNIT_CACHE` cannot be reached from
`analyse_project`'s own `phase_parse` call, however it's patched. See Q2.

**None of R3.13's four query levels have their *query* types built anywhere in the workspace — but
one level's *key* type already exists, just not durably.** A grep for the four query levels'
names — `UnitSignature`, `ProjectGraph`, `TypeOf(`, `Body(`, `Tokens(FileId` — as code, not doc
prose, across every crate returns zero hits, confirming the draft's own claim. But `FileId`
(`bynk-syntax/src/span.rs:16`, `pub struct FileId(pub u32)`) already exists, built at phase 3
(T3.5, R2.4, "one counter, threaded from the per-project parse loop... the same shape T3.4 used
for `ExprId`") specifically to give a `Span` a stable, comparable file identity. **Settling found
a real limit to this, worth naming precisely: the counter is a local `let mut next_file_id: u32 =
0` inside `bynk_check::project_model::phase_parse`, reset to zero on every call** — real,
deterministic *within* one project analysis (its stated R2.2 purpose — attributing a label to the
right file inside one build), not *interned* durably *across* calls the way a cache key needs to
be. Phase 3's own R2.4 precondition ("stable interned identity usable as a key") is therefore only
half-true for the file level today: the identifier concept exists, a persistent path↔`FileId`
table across calls does not — real, additional work for Q2's own slice (P8.4), not a freebie
inherited from phase 3. `bynk-project` (2,458 lines across nine files — `discovery.rs` 616,
`paths.rs` 623, `graph.rs` 331, `roots.rs` 319, `schema_registry.rs` 220, `consistency.rs` 183,
`json.rs` 94, `lib.rs` 52, `diagnostics.rs` 20) is the crate this phase's `ProjectGraph` sits in,
per phase 4's own placement — but its closest types, `graph.rs`'s and `roots.rs`'s own, are
untyped and name-keyed (confirmed fresh: `detect_consumes_cycles` and its callers key every map by
plain `String`, not a `UnitId`), exactly as ADR 0326 described them at phase 4's retirement.

**One piece of the firewall already exists, and settling found it's narrower than the draft even
suspected — narrow enough that no existing type in the workspace is close to signature-shaped.**
ADR 0200's cross-context contract hash — the piece R3.14's own rationale cites as "the query
already exists in substance" — is real: `combined_types_for` (`bynk-check/src/symbols.rs:1147`,
22 lines) folds a unit's own declared types with its direct `uses` targets' types into one
`HashMap<String, Arc<TypeDecl>>`, called from **7 real call sites across 2 crates** (confirmed
fresh: `bynk-check/src/symbols.rs:862`, `analysis.rs:666`, `check_pipeline.rs:284`;
`bynk-emit/src/project.rs:927,2227,2402,2428` — a plain grep turns up 24 textual hits, but 17 are
the definition, imports and doc comments, including `bynkc/tests/contract_hash.rs:16`, which names
the function in a doc comment but never calls it). It computes **types only** — one of design
notes §15's four required-annotation categories (cross-context type references), and none of the
other three (function/handler declarations, agent storage declarations, capability sets via
`given`). **Settling checked whether the natural next-broadest thing — `UnitTable`
(`bynk-check/src/symbols.rs:295`), the per-unit table `combined_types_for` itself reads —
already carries those other three categories in a body-free shape, and found it does not:**
`UnitTable.fns: HashMap<String, Arc<FnDecl>>` and every `Handler` inside `UnitTable.agents`/
`UnitTable.services` both carry a full `body: Block` (`bynk-syntax/src/ast.rs:2005,1208`)
alongside their signatures — editing a function or handler body changes the `FnDecl`/`Handler`
value itself, not just something adjacent to it. `UnitTable`'s own `store_fields`
(`StoreField`, `ast.rs:945`) is closer — `name`/`kind: StoreKind` are body-free, but `init:
Option<Expr>` (the field's initialiser) is not. **No existing type in the workspace is already
`UnitSignature`-shaped; every one that's close (`UnitTable`'s own decl types) carries bodies
`UnitSignature` must strip.** One further, direct in-repo precedent bears on Q1 itself:
`combined_types_for_unit_info` (`symbols.rs:1170`), a sibling function reimplemented against
`UnitInfo` rather than calling `combined_types_for` directly, exists *because* the two call
contexts have genuinely different shapes (project-wide flat maps vs. a per-unit emission
prologue) — a real, contemporaneous example in the same file of "build alongside, don't widen"
when a consumer's shape differs enough. See Q1.

**No memoisation infrastructure of any kind exists below the file-overlay layer.** Zero
occurrences of `salsa` in any `Cargo.toml` or `Cargo.lock` in the repository. Zero occurrences of
`Database`, `MemoTable` or `memo_table` as identifiers. This is exactly R3.15's own stated
posture — "the architecture is adopted, the framework is deferred" — confirmed true today, not
merely claimed: there is nothing to migrate away from, no accreted hand-rolled cache to reconcile
with a real query decomposition, no framework dependency to argue about removing. **Settled (Q3):
this phase adds none either** — the granularity and the firewall are this phase's whole business.

**The probe as literally named cannot be measured — not "not yet measured," structurally cannot
be, until the query levels it attributes latency to exist.** §3.0's baseline records "not
measured"; it still reads that way, because no `criterion` harness, no `Instant`/`Duration`
instrumentation and no benchmark of any kind touches the diagnostic path anywhere in the repo
today. Every other phase on this trajectory found its own gated probe needed correcting once real
work started (`ast_importers`'s floor of 5, four separate argued floors at phase 7's retirement);
this is the first phase where the correction is visible *before* any slice lands, closed here as
Q5, rather than discovered mid-track.

**One correction to the trajectory document itself, the same "evidence ages" housekeeping every
phase's opening has carried.** §9 describes `cargo xtask greenfield-status` as "specified, not
yet built (track slice T0.0)." It has existed since phase 0–2's own track (`xtask/src/greenfield_status.rs`,
3,419 lines, `design/greenfield-status.md` the committed, CI-gated output) and now runs thirteen
gated probes plus four trend-only probes. This phase's probes (§5) are the fourteenth gated and
fifth trend-only probes — following `ts_writes`/`ts_any`'s own precedent of landing as an
early, dedicated slice (P7.0/[#1296](https://github.com/accuser/bynk/issues/1296)), not a
retirement-week afterthought.

---

## 2. What this track is not

- **Not adopting salsa, and not building a hand-rolled memo table either — settled as one
  decision, not two (Q3).** R3.15 frames "salsa, a hand-rolled table, or nothing at all" as three
  options; this phase picks the third. Neither branch's own trigger has fired: salsa's trigger is
  a hand-rolled table measurably becoming the bottleneck, and a hand-rolled table has nothing to
  be measured against until the granularity this phase builds exists. This track's business is
  the granularity (R3.13) and the firewall (R3.14); a scheduler of either kind stays a separate,
  later decision with its own evidence requirement (keystroke-to-diagnostic latency on a
  multi-context project, attributed by level — data this track's own work is a precondition for
  collecting, not a deliverable of it).
- **Not rebuilding `bynk-project`'s discovery machinery wholesale.** `discovery.rs` and `paths.rs`
  (616 and 623 lines) stay; this track's business is giving their output a typed, query-shaped
  identity (`ProjectGraph`), not re-deriving what they discover.
- **Not a further crate re-graph.** R10.5 (`bynk-driver` consolidation) is named in the reference
  but has no trigger; this phase's crate surface is confined to `bynk-project` and `bynk-check`,
  the two crates R3.13's four query levels actually live in.
- **Not a new LSP-facing feature.** `editor-currency.md`'s surface — hover, completion, scaffolds,
  codelens — is untouched; this track's business is what those features call underneath, not what
  they show.
- **Not re-opening R8.16's per-consumer surface generation.** ADR 0388 split R8.16 down the
  middle: the generation logic itself (already correct, per `the-typescript-tree.md`'s own
  verification) stays phase 7's closed business; only its data-model half — a typed `ProjectGraph`
  to build it from, instead of an untyped `HashMap` — is this track's.
- **Not building `Artefacts` (R7.8) its own signature concept.** Settled by Q1: this phase's
  `UnitSignature` is check-side only. If an emit-side query is ever proposed, it is a new,
  separate decision with its own trigger — not inherited from this phase's own scope.

---

## 3. Design questions — settled

> **Provenance: Q1, Q2 and Q4 each turned during this settling pass on one concrete fact the
> draft hadn't yet checked — in every case the fact narrowed the option space, it didn't reverse
> the draft's own leaning.**
>
> Q1 leaned toward weighing in-place extension as "materially cheaper" against 7 call sites; a
> direct read of what `combined_types_for` actually returns (types only) against what
> `UnitTable`'s own decl types actually carry (signatures, but never without a body) found no
> existing type is close enough to widen — the real choice was never "widen vs. build," it was
> "build fresh, reusing what already reuses cleanly."
>
> Q2 leaned toward "share completion's existing cache" reading like the cheap option; a fresh
> check of `bynk-check`'s and `bynk-ide`'s own `Cargo.toml` dependency edges found that option is
> not available at all — `bynk-check` cannot depend on `bynk-ide`. The real choice was always
> "build a new shared cache one layer down," just not visible as *the only* option until checked.
>
> Q4's field list is close to the draft's own working list (design notes §15's four categories),
> sharpened by finding exactly which struct fields on `FnDecl`/`Handler`/`StoreField` carry a body
> or an initialiser and must be excluded, rather than left as "audit needed."
>
> Q3 and Q5 settled exactly as the draft's own working candidates framed them, once Q1/Q2/Q4 gave
> them something concrete to settle against.

### 3.1 Q1 — Is `UnitSignature` built by extending ADR 0200's `combined_types_for` in place, or as a new, parallel type? **Settled.**

**Decision: a new type, `UnitSignature`, that reuses `combined_types_for`'s existing output
unchanged as one field and adds new fields built fresh from `UnitTable`. `combined_types_for`
itself is not touched — its 7 call sites, and its contract for them, are untouched.**

`combined_types_for` returns `HashMap<String, Arc<TypeDecl>>` — one of design notes §15's four
required-annotation categories (cross-context type references), and structurally incapable of
carrying the other three (it never reads `UnitTable.fns`/`.agents`/`.services`/`.capabilities` at
all). Widening its *return type* to also carry function/handler signatures, storage declarations
and capability sets would not be "widening a narrow function" — it would be replacing its purpose
entirely, while its 7 real callers (confirmed fresh: 3 in `bynk-check`, 4 in `bynk-emit` — the
draft's own count already corrected a stale 24-textual-hit grep down to this figure and settling
reconfirmed it against the current tree) still want exactly its current, narrow, types-only
shape for cross-context resolution and the contract hash. Building `UnitSignature` as a superset
*type* that **contains** `combined_types_for`'s own output as one field, unchanged, sidesteps
both risks the draft weighed: the 7 call sites never see a changed function, and `UnitSignature`
does not duplicate the types table by hand — it is that table plus new fields, not two competing
copies of it (satisfying phase 1's own "no fact in two hand-synced copies" invariant by
construction, not by discipline).

The new fields, built directly from `UnitTable` (`bynk-check/src/symbols.rs:295`) and its decl
types, not reinvented:

- **Function/handler declarations** — from `UnitTable.fns` (`FnDecl`: `name`, `type_params`,
  `params`, `return_type`, `has_self` — explicitly *not* `body`, and explicitly *not*
  `requires`/`ensures`; see Q4) and from every `Handler` reachable via `UnitTable.agents[*]` and
  `.services[*]` (`method_name`, `params`, `return_type`, `given` — not `body`).
- **Capability sets via `given`** — the same `Handler.given: Vec<CapRef>` fields above, plus
  `ProviderDecl.given` and `ServiceDecl.default_given`, plus `UnitTable.exported_capabilities:
  HashSet<String>` copied as-is (already just names).
- **Agent storage declarations** — from `UnitTable.agents[*].store_fields` (`StoreField`:
  `name`, `kind: StoreKind` only — explicitly *not* `init: Option<Expr>`, an initialiser
  expression, and not `annotations`, which govern internal storage behaviour, not the field's
  externally-relevant shape).
- **Cross-context type references** — `combined_types_for`'s own output, unmodified.

`Artefacts` (phase 7's typed emit-side document set, R7.8) gets no signature concept of its own
in this phase — the annotation policy this phase's firewall is built on (design notes §15) is a
check-side contract, and nothing in this phase's own scope proposes an emit-side query to key
against it. If one is proposed later, it's that proposal's decision to make, not inherited here.

### 3.2 Q2 — Does the file level need any work at all, or is the diagnostics path's missing cache this track's whole file-level business? **Settled.**

**Decision: build one real, shared `Tokens(FileId)`/`Ast(FileId)` cache in `bynk-project`,
migrating both `bynk-ide::completion` and `bynk_check::analyse_project`'s own `phase_parse` onto
it — not a `bynk-ide`-local patch, because that option does not exist in this codebase's crate
graph.**

`PROJECT_UNIT_CACHE` (`bynk-ide/src/completion.rs:1520`) already closes #733 for completion, and
is a real, working, content-keyed cache — but it lives in `bynk-ide`, and `bynk-ide`'s own
`Cargo.toml` depends on `bynk-check` (`bynk-check.workspace = true`, "`analyse_project`... lives
here too"), not the reverse. `analyse_project` and its `phase_parse` step live in `bynk-check`,
which structurally **cannot** reach a `bynk-ide`-owned cache — not a design preference, a
dependency-direction fact, confirmed against both crates' manifests during settling. So "give the
diagnostics path completion's existing cache" was never actually one of the draft's three
options; the real choice is between building a new shared cache in a crate both paths can reach,
or leaving the diagnostics path uncached and filing the gap as separate paydown outside this
track. Given R3.13 names `Tokens(FileId)`/`Ast(FileId)` as real query types, not an incidental
optimisation, and `bynk-project` already sits below both `bynk-check` and `bynk-ide` (and is
where `ProjectGraph` is going per Q1/§6's own P8.3), that crate is the natural, and only
structurally reachable, shared home.

**One real dependency this decision surfaces, not free:** as §1 found, `FileId` exists but is
reallocated by a local counter reset to zero on every `phase_parse` call — not durable across
calls. A `Tokens(FileId)` cache keyed on today's `FileId` would collide or thrash across separate
requests. P8.4 (§6) therefore also builds the missing piece: a persistent path↔`FileId`
interning table living in `bynk-project` alongside discovery, so a given file keeps the same
`FileId` across every call for the life of one analysis session — not only within a single
`phase_parse` invocation, as today. Once that exists, `bynk-ide::completion`'s own
`cached_project_unit` and `bynk_check::analyse_project`'s `phase_parse` both read through the same
`bynk-project`-owned cache; `PROJECT_UNIT_CACHE` retires as a `bynk-ide`-local duplicate rather
than staying a second, divergent cache beside the new one.

### 3.3 Q3 — Is a hand-rolled memo table this track's own deliverable, or does the track stop once the query keys and the firewall exist? **Settled.**

**Decision: the track stops at the query decomposition and the firewall proof. No memo table
ships in this phase.**

R3.15's own rationale is explicit and is taken at face value: "the hard part is R3.13/R3.14. Once
queries have the right keys and the firewall holds, a scheduler is a few hundred lines." The
draft's own worry — that an un-memoised decomposition leaves the probe unmeasurable — is resolved
differently than by building a scheduler to satisfy it: Q5 settles the gated probe as a
structural existence-and-proof check (does `UnitSignature`/`ProjectGraph`/`Body`/`TypeOf` exist as
real types, proved stable), not a latency number, so nothing in this phase's own completion
criterion actually requires a working cache behind the new query types. Building one anyway would
take on real, separate risk this phase doesn't need: cache-invalidation correctness under
concurrent IDE writes is a materially different, harder problem than proving a type's shape is
stable under a body edit, and R3.15's own text frames it as a decision with its own trigger
("measurably the bottleneck") that structurally cannot fire before the granularity exists to be a
bottleneck in. Keeping the memo table an explicit forward reference (§7), on the same footing as
salsa itself, keeps this phase's risk bounded to what R3.13/R3.14 actually require.

### 3.4 Q4 — What does `UnitSignature` actually need to contain, and what proves R3.14 holds? **Settled.**

**Decision: the field list is Q1's own four groups above, each audited field-by-field against the
real `FnDecl`/`Handler`/`StoreField`/`ProviderDecl`/`ServiceDecl` shapes to exclude every body or
body-adjacent field; stability is proved by a required, purpose-built property fixture — edit a
body only, rebuild, assert the signature is unchanged — not by reuse of `differential_analysis.rs`
as-is.**

Design notes §15's four categories map cleanly onto real AST shapes already in the tree (Q1 lists
the exact fields). Two exclusions are worth naming explicitly because they were the ones most
tempting to include: `FnDecl.requires`/`.ensures` (preconditions/postconditions, `ast.rs:2001,2004`)
are predicates scoped over parameters and the result — architecturally close to a signature, but
excluded here, because they aren't part of design notes §15's own required-annotation list and a
caller's own type-checking of a call site never needs them (only `Body`/`TypeOf`-level analysis of
the callee's own contract-checking does) — including them would make `UnitSignature` unstable
under edits that don't change what a caller needs, defeating R3.14 rather than serving it.
`StoreField.annotations` (`@indexed`/`@ttl`/etc., `ast.rs:955`) are excluded on the same
"invisible internals" reasoning — they govern the field's own storage behaviour, not its
externally-relevant shape, and design notes §15's own "visible boundaries, invisible internals"
line is the reasoning being applied, not a new one being invented for this decision.

**On the proof:** phase 4's `differential_analysis.rs` (`bynk-check/tests/`) is the closest prior
technique on this trajectory, but settling checked its actual shape rather than assume the fit —
it compares two **analysis paths'** output for parity, not one type's stability under an edit. It
is precedent for "prove an architectural invariant mechanically, not by inspection" as a house
discipline, not a template to copy verbatim. R3.14's own literal text ("stable under an edit to
any `Body(DefId)` within that unit") needs its own, new fixture shape: build `UnitSignature` for a
fixture unit, edit only a handler or function **body** (never a parameter, return type, storage
field or `given` clause) in the source, rebuild, and assert equality — a required part of P8.2
(§6), not an optional strengthening, since R3.14 is this phase's own named firewall and its own
§9 risk section already treats getting this proof right as the track's central stake.

### 3.5 Q5 — What does the gated probe actually measure, given the literal one can't be? **Settled.**

**Decision: a new gated probe, `incremental_query_types` — a one-time existence-and-proof check,
not a shrinking count — plus a trend-only `keystroke_latency` probe reporting "not measured" until
a scheduler exists to time it.**

`incremental_query_types` reads **satisfied** once, and only once: `UnitSignature`, `ProjectGraph`
and the `Body`/`TypeOf` query functions exist as real Rust types/functions in `bynk-project`/
`bynk-check` (not doc prose — the same "grep for the real identifier" standard §1 already
applied), the shared `Tokens(FileId)`/`Ast(FileId)` cache (Q2) is wired into both `bynk-ide`'s
completion path and `bynk_check::analyse_project`, and P8.2's property fixture (Q4) passes,
proving `UnitSignature` stability under a body edit mechanically rather than by inspection. This
is the first gated probe on this trajectory whose target is a proof, not a count trending to zero
or an argued floor — R3.13/R3.14 describe a property to construct, not a defect to exhaust, so a
shrinking-count probe would be the wrong shape regardless of how it's tuned. `keystroke_latency`
follows `test_density`'s own precedent (`xtask/src/greenfield_status.rs`'s `run_trend`) for a
probe that's reported, never gated — it stays "not measured" through this phase's own retirement,
per Q3's decision that no scheduler ships here to produce a real number.

---

## 4. Posture

Extends ADR 0059 as amended by ADR 0309, the same standing properties every track on this
trajectory has run under: behaviour-preserving by default (a slice that changes observable
behaviour states so, ships a fixture, gets a CHANGELOG entry — P8.4's shared file-level cache is
the one slice here with real behavioural stakes, since a wrong cache invalidation would produce
stale diagnostics silently); short-lived branches, one PR per slice; no per-increment ADRs beyond
what §11 front-loads; every slice citing `Closes-Rule:`.

---

## 5. The completion criterion

Two probes, as settled by Q5:

- **`incremental_query_types`** — a one-time existence-and-proof gate: do `UnitSignature`,
  `ProjectGraph`, `Body`/`TypeOf` exist as real types/functions in `bynk-project`/`bynk-check`; is
  the shared `Tokens(FileId)`/`Ast(FileId)` cache wired into both completion and diagnostics; does
  P8.2's property fixture pass, proving `UnitSignature` stability under a body edit. Retires at
  **satisfied**, not 0 or a floor — the first probe on this trajectory shaped as a proof rather
  than a shrinking count.
- **`keystroke_latency`** (trend-only, not gated) — reported as "not measured" through this
  phase's own retirement, per Q3: no scheduler ships in this phase to produce a real number.

Both need adding to `xtask/src/greenfield_status.rs` (13 gated + 4 trend probes exist today) —
P8.0, this track's own first slice, mirroring `the-ir.md`'s P6.0 and `the-typescript-tree.md`'s
P7.0 both being real instrumentation work, not ceremony.

---

## 6. Slice decomposition

§3 is settled; all six slices below are accepted, buildable, not forward references.

| Slice | What it does | Rules | Gated on |
|---|---|---|---|
| **P8.0** | `incremental_query_types` (gated) and `keystroke_latency` (trend) probes added to `xtask/src/greenfield_status.rs`, per §5 | instrumentation | — |
| **P8.1** | `UnitId` and `UnitSignature` built in `bynk-check` — `combined_types_for`'s existing output reused unchanged as one field, plus new fn/handler/storage/capability-set projections read fresh from `UnitTable`, per Q1/Q4's exact field list | R3.14 | — |
| **P8.2** | Property fixture proving `UnitSignature` stability under a body-only edit, per Q4's own new fixture shape (not `differential_analysis.rs` reused as-is) | R3.14 | P8.1 |
| **P8.3** | Typed `ProjectGraph` in `bynk-project` (keyed by `UnitId` from P8.1), replacing `graph.rs`/`roots.rs`'s untyped, name-keyed shape (ADR 0326/ADR 0388's own deferral) | R3.13, R8.16 (data-model half) | P8.1 |
| **P8.4** | A durable path↔`FileId` interning table plus a shared `Tokens(FileId)`/`Ast(FileId)` cache in `bynk-project`; `bynk-ide::completion` and `bynk_check::analyse_project`'s `phase_parse` both migrate onto it; `PROJECT_UNIT_CACHE` retires as a duplicate | R3.13 | — |
| **P8.5** | `Body(DefId)`/`TypeOf(DefId)` as real, pure, `DefId`-keyed query functions, decomposed out of `analyse_project`'s current monolithic per-file pass — no memo table wrapping them yet, per Q3 | R3.13 | P8.1 |

**Settled slice count: 6.** Tightened from the draft's own provisional 6–9 range once Q3 ruled out
P8.6 (a hand-rolled memo table) entirely rather than leaving it conditional — matching the
trajectory's own row-5, medium-confidence sizing almost exactly, the first phase on this
trajectory whose settled count doesn't need the sizing revised upward the way phase 5, 6 and 7 all
did.

---

## 7. Out of scope — forward references, not refusals

| Item | Phase | Entry condition |
|---|---|---|
| A demand-driven query framework (salsa or a hand-rolled memo table) | *unopened — no trigger yet* | R3.15's own trigger: a hand-rolled memo table measurably becomes the bottleneck — cannot fire before this phase's own granularity (P8.1–P8.5) exists to be a bottleneck in |
| R10.5's `bynk-driver` consolidation | *unopened — no trigger yet* | named in the reference (Part 10) but not this phase's own invariant |
| A lossless CST (rowan) | *unopened — no trigger yet* | Part 15.1's own trigger: per-file reparse timings on the largest real file measured as costly — P8.4's own interning table is the first place such a timing could be collected, but collecting it is not this phase's job |
| An emit-side `UnitSignature`-equivalent keyed on `Artefacts` (R7.8) | *unopened — no trigger yet* | Q1's own settled scope: this phase's firewall covers the check side only; an emit-side query would need its own proposal and its own trigger |

---

## 8. Keeping the reference true

This phase's own probes needed building from nothing — `xtask/src/greenfield_status.rs` has
thirteen gated and four trend probes today, none phase-8-shaped. P8.0 is this track's own first
slice for exactly that reason, the same "instrumentation first" precedent `the-ir.md`'s P6.0 and
`the-typescript-tree.md`'s P7.0 both set.

One correction this track's settling pass carries, the same "evidence ages" housekeeping every
prior phase's opening has done: §9's claim that `cargo xtask greenfield-status` is "specified, not
yet built" is stale — it has existed since the phase 0–2 track and gates thirteen probes today.

---

## 9. Risks

**P8.1's `UnitSignature` field list is a settled decision, not a discovered one — the residual
risk is in the fixture, not the design.** Q4 excluded `requires`/`ensures` and `StoreField`
annotations on stated reasoning (caller-irrelevant, "invisible internals"); if a future slice
finds a real caller-visible dependency on either, that is new evidence against this settling's own
reasoning, worth a fresh look rather than a silent field addition.

**P8.4 is the one slice with real behavioural stakes.** A wrong invalidation rule in the shared
`Tokens(FileId)`/`Ast(FileId)` cache would produce stale diagnostics silently — the same failure
class #733 fixed for completion, reopened at a different layer if the new interning table's
content-equality check has a gap. Worth a dedicated staleness fixture (edit a file, assert the
cache actually invalidates), not just a byte-golden pass, per this trajectory's own repeated
"tests must prove the specific claim" lesson (`the-typescript-tree.md`'s own P7.3/P7.6 findings).

**The evidence ages.** Every fact, line number and count in this doc was measured against `main`
on 29 August 2026, several (the crate-dependency direction, `FileId`'s own allocation, `UnitTable`'s
field shapes) refreshed again during this settling pass on the same date. Re-check before a slice
proposal cites one, per every prior track's own §9.

---

## 10. What this phase causes

**This is the trajectory's last phase.** Its retirement is not a forward reference to a ninth
phase — trajectory §1's endpoint ("the compiler Bynk ships today, feature for feature, rebuilt on
the architecture in `bynk-greenfield-compiler.md`") is reached when this phase's probe (§5) is
satisfied, not before. What this phase does *not* close, named so a future reader does not mistake
silence for completeness: R3.15's scheduler decision (§7, its own separate trigger, settled here as
*not this phase's business* — Q3), R10.5's `bynk-driver` consolidation (§7, no trigger), rowan's
lossless-CST question (§7, its own trigger against real reparse timings P8.4 could — but is not
required to — start collecting), and an emit-side signature concept for `Artefacts` (§7, Q1's own
settled scope boundary).

---

## 11. ADRs

Per ADR 0167 step 2, load-bearing, hard-to-reverse decisions land before slicing (numbers assigned
at merge by the stamp, referred to by letter until then — see
`design/pending/incrementality-settling.md` for the full text):

- **ADR-A** — `UnitSignature` is a new type wrapping `combined_types_for`'s existing output
  unchanged, plus new fn/handler/storage/capability-set projections read fresh from `UnitTable`
  (§3.1, Q1). The most load-bearing of the set — it fixes the shape every later slice (P8.2–P8.5)
  builds against, and settles that `Artefacts` (phase 7) gets no signature concept in this phase.
- **ADR-B** — this track builds no memo table; the granularity and the firewall proof are the
  whole deliverable, R3.15's scheduler decision deferred whole (§3.3, Q3). Fixes the completion
  criterion's shape (§5) and the settled slice count (§6).
- **ADR-C** — the gated probe is `incremental_query_types`, a one-time existence-and-proof check,
  not a shrinking count; `keystroke_latency` stays trend-only (§3.5, Q5).

## Threat model (per the ADR 0076 trigger check — not ticked, kept for the template's required section)

None, because this phase's invariant — `UnitSignature` stability, cache correctness — is a
performance and architecture property. A defect here produces wrong or stale analysis results
(an availability/correctness failure inside the editor), not an authorization, capability or data-
exposure failure; no new trust boundary is introduced or crossed.

---

## 12. Retirement

Mirrors every prior track on this trajectory: retires when §5's structural probe reads satisfied,
with the trend probe reporting whatever number instrumentation allows at that point, not gated on
it. The retirement PR removes this doc, appends its closing summary to
[`../archive/retired-tracks.md`](../archive/retired-tracks.md), and closes the spine issue — and,
uniquely among this trajectory's tracks, closes `../bynk-compiler-trajectory.md` itself, per §1's
own endpoint.
