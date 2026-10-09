---
level: patch
changelog: "`ExprKind::ConstructorCall` is gone. The parser has not built it since grammar v0.2 (`T.of(x)` and `T.Variant(x)` parse as a `MethodCall` on an `Ident` receiver), so its arms in the resolver, checker, linearity, context checks, wire defaults, emitter, formatter, LSP and sequence diagrams never ran and are removed with it, as is the hand-built node the slot-coverage test (#1832) used to cover it. No behaviour changes"
---
