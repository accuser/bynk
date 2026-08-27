---
level: patch
changelog: Arc E slice 6 converts `bynk-emit`'s `emit_sum`/`emit_sum_codec` to build real `TsDecl`/`TsStmt`/`TsStmt::Switch` trees instead of `writeln!`-ing strings, adds `TsSwitchCase::case_braced` to `bynk-ts` for a real unbraced-non-default-case shape, and narrows the sum-codec's own `(value as any).<field>` cast to a bare, TypeScript-narrowed `value.<field>` (`ts_any` 31 → 30), reblessing the small fixture set that cast touched.
---
