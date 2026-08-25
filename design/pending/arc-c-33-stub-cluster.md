---
level: minor
changelog: Arc C, slice 33 — the stub cluster (`emit_stub_class`/`emit_stub_rhs` in `tests_emit.rs`) builds real `bynk_ts` fragments, the third of `tests_emit.rs`'s own 7 proposed slices (per its dedicated grounding pass). Adds `bynk_ts::TsBinaryOp::LessThan` and `TsSwitchCase.default_braced`, and deletes the now-dead `emitter::ts_type_ref_qualified_multi`. No behaviour change — every fixture with a `stub` clause emits byte-identical TypeScript.
---
