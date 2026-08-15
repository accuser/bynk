---
level: patch
changelog: "bynk-emit's emit_agent reads each Cell/Map/Cache/Log state field's TyId through the new lower_store_field_shape_ir IR reader (a shape-only sibling of lower_store_field_ir that never lowers a Cell field's init expression) instead of ts_type_ref on the raw AST TypeRef, and resolve_store_field_ty now falls back to Ty::Unit on an unresolvable store-field type instead of an ADR 0334 panic, matching the checker's own silent-fallback posture for a state the checker accepts but never actually validates (internal only, byte-identical output)"
---
