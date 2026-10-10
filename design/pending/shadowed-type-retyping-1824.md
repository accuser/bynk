---
level: minor
changelog: "A unit's own type that shadows a directly used commons' type is `bynk.uses.name_conflict` where a value crosses between the unit's code and an imported declaration's position of that type (`Run { repo: 42 }`, `r.repo + 1`, a method or function argument); passing one such position straight to another stays legal (#1824). Before, such a program passed `check` and failed `tsc`. The rule is conservative: a value routed between two such positions through a `let`, a shorthand field or an imported generic is rejected too, though its TypeScript was correct; pass it directly, or rename the local type"
---
