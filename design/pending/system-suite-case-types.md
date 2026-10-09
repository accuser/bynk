---
level: patch
changelog: "A `system` suite's case is lowered with its checked types, as a unit suite's is (#1855). The emit path lowered it with an empty type table, so a literal at a refined record field (`Line { sku: \"o1\", qty: 2 }`) went to the codec unbranded and the test module failed `tsc` (TS2322) though `check` accepted it. The case is now typed on the emit path too, so such a literal is branded, and an `expect` over primitives compares with `===` rather than the structural `__bynkEq`, matching the unit tier"
---
