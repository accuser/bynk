---
level: patch
changelog: "bynk-emit reads a handler's/provider's given clause through the new lower_handler_given_ir/lower_provider_given_ir IR readers everywhere it feeds a deps-type or cross-context decision (project.rs's plan_agent_given_deps and EmitProjectCtx::agent_method_givens, emitter.rs's agent_given_caps_used, emitter/emit.rs's effective_given/build_deps_object_ty_with_surface/cap_ref_ty, emitter/workers.rs's worker_cross_caps), instead of walking bynk_syntax::ast::CapRef directly at each site"
---
