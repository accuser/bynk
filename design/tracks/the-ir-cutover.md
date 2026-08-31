# The IR cutover

**Track issue (spine):** [#1542](https://github.com/accuser/bynk/issues/1542)
**Realises:** [`bynk-greenfield-compiler.md`](../bynk-greenfield-compiler.md) R6.13 — "Declarations are
IR nodes... never an AST declaration."
**Continues, does not reopen:** [`#1137`](https://github.com/accuser/bynk/issues/1137) (`the-ir.md`,
phase 6), retired 19 August 2026 at an argued floor (`ast_importers` = 5), archived in
[`../archive/retired-tracks.md`](../archive/retired-tracks.md).

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
- [ ] Slice 3.2 — the coordinated expr/stmt-core cutover (was Slice 6)
- [ ] Slice 4 — handler/body (was Slice 5)
- [ ] Slice 5 — item assembly (was Slice 4; absorbs old Slice 3 in full)

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
