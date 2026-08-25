---
level: minor
changelog: Arc C, slice 28 — the ICU-formatting cluster (`emit_message_entry_renderer`/`emit_icu_placeholder`/`emit_sub_message`) builds real `bynk_ts::TsExpr` fragments instead of hand-written strings, closing step (11) of the TypeScript-tree conversion track. Adds two small `bynk-ts` primitives along the way: `TsBinaryOp::Add` (string concatenation) and a new `print_expr` fragment entry point. No behaviour change — every message-bundle fixture (literal text, plain placeholders, and all four ICU kinds: plural/select/number/date) emits byte-identical TypeScript.
---
