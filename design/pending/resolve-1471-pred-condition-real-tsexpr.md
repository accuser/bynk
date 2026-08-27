---
level: patch
changelog: Resolves #1471 — `pred_condition_and_message`'s condition side now builds a real `bynk_ts::TsExpr` (`Binary`/`Call` nodes) instead of `format!`ing raw JS text, unblocked by two new `bynk_ts::TsBinaryOp` variants, `GreaterThanEq`/`LessThanEq`, at the same precedence tier as `GreaterThan`/`LessThan`/`InstanceOf`/`In`. Both real callers (`emit::emit_pred_check`, `serialisation::emit_inline_pred_check`) now consume the real expression directly, dropping the opaque `Ident(cond)` wrap; the explicit `Paren` around it stays unconditionally, matching pre-#1471 output byte-for-byte. `msg` stays the same opaque, already-`escape_ts_string`-escaped `String` it always was. Zero emitted-TypeScript change; `ts_writes` drops from 860 to 851 as a side effect.
---
