# Bynk — Status & Gap Audit

_Refreshed 18 June 2026 for **v0.54.0** (head `9da282c`). Scope: the whole `bynk`
repo — compiler (`bynkc`), driver (`bynk`), formatter (`bynk-fmt`), language
server (`bynk-lsp`), tree-sitter grammar, and the VS Code extension — assessed
against the language's own specs._

> This document supersedes the v0.9.2 audit (5 June 2026). The language has
> advanced ~45 increments since then: the whole intra-context behavioural layer,
> collections and generics, `Float`/JSON/string kernels, KV storage, the editor
> tooling arc (v0.24–v0.43), and the **actors** feature track (v0.45–v0.54) have
> all landed. The single live numbering authority is the **decision-record index**
> ([`decisions/README.md`](decisions/README.md)), which CI keeps complete by
> construction.

## How to read this audit

Bynk is described by three tiers of documents; conflating them produces a
misleading verdict, so this audit keeps them separate:

1. **The normative spec** (`site/src/content/docs/book/spec/`) plus the **decision records**
   (`decisions/`) are the authoritative "what exists now". The ADR index runs
   from v0.9.4 (ADR 0001) to v0.54 (ADR 0092) and is the spine of this audit.
2. **The design notes** (`bynk-design-notes.md`) and **type-system spec**
   (`bynk-type-system.md`) describe an aspirational **v1** language — events,
   sagas, a query algebra, the full storage-kind catalogue, agent invariants,
   held connections. Much of this is deliberately deferred and must **not** be
   scored as "missing".
3. **The tooling specs** (`bynk-lsp-spec.md`, `bynk-tree-sitter-spec.md`) and the
   forward roadmaps (`bynk-tooling-roadmap.md`, `bynk-engineering-roadmap.md`)
   sit alongside.

The headline: the compiler is **feature-complete for the cumulative v0 → v0.54
language**, with the entire surface wired end-to-end (parse → resolve → check →
emit) and emitted TypeScript verified under `tsc --strict`. What remains
genuinely "incomplete" is the large **v1 coordination surface** — events, sagas,
the query algebra and rich storage kinds, agent invariants — which is scheduled,
not broken.

> Verification note: this audit is grounded in the CI-enforced ADR index, the
> feature-track docs, and source reading (citations are `file:line` or fixture
> names). A full `cargo test` run was not re-executed for this refresh; the CI
> matrix (ubuntu/macOS/windows, `BYNK_REQUIRE_TSC=1`) is the live gate.

---

## 1. Executive summary

| Area | State | One-line verdict |
|---|---|---|
| **Compiler `bynkc`** (~42k LOC) | Feature-complete for v0–v0.54 | Whole language wired end-to-end; ~216 positive + ~40 negative fixtures; `tsc --strict` verifies every project fixture's emitted TypeScript. |
| **Driver `bynk`** (v0.46–v0.58) | Growing | Thin orchestrator over `bynkc` (override → PATH → sibling resolution). `bynk doctor` — environment check with a pinned output/exit contract (ADRs 0083–0084). `bynk new` (v0.58) — scaffold a complete, runnable project served by `dev` unmodified; offline file-writing, compile-tested template (ADR 0097). `bynk dev` (v0.57) — build + serve locally via `wrangler dev` in local mode; watch/rebuild on save (#524) and **multi-context local dev** (v0.167, ADR 0192) have both landed, so a multi-context project runs locally with live cross-context Service Bindings between its workers. The `doctor → new → dev` on-ramp arc is complete. `bynk deploy` (v0.154, slice 0) provisions KV and publishes; slice 2 (v0.170, ADR 0193) ships **every** context in Service-Binding dependency order, so a multi-context project deploys in one command; slice 1 (v0.171, ADR 0194) completes the per-context resource surface — queues are created by name before the push, and DO migrations ride `wrangler deploy`, whose applied-tag state the ledger deliberately does not mirror. Next intent: `new`'s `init`/`--template` follow-ups, deploy slice 3 (secrets), and the optional first-party `workerd` dev server. |
| **Actors track** (v0.45–v0.54) | ✅ Complete & closed | `actor` contracts, the `by` clause, BearerToken (JWT/HS256), Signature (HMAC-SHA256), multi-actor sum dispatch, authorisation invariants, cross-context `CallerId`. Q8 (replay/ordering) deferred to a future Events track. |
| **`bynk-fmt`** | Strong | Full formatter contract incl. comment preservation; idempotent, round-trip-tested over the corpus. |
| **`bynk-lsp`** | Rich | Diagnostics, hover, definition, completion, signature help, inlay hints, semantic tokens, codeLens, call hierarchy, implementation nav, folding/selection, workspace symbols, rename/references (v0.24–v0.43). The completion overhaul + editor polish shipped (ADRs 0093–0095, [`bynk-lsp-spec.md`](bynk-lsp-spec.md)); remaining: editor-agnostic setup docs + marketplace publishing ([#257](https://github.com/accuser/bynk/issues/257)/[#258](https://github.com/accuser/bynk/issues/258)). |
| **`tree-sitter-bynk`** | Lags the language | Strong v0–v0.5 grammar + highlights; behind on newer surface (`on http`/`from <protocol>`, `assert`-expr, `test`/`mocks`, `HttpResult`, actors). See [`bynk-engineering-roadmap.md`](bynk-engineering-roadmap.md). |
| **`vscode-bynk`** | Solid client | LSP client + status bar + scaffolds/walkthrough (v0.38); now bundles the server (B-0). Highlighting is TextMate, not the tree-sitter grammar. |
| **v1 coordination surface** (events, sagas, query algebra, rich storage kinds, agent invariants) | Deferred by design | Roadmap, not gap. |

---

## 2. What is done — the implemented language

The compiler runs a textbook pipeline — **lex → parse → resolve → check → emit**
(`bynkc/src/lib.rs`) — plus a two-pass multi-file project driver
(`bynkc/src/project.rs`, now decomposed into `project/{paths,discovery,
consistency,graph,symbols,diagnostics,tests_emit,validate}.rs`), two build
targets (`bundle`, `workers`), a test runner, integration tests, and a
formatter. The following are **fully wired end-to-end** and fixture-exercised
(ADR references are the authoritative increment markers):

- **Types**: refined types with the predicate vocabulary (`Matches`, `InRange`,
  `MinLength`/`MaxLength`/`Length`, `NonNegative`, `Positive`, `NonEmpty`),
  records, sum types (pipe and `enum` forms), opaque types (with access gated to
  the defining commons), and the built-in generics `Result`, `Option`, `Effect`,
  `HttpResult`, `ValidationError`, `()`.
- **Base types**: `Int`, `String`, `Bool`, and `Float` (a distinct base type
  erased to `number`, finite at the boundary — ADRs 0040–0044), with no implicit
  `Int`↔`Float` coercion (named conversions only).
- **Collections** (v0.20b, ADRs 0034–0039): built-in immutable `List` and `Map`
  with a thin kernel (`fold`/`foldEff`, `prepend`) and a Bynk-written combinator
  stdlib; value-keyable `Map` keys; list literals.
- **Generics & functions as values** (v0.20a, ADRs 0027–0033): `(params) => expr`
  lambdas, open-narrow generics (functions only, no bounds), argument-directed
  type inference, named functions as values, closures over capabilities.
  `Effect[T]` stays non-storable.
- **String & JSON** (v0.22): the string kernel (UTF-16 code units, ADR 0046),
  string interpolation `\(expr)` (ADR 0075), and the typed JSON codec with a
  compiler-known `JsonError` (ADRs 0045/0047) — no untyped `Json`.
- **Expressions / statements**: all operators, `if`/`else` as a value, `match`
  (exhaustiveness, unreachable/duplicate-arm checks), the `is` operator with
  branch-flow binding and refinement narrowing (ADR 0007), the `?` propagation
  operator, `let` / `let <-`, `commit`, and `assert` as an expression.
- **Effects**: `Effect[T]`, `<-` await, `given`-clause capability injection,
  providers with constructor-injection composition in topo order (cycles
  rejected — ADRs 0005/0006), `Effect.pure`, and tail-position auto-lift.
- **Architecture**: `commons`, `context` (with `exports opaque`/`transparent`),
  `uses` mixins, `consumes` dependency edges, capabilities, providers, services,
  agents (→ Durable-Object-style classes with `state`/`commit`; inline static
  state initialisers — ADRs 0003/0004), and **adapters** as a distinct
  logic-free unit kind (ADR 0010).
- **Cross-context** calls with structural compatibility checking and
  return-type rebranding; cross-context capability wiring by local instantiation
  (ADR 0008).
- **Services & protocols** (v0.44, ADRs 0077–0079): protocol on the header
  (`from <protocol>`), method-builders, a closed protocol set, and a `from`-less
  ⇒ `call`-only default.
- **HTTP**: `on http METHOD "/path/:id"` handlers, method routing, path-param
  binding, typed body deserialisation, and the `HttpResult[T]` status vocabulary
  (200/201/204/400/401/403/404/409/422/500).
- **Queues & cron** (v0.10, ADR 0002): consumer-only `on queue` with the
  `QueueResult` verdict (`Ack`/`Retry`, ADR 0078) and `on cron`.
- **Actors / boundary auth** (v0.45–v0.54 — track closed): `actor` contracts,
  the `by` clause (optional binder), context-sealed verified identities,
  BearerToken (compiler-generated JWT/HS256, ADR 0085), Signature (HMAC-SHA256
  webhooks, ADR 0089), multi-actor sum dispatch (first-wins, ADR 0090),
  authorisation invariants (refinement actors → 401/403 split, ADR 0091), and
  the cross-context `CallerId` (ADR 0092).
- **KV storage** (v0.23, ADRs 0050/0051): `Kv` with a binding-side list drain and
  camelCase write options (`putTtl`).
- **Platform & config** (v0.17–v0.19): config and IO as capabilities (no `needs`
  clause), secrets via injected env + `globalThis` probe, a minimal typed
  `fetch`, env threading for platform resources, and platform adapters under the
  reserved `bynk.*` prefix.
- **Tests**: `test` units with provider/context mocking (`mocks`), assertions,
  a readable runner, and **integration tests** over a simulated Node wire
  (v0.16, ADR 0009).
- **Build**: `bundle` and `workers` (per-context Worker bundles with generated
  `index.ts`, `compose.ts`, `wrangler.toml`), both shipping a shared
  `runtime.ts`; first-party sources authored as files and vendored via
  `include_str!` (v0.48, ADR 0086).
- **Quality gate**: every project-form fixture's emitted TypeScript is compiled
  under `tsc --strict --noEmit` (`bynkc/tests/tsc_verify.rs`); an
  emitted-output guard fails on placeholder markers.

---

## 3. Real gaps in the compiler (against current scope)

Genuine shortfalls within the language as already specified — not future
increments.

- **Spec/impl primitive divergence.** `bynk-type-system.md` §1.1 lists
  `Int | Decimal | String | Bool | Bytes | Timestamp | Duration | Unit` as
  primitives. The implementation now ships `Int`, `Float`, `String`, `Bool`,
  `Duration` (ADR 0112), `Instant` (ADR 0114 — the spec's `Timestamp`), `Bytes`
  (ADR 0142 — erased to `Uint8Array`, base64 on the wire, content equality), and
  `()`. Only `Decimal` (the spec name for the built `Float`, ADR 0040) and
  `Timestamp` (shipped as `Instant`) remain as spec-name divergences; no spec
  primitive is now wholly unbuilt.
- **`Int` precision.** `Int` literals validate as `i64` at lex time but emit to a
  JS `number`, so values beyond 2^53 lose precision at runtime. Decide: narrow
  to safe-integer range, or emit `bigint`.
- **Workers-edge type safety** — *closed in v0.176 (#642)*. The `workers` boundary
  carried its own codec dispatch, separate from the one the `bundle` and `Json`
  paths use, and it leaned on `as JsonValue` casts and an unvalidated identity
  deserialiser. All wire positions now route through the single generated-codec
  path, so the edge's static guarantees match the bundle path's. `Bytes` was the
  concrete casualty and is the concrete proof: it mis-round-tripped because the
  boundary cast it outbound while decoding it inbound, so ADR 0142 D8 diagnosed a
  bare `Bytes` rather than corrupt it; with one symmetric dispatch the
  restriction is retired and the diagnostic withdrawn. One bounded residue is
  named in `emission.md` §7.3.4b: the runtime-owned error types
  (`ValidationError`, `JsonError`, `HttpResult`, `QueueResult`) still pass through
  uncoded. The other — a context reaching a callee-owned type's codec through the
  callee's *module* rather than generating its own — is **closed in #661**: each
  Worker now generates its own cross-context codecs and imports no sibling
  context's module as a value. (ADR 0199 Decision G called that a prerequisite for
  the cross-context contract hash; ADR 0200 Decision H recorded the correction —
  the hash shipped in v0.177 without it.)
- **Brittle cross-context structural matching** — *closed in v0.177 (#643)*.
  Refinement predicates were compared positionally, so two structurally identical
  types whose predicates were written in a different order spuriously failed to
  match. They now compare as a **set**, through the same canonical normal form
  that backs the cross-context contract hash (ADR 0200) — so the matcher and the
  hash cannot disagree about what "the same refinement" is. The fix was not
  merely adjacent to the hash but a precondition for it: hashing source order
  would have 409'd two contexts that agree.
- **Open ADR.** ADR 0020 (adapter npm-dependency trust policy) is the one ADR
  still marked **Open**.

---

## 4. Deferred by design (the published roadmap)

These are **not** gaps; the specs schedule them.

- **Events / subscriptions** — the pub-sub model in design notes §7 (event
  emission, pattern-based subscription, fan-out). No `Events` track exists yet;
  the actors track's deferred **Q8 (replay/ordering)** rides with it.
- **Sagas / compensation** — the `Sagas` capability and LIFO compensation unwind
  in design notes §13.
- **Query algebra** — `Query[T]`, the builder/terminal vocabulary, time-window
  builders, and indexing in design notes §11.
- **Rich storage-kind catalogue** — the agent-local `Map`/`Set`/`Log`/`Queue`/
  `Cache`/`Ref`/`Held` storage model with the consistency rules in design notes
  §10/§12. (Distinct from what ships today: `Kv` binding storage + immutable
  `List`/`Map` collection values.)
- **Agent invariants** — ✅ runtime-checked invariants attached to agent state
  (design notes §14) **shipped in v0.80** (ADR 0107), distinct from the
  *authorisation* invariants on actors that shipped in v0.53. Two follow-ons stay
  deferred: the **static provable-violation pass**, and a **general
  typed-agent-fault channel** (to make an `InvariantViolation` caller-
  distinguishable rather than a bare 500).
- **Held resources** — `Connection`/WebSocket and a `workerd` dev server.
- **Core type-theory exclusions** (deliberate): subtyping, higher-rank/
  higher-kinded polymorphism, row polymorphism, type classes.

---

## 5. Tooling status

The editor tooling has largely caught up with the language; see
[`bynk-tooling-roadmap.md`](bynk-tooling-roadmap.md) for the forward plan. The LSP
track completed (decisions in ADRs 0093–0095, feature spec in
[`bynk-lsp-spec.md`](bynk-lsp-spec.md)); its remaining work is in issues
[#257](https://github.com/accuser/bynk/issues/257)/[#258](https://github.com/accuser/bynk/issues/258).

- **`bynk-fmt`** — full formatter contract incl. the hard comment-preservation
  requirement; idempotent and round-trip-tested. Remaining gap: comments buried
  inside expression sub-trees.
- **`bynk-lsp`** — the A/B-tier arc shipped across v0.24–v0.43: project
  diagnostics, the binding index, structured quick-fixes, inlay + semantic
  tokens, completion (types/fns/members/locals/keywords/snippets), signature
  help, codeLens reference counts, call hierarchy, implementation navigation,
  member-index kinds, folding/selection ranges. The completion overhaul and
  B-1/B-2 polish shipped (ADRs 0093–0095); remaining: editor-agnostic setup docs
  + marketplace publishing (issues #257/#258).
- **`tree-sitter-bynk`** — the biggest tooling lag: a strong v0–v0.5 grammar that
  has not been brought forward to the current surface (`from <protocol>` / `on
  http`, `assert`-expr, `test`/`mocks`, `HttpResult`, actors). Listed in the
  engineering roadmap.
- **`vscode-bynk`** — LSP client, status bar, and B-2 polish (scaffolds,
  commands, walkthrough, problem-matcher) are in; the server is bundled (B-0).
  Highlighting still uses a hand-written TextMate grammar rather than the
  tree-sitter grammar.

---

## 6. Roadmap

**Priority is sequenced by [`bynk-adoption-sequencing.md`](bynk-adoption-sequencing.md)
(#540 §7(2)):** the three **adoption blockers — deploy → migrations → ecosystem
posture, in that order — come first**, ahead of the language-vision tracks below,
which are reordered *behind* them. Tooling **depth is frozen** (currency is not —
see that record). The reason: none of the coordination-layer tracks moves
adoption until a team can ship, evolve stored state, and share code.

The forward plan lives in dedicated, domain-scoped docs:

- **Adoption blockers (first)** — **deploy** ([`tracks/deploy.md`](tracks/deploy.md),
  spine [#558](https://github.com/accuser/bynk/issues/558)); a **state-migrations**
  track (to be opened); an **ecosystem/packaging** track (to be written). Deploy
  and migrations gate 1.0 ([`bynk-1.0-definition.md`](bynk-1.0-definition.md),
  §7(4): 1.0 = Foundations stability + deploy + state migrations; ecosystem is
  1.0-optional).
- **Language vision (deferred behind the blockers)** — the next feature tracks,
  in rough order: an **Events** track (pub-sub + the deferred actors Q8
  replay/ordering), then **sagas/compensation**, the **query algebra + rich
  storage kinds**, **agent invariants**, and **held connections / WebSocket**.
  Deferred, not cancelled. Far-reaching features run as feature tracks per ADR
  0076 ([`tracks/`](tracks/README.md)); each slice becomes a `proposals/` entry.
- **Editor tooling** — [`bynk-tooling-roadmap.md`](bynk-tooling-roadmap.md)
  (LSP + VS Code); the LSP track completed (ADRs 0093–0095,
  [`bynk-lsp-spec.md`](bynk-lsp-spec.md); remainder in issues #257/#258).
- **Engineering** — [`bynk-engineering-roadmap.md`](bynk-engineering-roadmap.md):
  the CI/CD pipeline (Tier 4 publishing remains) and the compiler
  internal-quality refactor backlog.

**Hygiene to close out the current state:**

1. Add the implementation-status banner to `bynk-type-system.md` (Float vs
   Decimal; which primitives ship).
2. Resolve the `Int`-precision issue before Bynk handles large integers. (The
   workers-edge `any` half of this item closed in v0.176, #642.)
3. Bring `tree-sitter-bynk` up to the current surface (see engineering roadmap).
4. Close or re-scope the one **Open** ADR (0020, adapter dependency trust).

---

## 7. Bottom line

Bynk is a mature, end-to-end compiler that has executed its entire planned
MVP-and-beyond line: a refinement-and-effects type system, collections and
generics, the architectural primitives (contexts, services, agents, adapters,
providers, capabilities), HTTP/queue/cron transports, a complete boundary-auth
**actors** story, KV storage, a rich language server, and a `tsc --strict`
quality gate. The remaining "incomplete" surface is the **v1 coordination
layer** — events, sagas, the query algebra and rich storage kinds, agent
invariants, held connections — which the design notes have always scheduled for
later tracks. The honest verdict: **substantially complete against its own
shipped scope, with a clearly-bounded and deliberately-deferred v1 vision still
ahead.**
