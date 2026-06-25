# 0115 — `Query[T]` is a first-class, by-reference, non-storable type; lazy/eager evaluation is decided by receiver provenance, generalising ADR 0110 from op-set to evaluation strategy

- **Status:** Accepted (query-algebra track, slice 0 settling; 2026-06-25)
- **Track:** `design/tracks/query-algebra.md` (slice 0 — the foundational `Query[T]` model + lazy/eager dispatch ADR; constrains every later slice). Settles track Q2 and Q3.
- **Realises:** `design/bynk-design-notes.md` §11 ("Query Algebra" — the lazy-storage / eager-in-memory split, `Query[T]` as a first-class type, agent-locality).
- **Relates:** ADR 0110 (receiver-provenance dispatch for storage vs value `Map` — this generalises it from *op set* to *evaluation strategy*); ADR 0031 (`Effect` is non-storable — `Query` reuses the posture); ADR 0030 (function types are non-boundary — same); ADR 0113 (`Cache`'s eviction-specific `given Clock` — the contrast for D5's "no new capability"). The combinator **vocabulary** itself (builders/terminals, `Ordering`) is ADR 0116; the **Durable-Object lowering** is a later ADR.

## Context

§11 specifies one combinator vocabulary shared by agent-local storage and
in-memory collections, where **the receiver's type decides evaluation timing**: a
chain against a `store` field (`Map`/`Set`/`Log`) is lazy — it builds a `Query[T]`
that nothing executes until a terminal — while a chain against an in-memory value
(`List`, value `Map`/`Set`) is eager. This ADR settles the *model*: what `Query[T]`
is, how lazy/eager dispatch works in the checker, and the type/effect rules that
constrain the vocabulary (ADR 0116) and the lowering (later).

The checker already disambiguates storage from value collections by **receiver
provenance** (ADR 0110): a method call whose receiver roots in a `store` field
hits the storage op set; a value receiver hits the immutable collection. ADR 0110
moved provenance from *which op set*; this ADR moves it one step further to *which
evaluation strategy* — the same mechanism, a larger consequence.

## Decisions

**D1 — `Query[T]` is a first-class, by-reference type.** A new `Ty::Query(T)`,
nameable anywhere a type is written: returnable from a pure helper, passable as an
argument, bindable to a `let` (the §11 / track §3 `pendingExpiredAt` example
returns `Query[Reservation]`). `Query[T]` has **reference identity** — two queries
are computational descriptions, not values, so `==`/`!=` between `Query`s is **not
admitted** (`bynk.types.query_not_comparable`). It is **not** a collection: it has
no `length`/`get`/indexing; it is built by builders and consumed by terminals
(ADR 0116) and nothing else.

**D2 — `Query[T]` is non-storable and non-boundary, reusing the `Effect`/`Fn`
posture (ADRs 0031/0030).** A `Query[T]` may **not** be a `store` field, a record
field, a `Map`/`Set`/`Log` element, or any persisted position
(`bynk.types.query_not_storable`), and may **not** cross a context boundary — a
service-call parameter or return, an event payload, a queue element
(`bynk.types.query_not_boundary`). A query is **built, passed within the agent,
and executed** — never persisted or sent. The diagnostics reuse the
non-storable/non-boundary machinery `Effect` and `Fn` already drive.

**D3 — Lazy/eager dispatch by receiver provenance, generalising ADR 0110 from op
set to evaluation strategy.** A combinator chain is dispatched by what its **root
receiver is**:

- a **`store` field** (`store_maps` / `store_sets` / `store_logs`) is **lazy** —
  builders return `Query[T]`, terminals return `Effect[T]` (ADR 0116);
- an **in-memory value** (`List`, value `Map`/`Set`) is **eager** — builders
  return the collection, terminals return `T`.

The checker tracks "query-rootedness" through the chain by the receiver *type*,
not a separate flag: once a builder on a `store` field returns `Query[T]`, every
subsequent builder resolves on `Ty::Query` and stays lazy, and a terminal on
`Ty::Query` is `Effect`-typed. An in-memory chain never produces a `Query`, so it
stays eager throughout. No new grammar — builders/terminals are ordinary method
calls (track §4).

**D4 — A terminal's result has left the lazy domain; there is no implicit
re-lazification.** A storage terminal yields an ordinary in-memory value inside an
effect — `q.collect : Effect[List[T]]`, `q.first : Effect[Option[T]]`. Once
awaited (`let xs <- q.collect`), `xs : List[T]` is an in-memory value and chaining
combinators on it is **eager** (D3). The boundary is crisp: `Query[T]` is the only
lazy thing; everything a terminal produces is eager. The mixed case (a query
terminal returning a `List`, then chained) is therefore just two ordinary phases —
lazy build/execute, then eager transform — with no special rule (track Q2).

**D5 — The storage-read effect folds into the agent's storage capability; no new
`given`.** A terminal against a `store` field is `Effect`-typed and awaited with
`<-`, exactly like the existing entry ops (`map.get`, ADR 0110) — and needs **no
extra capability**: the read folds into the storage capability the agent's `store`
fields already carry (contrast `Cache`'s eviction-specific `given Clock`, ADR
0113, which is a *different* capability). A pure helper that **builds** a `Query[T]`
without terminating it has **no effect** — building is pure, terminating against
storage is effectful, and the lazy/eager split lines up with the pure/effectful
split (§11; track Q8).

**D6 — Agent-locality is a structural consequence of D2.** A `Query[T]` reaches
only the owning agent's storage. Because it is non-boundary (D2), a query
**cannot** be passed to another agent, so it cannot reach across the boundary by
construction — cross-agent data flow stays message-passing (a typed call returning
data). This is the §11 scoping guarantee, obtained for free from the non-boundary
rule rather than as a separate check.

## Consequences

- **Checker.** A `Ty::Query(Box<Ty>)`; `Query` reserved as a built-in type name
  (like `List`/`Map`). Receiver-provenance dispatch extended from op-set (ADR
  0110) to evaluation strategy (D3): a `store`-rooted chain resolves builders to
  `Query[T]` / terminals to `Effect[T]`, a value-rooted chain stays eager; method
  resolution on `Ty::Query` for chained builders/terminals. Non-storable /
  non-boundary / not-comparable enforcement (D1/D2) reusing the `Effect`/`Fn`
  diagnostic paths. The `store_logs` scope joins `store_maps`/`store_sets` when
  `Log` lands. The **signatures** of the builders/terminals are ADR 0116.
- **Emission.** Deferred to the per-slice Durable-Object lowering ADR: a lazy
  storage `Query` lowers to a DO read (scan by default; index lookup under
  `@indexed`), an eager in-memory chain to TS array/object operations. This ADR
  fixes only that the two lower differently and that a `Query` value never
  escapes the agent.
- **Scope held / named.** The vocabulary and `Ordering` (ADR 0116); `@indexed`
  routing (a later ADR); the DO lowering and cross-shape joins (a later ADR);
  reactive/streaming/materialised-view queries (§11 deferred). Named here, not
  silently dropped.

## Alternatives considered

- **A keyword/marker to choose lazy vs eager.** Rejected (D3): provenance already
  carries it (ADR 0110); a marker is the dialectal duplication §2 forbids.
- **`Query[T]` storable/sendable (e.g. ship a query to another agent).** Rejected
  (D2/D6): it would re-open the distributed-join failure modes §11 structurally
  rules out, and a description holding captured closures is not a value.
- **Implicit re-lazification of a collected `List` back into a `Query`.** Rejected
  (D4): the lazy/eager boundary must be legible from the type; a `List` is eager,
  full stop.
- **A separate storage-read capability (`given Storage`).** Rejected (D5): the
  read is already implied by the `store` fields; an extra `given` is ceremony §11
  explicitly avoids.
