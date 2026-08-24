---
level: minor
changelog: Arc C, slice 11 — `emit_type`/`emit_refined_type`/`emit_record_type`/`emit_sum_type` build real `bynk_ts` nodes internally, fully closing step (2) of the design pass's own decomposition order (the dispatcher and 3 object-builders deferred by slice 9, unblocked by slice 10's `emit_attached_methods` signature change) — adds `TsDecl::Interface.type_params`/readonly `TsTypeMember` members, `TsType::Intersection`, `TsType::Union`'s multi-line rendering mode, and `TsExpr::Arrow.generics`/`.return_type` to `bynk-ts`
---
