---
level: minor
changelog: Arc C, slice 37 — the top-level module assemblers (`emit_integration_module`/`emit_test_module` in `tests_emit.rs`) build real `bynk_ts` fragments, the seventh and final of `tests_emit.rs`'s own 7 proposed slices (per its dedicated grounding pass) — closing Arc C entirely. Adds `TsDecl::ImportDefault` (a default import, `bynk-emit`'s first real one) to `bynk-ts`. No behaviour change — every test/integration module emits byte-identical TypeScript.
---
