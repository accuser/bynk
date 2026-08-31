---
level: patch
changelog: Slice 3.1 of the IR cutover (`#1542`) — `bynk-lower`'s two live `todo!()` gaps close: a bare free-function-value reference now lowers to a new `IrExprKind::FnRef`, and the previously-suspected missing-`Callee` case turned out to already be unreachable once traced against the real checker pipeline
---
