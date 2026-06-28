# 0130 — held-resource linearity: the `Connection[F]` type and the API-discipline ownership pass (§2.9 realised; §3 step 11); held values are non-serialisable, storable only in `Cell`/`Map`, disposed before scope exit

- **Status:** Accepted (real-time track, slice 2; 2026-06-27).
- **Provenance:** the third slice of the real-time / WebSocket feature track (`design/tracks/websocket.md`), authored with its slice. Consumes proposal `design/proposals/v0.102-held-resource-linearity.md`; settles the track's **Q5** (within-handler fault paths) and **Q6** (linearity diagnostics).
- **Realises:** `bynk-type-system.md` **§2.9** (Held resources — "settled in shape" since the design notes, built here) and its **§3 step 11**, *"Check linearity for held resources"*, previously named but unimplemented. Promotes §2.9.7 from *Open* to settled for the within-handler subset.
- **Relates:** [ADR 0128](0128-stream-value-over-time-primitive.md) (`Stream[T]` — the non-serialisable-family wiring this mirrors for `Connection`); [ADR 0109](0109-handler-atomic-commit.md) (the handler-atomic commit the stored-connection rollback rides); the slice is the carrier the `from WebSocket` protocol (slice 3) will produce.

## Context

The connection leg of the track needs a value the design notes settled in shape
but never built: an "API-discipline-driven linearity" model — `Held[T]` /
`Connection[F]`, four ownership states, a closed origin, mandatory disposal, and
storage rules (§2.9). This slice builds it **without** the `from WebSocket`
protocol that produces connections in production: the type machinery is tested in
isolation against **hand-written held sources** — a capability that returns a
`Connection` — because the discipline is a set of **compile-time rejections** that
need no running socket.

## Decisions

- **D1 — `Connection[F]` is the one concrete instance of a closed `Held` kind.** A
  `TypeRef::Connection`/`Ty::Connection` (wired through parser/AST/checker/emitter
  as `Stream` was), with a `Ty::is_held` predicate — `true` for `Connection`, the
  single extension point for future held types (file handles, DB connections).
  `Connection` joins the non-serialisable family (one arm per existing site:
  `json_codable`, boundary validation, the `==` guard), so it is **non-serialisable,
  non-boundary, and not value-comparable** (held values have identity, not
  value-equality — `bynk.types.held_not_comparable`). Held types are **not
  user-definable**; the kind is closed at v1.

- **D2 — operations: `send` (non-consuming) and `close` (consuming).** `c.send(f)`
  writes a frame typed against `F` (a wrong-shaped frame is a compile error at the
  `.send` site) and leaves the binding **owned**; `c.close()` is **consuming**.
  Following Bynk's method-first idiom (`Stream.of`, `Duration.millis`), `close` is a
  **method** (`conn.close()`), refining §2.9's free-function `close(conn)` notation.

- **D3 — storage admission (§2.9.3).** A held value is admitted in
  `Cell[Option[Connection]]` and `Map[K, Connection]` — an **exception** to the
  serialisable-value rule, since the platform preserves connections across
  hibernation, not JSON (§2.9.6) — and **rejected** in `Set`/`Log`/`Cache`
  (`bynk.held.unsupported_storage`: `Set` needs value-equality; `Log`/`Cache` would
  retain or evict a held resource without disposing it). `put` consumes (transfer
  in), `remove` removes-and-closes.

- **D4 — the linearity pass is a flow-sensitive post-pass (§3 step 11), not woven
  into the type walk.** It runs after `type_of_block`, reading the populated
  `expr_types` side-table; it threads each held binding through **owned →
  borrowed → owned** / **owned → consumed**, enforcing: single-owner; **mandatory
  disposal** (a binding still owned at scope exit is `bynk.held.leak`); **no
  use-after-consume** (`bynk.held.use_after_consume`); and **branch unification**
  (every `if`/`match` arm must leave each binding in the same state, else
  `bynk.held.branch_divergence`, the Q6 diagnostic). A separate pass — rather than
  weaving into `type_of_block`, whose `if`/`match` pop branch scopes *before*
  unifying types — keeps the ownership branch-join explicit and the type walk
  untouched. The pass is **bounded** (a fixed operation vocabulary over three
  states, no general dataflow lattice) — the reason §2.9 chose API-discipline
  linearity over a general affine system.

- **D5 — fault paths (Q5): settle the within-handler subset, defer cross-context.**
  A connection owned at abnormal exit is **implicitly consumed by the runtime**
  (the pass therefore does not require an explicit `close` on every fault path,
  only no *use* after consumption); a stored connection rides the handler-atomic
  commit (ADR 0109) and rolls back with the rest of agent state; a `Sagas.compensate`
  may operate on a connection already in state but not on one still held locally.
  The **cross-context** transfer-fault has no surface until the protocol exists, so
  it stays deferred to slice 3 — where the platform handoff (`acceptWebSocket` into
  the DO) gives the single-owner invariant its runtime backing.

- **D6 — emit against a runtime `Connection<F>` interface; no implementation.**
  `send`/`close` lower to method calls on a runtime `Connection<F>` interface
  (`send(frame): Promise<void>`, `close(): Promise<void>`); a file naming the type
  imports it from the runtime. The concrete implementations — a capture-and-inspect
  `TestConnection` for bundle/test and the hibernatable-WS binding for Workers —
  land with the protocol (slice 3). Positive fixtures emit and type-check under
  `tsc --strict` against the interface.

## Consequences

- The §2.9 discipline is built and fixture-proven: a connection received from a
  capability is sent on, then closed (`235_held_connection`, which also stores into
  / removes from a `Map[K, Connection]`), emits and `tsc --strict`-checks; the
  violations — leak, use-after-consume, branch divergence, `Set` storage, boundary,
  `==` — are each a negative fixture (`250`–`255`).
- The **record-of-held** case (a `Connection` field in a user record) is rejected
  via the existing boundary check (`bynk.types.held_at_boundary`) — a record field
  is a serialisable position; a distinct "in record" message is a deferred nicety.
- **Deferred and named** (§2.9.9 / the track): the **held-aware iteration borrow
  surface** (`forEach`/`parTraverse` lending borrowed `&Connection` refs — the
  broadcast-to-all-connections pattern) — the pass carries the borrow machinery
  (`bynk.held.consume_on_borrow`), but storage `Map` has no held-aware `forEach`
  yet, so it lands with the pattern that needs it; user-defined borrowing functions;
  cross-context fault propagation; and the `from WebSocket` protocol + runtime
  connection (slice 3, security-bearing).
