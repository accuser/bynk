---
level: patch
changelog: Arc E slice 5 converts `bynk-emit`'s `emit_record`/`emit_record_codec` to build real `TsDecl`/`TsStmt` trees instead of `writeln!`-ing strings, with zero emitted-output change, and adds a narrow `TsBinaryOp::In` to `bynk-ts` for the field-default prevalidation guard this needed.
---
