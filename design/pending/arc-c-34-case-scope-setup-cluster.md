---
level: minor
changelog: Arc C, slice 34 — the case/scope-setup cluster (`emit_test_scope_setup`/`emit_test_case_function` in `tests_emit.rs`) builds real `bynk_ts` fragments, the fourth of `tests_emit.rs`'s own 7 proposed slices (per its dedicated grounding pass). Adds `bynk_ts::TsBinaryOp::InstanceOf`. No behaviour change — every fixture reaching a test case emits byte-identical TypeScript, with source-map splice offsets confirmed unaffected.
---
