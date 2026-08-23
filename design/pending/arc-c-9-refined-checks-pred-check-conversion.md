---
level: minor
changelog: Arc C, slice 9 — `emit_refined_checks`/`emit_pred_check` build real `bynk_ts` nodes internally, correcting step (2) of the design pass's own decomposition order (3 of 6 named functions are entangled with step (3)'s not-yet-converted `emit_attached_methods`, deferred to a follow-up slice gated on a `TsObjectEntry::Raw` decision) and closing a real, currently-unreachable double-escaping bug in how `PredKind::Matches`'s own pre-escaped message text would have been carried
---
