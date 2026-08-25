---
level: minor
changelog: Arc C, slice 30 — `emit_project`'s own header/import/rebrand cluster (`write_header`/`write_header_single`/`emit_project_imports`/`emit_cross_context_namespace_imports`/`emit_context_rebrands`/`emit_boundary_helpers`, plus `emit_project` itself) builds real `bynk_ts` fragments, closing step (10) of the TypeScript-tree conversion track — every step in the conversion order is now landed or decided, leaving only the final `tests_emit.rs` pair. Adds `type_only` to `bynk_ts::TsDecl::ImportNamespace` along the way, matching its sibling `TsDecl::Import`. No behaviour change — every project-form fixture (all of them reach `emit_project`) emits byte-identical TypeScript.
---
