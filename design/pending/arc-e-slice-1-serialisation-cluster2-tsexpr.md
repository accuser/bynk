---
level: patch
changelog: Arc E, slice 1 — `serialisation.rs`'s expression serialise/deserialise builders (`serialise_field_expr`/`_via`/`_wire`, `serialise_expr`/`_via`, `deserialise_ref_via`, `serialise_ref_via`, `deserialise_expr`/`_via`) now build a real `bynk_ts::TsExpr` instead of a hand-written `String`, closing cluster 2 of `serialisation.rs`'s own four-cluster decomposition; `TsExpr::Arrow.body` widens to a new `TsArrowBody` (`Expr`/`Block`) to represent the `Float` non-finite guard's statement-bodied IIFE, the first real block-bodied arrow anywhere in the tree. No behaviour change — every fixture emits byte-identical TypeScript.
---
