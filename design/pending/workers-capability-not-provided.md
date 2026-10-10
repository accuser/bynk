---
level: minor
changelog: "Built for Workers, a context that requires a capability it declares (`given Mailer`) but has no `provides Mailer = …` for is now `bynk.capability.not_provided` (#1822). The Workers composition root passed `{}` where the capability was required, so the Worker failed `tsc` (TS2345) while the bundle build passed. The bundle target is unchanged: there the host may supply the capability through `__makeSurface(deps)`"
---
