---
level: patch
changelog: P6.0 (design/tracks/the-ir.md §6) — bynk-check gains a resolved `Callee` classification for every call-shaped expression check_call/check_static_call/check_method_call/check_cross_context_call/check_cross_context_capability_call/check_test_service_address dispatch, recorded once during checking (TypedCommons::callees) rather than re-derived; bynk-emit's lowering is unchanged and un-consumed by this slice (#1139)
---
