# 0158 — CORS is a declarative per-service `cors { }` policy, lowered to a synthesised `OPTIONS` preflight and `Access-Control-*` stamping — not a general response-header surface

- **Status:** Accepted (v0.131; 2026-07-03)
- **Provenance:** the v0.131 CORS increment — a single-increment language + emitter change addressing #396. A `from http` service emitted a fixed header set (`content-type`/`location`/`cache-control`) and had no `OPTIONS` handler, so a Bynk Worker could not be called cross-origin from a browser. Deferred from the retired in-browser track (ADRs 0136–0140); surfaced building the playground snippet-share backend (#392), which sidestepped CORS with a Cloudflare route rather than letting Bynk express it.
- **Realises:** a Bynk HTTP API a browser can consume cross-origin — the preflight path and the `Access-Control-*` headers — declared once per service.
- **Relates:** ADR 0126 (the `HttpResult` RFC 9110 status vocabulary — the response *status* surface this sits beside, and the closed registry this deliberately does **not** touch); ADR 0143 (the `Raw` body — the "Bynk owns the service tier's wire form, not presentation" boundary this reasons from); ADR 0111 (`@`-annotations — the "grammar lenient, checker strict; unknown name is a diagnostic" precedent the `cors` fields follow); ADR 0156 (the editor surface tracks the language — the hover/completion delta this carries).

## Context

CORS is not presentation (the frontend tier — Cloudflare Pages — which Bynk does
not own, per ADR 0143). It is **wire behaviour of an HTTP service**: which origins
may call it, and the preflight contract that gates the call. The service tier is
exactly what Bynk owns; it already owns the response's status vocabulary (ADR 0126)
and body shape (ADR 0143). Access control at the browser boundary is the same kind
of thing — a property of the service, expressible declaratively and lowered by the
compiler, the way `by Visitor` lowers to an auth seam. So the shape should match the
rest of the tier: one closed, declarative policy the compiler reads, not an open
header escape hatch.

## Decisions

**D1 — A declarative per-service CORS policy, not a general response-header surface.**
The alternative — a per-handler `headers { … }` map or a `withHeader` on
`HttpResult` — is hand-rolled plumbing, and it would crack open the closed
`HttpVariantPayload` registry (ADR 0143 D1) that gives completion and the runtime
their single source of truth. A focused `cors` policy stays closed and covers the
concrete need in #396 and every future Bynk HTTP API. A general header surface
remains a separate, re-openable decision this increment does not open.

**D2 — The policy is a `cors { }` section in the service body, in header position.**
Not an annotation (a flat `@cors(...)` reads badly for a multi-field record) and not
a `from http(…)` clause argument (the `from` clause carries the *protocol* binding —
`queue("name")`, `WebSocket(in:,out:)` — not access *policy*). A named section
mirrors the agent phase order (`store` → `invariants` → handlers) and leaves room
for sibling service-policy sections later (rate-limit, cache) without another grammar
fork. `cors` is a **contextual keyword** (like `store`/`key`), recognised only in
service-body item position, so it stays usable as an ordinary identifier elsewhere —
no reserved-word churn. At most one `cors { }` per service (`bynk.parse.duplicate_cors`).

**D3 — `Access-Control-Allow-Methods` is derived from the routes, not declared.** The
service already enumerates its methods (`on POST`, `on GET`, …); restating them in
`cors` is a second source of truth that will drift. The preflight advertises the
union of the service's route methods plus `OPTIONS`. This is the same
"the registry is the enumeration" move as ADR 0126 — a route added later is
automatically preflightable, nothing to keep in sync.

**D4 — Origin model: a static allowlist of string literals, plus a `"*"` wildcard;
reflect the matched origin with `Vary: Origin`; no dynamic predicate in v1.** A
concrete allowlist compares the request `Origin` and **reflects** the matched value
(never echoes an unvalidated origin), adding `Vary: Origin` so a shared cache does not
serve one origin's grant to another. `["*"]` emits a literal `*` (and needs no
`Vary`). A no-match omits `Access-Control-Allow-Origin` entirely — the browser blocks,
fail-closed, the same posture as the auth seam. A computed origin predicate is a named
follow-on.

**D5 — The synthesised preflight is answered before the `by Actor` / Bearer auth seam,
unauthenticated.** A CORS preflight is by spec credential-less and method `OPTIONS` —
the browser sends it before it will attach `Authorization` or cookies. If the
preflight hit the auth seam it would `401` and the real request would never fire. So
the preflight is a dedicated branch **ahead of** the route dispatch, returning `204` +
the `Access-Control-*` headers with no handler invocation. The real (non-`OPTIONS`)
request still runs the full auth seam unchanged — preflight approval is not request
authorisation.

**D6 — `credentials: true` with `origins: ["*"]` is a compile-time error.** The Fetch
spec forbids `Access-Control-Allow-Credentials: true` alongside a wildcard ACAO (the
browser rejects it at runtime — a silent failure). Bynk catches it at the boundary
(`bynk.http.cors_wildcard_credentials`), the fail-at-compile posture the language
takes everywhere else.

**D7 — `Access-Control-Allow-Headers` defaults smartly, overridable.** Default =
`content-type` (the JSON body always needs it), plus `Authorization` **derived** when
the service has any Bearer-seam route (`route.bearer`), the same single-source move as
D3. An explicit `headers: […]` overrides. `maxAge` defaults to omitted (browser
default) and accepts a `Duration` (`1.hours`) lowered to `Access-Control-Max-Age`
seconds; `credentials` defaults `false`.

**D8 — Additive; minor bump v0.131. `HttpResult` untouched.** No variant, shape, or
existing behaviour changes; a service without `cors { }` emits byte-for-byte identical
output (the existing `1xx`/`2xx` HTTP fixtures are unmoved). `HttpResult` and its
registries (`HTTP_VARIANTS`, `HTTP_STATUS`) are **not** touched — this is a
service-policy layer *around* the result lowering. The grammar is lenient (any
`name: value` field parses); the closed field set (`origins`/`headers`/`credentials`/
`maxAge`), the value shapes, and D6 are enforced by the checker
(`bynk.http.cors_*`), following the ADR 0111 annotation precedent.

**D9 — CORS is lowered only in the Workers entry.** There is no separate bundle-mode
HTTP router in the emitter (`bynkc test` stands integration/system participants up as
real Workers and dispatches through the Workers `fetch` in-process). So the preflight
branch and the `applyCors` stamping live in `workers_entry.rs`; a dedicated
bundle-mode HTTP entry is not built (it does not exist, and building one is out of
proportion to this increment). CORS behaviour is exercisable end to end through the
Workers output the integration harness already drives.

## Consequences

- A `from http` service becomes browser-callable cross-origin by adding a `cors { }`
  block; the playground snippet-share backend (#392) can drop its Cloudflare `/api/*`
  route workaround.
- The runtime `http.ts` gains a `CorsPolicy` interface and two helpers —
  `applyCors(response, policy, origin)` (mutates `response.headers` in place, so it
  works uniformly across every `httpResultToResponse` shape, including the `Raw`
  bytes and the SSE `ReadableStream`, without reconstructing the body) and
  `corsPreflightResponse(policy, origin)`. The shipped `runtime.ts` is regenerated by
  the bundler, guarded by the drift check.
- `workers_entry.rs` synthesises one `CorsPolicy` constant per CORS-enabled service,
  an `OPTIONS` preflight branch matching that service's route paths (before the route
  dispatch), and wraps each of that service's route responses in `applyCors`.
- Foot-guns handled: `credentials` + `*` is a compile error (D6); cross-origin cache
  poisoning is prevented by `Vary: Origin` on the reflected-origin path (D4);
  preflight-bypasses-auth is intentional and spec-correct (D5).
- Named follow-ons: a computed/predicate origin policy; a general response-header
  surface (should it ever be wanted); sibling service-policy sections (rate-limit,
  cache); non-`Http` protocols remain rejected (`bynk.http.cors_not_http`).
