# 0162 — HTTP method correctness is a synthesised router behaviour derived from the route table — `405 + Allow`, a plain `OPTIONS`, and `HEAD` from `GET` — not a change to the `HttpResult` sum

- **Status:** Accepted (v0.139; 2026-07-04)
- **Provenance:** the v0.139 method-correctness increment — a single-increment emitter + runtime change addressing #489. The entry router answered *every* non-matching request with `404` (`emit_worker_entry`, `workers_entry.rs`), so a wrong-method request to a live route was indistinguishable from a missing one, no `Allow` header was ever emitted, and `HEAD`/`OPTIONS` were unimplemented. Surfaced during the v0.131 CORS work (ADR 0159), whose preflight branch had to special-case `OPTIONS` *around* exactly this fall-through.
- **Realises:** the method half of RFC 9110 conformance for a `from http` service — the three things a correct origin server does at the routing boundary before a handler runs (`405 + Allow`, `OPTIONS`, `HEAD`), derived once from the routes the service already declares.
- **Relates:** ADR 0159 (CORS — the "route enumeration is the source of truth" (D3) and "behaviour layer *around* the closed result sum" (D8) postures this generalises, and the preflight branch this refines); ADR 0126 (the `HttpResult` RFC 9110 status vocabulary — the closed registry this deliberately does **not** touch, and whose `MethodNotAllowed` `None` payload (D4) this honours); ADR 0143 (the `Raw` body, one of the variant families `HEAD` strips); ADR 0156 (the editor surface tracks the language — here a *no-op* delta, stated explicitly).

## Context

CORS (ADR 0159) made the case that a cross-origin policy is *wire behaviour of an
HTTP service* the compiler owns and lowers, not header plumbing the author
hand-rolls. The same argument applies one rung lower, to the method contract
itself. "Which methods does this path answer, and what happens to the others" is
not application logic — it is a property the service fully determines the moment
it declares its routes. `on GET("/links/:code")` and `on POST("/links")` between
them *say* that `/links` takes `POST` (and `OPTIONS`, and no `GET`), and that
`/links/:code` takes `GET` (and `HEAD`, and `OPTIONS`, and no `DELETE`). The
router threw that knowledge away at the fall-through and returned `404` for all of
it. This is the same "the enumeration is the source of truth" move as ADR 0159 D3,
turned on the router's own error path — and the same "behaviour *around* the
result lowering" posture as ADR 0159 D8. The difference from CORS: correct method
semantics have **nothing to declare** and no sensible "off", so there is no config
surface.

## Decisions

**D1 — Always on, no config surface — a router correctness fix, not a policy block.**
Unlike `cors { }` (a per-service *security policy* with real choices — which
origins, credentials), correct method semantics have no meaningful configuration
and no reason to be opt-in: an HTTP service that `404`s a wrong method, or has no
`OPTIONS`/`HEAD`, is simply wrong. So there is no grammar, no service-body section,
no annotation — the behaviour is synthesised for every `from http` service
unconditionally. *Consequence, stated plainly:* this is **not** additive-and-inert
the way CORS is (ADR 0159 D8) — it changes the emitted router of *every* HTTP
context (the fall-through region), so every `expected/workers/**/index.ts` fixture
regenerates, and a client that (unusually) depended on a wrong-method `404` now
sees `405`. That is a behavioural correction, not a contract break — Bynk never
promised `404` there.

**D2 — The per-path method set is one shared `emit`-time table, generalising the
landed CORS derivation (ADR 0159 D3).** CORS already computed allow-methods per
service to fill `Access-Control-Allow-Methods`. This increment needs the same
derivation for the `Allow` header, the `OPTIONS` answer, and the `HEAD` synthesis —
across the *whole* context, including services with no `cors { }`. So the rule is
lifted into one helper (`derive_allowed_methods`: the union of a path's declared
methods, `+ HEAD` when `GET` is present, `+ OPTIONS` always, alphabetical), and
`build_cors_services` is **refactored onto it** as one consumer. One enumeration,
three readers (preflight, `Allow`, `HEAD`). *Consequence:* a CORS-enabled service
that answers `GET` now advertises `HEAD` in its `Access-Control-Allow-Methods` too
(a cross-origin `HEAD` is a real request) — the honest result of the single table,
and the one CORS-surface change beyond the fall-through.

**D3 — `HEAD` is synthesised from `GET` — run the `GET` handler, return its status
and headers with an empty body.** Per RFC 9110 §9.3.2 a resource that answers `GET`
answers `HEAD` with identical headers and no body. The `GET` dispatch guard widens
to `method === "GET" || method === "HEAD"`, and on a `HEAD` the built `Response` is
replaced by `headResponse(response)` (`new Response(null, { status, headers })`) —
the same "rebuild an already-constructed `Response`" shape as the landed
`applyCors`. The handler **does run** (so its headers are the real ones a `GET`
would produce); this is the faithful reading and matches how mainstream servers
route `HEAD`. Two consequences are documented, not hidden: a `HEAD` incurs the
handler's effects and latency (a `HEAD`-without-execution optimisation is a named
follow-on), and `content-length` is **omitted** (the body is never materialised —
permitted, §9.3.2 "MAY"). To make "not drained" hold on every runtime, the SSE
helper (`sseResponse`) was made **lazy** (a `pull` source at `highWaterMark: 0`):
the source stream advances only when a reader pulls, so a `Streaming` `GET`
answered as `HEAD` — whose body is discarded unread — never advances the source at
all (and a real `GET` streams under backpressure rather than buffering up front).

**D4 — Ordering and composition with the auth seam and CORS.** The synthesised
`OPTIONS`/`405`/`HEAD` answers sit at the router's method boundary, so: (1) a plain
`OPTIONS` and a `405` are answered **before** the `by Actor`/Bearer seam,
unauthenticated — consistent with ADR 0159 D5 (method discovery and rejection are
credential-less). (2) `HEAD` runs the `GET` handler and therefore runs `GET`'s auth
seam unchanged. (3) **With CORS:** a *real* preflight is distinguished from a plain
discovery `OPTIONS` by the presence of `Access-Control-Request-Method` — a
preflight (has it) is answered by the landed CORS preflight branch with the
`Access-Control-*` headers; a bare `OPTIONS` (lacks it) falls through to the
generic `204 + Allow` here. This is a small, spec-correct refinement to the landed
preflight branch so the two `OPTIONS` answers compose instead of colliding.

**D5 — For a CORS-enabled service, the synthesised `405`/`OPTIONS` responses are
passed through the landed `applyCors` helper.** A cross-origin `405` (or `OPTIONS`)
with no `Access-Control-Allow-Origin` is invisible to the browser's JS — the exact
failure class #396 fixed for the success path. So the synthesised responses are
CORS-stamped for CORS services via `applyCors`, reusing that machinery rather than
duplicating it. For a non-CORS service the responses are emitted plain.

**D6 — The router-synthesised `405` carries `Allow`; the author-returnable
`MethodNotAllowed` variant stays as ADR 0126 D4 left it (bodyless, no `Allow`).**
The synthesised `405` is a *router* response (like the preflight and the `404`), so
it never touches `HttpResult` and can carry `Allow` directly from the table (D2).
The *author-chosen* `MethodNotAllowed` is a different thing — a handler explicitly
denying — and ADR 0126's Consequences already ruled that an `Allow` on it must be a
**payload shape** (the `Location` precedent), not a runtime special-case. Making an
author restate a route-derived `Allow` as a payload is precisely the "second source
of truth that drifts" ADR 0159 D3 rejects — so this increment leaves the variant
bodyless and records the payload-shape version as a re-openable follow-on.
`httpResultToResponse`, `HttpResult`, `HTTP_VARIANTS`, and `HTTP_STATUS` are **not**
changed.

**D7 — Lowered only in the Workers entry (ADR 0159 D9).** There is no separate
bundle-mode HTTP router: `bynkc test` stands integration/system participants up as
real Workers and dispatches through the Workers `fetch` in-process, so the
method-aware fall-through and the `HEAD` guard live only in `workers_entry.rs` and
are exercised end-to-end through that output (the `http_method_behaviour` harness).
A second, bundle-mode lowering is neither built nor needed.

## Consequences

- Every `from http` service answers its full method contract: a wrong method to a
  live path is `405 + Allow`, a plain `OPTIONS` is `204 + Allow`, and a `HEAD` to a
  `GET` route returns `GET`'s status/headers with an empty body — all derived from
  the declared routes, nothing written. An unknown path is still `404`.
- The runtime `http.ts` gains one pure helper, `headResponse(response)`, and
  `sseResponse` becomes lazy (pull-based, `highWaterMark: 0`); the shipped
  `runtime.ts` is regenerated by the bundler, guarded by the drift check. The
  `HttpResult` sum and its registries are untouched.
- `workers_entry.rs` gains a per-path method table (`build_path_method_table` over
  the shared `derive_allowed_methods`), the widened `GET`/`HEAD` guard, the
  `Access-Control-Request-Method` refinement to the CORS preflight branch, and the
  method-aware fall-through (with `applyCors` for CORS paths) replacing the blanket
  `404`.
- Every HTTP `expected/workers/**/index.ts` fixture regenerates (the fall-through
  and `GET` dispatch change); the CORS fixture also gains `HEAD` in its
  allow-methods and the preflight discriminator (D2, D4).
- Grammar, AST, and checker are **unchanged** — `HEAD`/`OPTIONS` are not
  author-declarable (`HttpMethod` stays `Get`/`Post`/`Put`/`Patch`/`Delete`), so
  there is nothing new to parse or reject. The editor surface (hover, completion,
  semantic tokens, signature help) is likewise unchanged: this increment adds no
  author-facing surface (ADR 0156 — stated as a no-op delta, not silence).
- Named follow-ons: a `HEAD`-without-handler-execution path (and a materialised
  `content-length`); an `Allow` **payload shape** for the author-returnable
  `MethodNotAllowed` (ADR 0126's deferred header-shaped-status precedent); and,
  once a general response-header surface exists (ADR 0159 D1's deferred follow-on),
  letting an author extend `Allow`.
