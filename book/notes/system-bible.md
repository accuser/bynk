# Continuity bible — the one `commerce` system

This note is editorial planning, not manuscript prose. It defines the single
service the book builds across all fourteen chapters, so that Chapter 12 can
*open the system the reader has been building* rather than introduce a fresh
one. Every snippet in `snippets/` must conform to the names and shapes fixed
here.

Status: **draft for author sign-off.** The six decisions in the last section are
the ones worth confirming before the Phase 2 re-thread begins.

## The spine: one order pipeline

A single online store, namespace `commerce`, followed from browse to fulfilment.
Each chapter turns the reader's attention to one architectural fact *of this
system*; nothing is a parallel demonstration.

```
Visitor browses
  → Customer authenticates                      (ch 7 caller)
    → adds lines to their Basket                 (ch 5 state, ch 7 caller-as-key)
      → checks out: an Order is created          (ch 6 lifecycle)
        → Stock is reserved                      (ch 9 agent, ch 12 sequencing)
          → payment is authorised                (ch 3 failure, ch 4 effects)
            → confirmation email (queue),
              daily reconciliation (cron),
              live tracking (WebSocket)           (ch 8 entry protocols)
      → a return releases stock                   (ch 10 diagnostics: the cycle)
  ⇒ the whole system read end to end              (ch 12)
  ⇒ and priced                                    (ch 13)
```

The prologue's "four boxes" = HTTP (`commerce.orders`), business logic (orders +
payment), storage (the agents), external provider (`Bank`). The epilogue returns
to exactly these, now grown.

## Contexts (deployable boundaries)

| Context | Owns / presents | Consumes | First appears | Extended in |
|---|---|---|---|---|
| `commerce.values` | shared vocabulary commons (no runtime edge) | — | ch 2 | 3, 5, 6, 9, 12 |
| `commerce.orders` | `Order` agent, `Basket` agent, HTTP API | payment, inventory, notifications, audit | ch 1 | 3, 5, 6, 7, 12 |
| `commerce.payment` | `authorise`/`charge` service; `Bank`,`Audit`,`Log` caps | — | ch 1 | 3, 4, 12 |
| `commerce.inventory` | `Stock` agent (per `Sku`), reservation service | — | ch 9 | 10, 12 |
| `commerce.pricing` | pricing service; `Rates`,`Audit` caps | — | ch 9 | — |
| `commerce.notifications` | `Mailer` cap; HTTP + queue + cron entry points | — | ch 8 | — |
| `commerce.tracking` | WebSocket gateway; `Tracking` agent | — | ch 8 | — |
| `commerce.returns` | return-eligibility service | inventory | ch 10 | — |
| `commerce.audit` | internal `on call` handler | — | ch 7 | — |
| `commerce.catalog` | slug normalisation, metrics counter; `Slug` cap (TS adapter) | — | ch 11 | — |

Retired namespaces: **`shop.*` (ch 12) and `support.*` (ch 11) become
`commerce.*`.**

## Shared vocabulary — `commerce.values`

One commons owns every cross-cutting domain type. Introduced in ch 2, imported
(`uses commerce.values`) wherever needed.

| Type | Kind | Rule | Introduced |
|---|---|---|---|
| `OrderId` | opaque `String` | `NonEmpty` | ch 2 |
| `CustomerId` | opaque `String` | `NonEmpty` | ch 2 |
| `Sku` | opaque `String` | `NonEmpty` | ch 5 |
| `Quantity` | refined `Int` | `InRange(1, 100)` | ch 2 |
| `Cents` | refined `Int` | `>= 0` (or `Positive` at use sites) | ch 3 |

`OrderId`/`CustomerId`/`Sku` deliberately share base + predicate so the
opacity argument (ch 2) and the key-distinctness argument (ch 5) both bite.

## Actors

| Actor | Scheme / basis | Identity | Introduced |
|---|---|---|---|
| `Visitor` | none (explicit public) | — | ch 7 |
| `Customer` | `Bearer`, `AUTH_JWT_SECRET` | `CustomerId` | ch 7 |
| `Admin` | refines `Customer` + `admin` claim | `CustomerId` | ch 7 |
| `Caller` | built-in, `on call` | calling context name | ch 7 |
| `Subscriber` | WebSocket upgrade auth | `CustomerId` | ch 8 |

**`User`/`UserId` from the current ch 7 are retired in favour of
`Customer`/`CustomerId`** so the whole book uses one principal, and so ch 7's
`Basket(u.identity)` and ch 12's `Customer` actor are the same thing.

## Capabilities

| Capability | Operations | Owner context | Introduced |
|---|---|---|---|
| `Bank` | `charge` (→ auth ref / `PaymentError`) | payment | ch 4 |
| `Audit` | `record` | payment, pricing | ch 4 |
| `Log` | `info` | payment | ch 4 |
| `Rates` | `lookup` | pricing | ch 9 |
| `Mailer` | `send` (→ `Delivery`) | notifications | ch 8 |
| `Slug` | `normalise` (TS adapter) | catalog | ch 11 |
| `bynk.cloudflare` KV | platform-native | catalog cache | ch 11 |

**`Gateway` from the current ch 12 is retired; payment's effect capability is
`Bank` throughout** (established in ch 4). Payment's *service* is `charge`.

## Agents (canonical shapes)

These are pinned so later chapters extend, never contradict, earlier ones.

### `Basket` — key `CustomerId` (ch 5, caller in ch 7)
One active basket per customer. Store: `lines: Map[Sku, Quantity]`,
`note: Cell[Option[String]]`, `revision: Cell[Int]`. Handlers: `setLine`,
`leaveNote`, `snapshot`. **Key change from draft: `Basket` is keyed by
`CustomerId`, not `BasketId`** — this makes ch 7's "the key *is* the
authenticated caller" land exactly, and removes a redundant identifier.

### `Order` — key `OrderId` (ch 6 lifecycle; assembled in ch 12)
Canonical full shape (ch 6 presents the invariant/transition subset; ch 12 reads
the whole):
- `status: Cell[OrderStatus]` — sum `Draft | Placed | Paid | Rejected`, start `Draft`
- `customer: Cell[Option[CustomerId]]`
- `reserved: Cell[Bool]` — start `false`
- `paymentRef: Cell[Option[String]]` — start `None`
- invariant `paid_has_payment_ref`: `status == Paid implies paymentRef.isSome`
- invariant `paid_has_reservation`: `status == Paid implies reserved`
- transition `paid_is_terminal`: `old.status == Paid implies new.status == Paid`
- handlers: `begin`, `markReserved`, `pay`, `reject` (+ ch 6 maintenance
  handlers that break contracts on purpose)

The `reserved`/`Rejected` concepts are introduced in ch 6 as part of the
lifecycle so ch 12's "rejected order keeps its reservation" finding is a
property of the *same* agent, not a new one.

### `Stock` — key `Sku` (ch 9, read in ch 12)
Store: `available: Cell[Int]`, `reserved: Cell[Int]`. Invariant: both `>= 0`.
Handlers: `receive`, `ship`/`reserve`. Drives the ch 9 `History[Stock]` example
and the ch 12 atomicity reading.

### `Tracking` — key trackingId (ch 8)
Holds an owned `Connection[ServerFrame]`; `on message`/`on close` recover it.

### `Counter` — key label (ch 11 topology/emission example)
Minimal keyed state so the bundle-vs-Workers emission table has a real agent.

## Chapter ledger — what each chapter touches

| Ch | Architectural fact | System delta | Snippet renames needed |
|---|---|---|---|
| 1 | boundary → context | `commerce.orders` consumes `commerce.payment` | none (already `commerce`) |
| 2 | identity/validity | `commerce.values` (was `order_values`) | `order_values` → `values` |
| 3 | failure | `PaymentError`, `OrderError`, `Option`/`Result` | none |
| 4 | effects/capabilities | `Bank`,`Audit`,`Log`, providers | none |
| 5 | state ownership | `Basket` agent **keyed by `CustomerId`** | `BasketId`→`CustomerId` key |
| 6 | transitions | canonical `Order` agent + contracts | align to canonical shape |
| 7 | caller | `Customer`/`Admin`/`Visitor`/`Caller`; `Basket(u.identity)` | `User`→`Customer` |
| 8 | entry protocols | `notifications` + `tracking` | `commerce.*` already |
| 9 | tests | `pricing` + `inventory.Stock` | `commerce.*` already |
| 10 | diagnostics | `returns`↔`inventory` cycle | `commerce.*` already |
| 11 | host/emission | `catalog` (was `support.dashboard/counters`) | `support`→`commerce.catalog` |
| 12 | whole system | assemble orders+inventory+payment+values | **`shop`→`commerce`, reconcile Order/Stock/Bank** |
| 13 | costs | plugin host stays deliberately-TS (a *contrast*, not commerce) | keep; frame as a commerce *extension* host |
| 14 | epilogue | return to the four boxes, grown | prose only |

The one intentional non-`commerce` example is ch 13's open-plugin host — it is a
*counter*example (a system whose openness is the architecture), so it stays
plain TypeScript by design.

## Decisions to confirm before Phase 2

1. **Namespace `commerce`** everywhere (retire `shop`, `support`). ✅ assumed.
2. **`commerce.values`** as the single vocabulary commons (retire `order_values`). ✅ assumed.
3. **`Customer`/`CustomerId`** as the one principal (retire `User`/`UserId`). ← confirm
4. **`Basket` keyed by `CustomerId`** — one active basket per customer, so the
   key equals the caller identity in ch 7. ← confirm (this is the biggest
   modelling change; the alternative is keeping `BasketId` and weakening ch 7's
   "identity is the key" point).
5. **`Bank`/`charge`** as payment's capability+service everywhere (retire ch 12's
   `Gateway`). ← confirm
6. **Canonical `Order` agent** (Draft/Placed/Paid/Rejected + `reserved`) pinned
   above, introduced in ch 6, assembled in ch 12. ← confirm
