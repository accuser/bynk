# 0362 — `BodyMode::TestCase::test_service_handlers` carries `IrHandlerKind`; `system_http_route_body`/`HttpMethod::from_ident` found narrower in scope than the plan's own row

- **Status:** Accepted (v0.249.14)

summary: Phase E of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.37) — the achievable third of this row lands; the rest investigated and found out of reach, matching this phase's own recurring pattern

**Context.** This row named three sites: `BodyMode::TestCase`'s `test_service_handlers` field, its
sibling `system_http_route_body`, and the two `HttpMethod::from_ident` call sites, all in
`emitter/lower.rs` (mirrored in `emitter.rs`'s own `BodyMode` definition).

**`test_service_handlers`: converted.** Its only real use — recovering a cron/queue handler's own
position index among same-kind handlers, to match the emitted `cron_<svc>_<i>`/`queue_<svc>_<i>` key
— only ever matches `HandlerKind::Cron { expr }` (a plain `String`) and `HandlerKind::Message` (no
fields), both of which `IrHandlerKind` mirrors exactly (P6.24a). Converted the field
(`HashMap<String, Vec<IrHandlerKind>>`), the accessor, the `BodyMode::TestCase` definition in
`emitter.rs`, and `project/tests_emit.rs`'s own `target_service_handler_kinds` builder (an excluded
file — cutting over its own return type costs nothing against the probe, and it already had
`lower_handler_kind_ir` available).

**`system_http_route_body`: investigated, left as raw AST.** Its `TypeRef` value is inserted at
`emit_system_http_support` (`project/tests_emit.rs`), which walks a pre-check `UnitTable` with no
`TypedCommons`/`tys` resolution context in scope — there is no `TyId` available to store even if the
field's own type changed, the same "no resolution context at this pipeline stage" shape P6.20/P6.34
both found. Its sole consumer, `serialise_expr_via`, is `emitter/serialisation.rs`'s own codec
renderer — ruled phase 7 by P6.33. Converting this field would have no real IR-native target on either
end: no `TyId` to produce it, and a phase-7-excluded function to consume it. Left alone.

**`HttpMethod::from_ident`: investigated, left as raw AST.** Both call sites parse a call's own method
name into `HttpMethod` purely to pass it to `http_handler_method_name` — a Q7-deferred rendering-key
utility (`emitter/emit.rs`) that itself takes `HttpMethod` by value, threaded through further AST-typed
helpers. The identical "downstream Q7 consumer needs the AST type regardless" shape this plan's own
Phase C/E slices (P6.30's `Http` arm, P6.31's route tables) already established and left alone for the
same reason.

**Consequences.** `ast_importers`: **7 → 7**, unaffected — `emitter/lower.rs` retains other, still-open
AST surface (`system_http_route_body`, `HttpMethod::from_ident`, Q7 rendering params, and the
`#[cfg(test)]` residue P6.38 targets next) regardless of this slice landing. Verified by a full
zero-diff bless against the entire e2e fixture corpus and a full `cargo test --workspace`.
