---
level: patch
changelog: Arc E slice 7 converts `bynk-emit`'s `emit_generic_helpers_qualified`'s remaining `ResultInst`/`OptionInst`/`ListInst`/`MapInst` arms to build real `TsDecl`/`TsStmt` trees instead of `write!`-ing strings, and adds a narrow C-style `TsStmt::For` to `bynk-ts` for the `List`/`Map` deserialise-side indexed element loop.
---
