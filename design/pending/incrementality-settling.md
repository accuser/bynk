---
level: patch
changelog: Settle phase 8 of the compiler trajectory (`design/tracks/incrementality.md`) — `UnitSignature` is a new type wrapping ADR 0200's `combined_types_for` unchanged plus fresh fn/handler/storage/capability-set projections, the file level gets one shared `Tokens(FileId)`/`Ast(FileId)` cache in `bynk-project` (not a `bynk-ide`-local patch — the crate graph rules that out), this track builds no memo table, and its own gated probe is a one-time existence-and-proof check rather than a shrinking count
---

## ADR: incrementality-unit-signature-shape
title: `UnitSignature` is a new type wrapping `combined_types_for`'s existing output unchanged, plus fresh fn/handler/storage/capability-set projections read from `UnitTable`
summary: No existing type in the workspace is already signature-shaped enough to extend in place — `combined_types_for` only ever computed one of design notes §15's four required categories, and every closer candidate (`UnitTable`'s own decl types) carries a full body

**Context.** R3.14 needs `UnitSignature` to cover four categories design notes §15 already names
as required annotations: function/handler declarations, agent storage declarations, cross-context
type references, and capability sets via `given`. ADR 0200's `combined_types_for`
(`bynk-check/src/symbols.rs:1147`) was this track's opening candidate for "the query already
exists in substance," but settling found it computes exactly one of the four categories
(cross-context type references, as a plain `HashMap<String, Arc<TypeDecl>>`) and is structurally
incapable of the other three — it never reads `UnitTable.fns`, `.agents`, `.services` or
`.capabilities` at all. It has 7 real call sites across `bynk-check` and `bynk-emit` (confirmed
fresh against the current tree: `symbols.rs:862`, `analysis.rs:666`, `check_pipeline.rs:284`,
`bynk-emit/src/project.rs:927,2227,2402,2428`), every one depending on its current, narrow,
types-only return shape for cross-context resolution and the contract hash itself.

Settling also checked the next-broadest candidate — `UnitTable` (`symbols.rs:295`), the per-unit
table `combined_types_for` itself reads from — to see whether *it* was already close to
signature-shaped. It is not: `UnitTable.fns: HashMap<String, Arc<FnDecl>>` and every `Handler`
reachable through `UnitTable.agents`/`.services` both carry a full `body: Block`
(`bynk-syntax/src/ast.rs:2005,1208`) alongside their declared signature — an edit to a function or
handler body changes the `FnDecl`/`Handler` value itself. `StoreField` (`ast.rs:945`) is closer —
`name`/`kind: StoreKind` are body-free — but carries `init: Option<Expr>`, an initialiser
expression, which is not. No existing type is `UnitSignature`-shaped without stripping something
that would break R3.14's own stability requirement.

One direct, contemporaneous in-repo precedent bears on the "widen vs. build" choice itself:
`combined_types_for_unit_info` (`symbols.rs:1170`) is a sibling function, deliberately
reimplemented against `UnitInfo` rather than calling `combined_types_for` directly, because the
per-unit emission prologue it serves has a genuinely different shape (a caller already holding
`UnitInfo`, not the flat project-wide `unit_tables`/`unit_uses` maps) than `combined_types_for`'s
own callers. This codebase already treats "build a parallel function alongside, rather than widen
one whose contract genuinely differs" as the right move when call-context shapes diverge — the
same reasoning this decision applies at the type level.

**Decision.** `UnitSignature` is a new type. It contains `combined_types_for`'s own output as one
field, completely unmodified — `combined_types_for` itself is not touched, and its 7 call sites
keep their current contract exactly as today. The remaining three categories are built fresh,
directly from `UnitTable`, each stripped to signature shape:

- Function/handler declarations: `FnDecl`'s `name`/`type_params`/`params`/`return_type`/`has_self`
  (not `body`, not `requires`/`ensures` — see the differential-fixture ADR's own field-exclusion
  note) and every `Handler`'s `method_name`/`params`/`return_type`/`given` (not `body`).
- Capability sets via `given`: the same `Handler.given`/`ProviderDecl.given`/
  `ServiceDecl.default_given` fields, plus `UnitTable.exported_capabilities` copied as-is.
- Agent storage declarations: `StoreField`'s `name`/`kind: StoreKind` only — not `init`, not
  `annotations`.

`Artefacts` (phase 7's typed emit-side document set, R7.8) gets no signature concept of its own in
this phase. Design notes §15's annotation policy — the firewall's own foundation — is a check-side
contract; nothing in this phase's scope proposes an emit-side query to key one against.

**Consequences.** Every one of `combined_types_for`'s 7 existing callers is unaffected by this
track landing — no widened signature to thread through them, no risk to `bynkc/tests/contract_hash.rs`'s
own no-false-positive guarantee (which never called the function directly, only named it in a doc
comment, so it wouldn't have caught a signature change either way). `UnitSignature` composes
`combined_types_for`'s output rather than duplicating it, satisfying phase 1's "no fact in two
hand-synced copies" invariant by construction. P8.1 (the track doc's own §6) is the slice that
builds this; P8.2–P8.5 build against its exact field list.

## ADR: incrementality-shared-file-level-cache
title: The file level gets one shared `Tokens(FileId)`/`Ast(FileId)` cache in `bynk-project`, migrating both completion and diagnostics onto it — not a `bynk-ide`-local patch, which the crate graph rules out entirely
summary: `bynk-check` cannot depend on `bynk-ide`, so "give the diagnostics path completion's existing cache" was never actually an available option — and `FileId`, the natural key, exists but isn't durable across calls yet either

**Context.** `bynk-ide/src/completion.rs`'s `PROJECT_UNIT_CACHE` already closes issue #733 for
completion requests — a real, working, content-keyed parse cache. The draft's own framing treated
"share it with the diagnostics path" as the cheap option among three. Settling checked the actual
crate dependency graph rather than assume the option was available: `bynk-ide`'s own `Cargo.toml`
depends on `bynk-check` (`bynk-check.workspace = true` — "`analyse_project`... lives here too"),
and `bynk-project ← bynk-check ← bynk-ide` is the confirmed direction throughout. `analyse_project`
and its `phase_parse` step (the diagnostics path R3.13's probe is named after) live in
`bynk-check`, which cannot reach a cache owned by the crate that depends on it. This is a
structural fact, not a design preference — "patch `bynk-ide`'s own cache to also serve
diagnostics" cannot work regardless of how it's written.

Separately, settling checked whether `FileId` (`bynk-syntax/src/span.rs:16`, built at phase 3 for
R2.2/R2.4) is already the durable key `Tokens(FileId)` would need. It exists, and is a real,
per-file identifier — but `bynk_check::project_model::phase_parse` allocates it from a local `let
mut next_file_id: u32 = 0`, reset to zero on every call. It is stable *within* one project
analysis (its stated purpose: attributing a diagnostic to the right file inside one build), not
*interned* durably *across* calls the way a cache key spanning many keystrokes needs to be.

**Decision.** Build one real cache — a `Tokens(FileId)`/`Ast(FileId)` query type — in
`bynk-project`, the one crate both `bynk-check` and `bynk-ide` can reach (and where
`ProjectGraph`, this phase's other file-adjacent type, already lives per phase 4's own placement).
This requires, as part of the same slice, a durable path↔`FileId` interning table (also in
`bynk-project`, alongside discovery) so a given file keeps the same `FileId` across separate
calls for the life of an analysis session, not only within one `phase_parse` invocation as today.
Both `bynk-ide::completion`'s `cached_project_unit` and `bynk_check::analyse_project`'s
`phase_parse` migrate to read through this one cache; `PROJECT_UNIT_CACHE` retires as a
`bynk-ide`-local duplicate once the migration lands.

**Consequences.** This closes R3.13's own probe-namesake bug (the diagnostics path's uncached
`analyse_project` call) for real, not by proxy — the actual call site that re-parses on every
keystroke gets a cache it can reach, rather than one it structurally can't. It is a larger slice
than "share an existing cache" would have been, since the interning table is new work, not
reused — named explicitly so it isn't discovered as a surprise mid-slice. It is also the one
slice in this phase with real behavioural stakes: a wrong invalidation rule here produces stale
diagnostics silently, the same failure class #733 fixed for completion, reopened at a different
layer if the new cache's own content-equality check has a gap — worth a dedicated staleness
fixture, not just a byte-golden pass.

## ADR: incrementality-no-memo-table-this-phase
title: This track builds no memo table of any kind — the granularity and the firewall proof are the whole deliverable, R3.15's scheduler decision deferred whole
summary: R3.15's own rationale text is taken at face value; the probe's own shape (a proof, not a latency number, per the third ADR in this set) means an un-memoised decomposition is not actually unmeasurable, so nothing forces a scheduler build here

**Context.** R3.15 names three options — salsa, a hand-rolled memo table, or nothing — and defers
the choice to a later, separate trigger ("a hand-rolled memo table... measurably the bottleneck").
The draft's own worry was that stopping at the query decomposition, with no cache behind it,
would leave nothing for the phase's own probe to measure — implicitly pressuring this track toward
building at least a minimal hand-rolled table just to have a number to report.

**Decision.** No memo table ships in this phase, hand-rolled or otherwise. The gated probe (see
the companion ADR on its shape) is settled as a one-time existence-and-proof check — do the query
types exist, and is `UnitSignature` proved stable under a body edit — not a latency number, so the
draft's own pressure toward "build something just to measure it" is resolved by changing what's
measured, not by building a scheduler. R3.15's own trigger for either salsa or a hand-rolled table
is "measurably the bottleneck," which cannot fire before real query types exist to be a bottleneck
in — building one now would be committing to solve a problem (cache-invalidation correctness
under concurrent IDE writes) this phase has no evidence yet even exists at a scale worth solving.

**Consequences.** This phase's own risk stays bounded to R3.13 (granularity) and R3.14 (the
firewall) — real, load-bearing architectural commitments — without also taking on cache-
correctness risk this phase's own evidence doesn't yet justify. `keystroke_latency` (the trend-only
probe) stays "not measured" through this phase's retirement; a future, separate track (or a later
slice of this one, if reopened) owns the scheduler decision once real evidence of a bottleneck
exists to trigger it. The settled slice count (§6 of the track doc) is 6, not the draft's
provisional 6–9 — dropping the conditional "P8.6, memo table" slice entirely rather than leaving
it open.
