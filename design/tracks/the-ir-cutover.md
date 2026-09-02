# The IR cutover

**Track issue (spine):** [#1542](https://github.com/accuser/bynk/issues/1542)
**Realises:** [`bynk-greenfield-compiler.md`](../bynk-greenfield-compiler.md) R6.13 — "Declarations are
IR nodes... never an AST declaration."
**Continues, does not reopen:** [`#1137`](https://github.com/accuser/bynk/issues/1137) (`the-ir.md`,
phase 6), retired 19 August 2026 at an argued floor (`ast_importers` = 5), archived in
[`../archive/retired-tracks.md`](../archive/retired-tracks.md).
**Status (re-settled 1 September 2026):** **the cutover stops at Slice 3.1.** Slice 3.2's own
evidence (§10.1) falsified the premise §5 rests on — that the cutover is a mechanical retype whose
output is byte-identical — and the branch that tested it built the exact second path §5 said must not
exist. §10 records the decision this track's spine now carries instead: repoint the one production
detour, **delete** the unconsumed lowering and the IR types only it constructs, keep the adopted
analysis helpers, and record the refusal in R15.1's four-field form with a named trigger. §§1–9 stay
as written — they are the record of what was tried, not a plan still in force — and this doc retires
(closing summary to `../archive/retired-tracks.md`) when §10.5's slices land.

## 1. What this is, and isn't

`bynk-emit`'s expression, statement, and declaration lowering still derives most of its own dispatch
decisions by walking `bynk_syntax::ast` directly — re-classifying facts `bynk-check`'s checker and
`bynk-lower`'s IR-construction pass already resolved once. Phase 6 built the IR
(`bynk-ir`/`bynk-lower`, carved into their own crates at P7.d1, `#1414`) and settled, but never
executed, the plan to route the emitter's reads through it. This track executes that plan.

**This is not phase 6 reopened.** Phase 6's own retirement argued its floor (`ast_importers` = 5)
explicitly and in the open (P6.58); that argument is not revisited here. What this track picks up is a
specific piece of already-designed work phase 6's own doc named internally as **"the cutover"**
(`#1175`, settled as that track's own Q7) but never cut slices for. Q7's decision is inherited, not
reopened:

> `emitter/lower.rs` keeps writing strings — the cutover changes what its functions read, not what
> they return.

Concretely: `emitter/lower.rs`'s canonical entry point, `lower_expr(e: &Expr, cx: &mut LowerCtx) ->
Lowered`, moves to take `&IrExpr` in place of `&Expr`; `Lowered { pre: Vec<String>, expr: String }` is
untouched. This is not a rewrite to tree-native output — that's phase 7's `bynk-ts`, already shipped,
out of scope here.

## 2. Prior art this track inherits, and does not relitigate

Three prior decisions bound this track's scope. Reopening any of them needs new evidence, not a fresh
opinion:

- **ADR 0381** traced and declined six specific conversions against the current tree at the time
  (`collect_json_codec_roots`, `refined_or_opaque_base`/`emit_context_rebrands`,
  `sum_owner_of_variant`/`positional_field_name`, `is_refined_is_check`, `ts_binop`), each for a
  structural reason (no `Callee` entry exists; reads only a discriminant; adds real resolution work
  for zero benefit; the read just relocates, doesn't disappear). §3.5 below re-confirms these still
  hold against today's tree.
- **ADR 0366 / P6.41** ruled that `TypeShape::Refined { base: BaseType, .. }` and
  `IrPat::Refined { refinement: Refinement, .. }` keep embedding AST types through phase 7. The 30
  August 2026 post-restructuring review (`../reviews/2026-08-30-post-restructuring-review.md` §1.3)
  reads this as an undiscovered gap in `bynk-ir`'s AST-firewall; it is not — it is this exact,
  already-argued residue. Not in scope here either.
- **ADR 0355** declined widening `emit_worker_compose`'s `Message` arm to read
  `lower_protocol_ir_from_commons` because the function only has `table: &UnitTable`, not a
  `&TypedCommons`, and threading one through was judged out of proportion to that slice. §3.3 below
  re-examines this given phase 8 landed since, and confirms the decline still holds — for a stronger
  reason than was visible in 2026-08.

## 3. Settling the open design questions

### 3.1 Q1 — does Q7's settled shape still hold against the current tree?

**Closed: yes, unchanged.** `bynk-emit/Cargo.toml` already carries `bynk-ir.workspace = true` and
`bynk-lower.workspace = true` — the crate split (P7.d1) changed where the types live, not the shape of
the cutover Q7 described. `lower_expr`'s signature and `Lowered`'s definition
(`bynk-emit/src/emitter/lower.rs:838-920`) are exactly the shape Q7 anticipated (T2.1/R6.2, already
landed independently by the July review plan's Wave 4). `LowerCtx::commons()` already exposes
typed-commons-equivalent access at the point `lower_expr` is called, so the argument-type swap has
context available where it's needed. No blocker found; nothing here needs re-arguing before slicing.

### 3.2 Q2 — is the workers.rs/workers_entry.rs rendering-signature cascade un-deferred by phase 7's retirement, and how big is it really?

**Closed: yes, un-defer it — and it is small, not a cascade.** ADR 0355 deferred
`HttpRoute::method`/`path` and three wrapper functions explicitly "until phase 7's printer" exists.
Phase 7 (`the-typescript-tree.md`, `#1293`) retired 29 August 2026.

Investigation found the IR-side type was already built and is already adopted elsewhere:
`IrHandlerKind::Http { method: IrHttpMethod, path: String }` (`bynk-ir/src/lib.rs:1681`) is a real
payload, and `http_handler_method_name_ir` (`bynk-emit/src/emitter/emit.rs:1263`, doc-commented
"P6.51, `the-ir.md` §6b") already has four production callers (`emit.rs:1297`, `emit.rs:2670`,
`lower.rs:2129`, `lower.rs:2245`). P6.51 already did this conversion for most of the surface — what's
left is exactly the two discard sites this track's spine issue named
(`emitter/workers_entry.rs:380-384`, `emitter/workers.rs:606-613`, which build `IrHandlerKind::Http`,
then re-destructure `h.kind` via `unreachable!()` for the values they already have) plus the AST-typed
signature/comparison surface around them: three wrapper signatures
(`emit_http_wrapper`/`emit_http_sum_wrapper`/`emit_http_oidc_wrapper`, `workers.rs:1527, 1699, 1820`),
the `HttpRoute::method` field itself (`workers_entry.rs:1679`), `derive_allowed_methods(methods: impl
Iterator<Item = HttpMethod>) -> Vec<String>` (`workers_entry.rs:1462`, fed from that field at `:1513`
and `:1579`), and four direct `route.method == HttpMethod::Get` comparisons (`workers_entry.rs:447`,
`:1803`, `:2114`, `:2168` — the fourth found only during Slice 1's implementation, a `HEAD`-from-`GET`
synthesis check this section's own review pass missed). `IrHttpMethod::as_str()`
(`bynk-ir/src/lib.rs:1704`, P6.51, a field-for-field mirror) makes all of these mechanical, no
behaviour change. ADR 0355's "cascade well beyond this slice's own scope" was sized against the
*whole* `HttpMethod` surface before P6.51 narrowed it; the remaining gap is bounded, if wider than a
first read of the two discard sites alone would suggest.

Converting these signatures retires every production caller of the AST-typed twin,
`http_handler_method_name` (`emit.rs:1253`) — its four callers are exactly the three wrappers
(`workers.rs:1534, 1706, 1830`) and `workers_entry.rs:1798`, all touched by this slice. After
conversion its only remaining caller is a test, `bynk-emit/src/project/tests_emit.rs:718`. Slice 1
therefore includes deleting `http_handler_method_name` and updating that test to call
`http_handler_method_name_ir` directly — left unreferenced it fails `-D warnings` on `dead_code`; left
called only by a test it's exactly the API-shaped-by-a-test residue `#1541` is separately cleaning up
elsewhere.

### 3.3 Q3 — does phase 8's `ProjectGraph` supply `emit_worker_compose`'s missing context?

**Closed: no — and for a stronger reason than ADR 0355 had.** `ProjectGraph`
(`bynk-check/src/project_graph.rs`, 174 lines) is pure unit-dependency topology: `units:
HashMap<UnitId, Unit>`, `files: HashMap<FileId, UnitId>`, `edges: Vec<(UnitId, UnitId, EdgeKind)>`
(`EdgeKind` = `Uses | Consumes | Provides`). Its own `[DECISION A]` explicitly defers a
`ContractHash`-equivalent; it carries no typed/checked content at all, so it cannot answer a
`ServiceProtocol::WebSocket` check regardless of whether `#1537` (phase 8's adoption decision) resolves
toward keeping or deleting it. `ProjectGraph` never bridges into `bynk-emit` today (zero references
workspace-wide).

Worse than ADR 0355 realized: `bynk-emit/src/project.rs:1419` documents that each unit's
`CheckedProgram` (and the `TypedCommons` it wraps) is **deliberately dropped at the end of that unit's
own per-file loop iteration** — "the only point in this pipeline that ever holds it," a memory-bounding
decision, not an oversight. `emit_worker_compose` runs later, in the compose stage, after every unit's
`CheckedProgram` is already gone. Threading `TypedCommons` through would mean keeping it alive across
the drop point for every unit, project-wide, for one `Message`-arm check — a materially bigger cost
than ADR 0355's own "out of proportion" framing already named.

**Consequence:** the `Message`-arm `ServiceProtocol::WebSocket` conversion stays declined. This is not
a dependency on `#1537` — phase 8's `ProjectGraph` doesn't supply what's needed whichever way `#1537`
resolves. (Comment posted on `#1537` updating the earlier cross-reference.)

### 3.4 Q4 — real first-slice ordering

§3.2's finding changes the answer from the spine issue's provisional guess (cheapest-file-first,
`serialisation.rs`). There are exactly **four discard sites** across the two questions this doc closes
— `emitter.rs:292`/`:482` (the spine's original `Type`/`Capability` item detours) and
`emitter/workers_entry.rs:380-384`/`emitter/workers.rs:606-613` (§3.2's `Http` handler-kind detours) —
all the same shape of fix: an IR value is already computed and sitting in hand, the code just
re-derives the same fact from the AST a second time instead of using it, with no new dependency
threaded anywhere. §3.2's `HttpMethod` signature/comparison surface around its two discard sites (the
three wrappers, the field, `derive_allowed_methods`, the three comparisons, and retiring
`http_handler_method_name`) is additional work in the same slice, not itself discard-site cleanup — the
full list is in §5, slice 1. Together this is a stronger first slice than raw import-count would have
been: it closes literal "worse than not using it" sites the 30 August review named outright, with zero
risk of the kind Q1–Q3 exist to screen for.

### 3.5 Q5 — do the ADR 0381/0366 exclusions still hold?

**Closed: yes, unchanged.** Direct check against the current tree (`track/the-ir-cutover`, based on
`origin/main`@`996911e2`): all seven function names ADR 0381's declined list carries
(`collect_json_codec_roots`, `refined_or_opaque_base`, `emit_context_rebrands`, `sum_owner_of_variant`,
`positional_field_name`, `is_refined_is_check`, `ts_binop`) exist unchanged, one definition each
(`bynk-emit/src/emitter.rs:1003, 1648, 1862, 2325, 4223, 4265, 5126`). Note for a future reader: ADR
0381's own title says six declined sites, but its list groups seven names into five bullets (two
bullets each cover a pair) — an arithmetic inconsistency in that ADR itself, not resolved here; it
doesn't affect this section's purpose, which is only to confirm none of the seven are back in scope.
`TypeShape::Refined`'s `base: BaseType` field (`bynk-ir/src/lib.rs:1324`) is unchanged. None of these
are back in scope.

## 4. A correction to the spine issue's own framing

`bynk-lower/src/lib.rs` has exactly 9 `todo!()` call sites, matching the 30 August review's count — but
they are not 9 equally-weighted gaps. Six (`Statement::Expect` at `:2345`, and `ExprKind::{Expect, Val,
Wire, Observation, Trace}` at `:3273–3292`) are test-sublanguage constructs the-ir.md's own completion
plan never proposed an IR target for (`:2345`'s own comment: *"not named by any rule this track
commissions"*) and are gated behind `ctx.in_test_body`, structurally unreachable through
`bynk-lower`'s single-file harness without bynk-check's heavier project-level test machinery (Decision
C, `#1145`) — wiring the real emitter in does not make these reachable, and they stay out of this
track's scope, matching the-ir.md's own posture. **Slice 3.1 (`#1564`) closed the remaining three,
shipped.** The final fallback (then `:3531`) is now `unreachable!()`, matching what its own comment
already claimed. The other two turned out not to be the "genuine, live gaps" first assumed: tracing
`:3346`'s three named Decision-C shapes against the current checker (and confirming empirically, via
the real per-context checking pipeline, not the bare single-file harness which under-populates
handler-body `Callee`s) found every one of them already Callee-recording — that branch is
`unreachable!()` too. `:3521`'s free-function-as-value case was real and is now
`bynk_ir::IrExprKind::FnRef`, a new variant (not `Global`, which is narrowly scoped to nullary
sum-variant constructors).

## 5. Slice decomposition (final)

**Reordered after the Slice 3 investigation — the original 3→4→5→6 ordering had the dependency
direction backwards.** The original "store/commit/invariant/transition" grouping (old Slice 3) is not
a coherent, independently-adoptable unit: `lower_store_field_ir`'s hard part
(`kind`/`indexed`) is *already* adopted through its sibling `lower_store_field_shape_ir` (called from
`emitter.rs:516`) — only its `init` field remains. `lower_commit_shape_ir`'s hard part
(`body_writes_state`) is likewise *already* adopted (`emit.rs:5472`, confirmed as R6.5's real
replacement for the deleted `block_writes_state`) — what's missing is constructing the real
`CommitShape` enum in place of the bare bool that adoption currently feeds inline, and the function's
own doc comment says that needs `body` already lowered to `IrExpr` — old Slice 6. `lower_invariant_ir`
and `lower_transition_ir`'s own doc comments say, near-verbatim, they are meant to be "called once per
agent's own invariant/transition list, by whichever future slice builds `IrItem::Agent` for real" —
i.e. item assembly (old Slice 4), not a standalone slice — and both internally call `lower_expr_ir`
(old Slice 6) themselves. So every real piece of old Slice 3 is downstream of either old Slice 4 or old
Slice 6, and old Slice 6 sits under both. **The expr/stmt-core cutover is the foundation the rest of
this track builds on, not the largest slice saved for last.** Renumbered accordingly; old Slice 3 folds
entirely into the new item-assembly slice (its own real home, per its functions' own doc comments).

- **Slice 0 (this doc).** No code.
- **Slice 1 — discard-site cleanup + the `HttpMethod` surface around it. Shipped (`#1556`).** The four
  discard sites (`emitter.rs:292`/`:482`, `workers_entry.rs:380-384`/`workers.rs:606-613`) needed no
  new dependency threaded — every site already held the IR value it needed. Alongside them, per §3.2:
  the three `emit_http_*_wrapper` signatures, `HttpRoute::method`, `derive_allowed_methods`, four
  `route.method == HttpMethod::Get` comparisons (one more than §3.2 counted — found during
  implementation), and deleting `http_handler_method_name` (its one remaining caller,
  `tests_emit.rs:718`, repointed to `http_handler_method_name_ir`). Front-loaded a small ADR reversing
  ADR 0355's `HttpMethod` deferral (§7).
- **Slice 2 — signatures. Already satisfied; no code needed.** Both functions fail the review's own
  strict "external-to-the-crate call site" test — which is why they were counted among the 15 — but
  neither is actually unwired: each is called by a same-crate sibling wrapper that *is* reached from
  production `bynk-emit` code. `lower_fn_sig_ir_from_types` is called by
  `lower_attached_fn_sig_ir_from_types`, called from `bynk-emit/src/project.rs`'s `build_emit_unit_ctx`
  (resolving `uses`-imported types' attached-method signatures). `lower_op_sig_ir_from_commons` is
  called by `capability_op_sig_from_commons`, called from `bynk-emit/src/emitter/lower.rs`'s
  `cap_op_param_names` — and, since Slice 1 added `lower_capability_ops_ir`, also reached via
  `emitter.rs`'s capability loop through the private `lower_op_sig_ir`. Checked as a sanity bound
  before trusting this generalises: `lower_store_field_ir` (then-Slice 3) had no such wrapper — comment
  mentions only, genuinely zero callers — confirming this was Slice 2's own shape, not a pattern across
  the other clusters (it wasn't — see above).
- **Slice 3 — expr/stmt core** *(was Slice 6)*. `lower_expr_ir`/`lower_block_ir` wired into
  `emitter/lower.rs`'s `lower_expr`. **This is the real next slice — everything below depends on it —
  and it does not decompose into many small, independently-mergeable pieces the way Slice 1 did.**
  Investigated before proposing: `lower_expr` (`lower.rs:920`) is the single funnel for a mutually
  recursive family — every one of the eleven `lower_*_kernel` functions, the match-compilation cluster
  (`lower_match_as_iife` → `build_match_iife` → `emit_match_case` → `emit_match_body` →
  `emit_match_if_chain`), `lower_method_call`/`lower_call`/`lower_if`/`lower_lambda`/
  `lower_field_access`, and the block/statement layer (`emit_block_inner`/`emit_statement`, still on
  the older `out: &mut String` sink convention `Lowered` replaced everywhere else) all call back into
  the shared lowerer for their own sub-expressions — `lower_list_kernel` alone makes 71 such calls.
  None of the ~46 AST-typed functions in this file is a leaf with no AST-typed dependents; a kernel's
  own parameter type can't move to `&IrExpr` independently of the caller that hands it that value
  already being IR-typed. The sink-vs-`Lowered` calling-convention question is **not** a separate
  blocker (`emit_statement` already bridges through `Pre`/`lower_expr` today; only what's *inside* that
  bridge needs to change type) — it converts for free alongside the rest, not as its own prerequisite
  slice. Sub-decomposition:
  - **Slice 3.1 — close the two live `todo!()` gaps. Shipped (`#1564`).** One (the missing-`Callee`
    guard) turned out to already be unreachable — every shape once suspected of missing a `Callee`
    traced, and confirmed empirically, to have one. The other (a bare free-function-as-value reference)
    was real and is now `IrExprKind::FnRef`. See §4 for the full account.
  - **Slice 3.2 — the coordinated cutover itself.** Converts the mutually recursive family (kernels,
    match compilation, `lower_expr`'s own dispatch, the block/statement sink layer) from AST-typed to
    IR-typed parameters, and flips all seven external entry points (`emit_method`, `emit_free_fn`,
    `emit_contract_guarded_body`, `emit_provider`, `emit_service`, `emit_agent`, `emit_ws_do_method` —
    all reaching this machinery through `emit_block_as_function_body_with_return`, `lower.rs:201`) to
    call `bynk_lower::lower_block_ir`/`lower_fn_body_ir` first and thread the result through. Lands as
    **one slice, not several independently-mergeable ones** — a half-converted state (some entry points
    reading IR, others still reading AST, for the same shared functions) would mean the family exists
    in two incompatible signatures at once: either a temporary duplicate copy of the whole mutually
    recursive machinery (real maintenance cost, discarded once migration finishes, and a live second
    copy to keep in sync in the meantime), or genuinely broken intermediate states that don't compile.
    Neither buys real safety over one well-tested PR. Internally sequenced as ordered commits within the
    one PR for reviewability (kernels and other near-leaves
    first, match compilation next, `lower_expr`'s own dispatch and the statement/block layer last, the
    seven entry-point call sites last of all) even though intermediate commits won't independently
    compile against a stable interface. The size is real (order of 46 signatures across 6,321 lines),
    but the risk profile is not the same as a feature change of that size: emitted output must be
    byte-identical (a mechanical retype, not new logic), and the full e2e fixture corpus's zero-diff
    bless (the same gate every `#1137` slice used) is exhaustive verification for exactly that claim —
    unblocks `#1225`'s dormant `@cache`/`@limit` accumulator work, which no shipped path currently
    reaches, as a real side effect of 3.2 rather than a reason to split further.
- **Slice 4 — handler/body** *(was Slice 5)*. `lower_fn_body_ir`, `lower_handler_ir`,
  `lower_service_handler_ir`. Depends on Slice 3 (fn/handler bodies lower through `lower_expr_ir`). The
  `Message`-arm `ServiceProtocol` check is explicitly **out of scope** — stays declined per §3.3.
- **Slice 5 — item assembly** *(was Slice 4, now absorbing old Slice 3 in full)*.
  `lower_fn_item_ir`, `lower_agent_item_ir`, `lower_service_item_ir`, `lower_provider_item_ir` — plus,
  as part of building a real `IrItem::Agent`: `lower_store_field_ir`'s `init` gap,
  `lower_commit_shape_ir`, `lower_invariant_ir`, `lower_transition_ir`. Depends on Slice 3 (item bodies)
  and Slice 4 (handler assembly feeds `IrItem::Agent`/`IrItem::Service`).

## 6. Slice status

- [ ] Slice 0 — this doc
- [x] Slice 1 — discard-site cleanup + the `HttpMethod` surface around it (`#1556`)
- [x] Slice 2 — signatures (already satisfied — see §5, no code landed)
- [x] Slice 3.1 — close `bynk-lower`'s two live `todo!()` gaps (`#1564`)
- ~~Slice 3.2 — the coordinated expr/stmt-core cutover (was Slice 6)~~ **Stopped, not merged
  (§10).** Reached 3 of 7 entry points on `slice3-2-expr-stmt-core` (19 commits from `31a949ae`),
  retained as a reference tag; §10.1 has the measurements.
- ~~Slice 4 — handler/body (was Slice 5)~~ — struck by §10; superseded by §10.5's D-slices.
- ~~Slice 5 — item assembly (was Slice 4; absorbs old Slice 3 in full)~~ — struck by §10; superseded
  by §10.5's D-slices.
- [x] Slice D0 — repoint `lower_event_subscriber_shapes_ir` off the body-lowering detour (§10.5; `#1574`)
- [x] Slice D1 — delete `bynk-lower`'s unconsumed lowering and its tests (§10.3; `#1576`)
- [x] Slice D2 — delete `bynk-ir`'s orphaned IR types; rewrite the crate doc (§10.3; `#1578`)
- [ ] Slice D3 — the R15.1 register entry, the ADR, the adoption probe, and this track's retirement
  (§10.4, §10.5)

## 7. Front-loaded ADR candidates

One: an ADR reversing ADR 0355's `HttpMethod`-cascade deferral, landing with Slice 1. §3.2 found the
situation materially changed since ADR 0355 (P6.51 narrowed the surface, phase 7 shipped) — the same
discipline ADR 0355 itself modeled when it deferred, rather than silently converting around the
now-stale deferral.

## 8. What stays declined — named, not silently dropped

- ADR 0381's six conversions (§3.5, unchanged).
- `TypeShape::Refined`/`IrPat::Refined`'s `BaseType`/`Refinement` embedding (ADR 0366/P6.41, §3.5,
  unchanged).
- `emit_worker_compose`'s `Message`-arm `ServiceProtocol` check (ADR 0355, §3.3 — reconfirmed for a
  stronger reason: the per-unit `CheckedProgram` drop, not just parameter-threading cost).
- Six of `bynk-lower`'s nine `todo!()` sites — the test-sublanguage constructs (§4), out of this
  track's scope the same way they were out of `the-ir.md`'s.

## 9. Threat model

None, because this track changes no language surface, no runtime behaviour, and no capability
boundary — internal dispatch-source relocation only, verified by the same means every `#1137` slice
used: a full zero-diff bless against the e2e fixture corpus, `cargo test --workspace`, `cargo clippy
--workspace --all-targets -- -D warnings`.

## 10. Re-settling (1 September 2026): the cutover stops here

### 10.1 What Slice 3.2 actually found, measured

§5 accepted Slice 3.2 on two premises: that the cutover is *a mechanical retype* ("emitted output
must be byte-identical … not new logic"), and that it therefore lands as *one slice* because "a
half-converted state … would mean the family exists in two incompatible signatures at once: either a
temporary duplicate copy of the whole mutually recursive machinery … or genuinely broken intermediate
states. Neither buys real safety." Both premises were tested on `slice3-2-expr-stmt-core` (19
commits from `31a949ae`, then `origin/main`). Read against `origin/main`@`067b94a4`:

| Measure | `main` | `slice3-2-expr-stmt-core` |
|---|---|---|
| `bynk-emit/src/emitter/lower.rs`, lines | 6,321 | 10,117 |
| `_v2` sibling functions duplicating an AST-typed lowering function | 0 | 56 |
| AST-typed lowering functions still present (`&Expr`/`&Block`/`&Statement`/…) | 26 | 32 |
| `todo!()` in production emitter code | 0 | 6 |
| `unreachable!()` in `emitter/lower.rs` | 8 | 26 |
| `ts_writes` (gated probe) | 809 | 1,079 |
| Entry points flipped to the IR path | 0 of 7 | 3 of 7 |
| e2e goldens accepted as "structurally unrecoverable" diffs | 0 | 2 |
| Follow-on issues filed for gaps in the new path | — | 7 (#1566–#1572) |

Each flip on that branch found and fixed a real behavioural difference between the two paths —
missing precedence-aware parenthesisation, a dropped constructor payload, a guard-blind
non-exhaustive-match check, a missing `?`-in-match-arm case, absent `cx.record_span` calls, `?`
losing its hoisted form, a lambda's `?` picking the wrong `embeds` conversion — every one a bug in
the *IR* path, fixed there, with the shipped AST path right each time. Three things follow, each of
which §5 said would mean the slice is not done:

1. **It is the duplicate copy, not the retype.** Every converted function is a `_v2` *sibling*; no
   AST-typed function was retyped and none was deleted. The two families coexist, and a per-body
   static gate (`body_uses_is_pattern`, `body_uses_record_spread`) chooses between them at each of
   the three flipped entry points. That gate is not migration scaffolding with a deletion date: it
   stays until [#1567](https://github.com/accuser/bynk/issues/1567) (R5.9, is-binding scopes) and
   [#1569](https://github.com/accuser/bynk/issues/1569) (spread representation) land, and both are
   *new representation choices in `bynk-lower`*, not mechanical work — #1569's own filing says so.
   This is `bynk-compiler-trajectory.md` §6's question 3 and `bynk-greenfield-compiler.md` P5,
   verbatim, reproduced by the track that was opened to close them.
2. **The output is not byte-identical, and cannot be made so from `bynk-emit`.** The two residual
   diffs ([#1568](https://github.com/accuser/bynk/issues/1568)) are information the IR does not
   carry; closing them means widening `bynk-ir`, not fixing `bynk-emit`. A "mechanical retype"
   whose every step is a semantic reconciliation against the old code generator is not a retype. It
   is a second code generator being brought up to parity with the first, feature by feature — which
   is a rewrite, and `bynk-compiler-trajectory.md` §2 names exactly that as the thing a migration is
   not.
3. **The remaining cost is at least the cost already paid, and open-ended.** Three of seven entry
   points, the `FnDecl`-bodied ones, took ~4,400 lines. The four handler-bodied ones are flagged in
   [#1566](https://github.com/accuser/bynk/issues/1566) as "materially higher risk" because
   `lower_ident_v2`'s store-cell/agent-`self`/actor-binder branches "were designed against
   `lower_handler_ir`'s own doc comment but never exercised against a real handler body." After
   them: `Callee::Cross` (#1570), the indexed-filter fast path (#1571), two unconfirmed edge cases
   (#1572), R5.9, spread, parens — and only *then* the deletion of the 26 AST-typed functions and the
   gate, which is the step that would make any of it a cutover rather than a fork. Q7's end state,
   after all of that, is still `String`s out of `emitter/lower.rs`.

### 10.2 A finding the 30 August review missed: the IR path already runs in production, and is discarded

The review's "fifteen entry points with zero production call sites" counted *direct* callers. Traced
transitively (a scratch build with those fifteen made private and `rustc`'s own dead-code analysis
read off), the expression lowerer is **reachable from production on `main` today** — through one
chain:

```
bynk-emit/src/project.rs:1445   lower_event_subscriber_shapes_ir(&program)
bynk-lower/src/lib.rs:1780        └─ lower_service_item_ir(s, program)        for every `from Events(E)` service
bynk-lower/src/lib.rs:1754           └─ lower_service_handler_ir(h, ..)     for every handler on it
bynk-lower/src/lib.rs:789               └─ lower_block_ir → lower_stmt_ir → lower_expr_ir   the whole body
```

and then keeps two booleans of the result: `two_param_handler` (the `Event` handler's parameter
count) and `schema_dispatch.is_some()` (from the protocol). Every handler *body* on every events
service in the project is lowered to `IrExpr` and dropped on the floor. Twenty e2e fixtures declare
`from Events(`; each pays for a full body lowering it never reads. The function's own doc comment
already says this — "`lower_service_item_ir` unconditionally lowers every handler's own *body* (not
just its declared shape)" — and cites the #1254 `catch_unwind` probe that found the corpus doesn't
panic through it. That is a detour of exactly the §1.2 shape the review *did* name at
`workers_entry.rs:380`, one level deeper, and it has two consequences for this decision:

- `lower_ident_ir`'s terminal `unreachable!()` (`lib.rs:3589`) is **live** on `main` through this
  chain, and its own safety argument does not cover it: the message says the arm is "structurally
  unreachable through `lower_fn_body_ir` (see its own doc comment): it has no
  store_fields/agent_state_ty/actor_binding parameter to carry any of them" — an argument scoped to
  `lower_fn_body_ir`'s callers, written before the events-service path above was a second production
  caller (`lower_service_handler_body_ir`), which it never considers. The corpus not containing the
  shape is what holds it off, not construction. (The six `todo!()`s are *not* in this category: each
  names the checker's `ctx.in_test_body` gate, so a legal events-service handler body cannot reach
  them — they go with D1 because they are dead, not because they are dangerous.) A shipped-compiler
  panic whose safety argument covers the wrong call graph, in code whose only production purpose is
  to be discarded, is the reason D0 lands first.
- Both values the caller needs are already available from adopted, shape-only helpers:
  `lower_protocol_ir` (`lib.rs:1449`, 3 production callers) carries `schema_dispatch`, and
  `lower_service_handler_signature_ir` (`lib.rs:645`, called from `emitter.rs:506`) carries the
  parameter list. Repointing `lower_event_subscriber_shapes_ir` at those two is a ~10-line change
  that closes the detour **under either option in §10.3** and is Slice D0 for that reason.

### 10.3 The decision: delete, with the inventory

Two options were priced. Both are stated so the choice can be argued with; the first is the one this
track now carries.

**Option A (chosen) — delete the unconsumed lowering; keep the adopted analysis helpers; record the
refusal.** `bynk-compiler-trajectory.md` §8: "A phase's estimate is wrong by a large factor … the
phase boundary is the stopping point, and the trajectory's value is what has already landed, not what
remains." Phase 6's retirement boundary is the coherent state; Slice 3.2 is the mid-phase state §2 of
the trajectory says is not safe to stop in — so the branch does not merge, and the question becomes
what `main` should hold. P5's answer is not "the available-but-unwired shape, indefinitely."

**Option B (declined, priced) — finish to parity, then delete the AST path.** Merge criterion would
be §5's own: one PR in which the 26 AST-typed lowering functions, the AST route through
`emit_block_as_function_body_with_return`, and both gate functions are *gone*. Remaining work is
§10.1(3)'s list, sized against the 3-of-7 already paid: not less than another ~4,400 lines of `_v2`
code plus three `bynk-ir` widenings, for an end state that still emits strings and closes R6.13 for
`emitter/lower.rs`'s *reads* only. If the language is settled enough to fund that, it is settled
enough to fund tree-native emission (`bynk_ts` nodes out of the lowerer) instead, which would retire
`ts_writes` rather than raise it — and that is a different track with a different trigger, not this
one continued.

**The deletion inventory**, computed on `origin/main`@`067b94a4` by the method §10.2 describes (make
the fifteen review-named entry points private, stub the §10.2 detour, read `rustc`'s dead-code
warnings; then grep each `bynk-ir` public item for a consumer in any remaining production source).
Reproducible from that description; the scratch script is not committed because the D-slices
re-derive it against the tree they land on.

**`bynk-lower/src/lib.rs` — 48 functions plus four fields of `LowerIrCtx`, roughly 3,000 of the
4,114 production lines (doc comments included), and up to 73 of 134 tests (~3,877 of 6,064
test-region lines).**

| Group | Items | Count |
|---|---|---|
| Public entry points: the review's fifteen minus the two Slice 2 found adopted, plus `lower_type_item_ir`/`lower_capability_item_ir` (Slice 1 replaced both with `lower_type_shape_ir`/`lower_capability_ops_ir`) | `lower_expr_ir` `lower_block_ir` `lower_fn_body_ir` `lower_fn_item_ir` `lower_handler_ir` `lower_service_handler_ir` `lower_service_item_ir` `lower_agent_item_ir` `lower_provider_item_ir` `lower_store_field_ir` `lower_commit_shape_ir` `lower_invariant_ir` `lower_transition_ir` `lower_type_item_ir` `lower_capability_item_ir` | 15 fns |
| The lowering context, slimmed, not deleted | `LowerIrCtx` (`lib.rs:74`) stays — `lower_service_handler_signature_ir` and the other kept helpers construct it for `resolve_type_ref`/`unit_ty` — but `rustc` reports "fields `scopes`, `tmp_counter`, `return_ty`, and `store_queryable` are never read" and "multiple methods are never used" once the functions above go; those four fields and their scope/temp/return methods are removed | 4 fields |
| Private helpers reachable only from the above | `lower_stmt_ir` `lower_question_ir` `lower_call_ir` `lower_lambda_ir` `lower_ident_ir` `lower_is_ir` `lower_pattern_ir` `lower_pattern_test_ir` `lower_arm_ir` `lower_exhaustive_ir` `lower_record_spread_ir` `lower_interp_part_ir` `lower_handler_body_ir` `lower_handler_signature_ir` `lower_service_handler_body_ir` `lower_provider_op_ir` `lower_policy_ir` `wrap_body_return` `fn_receiver_ty` `fn_rigid_type_vars` `peel_effect_ty` `embed_conversion_ir` `is_refined_is_check_ir` `refined_check_ir` `is_irrefutable_ir` `literal_base_of_ty_ir` `fold_and_ir` `fold_or_ir` `named_decl` `nullary_variant_owner` `variant_info_of` `collect_pattern_binding_tys` `ir_pat_contains_or` | 33 fns |
| `todo!()` sites | all six remaining (`lib.rs:2374, 3302, 3309, 3313, 3317, 3321` — the test-sublanguage arms §4 kept out of scope) sit inside `lower_stmt_ir`/`lower_expr_ir` and go with them; **`bynk-lower` ships with zero `todo!()` afterwards** | 6 |
| Tests | every `#[test]` in the trailing module that exercised the above — **D1 found 121 of 134, not ≤73**: 70 name a deleted function directly, and a further 51 reached the expression lowerer through a shared `lower_fn`/`handler_ir_of` helper the direct-name count could not see; the ~281 test-module helper fns pruned to what the survivors use. Twenty-one of the 121 pinned a *kept* helper only indirectly (`body_writes_state` through `CommitShape`; `lower_type_shape_ir`, `lower_capability_ops_ir`, `lower_provider_given_ir` and `lower_handler_kind_ir` through the item constructors that wrapped them; `lower_protocol_ir`/`lower_service_handler_signature_ir` through a queue fixture; `lower_store_field_shape_ir`'s `Ty::Unit` fallback) and were re-created as direct tests in the same slice, so the crate keeps 34 tests | 121 tests |

**What `bynk-lower` keeps — the AST-analysis helpers the review said earn their place, every one
with a production caller after D0:** `lower_handler_kind_ir` (25 production references), `lower_handler_given_ir`
(14), `is_effectful_return` (10), `lower_protocol_ir_from_commons` (6), `lower_provider_given_ir`
(6), `lower_protocol_ir` (3), `lower_actor_seam_ir` (3), `lower_type_shape_ir` (2),
`lower_store_field_shape_ir` (2), `body_writes_state` (2), `lower_capability_ops_ir` (2),
`lower_attached_fn_sig_ir_from_types` (2), `lower_service_handler_signature_ir` (2),
`lower_event_subscriber_shapes_ir` (1, repointed by D0), `lower_route_cache_ir` (1),
`lower_route_limit_ir` (1), `capability_op_sig_from_commons` (1); two that are `pub` today but
have no caller outside the crate — `lower_fn_sig_ir_from_types` (`lib.rs:1983`, called only by
`lower_attached_fn_sig_ir_from_types`) and `lower_op_sig_ir_from_commons` (`lib.rs:1929`, called only
at `:1874`/`:1908`) — which **D1 demotes to private**, since §10.4's adoption probe counts `pub`
items and would otherwise read 2, not 0; and the genuinely private support (`lower_op_sig_ir`,
`lower_event_pattern_ir`, `lower_http_method_ir`, `lower_cap_ref_ir`, `store_field_kind_and_indexed`,
`resolve_store_field_ty`, `duration_millis_annotation`). Roughly 1,000 production lines and ~2,200
test lines: the crate its own description already claims to be, "the small set of shared
AST-analysis helpers both `bynk-emit` and `bynk-lower` need."

**`bynk-ir/src/lib.rs` — 22 of 47 public items (D2 found, not 23), ~1,290 of 1,923 lines with
the crate doc, leaving 634.** Every item below has no consumer in any production source outside
`bynk-ir` once the `bynk-lower` set above is gone (`IrItem` is included because its only surviving
reference is the D0 detour's destructuring, which D0 removes). **D2's correction:** `EmbedIr` is
*not* orphaned — it is the payload of the kept `TypeShape::Sum::embeds`, an in-crate consumer the
out-of-crate grep could not see — and stays; and `IrExpr` had one in-crate consumer too, the kept
`StoreFieldIr::init` slot, which was always `None` after D1 and was removed with it:

| Group | Items |
|---|---|
| The expression IR | `IrExpr` `IrExprKind` (~235 lines) `IrStmt` `IrBinOp` `IrInterpPart` `GlobalRef` (~~`EmbedIr`~~ — kept, see above) |
| Patterns and match compilation | `IrPat` `IrArm` `BindingMode` `Exhaustive` `MatchForm` |
| Declarations and handlers | `IrItem` `IrHandler` `ProviderBody` `ProviderOpIr` `ActorBinder` `ConnectionBinder` `CommitShape` `IrPredicate` |
| Policy | `PolicyIr` `CorsIr` `SecurityIr` |
| Crate doc | `lib.rs:1–158` narrates the IR as a finished design, naming `lower_handler_ir`/`lower_service_handler_ir`/`lower_fn_body_ir` seven times; rewritten by D2 to describe the analysis-helper vocabulary that remains |

**What `bynk-ir` keeps — 24 items, each with a consumer outside the crate today:** `IrHandlerKind`
(six files), `IrHttpMethod`, `CapRefIr`, `ActorSeamIr`, `ProtocolIr`, `TypeShape`, `StoreFieldIr`,
`StoreKindIr`, `IndexIr`, `FnSig`, `OpSig`, `CacheIr`, `EventSubscriberShape`, `EventPatternIr`,
`EventPatternValueIr`, `ConstVal` (the event-pattern renderer at `emitter/lower.rs:5712`), the four
AST-walk helpers `block_uses_emit`, `walk_block_exprs`, `walk_exprs`, `match_needs_if_chain`, and
the four `pub const MUTATING_{MAP_CACHE,SET,LOG,CELL}_OPS` tables (`lib.rs:1840–1849`), consumed by
the kept `body_writes_state` (`bynk-lower/src/lib.rs:1316–1319`), and `EmbedIr` (per D2's
correction). 47 = 22 deleted + 25 kept.
`TypeShape::Refined`'s `BaseType`/`Refinement` embedding (ADR 0366) stays exactly as §2 already
argued.

**Elsewhere — and this is where D1/D2 would otherwise fail CI.** `.github/workflows/ci.yml:243`
runs `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace`, so a rustdoc intra-doc link from a
*surviving* item to a deleted one is a hard failure, invisible to `cargo test` and clippy. Surviving
items carry such links today: in `bynk-ir`, `CapRefIr` → `[IrItem]` (`:1014`), `OpSig` →
`[IrItem::Fn::receiver]` (`:1073`), `FnSig` → `[IrItem::Fn]` (`:1083`), `ProtocolIr` →
`[ConnectionBinder]` (`:1135`), `EventPatternValueIr` → `[GlobalRef]` (`:1200`), `TypeShape` →
`[IrItem::Type]`/`[EmbedIr]` (`:1307`, `:1328`), `StoreKindIr`/`IndexIr` → `[EmbedIr]` (`:1414`,
`:1426`), `IrHandlerKind` → `[IrHandler::kind]` (`:1683`); in `bynk-lower`, `lower_type_shape_ir` →
`[lower_type_item_ir]`/`[IrItem::Type]` (`:864–865`), `lower_event_subscriber_shapes_ir` →
`[lower_service_item_ir]` (`:1772`, `:1777`), `lower_capability_ops_ir` and
`capability_op_sig_from_commons` → `[lower_capability_item_ir]` (`:1833`, `:1846`), and `:1501`. **D1
and D2 each rewrite every surviving doc comment that links a deleted item**, not only the crate doc,
and run the strict `cargo doc` locally before pushing. Plain `//` comments naming deleted types are
not a CI failure but are the same defect: `bynk-check/src/checker.rs:684`, `:768` and
`checker/calls.rs:211` (`lower_service_handler_ir`/`lower_expr_ir`, already stale — they name the
pre-P7.12 module path); `bynk-emit/src/emitter.rs:496–498` (`IrItem::Service`, `IrHandler`),
`emitter.rs:5115–5116` (`IrBinOp` — worth *rewriting*, not deleting: it records why `ts_binop` was
never converted, a piece of §10.1(2)'s own argument), `project.rs:2860` (`IrItem::Provider`),
`emitter/emit.rs:2089–2093` (`IrItem::Capability::{def,ops}`), `emitter/wrangler.rs:45–46`
(`IrItem::Service`, `IrHandler::kind`). D1/D2 rewrite those too, so no comment in the tree names a
deleted item. No manifest, release-list, or probe change: `bynk-ir`/`bynk-lower` stay as crates and stay published,
`ast_importers` does not walk them, and `test_density` is a trend row. The Slice 3.2 branch's two
`bynk-lower`-side fixes (`ConstVal::Float` carrying its lexeme; a lambda `?`'s `embeds`) land
inside deleted code and are not salvaged — `ConstVal::Float` has no surviving consumer that renders
it (`emitter/lower.rs:5718` matches it only to reject it).

### 10.4 The refusal, in R15.1's four fields

Landed in `bynk-greenfield-compiler.md` Part 15.1's register by D3, alongside the entries that already
carry a `Tracked:` line, and mirrored on the spine at retirement. Written here first so it is
reviewed with the decision it records rather than after.

**A typed expression IR between the checker and the string-emitting lowerer.**
*Claim:* `bynk-emit/src/emitter/lower.rs` reads `bynk_syntax::ast` directly for expression,
statement and body dispatch, and stays the single lowering path. Declaration-level facts flow
through the adopted `bynk-ir`/`bynk-lower` analysis helpers; expression-level facts do not.
*Cost avoided:* a second expression code generator brought to feature parity with the first — measured
at ~4,400 lines and seven follow-on issues for three of seven entry points, with the remaining four
flagged as higher risk — plus the gate that keeps both reachable meanwhile, plus three `bynk-ir`
representation widenings (#1567, #1568, #1569) that exist only to let the new path reproduce the old
one's bytes. R6.13 stays closed at the declaration level phase 6 retired it at; it does not extend to
expression reads.
*Trigger:* an emit-side change that needs expression-level facts the checker already resolved and
`emitter/lower.rs` cannot get from `TypedCommons` without re-deriving them — **and** which the
tree-native route (lowering to `bynk_ts` nodes, retiring `ts_writes`, per phase 7's own "correction,
argued and accepted mid-phase") would not serve better. A second miscompile of the kind T2.1 closed
(hoisting, short-circuit) originating in AST re-classification would fire this; a desire for R6.13
purity alone does not.
*Evidence:* the adoption probe D3 adds (for each `pub` item in `bynk-ir`/`bynk-lower`, a production
call site outside the owning crate and outside a test — the review's Part 5 §8), reading **0
unconsumed** by construction after D1/D2 (D1's two demotions are what make it 0 rather than 2) and
gated as a ratchet so it can only fall; and
`emit_diagnostics`/`ts_writes` unchanged by D0–D3.
*Note:* refusing the expression IR is not refusing the IR — 21 `bynk-ir` items and 17 `bynk-lower`
entry points stay, with consumers. And it is not a refusal of tree-native emission, which is the
other way to reach the same end state and was never this track's scope.
*Tracked:* the spine, #1542, at retirement.

### 10.5 What lands, in order, and what closes

Four slices, each an ordinary sub-issue of the spine, each verified the way §9 already requires
(zero-diff e2e bless, `cargo test --workspace`, clippy `-D warnings`, `cargo xtask greenfield-status`
table current):

- **D0 — repoint the detour.** `lower_event_subscriber_shapes_ir` reads `lower_protocol_ir` and
  `lower_service_handler_signature_ir` instead of `lower_service_item_ir`. Zero-diff by construction
  (the two booleans it returns come from the same values). Lands first and alone because it is
  correct under either option in §10.3, and because it is the change that makes §10.3's inventory
  literally `rustc`-dead rather than inferred-dead.
- **D1 — `bynk-lower`.** Delete the 48 functions and slim `LowerIrCtx` per §10.3; demote
  `lower_fn_sig_ir_from_types` and `lower_op_sig_ir_from_commons` to private; delete the tests that
  exercised the deleted functions; prune the test module's helpers to what survives; rewrite every
  surviving doc comment that links a deleted item (§10.3 "Elsewhere" lists them) and the three
  `bynk-check` comments. `rustc` with `-D warnings` is the proof nothing reachable was removed;
  `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p bynk-lower` is the proof no link dangles.
- **D2 — `bynk-ir`.** Delete the 23 items; rewrite `lib.rs`'s crate doc to describe the helper
  vocabulary that remains; rewrite the nine surviving-item doc comments that link deleted items
  (§10.3 "Elsewhere") and the five `bynk-emit` plain comments; drop the `bynk_syntax::ast` imports
  only the deleted items needed (the `Block`/`Expr`/`MatchArm` imports the four walk helpers use stay,
  and so does ADR 0366's `BaseType`/`Refinement`). Same strict `cargo doc` proof, workspace-wide,
  since `bynk-emit`'s docs link into `bynk-ir`.
- **D3 — record and retire.** The ADR (superseding the-ir.md's Q7/#1175 "cutover" decision and
  ADR 0338's R5.9 deferral, both of which become moot); the Part 15.1 register entry from §10.4;
  the adoption probe, gated; this doc's closing summary to `../archive/retired-tracks.md`; the spine
  closed.

**The branch.** `slice3-2-expr-stmt-core` is tagged (`archive/slice3-2-expr-stmt-core`) and no PR
is opened for it. It stays readable as the evidence §10.1 cites.

**The satellite issues.** [#1566](https://github.com/accuser/bynk/issues/1566),
[#1567](https://github.com/accuser/bynk/issues/1567), [#1568](https://github.com/accuser/bynk/issues/1568),
[#1569](https://github.com/accuser/bynk/issues/1569), [#1570](https://github.com/accuser/bynk/issues/1570),
[#1571](https://github.com/accuser/bynk/issues/1571), [#1572](https://github.com/accuser/bynk/issues/1572)
exist only to bring the second path to parity; each closes as "not planned, superseded by #1542
§10" when this re-settling merges, with a one-line comment pointing here.
[#1225](https://github.com/accuser/bynk/issues/1225)'s `@cache`/`@limit` accumulator work, which §5
named as a side benefit of 3.2, was closed before this track opened and is not reopened by it.

**What this does not decide.** Phase 8's unadopted query layer
([#1537](https://github.com/accuser/bynk/issues/1537)) is the same decision on different evidence
and gets its own short settling note, not a ride-along here. The hygiene track
(`post-trajectory-crate-hygiene.md`, #1533) should re-cut its S4/S8 targets after D1/D2 land —
`bynk-lower/src/lib.rs`, its largest split target, drops from 10,177 lines to roughly 3,200.
