---
level: minor
changelog: A literal passed straight to a cross-context call is admitted to the parameter's refined type, checked against the callee's own declaration, so `t.pay.charge(250)` for `cents: Cents` checks in a `system` case and from a consuming context, with or without `uses` of the declaring commons, instead of failing as `bynk.boundary.structural_mismatch`; a literal outside the callee's refinement is `bynk.refine.literal_violates`, and a plain `Int` or a caller's own same-named type still does not cross
---
