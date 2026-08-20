---
level: patch
changelog: "P7.3: added a minimal typed TOML tree and printer (`bynk-emit/src/emitter/toml_doc.rs`) -- `emit_wrangler_toml` now builds a `TomlDocument` instead of writing text directly, and the printer escapes every string value unconditionally rather than only the two call sites that used to remember to (#1303). No observable output change: zero-diff across the whole `wrangler.toml` golden corpus."
---
