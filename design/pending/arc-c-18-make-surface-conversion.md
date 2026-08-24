---
level: minor
changelog: Arc C, slice 18 — `emit_make_surface` converts fully to real `bynk_ts` nodes (a real `TsDecl::Function` returning a real `TsExpr::multiline_object_entries` of per-service `TsObjectEntry::Method` entries), with no opaque carve-out at all — the first slice in this track to close that cleanly. Splits step (8) of the design pass's own decomposition order: the cross-context lowering cluster, step (8)'s other half, is a real, separate, harder remainder, deferred rather than silently folded into "step (8) done"
---
