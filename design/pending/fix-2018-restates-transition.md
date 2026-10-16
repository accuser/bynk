---
level: patch
changelog: `bynk.history.restates_invariant` now also catches a history property whose predicate restates a declared `transition` or `invariant` written with `is` (e.g. `run.all((s) => s.old.status is Closed implies s.new.status is Closed)`), as ADR 0155 and the testing reference already said it did (#2018)
---
