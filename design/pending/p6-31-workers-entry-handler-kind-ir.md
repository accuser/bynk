---
level: patch
changelog: "P6.31: emitter/workers_entry.rs's eight HandlerKind matches (route collection for HTTP/cron/queue/websocket-open/event handlers) now dispatch on the checker-classified IrHandlerKind instead of the raw AST HandlerKind directly -- same pattern P6.30 established in emitter/workers.rs. The HttpRoute-building arm re-derives method/path from the original AST for HttpRoute's own still-AST-typed fields (Q7-settled); the Cron arm's expr is a plain String in both forms, so no re-derivation was needed there. HttpRoute/QueueRoute's own struct fields (method: HttpMethod, handler: Handler, msg_type: Option<TypeRef>) and the file's three ServiceProtocol checks are left as raw AST -- emit_worker_entry has no TypedCommons in scope (the same constraint P6.30 found), and widening HttpRoute/QueueRoute or IrHandler is an explicit stop-and-defer per the completion plan rather than a speculative widening. ast_importers unaffected (8) -- workers_entry.rs retains other, still-open AST names."
---

## ADR: workers-entry-handler-kind-ir

title: `emitter/workers_entry.rs`'s eight `HandlerKind` matches dispatch on `IrHandlerKind`; `HttpRoute`/`QueueRoute` and `ServiceProtocol` left as named residue

summary: Phase C of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.31) — the sibling conversion to P6.30, in `workers.rs`'s entry-point counterpart

**Context.** `emit_worker_entry` (and the route-collection loops it drives) matched raw AST
`HandlerKind` at eight sites to build the `http_routes`/`cron_routes`/`queue_routes`/`ws_open_routes`
tables and to locate `Call`/`Event` handlers for the compose surface — the same shape of dispatch
`emit_worker_compose` (`emitter/workers.rs`) had before P6.30. `lower_handler_kind_ir` (P6.24a) is
pure and unconditional, needing no `TypedCommons`/`CheckedProgram`, so every site converts directly.

**Decision.** All eight `HandlerKind` sites now dispatch on `lower_handler_kind_ir(&h.kind)`'s
`IrHandlerKind` result. Two shapes: the `Cron` arm's `expr` field is a plain `String` in both
`HandlerKind` and `IrHandlerKind`, so the IR match binds and uses it directly, no re-derivation needed;
the `Http` arm (building `HttpRoute`, whose own `method`/`handler` fields stay AST-typed, Q7-settled)
re-derives `method`/`path` from the original `h.kind` via the same safe `let HandlerKind::Http { .. }
= &h.kind else { unreachable!() }` pattern P6.30 established. The remaining six sites are plain
`matches!` predicates with no fields to extract, converting mechanically (the same idiom `emitter.rs`
already uses for its own `has_http`/`has_queue` checks).

**What stays raw AST, named rather than silently left unconverted — same constraint P6.30 found.**
`emit_worker_entry` and every helper it calls (`build_cors_services`, `build_security_services`, the
`Message`/queue-name lookup) have no `TypedCommons` in scope, only a `table: &UnitTable` — so the
file's three `ServiceProtocol` checks (`Queue`/`Http` ×2) are not convertible without threading a new
parameter through, out of proportion to this slice. `HttpRoute::method: HttpMethod` and
`::handler: Handler`, and `QueueRoute::msg_type: Option<TypeRef>`, are likewise left alone — per the
completion plan's own explicit guidance for this slice ("if `IrHandler` can't carry enough to render,
stop and let §6a.D decide rather than widening it speculatively"), widening these structs is a design
question for the re-settling slice (§6a.D), not a mechanical conversion this slice should make
unilaterally.

**Consequences.** `ast_importers`: **8 → 8**, unaffected — `emitter/workers_entry.rs` retains other,
still-open AST names (the three `ServiceProtocol` checks, `HttpRoute`/`QueueRoute`'s own fields, Q7
rendering-signature params). Verified by a full zero-diff bless against the entire e2e fixture corpus
and a full `cargo test --workspace`.
