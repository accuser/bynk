---
level: patch
changelog: 'A service, agent or bodied provider can no longer share its name with a function, asset, type, event, or each other in the same unit (`bynk.resolve.name_conflict`): the checker accepted these, and the emitted TypeScript declared the name twice and failed `tsc` (#1925)'
---

## ADR: one-value-namespace
title: A unit's services, agents and providers share one namespace with its functions, assets, types and events
summary: Every top-level declaration except a capability or an actor is one scope of the emitted module, so none may share a name

**Context.** The emitter writes a unit's functions, assets, types, events,
services, agents and bodied providers into one TypeScript module scope
(`export const thing = …`, `export class Name`). The checker compared only
functions, types, events and assets, so a service, agent or provider named
like any of them, or like each other, checked clean, and the build failed
`tsc` with a duplicate declaration. Each of the 15 such pairs of kinds, and
two providers under one implementation name, fails `tsc --strict` (#1925,
found in the review of #1924).
In a suite the clash is visible in Bynk itself: with `asset api` and
`service api`, both `api.GET("/")` and `api.url` checked.

**Decision.**

- One namespace per unit: a function, asset, type or event, service, agent
  and provider with a body (by its implementation name) MUST NOT share a
  name, across all of a unit's files: `bynk.resolve.name_conflict`, reported
  at the service, agent or provider. Two providers of different capabilities
  under one implementation name conflict too.
- Capabilities and actors keep their own namespaces: they emit no
  declaration under their name in that scope, and every pairing with them
  compiles.
- An external (bodiless) provider emits no class, so it takes no name.
- Not chosen: renaming services in the emitted TypeScript. `tsc` would pass,
  but a name would still mean two things in a suite.

**Consequences.** No working program is rejected: each newly rejected program
already emitted TypeScript that failed `tsc`. A checker test runs every pair of kinds, in both
orders and casings, and matches `CommonsItem` exhaustively, so a new
declaration kind does not compile there until it takes a side. Shadowing
across `uses` (a local service shadowing an imported function) is a separate
rule (#1925, second half).
