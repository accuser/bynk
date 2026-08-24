---
level: minor
changelog: Arc C, slice 16 — `emit_provider`'s own per-op methods build real `bynk_ts::TsClassMethod` fragments (printed through a new `bynk_ts::print_class_method` entry point, each body source-mapped via a per-method sub-builder/`merge`), fully closing step (6) of the design pass's own decomposition order. The class's own wrapper and factory `const` stay hand-written text — a deliberate boundary (building the whole class as one tree would need every method's body captured for `Raw`-embedding, and this class's own real spacing differs from `TsDecl::Class`'s established policy), not a remaining gap
---
