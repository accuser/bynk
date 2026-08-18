---
level: patch
changelog: "P6.18: `EmitProjectCtx::imported_methods`/`emit_forwarded_methods` (a `uses`-imported type's attached-method forwarding) reads a resolved `FnSig` (`params`/`return_ty` as real `TyId`s, via a new narrow `lower_fn_sig_ir_from_types` reader) instead of a raw `FnDecl`'s unresolved `TypeRef`s — the first real emitter consumer of `bynk-emit::ir`'s signature-only lowering pattern for a `fn`, byte-identical output confirmed by zero-diff bless"
---
