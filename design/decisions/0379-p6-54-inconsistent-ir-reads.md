# 0379 — `emit.rs` stops re-deriving facts it already has an IR reading for

- **Status:** Accepted (v0.249.31)

summary: Not conversions but inconsistencies — each site sat beside a place that had already done the identical resolution through the IR

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b, Phase H) named several
`emit.rs` sites where a raw-AST read sat next to an already-computed IR value answering the exact same
question — a place a future change to the IR path could silently fail to reach the AST path, since
the two were never actually the same computation. Re-verifying the plan's own line citations against
the current tree (post P6.50–P6.53) found several already resolved by earlier slices as a side effect
(`emit_service`'s own `async_kw`/`async_tail` already read `ir_effectful`, not
`is_effectful_return(&h.return_type)`, contradicting this plan row's stale citation) — corrected here
rather than re-implemented.

**Decision.** Six real sites converted:

- `emit_service`'s `schema_dispatch_env_binder` prologue read `handler.params.get(1)`/`.first()` for a
  param's own *name* while `ir_params: &[(String, TyId)]` (the already-lowered signature, used two
  lines earlier for the same handler's own emitted parameter list) sat unread. Both call sites now
  index `ir_params` instead.
- `emit_agent`'s per-handler loop called `is_effectful_return(&h.return_type)` twice for the same
  handler in the same iteration (`async_tail`, then `async_kw` ~50 lines later) — not an IR
  inconsistency, a plain duplicate computation of a pure function. Computed once, reused.
- `cross_context_caps_used`/`cross_context_cap_namespaces` (both walk every service/agent handler's
  `given` clause, plus the provider path for the latter) read raw `CapRef`s via `.key()`/`.prefix()`
  when `ir::lower::lower_handler_given_ir`/`lower_provider_given_ir` already exist for exactly this
  extraction and are used elsewhere in this same file. Both now iterate `CapRefIr { name, context }`
  instead.
- Three more `HandlerShared::capabilities` sites (`emit_service`, `emit_agent`, `emit_ws_do_method`)
  each independently re-derived `h.given.iter().map(|c| c.key().to_string()).collect()` — the same
  three-line pattern, three times, now one call to `lower_handler_given_ir` each.
- `topo_order_providers`'s own dependency walk (`p.given.iter().filter(|d| !d.is_cross_context())…`)
  now reads `lower_provider_given_ir(p)`'s `CapRefIr::context.is_none()` instead.

`emit.rs`'s two `ActorDecl` forwarding-annotation parameters (`any_service_binds_caller`'s and
`ws_open_hosts_for`'s own `actors: &HashMap<String, ActorDecl>`) retarget their import from
`bynk_syntax::ast::ActorDecl` to `bynk_check::actors::ActorDecl` — P6.49's re-export, for consistency
with `project.rs`'s own import path; neither site ever reads an `ActorDecl` field, only forwards the
table.

**Consequences.** `ast_importers`: **unaffected (5)** — `emit.rs` stays counted regardless; `CapRef`
dropped from its import list entirely (every remaining `.given` walk now goes through a lowering
function). Each conversion closes a real defect class: a place where the IR-derived value and the
AST-derived value could diverge with nothing to catch it, now structurally the same read. Verified:
zero-diff bless over the full e2e fixture corpus, `cargo test --workspace`, `cargo clippy --workspace
--all-targets`, `cargo fmt --all -- --check`.
