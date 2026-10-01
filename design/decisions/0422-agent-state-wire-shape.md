# 0422 — Agent state is stored in the wire shape and decoded on load

- **Status:** Accepted (v0.291)

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
