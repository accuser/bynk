---
level: minor
changelog: "P7.9: bynk-emit's ts_base/ts_type_ref/ts_type_ref_qualified/ts_type_ref_qualified_multi/ts_type_ref_with/ts_ty construct a real bynk_ts::TsType internally and print it through a new bynk_ts::print_type, instead of hand-format!-ing text -- every function keeps its exact -> String signature, every real caller (~110-115 sites) unchanged. TsType::Array gains a readonly modifier; TsType::Fn and TsType::Union added (Union closes a real gap found during implementation: ts_ty's own Ty::ActorSum arm builds a genuine union-of-tagged-object-literals type no existing shape could represent). Zero output change, verified against the full fixture corpus and 23 new direct tests pinning every real type shape from both families (#1315)."
---
