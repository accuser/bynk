---
level: minor
probes: emit_diagnostics, diagnostic_coverage
changelog: A field read on a value of a consumed context's type is checked — rejected for an `exports opaque` type (`bynk.context.opaque_inspection`), typed against the providing context's declaration for an unexported one, and rejected as `bynk.boundary.ambiguous_type` where two consumed contexts declare the name differently
---

## ADR: consumed-field-reads
title: Field reads on consumed contexts' values follow what the consumer may inspect
summary: Opaque exports reject field reads on values; unexported types are read through the consumer's projection

**Context.** A consumer can hold a value whose type it does not name: a
consumed service's return type that its context does not export, or the
unexported type of an exported record's field (§6.5, #1932). The checker
typed a field read by looking the receiver's type up in the consumer's own
table, and a miss returned no type and no diagnostic, so `b.zzz`, or a
`b.a` used at the wrong type, passed `bynkc check` and failed `tsc` (#1948).
Separately, `exports opaque` is meant to hide a type's shape, but
`bynk.context.opaque_inspection` caught only `T.X` on an opaque sum's type
name and `match` on an opaque value: `b.a` on an opaque export's value
checked.

**Decision.** A field read on a value whose checked type is an `exports
opaque` type of a consumed context is `bynk.context.opaque_inspection`,
however the value was bound. A field read on a value of a type the consumer's
table lacks falls back to `checker::projected_types` (#1941): the consumer's
table plus each consumed context's types that every declaring context agrees
on. The field's own type resolves against the same projection, so an unknown
field is `bynk.types.unknown_field` and a wrongly used one an ordinary type
error. A name the projection leaves ambiguous (declared with different shapes
by two consumed contexts, by neither the consumer) cannot be traced to one
declaration, and a read of it is `bynk.boundary.ambiguous_type`. Record spread
over such a base resolves its declaration the same way. Instance methods are
unchanged: a consumer calls only an export's methods, opaque or not.

**Consequences.** Every field read on a consumed context's value is either
typed or rejected; none is silently untyped. A consumer may still read the
fields of an unexported type it holds, since nothing about that shape is
hidden, only its name. Record spread over an opaque or unexported consumed
type is a construction outside the owning context and is not yet rejected;
that is left to its own change.
