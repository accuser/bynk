---
level: patch
changelog: "Four walks that skipped `match`-arm guards now visit them (#1800). An agent state write (`:=`) inside a guard was never committed: the handler got no commit wrapper, so the write was lost, and on the bundle target the output failed `tsc`. An effect (`<-`) in a guard inside a lambda was rejected as `bynk.effect.bind_in_pure_context`, where the same effect in an arm body made the lambda effectful. In the editor, extract-function now finds a run inside a guard, and no longer lifts a run whose guard holds a `<-` (into a function returning `()`) or a `:=`"
---
