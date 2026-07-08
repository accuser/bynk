# 0173 — `Map.values()` and broadcast collect-all; owned `List[Held]` a non-goal

- **Status:** Accepted (v0.149)
- **Provenance:** resolves #570 (a held-bearing `List[Connection]` is modelled by
  `storage_value_is_held` but not constructible — surfaced by the #569 review,
  where the requested held-collection fixture for `traverseAll`/`parTraverseAll`
  could not be written). Supersedes #570's recommended Decision A (a borrow-scoped
  `Map.values()` yielding a `List[Connection]`) after investigation, in favour of
  the lower-risk broadcast route.
- **Realises:** (1) the `keys()` sibling `Map.values() -> List[V]` on the
  in-memory value `Map`; (2) the collect-all iterators `traverseAll`/
  `parTraverseAll` (ADR 0172) reaching a `store Map[K, Connection]` via the query
  broadcast, so "send to every live connection and gather a `Result` per one" is
  expressible; (3) a stated boundary: an *owned* `List[HeldResource]` is
  intentionally not constructible.
- **Relates:** ADR 0172 (`List.traverseAll`/`parTraverseAll`, the collect-all
  shape reused here), ADR 0135 (`Map`/`Query` broadcast `forEach`/`parTraverse` —
  the borrow machinery reused), ADR 0130 (held-resource linearity — single-owner,
  disposed-in-place, borrow-only outside its store), ADR 0116 (the `List`/`Query`
  combinator vocabulary), ADR 0156 (the editor surface tracks the language).

## Context

#570 flagged an asymmetry in the checker. The held-resource linearity pass
recognises a held-bearing `List` — `storage_value_is_held` returns true for
`Ty::List(Connection)` (`bynk-check/src/checker/linearity.rs`) — and the borrow
gate lends held elements for `forEach`/`parTraverse`/`traverseAll`/
`parTraverseAll`/`update`. But **no surface yields a `List[Connection]`**: a
value `Map`/`List` cannot hold a `Connection` (the boundary/linearity rules
reject it, `bynk.types.held_at_boundary`), and a connection is minted only at the
WebSocket edge and stored into a `store Cell`/`Map`. So the `List`-held borrow
path was unreachable and untestable — the #569 reviewer's requested held fixture
could not be written.

Two things were actually missing, and they are separable:

- **A value-`Map` `values()`.** `keys() -> List[K]` shipped (ADR 0116) but its
  obvious pair `values() -> List[V]` did not. Purely a value-collection
  convenience; held resources never enter a value `Map`, so this touches nothing
  held.
- **Collect-all over live connections.** `forEach`/`parTraverse` reach a
  `store Map[K, Connection]` (they are query ops — the storage map lifts to
  `Query[Connection]`, ADR 0135), but the ADR 0172 collect-all iterators
  `traverseAll`/`parTraverseAll` were **`List`-only**, absent from `is_query_op`,
  so `conns.traverseAll(…)` was `bynk.store.unknown_op`. The collect-all fan-out
  could not reach connections at all.

## Decisions

**A — `Map.values() -> List[V]` mirrors `keys()`, exactly.** A zero-arg value
kernel arm returning `List[V]`, lowered to `[...(m).values()]` over the in-memory
`ReadonlyMap` (the `keys()` shape). No held interaction — a value `Map` cannot
hold a `Connection`.

**B — Wire `traverseAll`/`parTraverseAll` into the query broadcast, reusing the
existing borrow machinery.** Adding both to `is_query_op` + a
`check_query_kernel_method` arm makes `conns.traverseAll(f)` on a
`store Map[K, Connection]` lift to `Query[Connection]` and route through the
*same* held-borrow path `forEach`/`parTraverse` already use — the closure's
`Connection` parameter is lent as `Held::Borrowed` (`send` allowed,
`close`/transfer rejected as `bynk.held.consume_on_borrow`). The signature is the
ADR 0172 one: `f: T -> Effect[Result[U, E]] -> Effect[List[Result[U, E]]]`,
gathering every outcome (no short-circuit; an `Err` is a value). The emitter adds
the two terminals to `lower_query_method` (a sequential push-collect and a
`Promise.all` collect over the map's values). This is what actually resolves
#570's coverage gap: the held-borrow path for the collect-all iterators is now
**reachable and fixtured** (`negative/332_broadcast_collect_all_consume` exercises
`consume_on_borrow`; `positive/338_ws_broadcast_collect_all` the happy path) —
via the `Map` broadcast, not a constructed `List[Held]`.

**C — An *owned* `List[HeldResource]` is an intentional non-goal.** The held
discipline (ADR 0130) makes a held value single-owner, disposed-in-place, and
storable only in `Cell`/`Map`; an owned `List[Connection]` would be a second
owner requiring its own disposal (with no disposal op) — a duplication the
discipline forbids. #570's Decision A (a borrow-scoped `Map.values()` yielding a
chain-only `List[Connection]`) would deliver the same broadcast capability as B
but only by **inventing a new linearity constraint** ("this value is legal only
as the immediate receiver of a borrowing iterator") in the flagship safety pass —
soundness-critical machinery for a niche value type. B reaches the same end (fan
out over live connections, collecting outcomes) through the *already-proven*
`Query` borrow path, so the new rule is unnecessary. The `Ty::List(Connection)`
arm in `storage_value_is_held` is left as harmless defensive coverage (it lends
correctly if a held `List` ever arises), but the language deliberately provides
no way to construct an owned one.

## Consequences

- `Map.values()` is available on every value `Map`; `m.keys()`/`m.values()` are a
  pair again.
- All four effectful iterators now reach a `store Map[K, Connection]`:
  discard (`forEach`/`parTraverse`) and collect-all (`traverseAll`/
  `parTraverseAll`), all through one borrow discipline.
- No new diagnostic (the collect-all arm reuses `bynk.types.argument_mismatch`
  for a non-`Result` function, as on `List`); no grammar change; no runtime
  change.
- The held-collection borrow path is now covered by fixtures for the collect-all
  iterators — the #569 review's coverage gap is closed.
- No emission churn: value `Map.values()` is net-new; the broadcast terminals are
  net-new `lower_query_method` arms; `forEach`/`parTraverse` lowering is untouched.

## Tooling (ADR 0156)

- **Hover / Completion / Signature help:** `Map.values()` is added to
  `MAP_METHODS`; the query broadcast terminals are dispatch-only (as
  `forEach`/`parTraverse` are on the query side) and gain no new registry entry
  beyond the `List` ones already present.
- **Semantic tokens / Formatter:** unchanged — ordinary method names.

## Alternatives considered

- **Borrow-scoped `Map.values() -> List[Connection]` (#570 Decision A).** The
  originally-recommended path. Rejected: it requires a new chain-only linearity
  rule (a held `values()` result may not be `let`-bound, returned, stored, or
  passed) to avoid a soundness hole — the `held_value` predicate does *not* look
  through `List`, so a `let vs = conns.values()` is today a silent blind spot.
  Decision B delivers the same broadcast capability with zero new linearity
  surface.
- **Pure cleanup — drop the `Ty::List(Connection)` modelling (#570 Decision B).**
  Honest but delivers no capability; the collect-all iterators would still not
  reach connections. Superseded by wiring the broadcast (this ADR's B), which
  both closes the coverage gap and adds the capability.
- **A held-aware `values()` restricted to storage maps.** Same soundness surface
  as Decision A for no gain over the broadcast route.
