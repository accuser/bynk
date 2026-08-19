---
level: patch
changelog: "P6.30: emit_worker_compose (emitter/workers.rs) dispatches on the checker-classified IrHandlerKind instead of matching the raw AST HandlerKind directly, for both its has_ws_open predicate and its main per-handler wrapper-selection match -- the decision of which wrapper to call is now IR-driven, though each wrapper's own body-rendering signature stays AST-parameter-driven (Q7-settled). emit_worker_compose has no TypedCommons/CheckedProgram in scope (only a table: &UnitTable), so the one remaining raw ServiceProtocol::WebSocket check inside the Message arm stays AST-typed -- lower_protocol_ir_from_commons is not reachable here without threading a new parameter through, a real scoping finding narrower than the completion plan's own estimate for this slice. ast_importers unaffected (8) -- workers.rs retains other, still-open AST names."
---

## ADR: worker-compose-handler-kind-ir

title: `emit_worker_compose` dispatches on `IrHandlerKind`, not raw AST `HandlerKind`; `ProtocolIr` conversion found not reachable here

summary: Phase C of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.30) — a declaration-read conversion narrower in scope than first estimated, and says so

**Context.** `emit_worker_compose`'s `has_ws_open` predicate and its main per-handler dispatch (seven
arms: `Call`/`Http`/`Cron`/`Message`/`Open`/`Close`/`Event`) both matched `bynk_syntax::ast::HandlerKind`
directly — the two remaining raw-AST-declaration reads in this function, the rest of which (the `Http`
arm's actor-seam selection) already reads `crate::ir::lower::lower_actor_seam_ir` following review of
#1209. `lower_handler_kind_ir(&HandlerKind) -> IrHandlerKind` (P6.24a) is pure and unconditional — no
`TypedCommons`/`CheckedProgram` needed, since it is a structural mirror that can never fail to resolve
— so both call sites were directly convertible with no new dependency threaded through.

**A genuine scoping finding, narrower than the completion plan's own estimate.** The plan's own P6.30
row named `lower_protocol_ir_from_commons` as reachable for the `Message` arm's
`matches!(service.protocol, ServiceProtocol::WebSocket { .. })` check. It is not: that function needs a
`&TypedCommons`, and `emit_worker_compose` only ever receives a `table: &UnitTable`
(`bynk-check::symbols`) — a project-wide compose-time symbol table, not a per-unit checked program.
Threading a new `TypedCommons` parameter through `emit_worker_compose` (and every one of its callers)
just to convert this one `ServiceProtocol` check is out of proportion to this slice; left as raw AST,
named explicitly rather than silently converted-around or silently left looking finished.

**Decision.** Both `HandlerKind` sites now match `lower_handler_kind_ir(&h.kind)`'s `IrHandlerKind`
result instead. The `Http` arm's own body still needs AST-typed `method`/`path` for its (Q7-settled,
still-AST-parameter-driven) wrapper functions — `emit_http_wrapper`/`emit_http_sum_wrapper`/
`emit_http_oidc_wrapper` each take `HttpMethod`, threaded through many further helpers
(`http_handler_method_name`, etc.) — so widening the arm to also carry `IrHttpMethod` would cascade
signature changes well beyond this slice's own scope; the arm instead re-derives `method`/`path` from
the original `h.kind` via `let HandlerKind::Http { method, path } = &h.kind else { unreachable!() }`,
safe because `lower_handler_kind_ir` is a pure mirror of the same source. This is the established
dispatch-vs-render split every P6.21-era slice has used: the *decision* reads IR, the *rendering*
keeps its Q7-deferred AST parameters.

**Consequences.** `ast_importers`: **8 → 8**, unaffected — `emitter/workers.rs` retains other,
still-open AST names (the `ServiceProtocol::WebSocket` check above, plus Q7 rendering-signature
params). Verified by a full zero-diff bless against the entire e2e fixture corpus — the plan's own
named risk (a handler-shape ordering hazard in `lower_actor_seam_ir`'s `Sum`→`Bearer`→`Oidc`→`Caller`
resolution) turned out to be already closed by review of #1209 before this slice started (confirmed
live: `workers.rs`'s own comment on the `Http` arm records the swap and its safety argument), so this
slice carried materially less risk than the plan estimated. Full `cargo test --workspace` green.
