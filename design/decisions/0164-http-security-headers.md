# 0164 — Security response headers are a declarative per-service `security { }` policy, split by risk — `nosniff` on by default, `HSTS` opt-in — stamped like CORS

- **Status:** Accepted (v0.141; 2026-07-04)
- **Provenance:** the v0.141 security-headers increment — a single-increment emitter + runtime change (plus a small grammar/checker addition) addressing #493. A `from http` response carried **no** security headers (`httpResultToResponse` set only `content-type`/`location`/`cache-control` per variant), so a JSON body could be MIME-sniffed by a browser into an executable type (a content-sniffing XSS vector), and there was no declarative way to assert HTTPS-only. Hand-plumbing each header is exactly what the CORS increment (0159 D1) ruled out.
- **Realises:** secure-by-default transport headers for a Bynk HTTP API — the one header that always helps a JSON service on by default, and the one header with a real footgun made a deliberate opt-in — declared once per service.
- **Relates:** ADR 0159 (the sibling `cors { }` policy this mirrors structurally — the section surface, the runtime-helper stamping shape, the grammar-lenient/checker-strict split, and the Workers-only lowering); ADR 0126 (the closed `HttpResult` status vocabulary this sits beside and deliberately does **not** touch); ADR 0143 (Bynk serves bytes + a content-type, not markup — the boundary that excludes CSP/`X-Frame-Options`); ADR 0111 (`@`-annotations — the "grammar lenient, checker strict; unknown name is a diagnostic" precedent the fields follow); ADR 0163/0162 (the caching `304` and method `405`/`OPTIONS` — the synthesised router responses this policy also stamps); ADR 0156 (the editor surface tracks the language — the hover/completion delta this carries).

## Context

Security response headers are wire behaviour of an HTTP service, not application
logic and not presentation (the frontend tier Bynk does not own, ADR 0143). They
are the same kind of thing as CORS (0159): a closed, declarative policy the
compiler stamps on the service's responses. So the surface should match — a named
service-body section, lowered by a runtime helper the compiler threads into the
entry router.

The one new wrinkle over CORS is the **default**. A security header you have to
remember to switch on is the one you forget, so the *safe* headers should be on by
default. But "on by default" is only defensible for a header with **no** downside —
which is not all of them. So the set splits by risk: default the header that is
always safe and actually matters for a JSON API, and make the footgun a deliberate
opt-in. This is the same "the compiler owns the wire behaviour" stance as CORS, with
the honesty not to auto-enable a header that can lock a developer out of their own
domain.

## Decisions

**D1 — A curated, closed set — `nosniff` and `HSTS` only in v1; a declarative policy, not a general response-header surface.** As with CORS (0159 D1), the alternative — a per-response header escape hatch — is hand-rolled plumbing that would crack open the closed `HttpResult` registry (ADR 0126/0143). A focused `security` policy stays closed and covers the concrete need in #493. `Content-Security-Policy` and `X-Frame-Options` are **excluded**: they constrain the script and framing of *markup*, and Bynk serves bytes + a content-type (ADR 0143), so they would have nothing here to govern — shipping them would be cargo-cult. The exclusion is re-openable: if Bynk ever serves HTML, the section is where they would land.

**D2 — The policy is a `security { }` section in the service body, in header position, beside `cors { }`.** Security posture is per-service, not per-route (unlike caching's `@cache`, 0163) — so a header-position section is the right granularity, the sibling-section slot 0159 D2 explicitly reserved. `security` is a **contextual keyword** (like `cors`/`store`), recognised only in service-body item position, so it stays usable as an ordinary identifier elsewhere — no reserved-word churn, and deliberately kept out of the `CONTEXTUAL_KEYWORDS` hover registry (as `cors` is) so it owes no keyword-hover path. At most one `security { }` per service (`bynk.parse.duplicate_security`). Fields: `nosniff: <Bool>` (default `true`) and `hsts: <Duration>` (positive, opt-in), record form, the `cors { }` shape.

**D3 — Split the set by risk — `nosniff` on by default (opt-out), `HSTS` opt-in; never the reverse.** `X-Content-Type-Options: nosniff` has **no** downside and mitigates a real attack on a JSON API — a browser MIME-sniffing a JSON/text response into HTML and executing it — so it is stamped by default on every `from http` response. `Strict-Transport-Security` has a real footgun: it **pins** the browser to HTTPS for `max-age` (breaking a custom domain served over HTTP in dev/staging, and hard to undo once cached), and on a platform like Cloudflare TLS/HSTS is frequently owned at the edge — so HSTS is **opt-in**, never defaulted. The alternative — make everything opt-in via `security { }`, fully additive and inert without a block (the CORS posture) — is rejected: it forfeits the one safe, valuable default, which is the whole point of a security-headers feature.

**D4 — Consequence of D3, stated plainly: this is *not* byte-inert.** Because `nosniff` is default-on, `build_security_services` synthesises a policy for **every** `from http` service (default `nosniff: true`, no HSTS), not only those with a block — the one place the lowering diverges from `build_cors_services` (which is inert without a section). So every `from http` response changes, and every HTTP `expected/workers/**` fixture regenerates — reviewed as a single-header delta. This contrasts with the method-semantics (0162) and caching (0163) increments only in *what* changes by default, not in kind; it is the same "always-on router behaviour" posture, applied to a header.

**D5 — `HSTS` emits `max-age` only.** `includeSubDomains` and `preload` are each their own pinning footgun (they extend the HTTPS pin to subdomains and to the browser preload list, both hard to undo), and are named follow-ons, not defaults. A positive `Duration` is required (`bynk.http.security_invalid_field`) — HSTS with a zero/negative `max-age` is nonsensical (0 actively *clears* the pin).

**D6 — Stamping mirrors CORS via an `applySecurityHeaders` runtime helper; it composes with `applyCors` (disjoint headers), and every synthesised router response is stamped too.** `applySecurityHeaders(response, policy)` mutates `response.headers` in place (the `applyCors` shape, working uniformly across the `Ok`/`Raw`/redirect/error/stream variants). It uses `set` (idempotent, last-writer-wins), so a header also set at the edge is overwritten. The CORS preflight, the method-semantics `405`/`OPTIONS`, and the caching `304` all get `applySecurityHeaders` too — `nosniff` on a `204`/`405`/`304` is harmless, correct defence-in-depth, and keeps the rule simple: *every response the service emits* carries the policy. CORS and security stamp disjoint header sets, so their relative order is not observable; the normative composition is `applySecurityHeaders(applyCors(…), …)`, security outermost.

**D7 — Additive to the vocabulary; minor bump v0.141. `HttpResult` untouched.** No variant, shape, or status changes; `HttpResult`/`HTTP_VARIANTS`/`HTTP_STATUS`/`httpResultToResponse` are byte-for-byte unchanged — this is a header layer *around* the result lowering (0159 D8's stance), not a change to it. The grammar is lenient (any `name: value` field parses); the closed field set (`hsts`/`nosniff`), the value shapes, and the `from http`-only restriction are enforced by the checker (`bynk.http.security_*`), following the ADR 0111 precedent.

**D8 — Security headers are lowered only in the Workers entry.** As with CORS (0159 D9), there is no separate bundle-mode HTTP router in the emitter; the `applySecurityHeaders` stamping and the per-service policy constants live in `workers_entry.rs`, exercised end to end through the Workers `fetch` the integration harness drives in-process.

## Consequences

- Every `from http` response carries `X-Content-Type-Options: nosniff` unless the service declares `security { nosniff: false }`; `security { hsts: 180.days }` adds `Strict-Transport-Security: max-age=15552000`. `security { }` off `from http` is a checker error (`bynk.http.security_not_http`); CSP/`X-Frame-Options` are never emitted.
- The runtime `http.ts` gains a `SecurityPolicy` interface and one helper, `applySecurityHeaders(response, policy)` (mutates `response.headers` in place, uniform across every response shape). The shipped `runtime.ts` is regenerated by the bundler, guarded by the drift check.
- `workers_entry.rs` synthesises one `SecurityPolicy` constant per `from http` service (every service, default `nosniff: true`), and wraps each of that service's route responses — and the synthesised preflight, `405`/`OPTIONS`, and `304` — in `applySecurityHeaders`.
- Because `nosniff` is default-on, this is not byte-inert: every HTTP `expected/workers/**` fixture regenerates (single-header delta). This is the deliberate cost of the safe-by-default posture (D3/D4).
- Foot-guns handled: HSTS is opt-in and `max-age`-only (D5), documented with the dev-over-HTTP and edge-termination caveats; `nosniff` uses `set` so it composes idempotently with an edge that already sets it (D6).
- Named follow-ons: `includeSubDomains`/`preload` for HSTS; `Referrer-Policy`; CSP and `X-Frame-Options` should Bynk ever serve HTML; and a context-level default so a whole context's services share one posture.
