# 0461 — A unit's own value hides a `uses`-imported type of its name; writing the name is the conflict

- **Status:** Accepted (v0.330.5)

**Context.** A `uses`-imported type holds a value slot in the emitted module: a
record emits `export const T = …` beside its interface, and the module imports
it as a value. A unit's own function, asset, service, agent or bodied provider of
the same name takes that slot. The checker accepted the pair, and the module
failed `tsc` wherever the unit wrote the type's name (#1927, review of #1926).
Probing showed how far that reached:
- A type the unit only *reaches* through an imported declaration never clashed.
- A value of the type flowing through the unit (`mk().cents`) never clashed.
- Constructing a bare variant of an imported sum (`Done`) did clash, because it
  emits `Status.Done`.
- A unit suite clashed even if no case named the type, because its test module
  bound both names.
- A local value named like a type a consumed context exports clashed the same
  way.

**Decision.**

- One rule for every import, as #1925 set for imported functions and assets: a
  unit's own declaration hides a `uses`-imported one of its name, and writing
  the hidden name is the conflict (`bynk.uses.name_conflict`). An unused shadow
  stays legal.
- For a type, "hidden" is #1807's state: resolvable but not nameable. The type
  stays in the composed table, so imported declarations that use it check and
  its values flow. The naming gate (`resolver::check_shadowed_type_names`, and
  `_in_suite`) walks every position #1807's gate walks, plus the construction
  of a bare variant of a hidden sum. Matching a bare variant emits the tag
  alone, so it stays legal.
- A local value that can answer a receiver does. A suite's `Status.GET("/")`
  addresses a service `Status`, and `Status.url` reads an asset `Status`. The
  resolver lets the asset answer before the type, and the test emitter defers
  to the checker's `Callee::TestService` before treating the receiver as a
  type.
- The emitter still names a hidden type wherever the lowering annotates a
  checked expression's type (#1778: a kernel's `(__x: T) => …`, an `if` slot,
  `List.empty`). In the module that hides it, `T` is the local value, and for
  an agent or provider that's a class, whose instance type captures the
  annotation. So while that module is emitted, a hidden type is spelled as an
  inline import type, `import("<module>").T` (`emitter::type_renames`, review
  of #1933). Agents are never `Ty::Named`, so within the unit the hidden name
  always means the imported type.
- A unit suite names no more than its target. Its test module leaves out the
  value (constructors) of a `uses`d type the target hides, but keeps its type
  alias (`type T = ns.T`). The alias lives in type space beside the target
  value's `const`, and the lowering's annotations need it.
- When two used commons declare the hidden name, one as a type and one as a
  function, the type gates the name whichever `uses` comes first. The local
  value hides both, but only the type stays composed (review of #1933).
- A local type or event over an imported type is #1824's type shadowing, and
  unchanged.
- Consumed exports follow that path's own precedent. A context's local value
  named like a type it consumes is `bynk.consumes.name_conflict` at the
  `consumes` clause, as a local type of that name already was. #1807's hidden
  state deliberately excludes context-owned types.
- Not chosen: rejecting at the declaration, which would hold types to a
  stricter rule than functions and assets; and letting the source name both
  the type and the value through emitter aliases, which would allow across
  `uses` what the same-unit rule forbids, while `Amount.x` stays ambiguous in
  Bynk source. (The emitter's inline import type above is not that: the
  source still may not write the name.)

**Consequences.** No working program is rejected through `uses`: each newly
rejected program wrote the hidden name, and its module failed `tsc`. Through
`consumes`, a local value named like an export is rejected even if the unit
never names the export, as a local type already was. #1925's matrix test gains
an imported-type row, and focused tests pin bare variants, flowing values, the
asset receiver and the consumed export.
