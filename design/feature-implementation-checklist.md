# Bynk — Design Notes vs. Implementation Checklist

_Comparing `design/bynk-design-notes.md` (working draft, 9 May 2026) against the
compiler as it stands (head past v0.54; agent invariants landed v0.80 / ADR 0107)._

**Legend:** ✅ implemented (parsed + checked + emitted) · 🟡 partial · ❌ not yet
implemented. Most ❌ items are **deliberately deferred** to the "v1 coordination
surface", not oversights — the design notes flag many as open decisions, and
`bynk-status-and-roadmap.md` schedules them explicitly.

---

## Declarations & top-level kinds

| Feature (design §) | Status | Notes |
|---|---|---|
| `type` — records, sum/ADT types | ✅ | pipe and `enum` sum forms; nominal records |
| `actor` contracts (§6) | ✅ | `by` clause, context-sealed verified identity |
| — auth: Bearer (JWT/HS256), Signature (HMAC), None, Internal | ✅ | ADRs 0085/0089 |
| — auth: mTLS | ❌ | explicitly out of v0 scope |
| `service` + protocols (§7) | ✅ | `from <protocol>`; `call`-only default |
| — `from HTTP` (methods, path params, typed body, `HttpResult`) | ✅ | 200–500 status vocabulary |
| — `from Queue` (consumer, Ack/Retry) and `Cron` | ✅ | ADR 0002/0078 |
| — `from Events` (subscription) | ❌ | deferred — no Events track yet |
| — WebSocket / Alarm protocols | ❌ | deferred (held resources) |
| `agent` — state + identity | ✅ | → Durable-Object-style classes, `state`/`commit` |
| `fn` — module-level pure + agent-level `given` | ✅ | generics, lambdas, closures over capabilities |
| `on` handler clauses | ✅ | Call / HTTP / Queue / Cron kinds (not alarm/event) |
| `store` storage fields | 🟡 | agent `state` record + `Kv` binding storage only; rich storage kinds ❌ |
| `event` declarations + `Events.emit` + `EventEnvelope` | ❌ | no `event` keyword in lexer |
| `context` / `commons` / `test` contexts | ✅ | all three top-level kinds |
| Visibility: opaque / transparent / private; `uses` / `consumes` / `exports` | ✅ | enforced in resolver |
| `provides` (capability substitution / providers) | ✅ | constructor-injection, cycles rejected |
| adapters (logic-free unit kind) | ✅ | ADR 0010 (beyond the design notes) |

## Storage types (§10)

| Feature | Status | Notes |
|---|---|---|
| Immutable `List[T]`, `Map[K,V]` collection values | ✅ | kernel + Bynk-written combinator stdlib |
| `Kv` durable binding storage | ✅ | ADRs 0050/0051 |
| `Cell`, `Set`, `Log`, `Queue`, `Cache` storage kinds | ❌ | deferred (rich storage catalogue) |
| `Ref[A]` agent handle, `Connection`/`Held[T]` | ❌ | deferred |
| Write forms `:=` vs `.update(fn)` | ❌ | current model is immutable `commit { ...s, … }` spread |
| Map ops `put`/`update`/`upsert`/`remove`/`get` (mutating) | ❌ | only immutable `get`/`insert`/`keys` on collection `Map` |
| Refinement annotations `@indexed` / `@ttl` / `@retain` / `@bounded` | ❌ | deferred |

## Query algebra (§11)

| Feature | Status | Notes |
|---|---|---|
| `Query[T]` lazy queries + builders (filter/map/sortBy/join/groupBy…) | ❌ | deferred; eager list combinators exist instead |
| Terminals (collect/first/count/fold/any/all/forEach) | 🟡 | available as **eager** stdlib list fns, not Query terminals |
| Log time-window builders (since/before/between/recent) | ❌ | depends on `Log` |
| Indexing / index routing | ❌ | deferred |
| `traverse` (sequential effectful iteration) | ✅ | in `bynk.list` |
| `parTraverse` / `traverseAll` / `parTraverseAll` | ❌ | only sequential `traverse` ships |

## Capabilities, effects & failure model (§5, §12, §13)

| Feature | Status | Notes |
|---|---|---|
| `given` capability injection, `Effect[T]`, `<-` await | ✅ | + `Effect.pure`, tail auto-lift |
| `Result[T,E]`, outcomes vs faults, `?` propagation | ✅ | exhaustive `match`, `is` narrowing |
| Built-in capabilities: Clock, Random, Fetch/Http, Logger, Secrets, Config/IO | ✅ | first-party `bynk.bynk` adapter |
| `Alarms` capability | ❌ | deferred (held resources) |
| Fire-and-forget send `~>` | ✅ | ADR 0106 |
| `Idempotency` capability (`dedup`) | ❌ | deferred |
| `Sagas` capability + LIFO compensation | ❌ | deferred |
| `attempt` / `recover` (fault → outcome) | ❌ | deferred (rides with sagas/failure work) |

## Refined types (§15)

| Feature | Status | Notes |
|---|---|---|
| Refinement predicates: `Matches`(regex), `InRange`, `MinLength`/`MaxLength`/`Length`, `NonNegative`, `Positive`, `NonEmpty` | ✅ | full vocabulary |
| Boundary validation (HTTP body, params, etc. before handler runs) | ✅ | constructor returns `Result[T, ValidationError]` |
| Refinement in storage / rehydration validation | 🟡 | applies where storage exists; rich-storage paths deferred |
| External schema generation (OpenAPI/JSON-Schema from refined types) | ❌ | design aspiration; not built |

## Type system (§15)

| Feature | Status | Notes |
|---|---|---|
| HM inference, closed sums, nominal records, opaque types, parametric generics | ✅ | |
| No subtyping / no effect inference (by design) | ✅ | capabilities declared, not inferred |
| Deliberate exclusions: subtyping, HKT, row poly, type classes | ✅ (excluded) | intentionally out of scope |
| Base types `Int`, `String`, `Bool`, `Float` | ✅ | `Float` per ADR 0040 |
| Spec'd primitives `Decimal`, `Bytes`, `Timestamp`, `Duration` | ❌ | type-system spec is aspirational here |

## Validation & invariants (§14)

| Feature | Status | Notes |
|---|---|---|
| `test` contexts, `Mock[T]`, `provides` mocking, `assert`, call capture | ✅ | + integration tests over simulated wire (ADR 0009) |
| Agent `invariant` (runtime-checked at commit) | ✅ | **shipped v0.80 / ADR 0107** |
| — static provable-violation pass | ❌ | deferred follow-on |
| — typed agent-fault channel (distinguishable `InvariantViolation`) | ❌ | currently a bare 500 |
| Property-based testing / scenarios | ❌ | intended as library-level, not language |

## Schema versioning (§7)

| Feature | Status | Notes |
|---|---|---|
| Field defaults on **agent state** records | 🟡 | inline state initialisers (ADRs 0003/0004) |
| Field defaults on value/event types | ❌ | tied to events |
| `@schema(N)` annotation, `via schema(…)` clause, `schemaVersion` envelope | ❌ | deferred with events |

## Syntax (§16)

| Feature | Status | Notes |
|---|---|---|
| Glyphs `->` `<-` `=>` `:=` `==`/`!=`/`<=`/`>=` `&&`/`\|\|`/`!` `?` `..` | ✅ | `:=` currently for state-write surface; `~>` send added (ADR 0106) |
| String interpolation `\(expr)`, numeric literal separators/bases | ✅ | |
| `is` (pattern-as-Boolean), `implies` (in invariants) | ✅ | ADR 0007 / v0.80 |
| No pipe `\|>`, no custom operators (by design) | ✅ (excluded) | |

---

## Bottom line

The **whole intra-context language is implemented end-to-end** (parse → resolve →
check → emit, verified under `tsc --strict`): types, refinements, generics,
collections, effects/capabilities, the architectural primitives
(context/commons/agent/service/adapter/provider), HTTP/Queue/Cron transports,
the full **actors / boundary-auth** story, KV storage, and — newest — **agent
invariants** (v0.80).

What is **not yet implemented is almost entirely the v1 cross-context
coordination surface**, deferred by design rather than missed:

1. **Events** — `event` decls, emission, pattern subscription, envelopes,
   schema versioning (`via schema`). *(Nothing in the lexer yet.)*
2. **Sagas / compensation** — `Sagas` capability, LIFO unwind, `attempt`/`recover`.
3. **Idempotency capability** — `dedup`.
4. **Query algebra** — `Query[T]`, builders/terminals, Log time-windows, indexing.
5. **Rich storage kinds** — `Cell`/`Set`/`Log`/`Queue`/`Cache`/`Ref`, mutable
   write forms (`:=` / `.update`), storage annotations.
6. **Held resources** — `Connection`/WebSocket, `Alarms`.
7. **`parTraverse` family**, **external schema generation**, **mTLS**, and the
   spec'd-but-unbuilt primitives (`Decimal`, `Bytes`, `Timestamp`, `Duration`).

The two genuine *gaps within current scope* (not roadmap) flagged by the
project's own audit: `Int` precision beyond 2^53 (emits JS `number`), and
`any`-typed boundary emission in `workers` mode.
