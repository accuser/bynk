---
level: patch
changelog: A `from http` route's boundary-rejection responses (`400`/`401`) now carry the service's security headers (`nosniff`/HSTS) and CORS, exactly as its handled `200` does — restoring ADR 0164 D6 on the rejection path (#659)
---

## ADR: boundary-rejection-security-headers

title: A `from http` route's rejections carry the service security policy — ADR 0164 D6 on the rejection path
summary: A boundary-rejection `400`/`401` a `from http` route emits is stamped with `applySecurityHeaders`/`applyCors` just as its handled `200` is, closing #659; scoped to service-addressed responses, with the service-less terminal `404`/`500`, the internal `/_bynk/call/` door, and websocket deferred as separate concerns.

**Context.** ADR 0164 D6 states the security-header rule as a universal: "*every response the service emits* carries the policy" — and the book repeats it ("`nosniff` is stamped by default on every response"). #659 found the rejection path violates it: the boundary-rejection responses a `from http` route emits — a `RefinementViolation`/`BoundaryError`/`MalformedJson` `400`, a `Signature`-seam `401` — were emitted as bare `new Response(…)` in the router (`bynk-emit/src/emitter/workers_entry.rs`), skipping the `applySecurityHeaders`/`applyCors` wrapper that the handled `200`, the `405`, the `304`, and the `413` all pass through. The `400`s are the one response class that **reflects attacker-controlled input** (the offending value is echoed into the JSON body), so the missing `nosniff` sat on exactly the responses where content-sniffing is a concern — the vector #493/ADR 0164 exist to close, still open on rejection. `nosniff` is defence-in-depth, not a turnkey XSS, but the invariant was documented, universal, and violated.

**Decision.**

- **D1 — every response a `from http` route emits is stamped, via one helper.** A `stamp_rejection(inner, cors_const, security_const)` helper produces `applySecurityHeaders(applyCors(inner, …), …)` — the same expression the happy path and the `413` build — and every rejection site in `emit_http_route_dispatch` (and the path-param `RefinementViolation` in `emit_path_param_construction`) now returns through it. The policy is the **addressed service's own**: a service that opts out (`security { nosniff: false }`) opts its rejections out too; a CORS-enabled service's rejection is CORS-readable, matching how its `405`/`413` already behave. This restores D6 for the public HTTP surface with no new invariant — a rejection is just another response the service emits.

- **D2 — the scope is service-*addressed* responses; three neighbours are deferred, each for a stated reason.** D6's "the service emits" presumes a service. Three response classes fall outside that and are left as separate follow-ons: (a) the **terminal `404`/`500`** at the end of `fetch` — no route matched (or an exception unwound), so there is no single service policy to apply, and a browser could host services with conflicting policies; a deployment-level default is a genuinely new decision, not a restoration of D6, and the bodies are static text with no reflected input. (b) The **internal `/_bynk/call/` door** — #659 scopes it out explicitly: it is the trusted Service-Binding channel between co-deployed Bynk services, not an attacker-facing edge, so the risk is not the same. (c) **WebSocket** (`ws_{sname}_open` in `workers.rs`) — structurally different and deferred track-wide; its rejections reflect only route-defined param names.

**Consequences.**

- The reflected-input `400`s — the highest-risk class — now carry `nosniff` (and HSTS/CORS when the service declares them). The HTTP Bearer/OIDC `401` was already stamped: those wrappers return `HttpResult.Unauthorized`, which flows through the entry's `httpResultToResponse` and is stamped there; only the bare-`new Response` sites were the gap.
- `http_security_behaviour.rs` — the one Rust suite that drives the emitted `fetch` in-process — gains the coverage it lacked: it asserted the `200`/`405`/`HEAD` D6 enumerated and never a rejection. It now drives a refined-path-param `400` and asserts (i) the reflected-input rejection carries the `store` service's `nosniff`+HSTS, and (ii) the `admin` opt-out service's rejection carries **no** `nosniff` — proving the fix stamps the per-service policy, not a blanket header. Fixture `299_http_security` gains a `ShortCode = String where MinLength(3)` route on both services to drive it.
- Ten route goldens re-blessed (the rejection sites gain the wrapper); no example changed; no language, grammar, checker, or runtime surface changed — this is an emitter behaviour fix (hence `patch`).
- **Not in scope:** the terminal `404`/`500`, the `/_bynk/call/` internal door, and websocket rejections (D2).
