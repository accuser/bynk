---
level: patch
changelog: "bynk-emit's emit_consumed_context_helpers now resolves a subscribed from Events(E) header's own event type through ProtocolIr::Events's already-resolved TyId (via lower_protocol_ir) instead of a raw match on ServiceProtocol::Events/TypeRef::Named, and emitter/lower.rs's cap_op_param_names (trace(Cap.op)/with-predicate observation lowering) now reads a capability op's parameter names off lower_op_sig_ir's already-resolved OpSig instead of a second, independent AST extraction; ast_importers stays at 7 (both files already counted for other reasons) (internal only, byte-identical output)"
---
