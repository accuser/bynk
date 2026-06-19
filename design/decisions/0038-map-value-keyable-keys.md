# 0038 — `Map` keys are value-keyable only

- **Status:** Accepted (v0.20b)
- **Spec:** §5.10, §6.2

## Context
The `ReadonlyMap` lowering compares object keys by reference — a record key
would make two structurally-equal keys distinct entries, silently. Value
equality for arbitrary types isn't expressible without bounded generics
(deferred).

## Decision
`Map` keys are confined to **value-keyable types**: `String`, `Int`, and
refined/opaque types over them (branded primitives keep JS value equality).
Record, sum, collection, and function keys are rejected with
`bynk.types.unkeyable_map_key`, checked at the resolver's type-reference
walk — the chokepoint every written `Map[K, V]` passes through. A **type
parameter is admitted in key position** (so `getOr[K, V](m: Map[K, V], …)`
is writable): it can only ever be instantiated through a concrete
`Map[K, V]` reference elsewhere, and that site is checked — by induction no
unkeyable map can be constructed.

## Consequences
The entries-array wire format (0035) deserialises keys with ordinary
refined-type re-validation. Revisit with bounded generics for
value-equality record keys.
