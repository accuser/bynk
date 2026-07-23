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

## Part II execution notes (what actually shipped, ch 4–8)

Two refinements to the plan above were made during the re-thread; both serve the
same "one system" goal at lower risk. Flagged for review.

1. **`Order` is introduced small in ch 6 and *extended* in ch 12, not stamped
   whole in ch 6.** Ch 6 keeps its focused lifecycle (`Draft | Placed | Paid`,
   `paymentRef`, the two contracts) because that is what teaches invariant vs
   transition cleanly, and because expanding it would break the chapter's
   `source-lines` ranges. Ch 12 grows the same agent to the canonical shape
   (adds `Rejected`, `reserved`, `customer`, and `paid_has_reservation`). This
   reads as the system evolving between chapters rather than as a seam — but it
   means ch 12 must present `Order` as an *extension* of ch 6's, keeping every
   ch 6 field and contract intact.

2. **The five core types are defined consistently in-context, not threaded
   through a single imported `commerce.values` everywhere.** `commerce.values`
   remains the showcased commons in ch 2–3 (where shared vocabulary is the
   topic). Elsewhere each context defines the types it uses with the *same*
   definitions (`CustomerId`/`Sku`/`OrderId` = `opaque String where NonEmpty`,
   `Quantity` = `Int where InRange(1, 100)`, `Cents` = `Int where Positive`).
   Rationale: a print reader cannot click through to a commons defined chapters
   earlier, so self-contained-but-identical definitions read better than a bare
   `uses`. Open to reverting to pure shared-commons if preferred.

Also settled during execution:

- **`Basket` key field is named `owner`** (`key owner: CustomerId`) in ch 5 and
  ch 7, matching the chapter title "state needs an owner".
- **`CustomerId` is `opaque` everywhere**, including as an actor identity, HTTP
  path parameter, agent key, and `Map` key — confirmed to compile in ch 7/8.
  Ch 12's prose still calls it "a refined `CustomerId`"; that wording must
  change to "opaque" when Part IV is re-threaded.
- Ch 4 needed no changes (already consistent: `Bank`/`Audit`/`Log`, `Cents`).

## Part III execution notes (ch 9–11)

- **`Stock` lives in `commerce.inventory`** (ch 9 was `commerce.stock`). The
  compiler enforces path/name matching (`bynk.project.inconsistent_commons_name`),
  so the file was renamed `stock.bynk` → `inventory.bynk`. This matches ch 10
  and ch 12. As with `Order`, `Stock` is introduced small in ch 9
  (`available`, `receive`/`ship`, `nonnegative`) and extended in ch 12
  (`reserved`, `reserve`).
- **Stock movement amounts are `Amount = Int where Positive`, not `Quantity`.**
  The basket `Quantity` is `InRange(1, 100)`; a stock receipt/shipment isn't
  bounded that way, so reusing the name would have been a false consistency.
- **Ch 11 `support.*` → `commerce.*`**: `support.dashboard` → `commerce.catalog`
  (normalises a product label, increments a view counter), `support.counters` →
  `commerce.metrics` (the `Counter` agent), `support.cache` → `commerce.cache`
  (the vendor-lock example). The `text.normalise` adapter stays as-is — it is a
  deliberately domain-neutral utility adapter, and the only non-`commerce`,
  non-`bynk` namespace left, which reads as intended.
- Ch 10 needed no changes (already `commerce.inventory`/`commerce.returns`).

## Part IV execution notes (ch 12–13) — the payoff

- **`shop.*` → `commerce.*`** across all four whole-system files; `src/shop/` →
  `src/commerce/`; `system-tree.txt` updated.
- **`Order` reconciled to extend ch 6.** Status is now
  `Draft | Placed | Paid | Rejected` (was `Open | Paid | Rejected`), so ch 12 is
  visibly ch 6's agent grown, not a different one. `begin` now does
  `Draft → Placed`. The single `paid_is_complete` invariant was split into the
  two the book already names: `paid_has_payment_ref` (carried from ch 6) and
  `paid_has_reservation` (new with the `reserved` field). Transition
  `paid_is_terminal` unchanged. Prose "snapshot invariant says" → "invariants
  say".
- **`Stock`**: invariant renamed `quantities_are_nonnegative` → `nonnegative` to
  match ch 9 (extended to cover `reserved`).
- **Payment**: `Gateway` → `Bank`, `DemonstrationGateway` → `DemonstrationBank`
  (bible decision #5, matching ch 4).
- **`commerce.values`**: `OrderId`/`CustomerId`/`Sku` are now
  `opaque String where NonEmpty` (were `String where NonEmpty && MaxLength(64)`);
  `Quantity` is `InRange(1, 100)`. Prose "refined `Sku`/`CustomerId`" → "opaque".
- **Opacity caught a real defect during the reconcile:** the payment stub
  interpolated `\(customer)` into its reference string, which an opaque
  `CustomerId` rightly refuses (`bynk.types.interpolation_non_scalar`) — no
  string form outside its owning commons. Fixed the stub to a fixed reference.
  This is the opacity guarantee from ch 2 doing its job in the payoff chapter.
- **`source-lines` ranges recomputed** for the rewritten `orders.bynk`
  (POST `57,87` → `62,92`; GET `88,92` → `93,97`; header `0,14` unchanged).
  `inventory`/`payments` edits were line-preserving, so their ranges held.
- **Ch 13 unchanged** — the plugin host is a deliberately generic TypeScript
  counterexample (open-by-design systems), and its references to ch 12's
  findings remain accurate.

## Status: re-thread complete (Parts I–IV)

Every snippet is one `commerce` system; ch 12 opens the accumulated, extended
codebase. Gate: 26/26. Manuscript builds. The one remaining "purer" option not
taken — threading a single imported `commerce.values` through every chapter
rather than consistent in-context definitions — is recorded under Part II notes.
- **Minor, noted for later:** ch 9's `Stock` is `key sku: String` and its
  payment stub takes `amount: Int` (not `Sku`/`Cents`), because ch 9 is a
  testing-focused minimal example. Left as primitives; revisit only if the
  ch 9↔ch 12 seam reads badly.
