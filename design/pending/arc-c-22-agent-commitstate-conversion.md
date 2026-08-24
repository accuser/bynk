---
level: minor
changelog: Arc C, slice 22 — `emit_agent`'s `commitState` method converts to a real `bynk_ts::TsClassMethod` fragment, closing step (9)'s own third sub-slice fully (the deferred remainder #1371 split off). Corrects that slice's own stated expectation: `commitState`'s predicate lowering needs no source-map sub-builder care at all — `record_span` is a documented no-op with no attached builder, and neither `LowerCtx` it constructs is ever given one
---
