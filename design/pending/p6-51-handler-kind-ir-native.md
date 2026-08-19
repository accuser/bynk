---
level: patch
changelog: "P6.51: IrHttpMethod gains as_str(); every HandlerKind read in emitter/emit.rs (collect_handler_labels, emit_service, emit_make_surface, emit_agent, ws_open_hosts_for -- 20+ sites) now reads lower_handler_kind_ir instead of matching the raw AST HandlerKind. ast_importers: unaffected (5) -- emit.rs stays counted via use super::*."
---

## ADR: p6-51-handler-kind-ir-native

title: Every `HandlerKind` read in `emitter/emit.rs` reads `lower_handler_kind_ir` instead

summary: One total conversion replaces every hand-written `HandlerKind` match across five functions — 32 places to miss a new handler kind become one

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b, Phase H) named `emit.rs`'s
`HandlerKind` matches as its single largest mechanical conversion — the *decision* half of "which
branch to take" re-derived from the raw AST enum at every one of five functions
(`collect_handler_labels`, `emit_service`, `emit_make_surface`, `emit_agent`, `ws_open_hosts_for`),
when the existing, pure `ir::lower::lower_handler_kind_ir(&HandlerKind) -> IrHandlerKind`
(`ir/lower.rs`, P6.24a) already provides the identical classification with no `TypedCommons`/
`CheckedProgram` needed at any of these call sites.

**Decision.** Converted every real `HandlerKind::` match in `emit.rs` to match
`lower_handler_kind_ir(&h.kind)` (or `&handler.kind`) against `IrHandlerKind` instead — field-for-field
identical arms, same guards (`ws_proto`, the `HandlerKind::Message if matches!(s.protocol, ...)`
WebSocket guard, unchanged — `ServiceProtocol` conversion is P6.52's own scope, not this slice's).
`emit_service`'s own loop body computes `handler_kind_ir` once and reuses it across
`is_ws_handler`/`kind_name`/`schema_dispatch_env_binder`/the pattern-guard check — four reads that
were four separate raw-AST matches before, now one lowering call.

Gave `IrHttpMethod` an `as_str()` (`ir.rs`, field-for-field mirror of `bynk_syntax::ast::HttpMethod
::as_str`) and added `http_handler_method_name_ir` (`emit.rs`), sharing its body with the existing
`http_handler_method_name` via a new private `http_handler_method_name_from_str(&str, &str) ->
String` helper — the two `HttpMethod`/`IrHttpMethod` enums render to identical strings, so this closes
both of `emit.rs`'s own `HandlerKind::Http { method, path }` sites (`collect_handler_labels`,
`emit_service`) without widening `http_handler_method_name`'s own signature, which stays as-is for its
six other call sites elsewhere in the crate (`workers.rs`, `workers_entry.rs`, `project/tests_emit.rs`,
`emitter/lower.rs` — out of this slice's scope, `emitter/lower.rs`'s two are P6.57's own target).

`HandlerKind` is now unused in `emit.rs` and dropped from its import list entirely (two doc-comment
mentions left as-is — they're prose, not code). `HttpMethod` stays, for `http_handler_method_name`'s
own unchanged signature.

**Consequences.** `ast_importers`: **unaffected (5)** — `emit.rs` stays counted through its own `use
super::*;` while `emitter.rs` imports the AST, and this slice does not touch `emitter.rs`. R6.13
defect closure: one total conversion replaces roughly 20 hand-written matches across five functions —
20 places a future handler kind could be added and missed become one. Verified: zero-diff bless over
the full e2e fixture corpus, `tsc --strict` verification (`bynkc/tests/tsc_verify.rs`, all six checks
— this slice changes emitted TS-shaping decisions), `cargo test --workspace`, `cargo clippy
--workspace --all-targets`, `cargo fmt --all -- --check`.
