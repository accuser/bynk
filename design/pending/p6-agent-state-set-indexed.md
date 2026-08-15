---
level: patch
changelog: "bynk-emit's emit_agent now derives which store fields are Sets, and each Map field's @indexed key list, from the already-lowered StoreFieldIr/StoreKindIr rather than re-walking the raw AST store fields and their @indexed annotations a second time — the Set-derived TypeRef is still read for the rehydration boundary check (serialisation.rs's own TypeRef-driven boundary, unconverted) (internal only, byte-identical output)"
---
