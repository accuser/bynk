# 0460 — A unit's services, agents and providers share one namespace with its functions, assets, types and events

- **Status:** Accepted (v0.330.3)

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
- Across `uses`, a local declaration of any kind shadows an imported function
  or asset of its name, as the emitted module's local declaration does: both
  compositions leave the import out (`compose_unit_symbols`,
  `compose_unit_assets`, through one predicate, `symbols::local_value_kind`).
  Before, only a same-kind local shadowed it, so the checker resolved
  `thing()` to the import beside a local `type thing`, against the module.
  A reference the local declaration cannot answer is `bynk.uses.name_conflict`
  at the reference, naming both, in function bodies (resolver) and handler
  bodies (checker). One it can answer resolves to it: a call to a local agent
  constructs the agent. An unused shadow stays legal, so no working program
  is rejected.
- Suites follow their target. A case names what the target names: a unit
  suite's view and test module leave a hidden import out (before, the module
  bound `const logo` for both the target's service and the used commons' asset,
  and failed `tsc` even when no case named it), and naming one is the same
  conflict. A `system` suite, whose cases name participants by qualified name,
  sees it as an unknown name. The file still ships, since the commons that
  declares it may use it.
- Not chosen: renaming services in the emitted TypeScript. `tsc` would pass,
  but a name would still mean two things in a suite.

**Consequences.** No working program is rejected: each newly rejected program
already emitted TypeScript that failed `tsc`. A checker test runs every pair of kinds, in both
orders and casings, and matches `CommonsItem` exhaustively, so a new
declaration kind does not compile there until it takes a side. A second test
runs every local kind against an imported function and asset, unused and used
in a function body and a handler body. One cell keeps another code: in a
function body, `thing.url` on a local type reads as a static member of the
type (`bynk.resolve.unknown_static_member`), rejected as before.

Not covered: a unit's value named like a type it imports via `uses`. An
imported type holds a value slot in the module too, so that pair still fails
`tsc` once the type is used (#1927, which decides the rule). Two used commons
declaring a function or asset the unit hides are no ambiguity:
`phase_uses_name_conflicts` shares `local_value_kind`.
