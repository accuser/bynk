---
level: patch
changelog: Resolves Arc F item 4's investigation (#1457) — `emit.rs`'s `emit_agent` and `emit_ws_do_method` each re-implemented `append_deps_field`'s exact splice pattern inline instead of calling the shared helper (added by review of #1209 specifically to collapse this repetition); both now call it directly. The grounding read found the real conversion of `cap_ref_ty`/`build_deps_object_ty_with_surface`/`workers_env_ty`/`surface_ty` to real `bynk_ts::TsType` nodes tractable and scoped it as its own issue (#1463, "Arc F, slice 4") rather than converting in this pass. Zero emitted-output change.
---
