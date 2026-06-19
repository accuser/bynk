# 0035 — `List`/`Map` built-in, immutable; lowerings, wire format, order

- **Status:** Accepted (v0.20b)
- **Spec:** §4.2.20, §6.2, §6.5, §7.3.7

## Context
Collections must serialise at boundaries (the whole point — Fetch's
missing-headers compromise, 0022, becomes retirable), so they cannot be
`bynk.*` library types: the boundary machinery has to know their shape.

## Decision
`List[T]` and `Map[K, V]` are **built-in generic types** at the TypeRef
level, like `Option`/`Result` (`Result` already carried two type
parameters, so `Map` had a template). Both are **immutable**: every
operation returns a new value, none mutates. Lowerings: `List[T]` →
`readonly T[]`; `Map[K, V]` → `ReadonlyMap<K, V>`.

Collections serialise at boundaries; nested functions still don't — the
0030 rule **looks through** collections, so a `List[Int -> Int]` field is
still `function_at_boundary`. A `List` serialises element-wise as a JSON
array. A `Map` serialises as an **entries array** `[[k, v], …]` — uniform
across `String`/`Int` keys, unlike a JSON object — and is
**insertion-ordered**, normatively: `keys` and the wire form are
deterministic, and updating an existing key keeps its position (JS `Map`
semantics, now spec semantics).

## Consequences
Handlers may take and return collections; per-instantiation
`serialise_List_<T>` / `deserialise_Map_<K>_<V>` helpers follow the
existing Result/Option pattern. Collection covariance holds for element and
value positions; `Map` *keys* must match exactly under `compatible` — key
widening would split a map's keys across refined/base identities at lookup.
