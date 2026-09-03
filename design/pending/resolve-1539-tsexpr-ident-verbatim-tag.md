---
level: patch
changelog: bynk-ts's `TsExpr::Ident` is a real identifier now, not an untagged `Verbatim`-shaped escape hatch — 4 `bynk-emit` call sites that smuggled a `this.<method>` member access or a `!(<pred>)` unary through it build the matching real node instead, and 9 more route through a new tagged `TsExpr::VerbatimExpr`, visible to `verbatim_origins`/`verbatim_sites` (#1539)
---
