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
then re-destructure `h.kind` via `unreachable!()` for the values they already have) plus three wrapper
signatures (`emit_http_wrapper`/`emit_http_sum_wrapper`/`emit_http_oidc_wrapper`, `workers.rs:1527,
1699, 1820`) and the `HttpRoute::method` field itself (`workers_entry.rs:1677`). No further fan-out
found: nothing downstream of these five signature sites re-declares `HttpMethod` as its own parameter
type. ADR 0355's "cascade well beyond this slice's own scope" was sized against the *whole*
`HttpMethod` surface before P6.51 narrowed it; the remaining gap is bounded and mechanical.

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
`serialisation.rs`). The four discard-site cleanups found in §3.2 plus the spine's original two
(`emitter.rs:292`/`:482`, the `Type`/`Capability` item detours) are the same shape of fix — an IR value
is already computed and sitting in hand, the code just re-derives the same fact from the AST a second
time instead of using it — and need no new dependency threaded anywhere. That is a stronger first slice
than raw import-count did: it closes literal "worse than not using it" sites the 30 August review named
outright, with zero risk of the kind Q1–Q3 exist to screen for. See the finalised decomposition below.

### 3.5 Q5 — do the ADR 0381/0366 exclusions still hold?

**Closed: yes, unchanged.** Direct check against the current tree (`track/the-ir-cutover`, based on
`origin/main`@`996911e2`): all six functions ADR 0381 declined
(`collect_json_codec_roots`/`refined_or_opaque_base`/`emit_context_rebrands`/
`sum_owner_of_variant`/`positional_field_name`/`is_refined_is_check`/`ts_binop` — seven names, one
declined pair) exist unchanged, one definition each. `TypeShape::Refined`'s `base: BaseType` field
(`bynk-ir/src/lib.rs:1324`) is unchanged. None of these are back in scope.

## 4. A correction to the spine issue's own framing

`bynk-lower/src/lib.rs` has exactly 9 `todo!()` call sites, matching the 30 August review's count — but
they are not 9 equally-weighted gaps. Six (`Statement::Expect` at `:2345`, and `ExprKind::{Expect, Val,
Wire, Observation, Trace}` at `:3273–3292`) are test-sublanguage constructs the-ir.md's own completion
plan never proposed an IR target for (`:2345`'s own comment: *"not named by any rule this track
commissions"*) and are gated behind `ctx.in_test_body`, structurally unreachable through
`bynk-lower`'s single-file harness without bynk-check's heavier project-level test machinery (Decision
C, `#1145`) — wiring the real emitter in does not make these reachable, and they stay out of this
track's scope, matching the-ir.md's own posture. One (the final fallback at `:3531`) is already
claimed, in its own comment, to be structurally unreachable through `lower_fn_body_ir` and left only as
a defensive catch-all — Slice 6 should add a verification (a debug assertion or a fixture proving it's
never hit) rather than a real implementation. The remaining two (`:3346` "no `Callee` recorded for this
call," `:3521` "bare ident names a free function used as a value") are genuine, live gaps that Slice 6
does need to close before it's safe to wire `lower_expr_ir` into a real caller.

## 5. Slice decomposition (final)

- **Slice 0 (this doc).** No code.
- **Slice 1 — discard-site cleanup.** `emitter.rs:292`/`:482` (`Type`/`Capability` item detours),
  `workers_entry.rs:380-384`/`workers.rs:606-613` (`Http` handler-kind detours), the three
  `emit_http_*_wrapper` signatures, and `HttpRoute::method`. Zero new dependencies threaded; every site
  already holds the IR value it needs. Front-loads a small ADR reversing ADR 0355's `HttpMethod`
  deferral (§6).
- **Slice 2 — signatures.** `lower_fn_sig_ir_from_types`, `lower_op_sig_ir_from_commons`.
- **Slice 3 — store/commit/invariant/transition.** `lower_store_field_ir`, `lower_commit_shape_ir`,
  `lower_invariant_ir`, `lower_transition_ir`.
- **Slice 4 — item assembly.** `lower_fn_item_ir`, `lower_agent_item_ir`, `lower_service_item_ir`,
  `lower_provider_item_ir`.
- **Slice 5 — handler/body.** `lower_fn_body_ir`, `lower_handler_ir`, `lower_service_handler_ir`. The
  `Message`-arm `ServiceProtocol` check is explicitly **out of scope** — stays declined per §3.3.
- **Slice 6 — expr/stmt core.** `lower_expr_ir`/`lower_block_ir` wired into `emitter/lower.rs`'s
  `lower_expr`. Requires closing exactly the two live gaps named in §4 (`:3346`, `:3521`) first, plus a
  verification (not an implementation) for `:3531`. Unblocks `#1225`'s dormant `@cache`/`@limit`
  accumulator work, which no shipped path currently reaches.

## 6. Slice status

- [ ] Slice 0 — this doc
- [ ] Slice 1 — discard-site cleanup
- [ ] Slice 2 — signatures
- [ ] Slice 3 — store/commit/invariant/transition
- [ ] Slice 4 — item assembly
- [ ] Slice 5 — handler/body
- [ ] Slice 6 — expr/stmt core

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
