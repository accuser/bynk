---
level: patch
changelog: bynk-check gains TypedCommons::actor_bindings — a service handler's own resolved by <binder>: <Actor> actor binding (handler_actor_binding's own return value, context_checks.rs), keyed by the handler's own span and persisted into CheckedProgram rather than discarded once check_service_decls's per-handler loop moves on. Closes the sole blocker IrHandler's own doc comment (bynk-emit/src/ir.rs) named for lower_handler_ir (P6.9, #1167) to stop being agent-only: a post-certify consumer can now read a service handler's actor binding back, though lower_handler_ir itself is not widened to do so here (tracked separately, #1171). No bynk-check emission/diagnostic behaviour change — every existing e2e/checker fixture passes unmodified (#1170)
---
