---
level: minor
changelog: "`==` is **structural**: records, sums, `Option`, `Result`, `List`, value `Map` and `Bytes` (including opaque) compare by value, and `==` on a type containing a function, `Effect`, `Query`, `Stream` or held `Connection` anywhere inside is rejected (`bynk.types.not_comparable`). Agent state is **stored in the wire shape** and decoded on load, so a `store` holding an enum, `Option`, `Result`, `Bytes` or value `Map` no longer faults with `RehydrationViolation` on every reload after its first write. An agent already bricked by that fault holds the old in-memory shape and must have its storage cleared once."
---

## ADR: structural-equality
title: `==` is structural, and defined only on equality-supporting types
summary: Equality compares values by shape through a runtime walker; a type containing a computation, stream or held value anywhere is not comparable

**Context.** Before #1652, `==`/`!=` lowered to host `===` for every type except
`Bytes` (ADR 0142 D4). That is reference equality on records, sums, `Option`,
`Result`, `List` and value `Map`s: `Some(1) == Some(1)` was `false`, a nullary
variant decoded from JSON was unequal to its constant, and an invariant written in
the spec's own idiom (`status == Paid implies …`) could only ever see its
antecedent true while the loaded value happened to be the same object as the
constant. ADR 0142 recorded this as "a separate decision", never taken. The
Settled type-system spec (§2.3.5) already specified structural equality for sums,
recursively. The checker rejected `==` only on a *top-level* `Stream` or
`Connection`, so `Option[Connection[F]]`, `List[(Int) -> Int]`, a record with an
`Effect` field and `Box[(Int) -> Int]` were all accepted. `is` patterns already
compared structurally (`x.tag === "Paid"`), and test stubs matched arguments by
`JSON.stringify`, a third semantics.

**Decision.**
1. `==`/`!=` are structural. The emitter dispatches on the operand's type
   (`lower_bin_op`), the same operand-typed shape as division:
   - a statically primitive operand (a base type other than `Bytes`, `()`, or a
     refined/opaque type over one) keeps host `===`/`!==`;
   - `Bytes`, including an opaque type over it, uses `__bynkBytesEqual`. Before
     this ADR the opaque case fell through to `===`.
   - everything else, and an operand with no recorded type, uses the runtime's
     `__bynkEq`.
2. `__bynkEq` walks the in-memory representation, with no type information. It
   uses `===` at primitive leaves (so IEEE `NaN != NaN` and `-0 == 0` hold at any
   depth), content for `Uint8Array`, element-wise comparison for arrays, size then
   key and value for `Map`, own keys and values for plain objects (so a sum's `tag`
   decides first), and identity for class instances. A type-directed `equals_T`
   per type was rejected: Bynk generic functions are not monomorphised, so `==` on
   a type parameter has no concrete type to dispatch on without dictionary
   passing.
3. The checker defines equality-supporting types recursively, as §2.3.5 does. A
   function, `Effect`, `Query`, `Stream` or held `Connection` anywhere inside the
   operand's type (inside a container, a generic argument, or a record or sum
   field reached through its declaration) rejects `==`. A new code,
   `bynk.types.not_comparable`, covers computations; `Stream` and `Connection`
   keep their existing codes.
4. Test stub argument patterns match with `__bynkEq`, so the language has one
   equality. The separate `JSON.stringify`-based `__bynkDeepEqual` is removed:
   under it every `Map` compared equal and `NaN` equalled `NaN`.

Rejecting `==` on non-primitive types instead was considered and rejected. It
would reject the spec's own Settled examples and the invariant idiom, lose
`Some(x) == y` and record `expect`s, and still leave stub matching needing
structural equality.

**Known limits.** Both fail permissively, as every case did before this ADR: the
walker compares the offending part by identity.
- A type argument inferred at a call to a generic function is not checked. `==`
  on a type parameter `T` is accepted in the generic body, so
  `fn same[T](a: T, b: T) -> Bool { a == b }` called as `same(f, h)` on two
  functions compiles. Closing it needs equality bounds inferred per generic
  function (transitively through the functions it calls) and checked at each
  instantiation, which is a checker feature of its own (#1688).
- A record imported from another unit is not walked through its fields, because
  the checker's type map holds only the current unit's declarations.

**Consequences.** Supersedes ADR 0142's note that whole-record `==` "remains
reference equality". Programs comparing structured values now get the answer the
spec states; no correct program depended on reference results. `is_keyable`
(`Map` keys, `distinct`, `groupBy`, `@indexed`) is unchanged: structural keys are a
possible follow-on, not part of this decision. Proved at runtime by
`bynkc/tests/fixtures/positive/1652_structural_equality` (behavioural, 9 cases)
and the runtime package's `equality.test.ts`. Negative fixtures
`1652_eq_*` pin the recursive rule.

## ADR: agent-state-wire-shape
title: Agent state is stored in the wire shape and decoded on load
summary: commitState serialises through the boundary codec and loadState decodes into state; no tolerant loader for the old in-memory shape

**Context.** ADR 0124 validated loaded agent state with the boundary deserialiser
(D1, "the same validator the HTTP/queue/event boundaries use"), but `commitState`
persisted the emitter's *in-memory* shape (`storage.put("state", s)`). Wherever
the two shapes differ (sums, enums, `Option` and `Result` are `tag`-shaped in
memory but `kind`-shaped on the wire; `Bytes` is a `Uint8Array` but base64 on the
wire; a value `Map` is a JS `Map` but an entries array on the wire), every load
after the first commit threw `RehydrationViolation`. Because a commit writes the
whole record, an agent with any such field anywhere was bricked by its first write
to *any* field (#1649). The gate also ran the full deserialiser on every handler
call and discarded the result, not once per cold load as ADR 0124's consequences
stated. Fixtures 139 and 155 blessed the faulty pair from June 2026, because no
test reloaded a key after writing it.

**Decision.**
1. Stored state is the **wire shape**. `commitState` writes
   `__encode<Agent>State(s)`, which serialises every codec-able value position (a
   `Cell`'s `T`, a store `Map`'s values, a `Cache` entry's `.v`, a `Log` entry's
   `.v`) with the boundary serialiser. Held maps, `Set`s and `@indexed` posting
   lists are already string-shaped and pass through.
2. `loadState` merges zero-then-stored, then `__rehydrate<Agent>State(merged,
   stored)` **decodes** each value position present in `stored` back into the
   in-memory shape, keeping ADR 0124's validation and its `RehydrationViolation`
   on failure. A field absent from `stored` keeps its in-memory zero and is not
   decoded, which keeps D4's additive evolution, now per field.
3. A stored key the current definition does not declare (a renamed or removed
   field) is carried through unchanged and written back. It is never silently
   dropped, so #539's migrations can still find it.
4. The other two readers of stored state decode the same way: the commit-time
   `transition` predicates' prior state (previously read raw and unvalidated), and
   the History-property driver.
5. **No tolerant loader** for the old in-memory shape. For every type that worked
   before, the in-memory and wire shapes coincide, so their stored bytes are
   unchanged. The only records in the old shape belong to agents already bricked
   by #1649. A generic `tag`→`kind` rewrite would corrupt any record with a field
   named `tag`, and threading a legacy mode through the shared deserialisers would
   risk widening the HTTP boundary. A bricked agent's storage must be cleared once
   (noted in the changelog).
6. A value with no wire form, such as a non-finite `Float`, fails **at commit** with
   the serialiser's existing error, so nothing persists and the agent is not
   bricked. That error surfaces as an untyped fault, not an
   `InvariantViolation`-class one; a typed agent-fault channel is the existing
   named follow-on (ADR 0107), not this decision.

Validating the in-memory shape instead (a third codec family) was rejected. It
breaks D1's "same validator" in spirit, and it leaves the disk format as emitter
internals with no canonical form, where #539's schema fingerprint needs one. The
wire shape is normative (§7.2), and `bynk-check`'s `canon_type` already
canonicalises it.

**Consequences.** Amends ADR 0124: D4's merge becomes per-field decode, and its
"one validation pass per cold load" consequence is corrected (per handler call,
unchanged in cost; the decode now keeps the result it already computed). Each
commit gains one serialiser walk, the same order as the V8 serialisation `put`
already performs. Proved at runtime on both stores: on the bundle target by
`bynkc/tests/fixtures/positive/1649_agent_state_round_trip` (behavioural,
11 shapes plus a `transition` that reads the decoded prior state) and reload cases added to fixtures 139 and 155; on real Durable Object
storage by `workers_runtime_smoke.rs::agent_state_round_trips_on_workerd`. #539
can build its fingerprint and `migrate` transform on this format. The load-time
decode writes through `stored`'s nested objects (a store `Map`, `Cache` entries,
`Log` entries), which `__merged` shares. That is sound because both backends
return a fresh copy from every `get` (workerd deserialises; the bundle target's
in-memory storage clones since #1660). A backend that returned the stored object
itself would decode an already-decoded value on the next load. Per-field absence
and unknown-key carry-through are pinned by
`bynkc/tests/store_behaviour.rs::agent_state_keeps_unknown_keys_and_defaults_missing_fields`.
