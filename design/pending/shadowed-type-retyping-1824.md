---
level: minor
changelog: "A unit's own type that shadows a directly used commons' type is `bynk.uses.name_conflict` where a value crosses between the unit's code and an imported declaration's position of that type (`Run { repo: 42 }`, `r.repo + 1`); passing one such position straight to another stays legal (#1824)"
---
