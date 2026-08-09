---
level: patch
changelog: P6.1 (design/tracks/the-ir.md §6) — bynk-emit gains an internal `ir`/`ir::lower` module pair: the full Part 6.2 `IrExpr`/`IrExprKind`/`IrStmt` shape, and a `&CheckedProgram → Ir` lowering pass implemented for Const/Local/Global/Record/Field/List/Block/If/And/Or/Not/Return/Await/Send/Pure. No consumer yet and no bynk-emit emission-behaviour change — P6.2 onward wires it in (#1141)
---
