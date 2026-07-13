# 0184 — Map queries expose keys through `.entries`/`.keys`/`.values`; an entry is the nominal `MapEntry[K, V]` record, not a tuple

- **Status:** Accepted (v0.158)
- **Provenance:** design-review finding #547 (Bynk Language Design Review
  2026-07-05, §8 Language P1 #8) — "A `Query` over `Map[K, V]` yields values
  only, so every example duplicates the key inside the value record (todo's
  `TodoItem.id`) — denormalisation the compiler should make unnecessary. Expose
  `entries()` or two-parameter lambdas so map queries can see keys." **Closes
  #547.**
- **Realises:** a `store Map[K, V]` field exposes three key-aware lazy queries —
  `map.entries : Query[MapEntry[K, V]]`, `map.keys : Query[K]`, and
  `map.values : Query[V]`. `.entries` lifts each entry into a `MapEntry[K, V]`
  record with fields `key: K` and `value: V`; the whole existing query
  vocabulary (`filter`/`map`/`sortBy`/`collect`/…) then runs over it unchanged,
  reading the key with `e.key` and the value with `e.value`. A read handler
  projects each entry into a named type before its terminal, so the stored value
  no longer needs a denormalised copy of its own key.
- **Relates:** ADR 0120 (joins/grouping take an `into:` combiner, *not* a pair
  type — bynk has no anonymous product; this ADR reuses that nominal discipline
  by making an entry a named record, not `(K, V)`), ADR 0183 (generic record
  types — `MapEntry[K, V]` is a generic-record instantiation and inherits its
  non-boundary rule for free), ADR 0115/0119 (the `Query[T]` model and its
  durable-object lowering that `.entries`/`.keys`/`.values` extend), ADR 0110 D5
  (the value-keyable Map-key rule that bounds which keys can be decoded on read).

## Context

The review is correct: a store-map query yielded `Query[V]` — values only, the
key discarded at the lift. Every keyed collection therefore duplicated its key
inside the value record (the todo example's `TodoItem.id` was both the map key
*and* a stored field), a denormalisation the author must keep in sync by hand.

The type-system design sketch already named the fix — `Map[K, V].entries :
Query[(K, V)]`, with a `(k, f)` "tuple pattern" lambda — but wrote `(K, V)` as if
it were a type bynk has. It is not. ADR 0120 settled that bynk is **nominal**:
no tuple, no pair, no `(A, B)`. So the sketch could not be implemented literally;
key exposure had to be grounded without a product type.

## Decisions

**[DECISION A] Expose keys through `.entries`/`.keys`/`.values` accessors, not
two-parameter keyed lambdas (Recommended: accessors).** The alternative — a
"keyed query" whose every builder/terminal lambda gains a `(k, v)` two-argument
form — is more ergonomic for key-filtering but threads a second query shape
through the entire vocabulary (checker and emitter), doubling the surface for one
increment. `.entries` instead lifts once into an ordinary `Query` of a record;
the existing single-argument vocabulary then applies with **zero** changes. It is
the smaller build and composes with every current and future query op for free.

**[DECISION B] A map entry is the nominal `MapEntry[K, V]` record, not a tuple
(Recommended: nominal record).** `.entries` yields `Query[MapEntry[K, V]]`, where
`MapEntry[K, V]` is `{ key: K, value: V }`. This is the same choice ADR 0120 made
for join rows: rather than introduce an anonymous product, name the shape. The
`(K, V)` of the original sketch becomes the record `MapEntry` with `.key`/`.value`
accessors instead of `.0`/`.1` — self-documenting, and it keeps the language's
"name your data" discipline intact. No tuple type is introduced.

**[DECISION C] `MapEntry` is compiler-known and non-boundary (Recommended: yes).**
`MapEntry` has no user `TypeDecl` — it is a compiler-known generic record whose
`.key`/`.value` fields resolve like `JsonError`'s (the resolution defers to a
user type of the same name if one is declared, so the built-in adds no new
reserved word — consistent with `List`/`Query` not being reserved either).
Because it is a generic-record *instantiation*, it is
**non-boundary** by the existing ADR 0183 rule (no monomorphised codec is
generated), so it can never be returned across a service/agent boundary. This is
the intended guardrail, not a limitation: an entry is an in-pipeline shape, and a
handler must project it into a named boundary type (`items.entries.map((e) =>
Row { sku: e.key, qty: e.value.qty })`) before a terminal leaves the pipeline.
`MapEntry` is not writable in a type annotation in v1 (its type is always
inferred at the `.entries` site); making it annotatable is an additive follow-on.

**[DECISION D] The accessors live on the store-map field, not on a lifted query
(Recommended: field-rooted).** `.entries`/`.keys`/`.values` are recognised only
when the receiver is a bare `store Map` field — a key-aware lift must see the map
itself, since a `Query[V]` has already dropped the keys. They are distinct from
the *in-memory* `Map[K, V]` value methods `.keys()`/`.values()` (which return
eager `List`s, ADR 0035) — the storage accessors are paren-less query builders,
the in-memory ones are method calls. `.entries` has no in-memory analogue in v1.
All three are also refused on a **held** `Map[K, Connection]`
(`bynk.held.query_accessor_on_held_map`): a held resource follows the single-owner
discipline (§2.9) and is iterated through its own broadcast surface
(`forEach`/`parTraverse`/`traverseAll`), never lifted into a key query or a
`MapEntry` record (which would hide the held value from the linearity pass's
`is_held` check).

**[DECISION E] A persisted key is decoded back to `K` on read (Recommended:
decode by base).** A Map key is stored as a JavaScript object key — always a
string. `.keys`/`.entries` therefore decode it back to its declared type:
value-keyable keys are `Int`/`String` and refinements/opaques over them (ADR 0110
D5), so an `Int`-based key (which erases to `number`) is parsed with `Number(…)`
and a `String`-based key passes through unchanged. The decode is total over the
admissible key domain.

## Consequences

- `bynk-check` gains the `MapEntry` compiler-known record (`map_entry_ty`, field
  resolution beside `JsonError`) and the three field-rooted accessors; a store
  field root no longer trips the cross-context-call heuristic
  (`root_ident_is_store_field`). `bynk-emit` lowers `.entries` to `Object.entries`
  zipped into `{ key, value }` records, `.keys`/`.values` to `Object.keys`/
  `Object.values`, with the `Number(…)` key decode where `K` is `Int`-based.
- The whole lazy query vocabulary applies to `.entries` unchanged, because a
  `MapEntry` query is an ordinary `Query`. Nothing new is needed per op.
- The todo example drops `TodoItem.id` from its stored shape: the id lives in the
  key, and the read handlers rejoin it through `.entries`. The denormalisation
  the review flagged is gone.
- Two-parameter keyed lambdas, an annotatable/boundary `MapEntry`, and `.entries`
  on in-memory maps all stay reachable and unshipped — additive later if demand
  appears, not cornered by this increment.
