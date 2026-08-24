---
level: minor
changelog: Arc C, slice 15 — `emit_capability` builds real `bynk_ts` nodes internally (`TsTypeMember::Method` gains `generics`/`doc` fields), splitting step (6) of the design pass's own decomposition order the same way slice 12 split step (4) — `emit_provider`'s own conversion (a full class with N real method bodies, each needing the established sub-builder/`merge` source-map treatment individually) is a named, deferred remainder, not converted by this slice
---
