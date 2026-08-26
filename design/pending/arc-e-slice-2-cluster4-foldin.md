---
level: patch
changelog: Arc E, slice 2 — `serialisation.rs`'s duplicate qualified-type-text renderer (`ts_type_ref_qualified`/`ts_inner_type`) is deleted and its 9 real call sites (8 internal, plus `lower.rs`'s `Json.decode[T]` test-scaffold arm) fold into the general `emitter.rs::ts_type_ref_qualified_multi_ts_type` renderer via a new adapter, `serialisation::qualified_ts_type`, that bridges this file's dotted-prefix qualifier convention onto that renderer's bare-namespace one. No behaviour change — every fixture emits byte-identical TypeScript.
---
