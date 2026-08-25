---
level: minor
changelog: Arc C, slice 32 — the small independent leaves (`emit_integration_harness`/`emit_test_deps`/`emit_ns_destructure`/`observation_call_record_types` in `tests_emit.rs`) build real `bynk_ts` fragments, the second of `tests_emit.rs`'s own 7 proposed slices (per its dedicated grounding pass). Also deletes the now-dead `emitter::ts_type_ref_qualified`, rerouting its own direct unit tests through its already-existing `TsType`-returning twin. No behaviour change — every affected fixture emits byte-identical TypeScript.
---
