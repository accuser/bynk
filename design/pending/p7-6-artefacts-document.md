---
level: minor
changelog: "P7.6: `Artefacts { docs: BTreeMap<PathBuf, Document> }` (Document::Ts/Toml/Json/Js/SourceMap/DebugSidecar) replaces `ProjectOutput.files: Vec<CompiledFile>` outright across bynk-emit, bynk-driver, bynk-strip, and bynk-wasm -- every real producer and consumer now reads/writes typed documents, not pre-rendered strings. `wrangler.toml`'s real `TomlDocument` reaches the write boundary unstringified, and `bynk-strip`'s own `main =` patch for a stripped Worker's manifest is now a structural in-tree mutation instead of a print-then-reparse. Source maps and debug sidecars become their own typed sibling documents, derived once instead of independently by two functions. No observable output change (#1309)."
---
