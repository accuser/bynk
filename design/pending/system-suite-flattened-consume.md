---
level: patch
changelog: "A `system` suite against a context that consumes a unit only for its capabilities (`consumes bynk { Clock }`, an adapter) now runs (#1856). The suite stood up every unit in the target's `consumes` closure as a participant Worker, so it imported `../workers/bynk/handlers.js`, which is never emitted (TS2307), and wired it a Service Binding the deployment never has. Only a context is a participant now: a capability-only unit is provided in-process, as the Worker's composition root provides it"
---
