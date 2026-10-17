---
level: minor
changelog: A literal passed straight to a cross-context call is admitted to the parameter's refined type, as at a local call, so `t.pay.charge(250)` for `cents: Cents` checks in a `system` case and from a consuming context instead of failing as `bynk.boundary.structural_mismatch`; a literal outside the refinement is `bynk.refine.literal_violates`, and a plain `Int` still does not cross as `Cents`
---
