# 0377 — `collect_handler_labels`'s and `ws_open_hosts_for`'s `ServiceProtocol::WebSocket` matches read `lower_protocol_ir_from_commons` instead

- **Status:** Accepted (v0.249.29)

summary: Corrects the record — P6.30/P6.31's "not reachable" finding does not apply to these two call sites, which hold a `TypedCommons` directly

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b, Phase H) found two of
`emit.rs`'s three remaining `ServiceProtocol` reads convertible, contradicting P6.30/P6.31's own
"not reachable" finding for `ProtocolIr` — that finding was specific to `emit_worker_compose`/
`emit_worker_entry`, which hold only a `UnitTable` with no `TypedCommons` in scope. `collect_handler_labels`
and `ws_open_hosts_for` are different call sites: both already take `commons: &TypedCommons` directly,
so `ir::lower::lower_protocol_ir_from_commons(&ServiceProtocol, &TypedCommons) -> ProtocolIr` (P6.24a)
is reachable with no context threading needed.

**Decision.** `collect_handler_labels`'s WebSocket guard (`matches!(s.protocol, ServiceProtocol::WebSocket
{ .. })`, gating whether an `on message` handler's label is `"message"` or a synthesised queue-style
name) now matches `lower_protocol_ir_from_commons(&s.protocol, commons)` against `ProtocolIr::WebSocket
{ .. }` instead.

`ws_open_hosts_for` converts its own destructuring `let ServiceProtocol::WebSocket { out_type, in_type }
= &s.protocol else { continue };` the same way, and — since the lowering call already resolves both
frame types to `TyId` — `WsOpenHost::{out_type: &'a TypeRef, in_type: &'a TypeRef}` become `{out_ty:
TyId, in_ty: TyId}`. This cascades through every renderer that reads them:

- Three `ts_type_ref(host.out_type)` sites (`emit_ws_do_method`, `emit_ws_open_fetch_branch`,
  `emit_ws_dispatch_handlers`) become `ts_ty(host.out_ty, tys)` — `emit_ws_do_method` already had a
  `TypedCommons` in scope (`commons.tys()`); `emit_ws_open_fetch_branch` gained a `tys: &Arc<Types>`
  parameter, threaded from `emit_agent`'s own `let tys = commons.tys();`.
- `serialisation::deserialise_expr(host.in_type, ...)` — the excluded codec renderer's own boundary,
  `TypeRef`-driven by definition (P6.33) — now round-trips `host.in_ty` back via `ty_to_type_ref`.
  `ty_to_type_ref` only returns `None` for a function/effect/type-variable `Ty`, none of which a `from
  websocket` service's own `in:` frame type can resolve to (`check_service_protocols` already
  constrains it to a codec-eligible type); a `None` here panics with an explicit internal-error
  message rather than silently degrading, the same ADR 0334 posture this track has used throughout for
  a resolution miss a certified program cannot actually produce.
- The `resolve_ty` closure survives (it still resolves the *message handler's own* `p.type_ref`, a
  raw AST param type outside this slice's scope) but loses its second argument: `resolve_ty(&p.type_ref)
  == resolve_ty(host.in_type)` becomes `resolve_ty(&p.type_ref) == host.in_ty`, since `host.in_ty` is
  already resolved — no need to re-derive it.

`ServiceProtocol` is now unused in `emit.rs` and dropped from its import list.

**Consequences.** `ast_importers`: **unaffected (5)**. Verified with extra care given the codec-boundary
touch: zero-diff bless over the full e2e fixture corpus, `tsc --strict` verification (all six checks),
and the live `ws_behaviour.rs` behavioural test (`websocket_chatroom_runs_on_bundle`, which actually
opens a socket and exchanges frames against the emitted output, not just diffs bytes), plus `cargo test
--workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all -- --check`.
