# Storage track — agent-local storage kinds

Persistent design doc for the **rich storage-kind catalogue** of
`design/bynk-design-notes.md` §10, the feature-track artefact of
[ADR 0076](../decisions/0076-feature-track-posture.md). It realises and sharpens
§10 (and the parts of §11/§12/§14 that storage touches); the design notes stay
the north star.

**Trigger (ADR 0076):** multi-increment ✔ (the substrate + six kinds + write
forms + annotations, ~6–8 slices) and surface-not-yet-settled ✔ (the concrete
grammar, the atomicity reconciliation, the `Map` name clash, and the `Queue` /
held-resource boundary are all open). Not a security boundary — so no
`/security-review` gate, but the atomic-commit semantics are a **correctness
boundary** and each slice runs `/code-review`.

## 1. Conceptual model (sharpened from §10)

Storage kinds are an agent's data abstractions: the only locus of mutation, the
only things owned by agent identity, the only constructs that talk to durable
storage. Each `store` field is an access-pattern slot of a declared kind. All
operations are `Effect`-typed and awaited with `<-`, with `Cell` as the single
sugared exception (implicit dereference in expression position; `:=` assignment;
auto-inserted await). Writes within a handler are staged and committed
**atomically at handler end**; refined element types validate on write and on
rehydration from durable storage.

The committed catalogue: `Cell[T]`, `Map[K,V]`, `Set[T]`, `Log[T]`,
`Queue[T]`, `Cache[K,V]`. (`Ref[A]` and the `Held[T]`/`Connection[F]` family
appear in §10 but belong with the held-resources / WebSocket track — see §6.)

## 2. The divergence this track closes

The compiler stands somewhere other than §10 (recorded in ADR 0107's context):

| Concern | Today | §10 target |
|---|---|---|
| Declaration | one immutable `state { … }` record | per-field `store name: Kind[…]` |
| Read | `self.state.f` | bare name (`Cell` implicit-deref); kind methods |
| Write | `commit { ...s, f: v }` spread | `:=` / `.update` / kind ops |
| Commit | **eager, per-statement, non-atomic** (`commitState`, ADR 0107 D6) | **staged, one atomic commit at handler end** |
| Kinds | immutable `List`/`Map` *values* + `Kv` binding storage | the six agent-local storage kinds |

The first three rows are the surface change settled by
[ADR 0108](../decisions/0108-state-record-to-store-fields.md) (`store` replaces
`state`). The fourth — restoring **true handler atomicity** — is a larger
semantic change ADR 0107 explicitly deferred (D6), and `store`'s `:=`/`.update`
semantics depend on it. It is bigger than state: it forces a ruling on how *each
class of effect* behaves on abort (state vs event emission vs `~>` send vs
cross-agent call — see §5, ADR 0109), so it only *partially* reverses D6. And it
is **intra-agent only** — cross-agent calls within a handler still stand and are
recovered by sagas (§13). It is this track's first load-bearing ADR, sequenced as
slice 0.

## 3. Concrete surface

```
agent Inventory {
  key sku: Sku

  store available:    Cell[Int where NonNegative]  = 0
  store reservations: Map[ReservationId, Reservation] @indexed(by: orderId) = {}
  store history:      Log[ReserveEvent]            @retain(30.days)

  invariant available_non_negative: available >= 0

  on call reserve(qty: Quantity, orderId: OrderId) -> Effect[ReserveOutcome] given Clock {
    if available < qty { InsufficientStock(available, qty) }
    else {
      <- available.update(a => a - qty)
      <- reservations.put(rid, Reservation { … })
      <- history.append(ReserveEvent { … })
      Reserved(rid)
    }
  }
}
```

`key`, the `on call` handler form, and the `-> Effect[…]` signature are **shipped
syntax this track leaves untouched**; the only new surface is `store`, the
`:=`/`.update` write forms, the kind operations, and the `@…` annotations — that
is exactly the diff a reader should see against an agent written today.

- **Field form:** `store <name>: <Kind>[…] [@annotations] [= initialiser]`
  (initialiser per ADRs 0003/0004).
- **Write forms:** `cell := v` (unconditional, idempotent on final state) vs
  `cell.update(fn)` / `map.update` / `map.upsert` (read-modify-write); the
  compiler errors when a `:=` RHS references its LHS, suggesting `.update` (§10).
- **Per-kind ops:** `Cell` (deref / `:=` / `.update`); `Map`
  (`put`/`get`/`update`/`upsert`/`remove`); `Set`
  (`add`/`remove`/`contains`/`size`/set algebra); `Log` (`append` + time-window
  reads); `Queue` (durable async stream); `Cache` (`Map` ops + TTL eviction).
- **Annotations:** `@indexed(by: …)`, `@ttl(…)`, `@retain(…)`, `@bounded(…)` —
  access-pattern/constraint hints, not implementation directives.

## 4. Internal architecture (the seams)

- **`bynk-syntax`:** a `store` field declaration (name, kind, type args,
  annotations, initialiser); the `:=` write statement and `.update`/kind-method
  call sites; removal of `state { }` / `commit`-spread at the parity slice.
- **`bynk-check`:** kind-aware method resolution (each kind exposes a fixed op
  set); the `:=`-references-LHS rule; `Cell` implicit-deref-in-expression /
  write-in-`:=`-position disambiguation; refinement-on-write typing; invariant
  handling restated onto `store` fields (amends ADR 0107): bare-name resolution,
  `Cell` reads in predicates as **pure reads of the staged value**, the
  referenceable surface limited to a **bounded single-element read** (`Cell` deref,
  `map.get(k)`, `set.contains(x)`; not `Cache`, not whole-collection scans like
  `reservations.values.all(…)` — that quantifier case rides the parity-slice
  amendment) **per ADR 0108 D5 (canonical)**, and the check evaluated against the
  **staged write-set before the atomic flush**.
- **`bynk-emit`:** lowering each kind to Durable Object storage; **staged writes
  flushed in one atomic commit at handler end** (replacing eager `commitState`);
  the **effect-release split on abort** (state staged-and-atomic; event emission
  staged-and-released-at-commit; `~>` sends immediate via `waitUntil` and standing
  on fault; cross-agent calls standing — see §5 / ADR 0109); index maintenance
  inside the commit; rehydration validation of refined fields on agent start.
- **Tooling (per-slice, part of "done"):** tree-sitter + TextMate grammar (both
  already lag the language), LSP completion/hover/signature for the new kinds and
  ops, the `bynk-fmt` `state→store` codemod, and the book/spec pages.
- **Track deliverable — a reserved-keyword ↔ TextMate drift test.** Assert every
  reserved keyword (now incl. `store`) appears in `bynk.tmLanguage.json`, so
  grammar lag is structurally impossible rather than per-slice-remembered — the
  failure mode that shipped `invariant`/`implies` without highlighting (fixed in
  PR #300).

## 5. Dependencies & the ADR slate

Front-loaded, hard-to-reverse ADRs land in the settling phase (ADR 0076 step 1),
in roughly slice order:

- **[ADR 0109](../decisions/0109-handler-atomic-commit.md) — handler-atomic
  commit** (accepted; slice 0's gate). Restore
  staged-write, single-commit-at-handler-end semantics, superseding eager
  `commitState`. This **partially reverses ADR 0107 D6**, and the real content is a
  per-effect-class ruling on abort:
  - *state* — staged and flushed atomically at handler end (the point);
  - *event emission* — staged and released at commit (§7 already says an aborted
    handler emits nothing), so it joins the atomic set;
  - *`~>` sends* (ADR 0106) — remain immediate (`waitUntil`) and **stand** on
    fault; ADR 0106 deferred the at-commit tier to Events;
  - *cross-agent calls* — **stand** (atomicity is intra-agent; recovery is sagas,
    §13).

  So D6 survives for `~>` and cross-agent effects but not for state/events. Must
  land with or before the `Cell` slice.
- **ADR 0108 — `store` replaces `state`** (accepted). The declaration-surface
  disposition + migration; 0108 settles the *surface*, 0109 the *semantics*.
- **Storage-kind representation & DO lowering** (settled per-kind, not as one ADR).
  Each kind's representation landed with its slice: `Cell` → state-record field
  (ADR 0109); `Map`/`Set` → `Record<string, V>`/`Record<string, boolean>`
  (ADR 0110); `Cache` → `Record<string, { v, exp }>` (ADR 0113). All commit
  wholesale through the ADR 0109 flush.
- **`Map`: storage kind vs collection value** — **[ADR 0110](../decisions/0110-storage-map-vs-value-map.md)** (accepted).
  Receiver provenance disambiguates: a `store` field is the storage collection,
  a value is the immutable one. Extended to `Set` (and `Cache`) the same way.

External dependencies (not in this track):

- **Query algebra** (§11) — its own ADR-0076 sibling track, sequenced **before
  the Set/Log slices**, whose read surfaces produce lazy `Query[T]` (Cell, Map
  put/get, and `state` removal do **not** need it).
- **`Idempotency` capability** (§12, deferred) — `Log.append` is the one
  non-idempotent write and `Queue` consumption is at-least-once; their safe-use
  story leans on it. The kinds can land first; the guidance references it.

## 6. Ordered slice decomposition

> **Track status: paused at v0.87** (2026-06-25). `Cell`/`Map`/`Set`/`Cache` and
> the annotation surface + `Duration` primitive have shipped (slices 0–3c). The
> track is paused pending the **query-algebra sibling track**, which slice 4
> (`Log`) depends on and which also unblocks `Map`'s deferred `@indexed`. Slice 5
> (`Queue`) is additionally gated on the open Q5 placement question. Resume by
> opening the query-algebra track (settling phase first), then `Log`.

| # | Slice | Depends on | Status |
|---|---|---|---|
| 0 | Handler-atomic commit + effect-release split (ADR 0109) | — | **shipped (v0.82, ADR 0109)** |
| 1 | `store` substrate + `Cell` + write forms (`state{}` removal + codemod **deferred** to a parity slice — `store` still coexists with `state{}`, ADR 0108 D3) | 0, ADR 0108 | **shipped (v0.82); parity slice deferred** |
| 2 | Storage `Map` (`put`/`get`/`update`/`upsert`/`remove`) — `@indexed` **deferred** to the query-algebra track | 1 | **shipped (v0.83, ADR 0110)** |
| — | *Query-algebra sibling track lands here (before Set/Log)* | 2 | external |
| 3 | `Set` (`add`/`remove`/`contains`/`size`) | 2 | **shipped (v0.84, ADR 0110)** |
| 3a | Annotation surface — `@` token, AST, closed registry, per-kind/per-slice gating (ADR 0111 D1–D3) | 2 | **shipped (v0.85)** |
| 3b | `Duration` primitive — literal (`5.minutes`) + base type + arithmetic/comparison + clock math (ADR 0112) | — | **shipped (v0.86)** |
| 3c | `Cache` (`Map` ops + `@ttl`, lazy check-on-read eviction; time via `given Clock`; ADR 0113) | 3a, 3b | **shipped (v0.87)** |
| 4 | `Log` (implicit timestamp, time-window reads, `@retain`) | query algebra, 3a, 3b | not started |
| 5 | `Queue` (durable async stream) | held-resources/delivery | not started |

`Ref[A]` and `Held[T]`/`Connection[F]` are **out of this track** — they ride the
held-resources / WebSocket track. Slice 5 (`Queue`) overlaps the platform Queue
*transport* (`from Queue`, already shipped) and at-least-once delivery; confirm
in the settling phase whether it belongs here or with held resources.

Slices 1–2 are "core Bynk" foundations (§2's layering lists `Cell`/`Map` as
foundational), so this track deliberately re-sequences ahead of the published
Events → Sagas order where those foundations are concerned — a call to confirm.

## 7. Open design questions (settle before the relevant slice)

1. ~~The atomicity mechanism and the **effect-release split** (slice 0 / ADR 0109)
   — output-gate batching vs a staged-write buffer flushed once; and the per-class
   ruling on abort.~~ **Settled — [ADR 0109](../decisions/0109-handler-atomic-commit.md):**
   staged write-set flushed once at handler end; state atomic, events at-commit,
   `~>` immediate-and-standing, cross-agent standing (partially reversing ADR 0107
   D6). Intra-agent only.
2. ~~`Map`/`Set`: one spelling for value-and-storage, disambiguated by receiver, or
   split names (§5).~~ **Settled — [ADR 0110](../decisions/0110-storage-map-vs-value-map.md):**
   one spelling, disambiguated by **receiver provenance** (a `store` field is the
   storage collection; a value is the immutable one). Applied to `Map` and `Set`.
3. ~~Annotation grammar and the closed annotation set; which are v1.~~
   **Settled — [ADR 0111](../decisions/0111-storage-annotation-surface.md).** A
   closed `@name(args)` registry (`@ttl`/`@retain`/`@indexed`/`@bounded`) in
   field-declaration position, each gated to its kind's slice; arguments are
   compile-time literals; `@ttl`/`@retain` take a new **`Duration`** primitive
   (`5.minutes`, lowering to `Int` millis) sequenced as a prerequisite slice
   (3b) before Cache (3c). The grammar + registry are v1; `@ttl` is the first
   functional annotation.
4. `Set` structural-equality semantics over opaque/transparent element types
   (§10) — the equality story membership and `==` rely on.
5. `Queue` placement (this track vs held-resources) and its delivery contract.
6. Rehydration-validation failure mode — fault vs structured boundary error.
7. Refinement migration on rehydration — beyond Q6's error *shape*, the policy
   when a refined element type **tightens across a deploy** so already-persisted,
   previously-valid data now fails rehydration. A versioning/migration concern, not
   just a fault-vs-boundary-error choice.
