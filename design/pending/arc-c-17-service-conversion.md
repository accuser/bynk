---
level: minor
changelog: Arc C, slice 17 — `emit_service`'s own handlers build real `bynk_ts::TsObjectEntry::Method` fragments (every param, including the synthetic `connection`/`__bynkSchemaEnv` ones, converts to a real `bynk_ts::TsParam`; each body source-mapped via a two-level sub-builder/`merge`, since the source-mapped content is nested inside a further opaque events-dispatch IIFE blob), fully closing step (7) of the design pass's own decomposition order in one slice. The object wrapper, `deps`'s own dynamically-built type, and the handler's whole prologue/body/epilogue stay hand-written/opaque — deliberate boundaries, not remaining gaps
---
