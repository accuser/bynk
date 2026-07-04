# 0165 — Request body-size limits are a synthesised `413` boundary rejection, declared by a per-service `limits { }` section with a per-route `@limit` override — a `Content-Length` fast-reject, opt-in and inert when absent

- **Status:** Accepted (v0.142; 2026-07-04)
- **Provenance:** the v0.142 body-limits increment — an emitter + runtime change plus a small grammar/checker addition. Realises the increment proposed in issue #494. A body-taking route (`POST`/`PUT`/`PATCH`) had **no** way to bound its request body: the entry router read and validated the body before the handler ran, so a client could stream an arbitrarily large payload the service buffered into memory before it could turn it away — a denial-of-service surface with no author-facing control.
- **Realises:** a declared byte ceiling on a `from http` service's body-taking routes, enforced as a cheap boundary rejection before the body is read — the request-side counterpart to the response-side header policies (CORS, security headers).
- **Relates:** ADR 0164 (the sibling `security { }` header-position section this places `limits { }` beside — the contextual-keyword section surface, the grammar-lenient/checker-strict split, the Workers-only lowering); ADR 0163 (the `@cache` handler annotation whose placement and reused `Annotation` AST the `@limit` override mirrors — the second handler-position annotation); ADR 0162 (the method-semantics `405` — the synthesised **boundary** response, answered *before the auth seam*, that this `413` posture follows); ADR 0159 (CORS — the opt-in, inert-when-absent posture (D8) this adopts over the security default-on posture, and `applyCors`, the stamping the synthesised `413` passes through); ADR 0126/0143 (the closed `HttpResult` status vocabulary this reuses the existing `413` from and deliberately does **not** touch); ADR 0111 (the `@`-annotation grammar-lenient/checker-strict precedent the fields and args follow); ADR 0166 (the numeric digit separators motivated by this increment's large byte counts); ADR 0156 (the editor surface tracks the language — the hover/completion delta for the new section and annotation).

## Context

An unbounded request body is wire behaviour of an HTTP service, not application
logic — the same kind of thing as CORS (0159) and the security headers (0164): a
closed, declarative policy the compiler enforces at the boundary. So the surface
should match — a header-position section the author declares, lowered by the entry
router.

It splits from the header policies on **one** axis: it is a **request-side** check,
not a response-side stamp. There is no header to add to a response; there is a
`Content-Length` to compare *before reading the body*. So unlike CORS and the
security headers, it is not an `applyX(response, policy)` helper — it is an inline
check in the route dispatch, ahead of the body read. But the rejection it produces
*is* a response, so it composes with the existing stamping.

And a `413 PayloadTooLarge` is not a value a handler returns. Like the method
`405` (0162) and the caching `304` (0163), it is a decision the **router** makes
from the request — here, from `Content-Length` — before the handler is ever
reached. So this is a behaviour layer *around* the result lowering, not a change to
the closed `HttpResult` sum.

## Decisions

**D1 — The rejection is a synthesised router `413`, not an author-returned
variant.** An over-cap request is refused by the router before the handler runs, so
it cannot be an `HttpResult` the handler returns — like the method `405` (0162 D6)
and the caching `304` (0163 D1), it is a synthesised **boundary** response. It
reuses the **existing `413`** status (`{ kind: "PayloadTooLarge", details: … }`),
so the closed `HttpResult` registry (ADR 0126/0143) is **byte-for-byte unchanged** —
no new variant, no new status. *Alternative rejected:* surfacing the raw body to a
handler that checks the size itself — that defeats the point (the body is already
buffered) and duplicates a boundary concern into every handler.

**D2 — A per-service `limits { }` section with a per-route `@limit` override;
route wins.** Body size has both a sensible service-wide default and a legitimate
per-route exception (an upload endpoint accepts more than the rest), so the surface
carries both granularities — the service **`limits { maxBody: <Int> }`** section in
header position (beside `cors { }`/`security { }`, a contextual keyword like them),
and a **`@limit(maxBody: <Int>)`** handler annotation (the `@cache` placement, ADR
0163, reusing the `Annotation` AST). The effective cap is **route `@limit` →
service `limits` → none**: the route override wins, mirroring how a per-declaration
annotation refines a section default. At most one `limits { }` per service
(`bynk.parse.duplicate_limits`) and one `@limit` per handler
(`bynk.http.limit_duplicate`). `@limit` is legal **only on a body-taking method**
(`POST`/`PUT`/`PATCH`) — on a `GET`/`DELETE` it is `bynk.http.limit_on_bodyless`,
since a bodyless method has no body to cap.

**D3 — `maxBody` is a positive `Int` byte count in v1; a byte `Size` literal is a
named follow-on.** The value is a raw byte count — `maxBody: 1_048_576` (1 MiB),
`maxBody: 26_214_400` (25 MiB) — a positive `Int`
(`bynk.http.limits_invalid_field` / `bynk.http.limit_bad_max_body`). A dedicated
byte `Size` literal (`1.mb`, `25.mb`), following the `Duration` playbook, would
read better but is a whole lexical/type feature of its own; it is deferred as a
named follow-on. This increment's large byte counts are exactly what motivated the
digit separators of ADR 0166, which make an `Int` byte count legible in the
meantime.

**D4 — Enforcement is a `Content-Length` fast-reject before the body read — a
cheap first line, not a hard guarantee.** The router compares the request's
`Content-Length` against the effective cap and rejects **before any body read**, so
an over-cap payload is never buffered. This is honest about its limits:
`Content-Length` can be **absent** (a chunked transfer) or **spoofed**, so the
check is a cheap fast-reject that pairs with the Workers platform's own
request-size cap, **not** a hard guarantee. A streamed-read cap that counts bytes as
they arrive and aborts mid-stream is the hard version, and is a named follow-on.

**D5 — Opt-in and inert when absent; the `413` is CORS/security-stamped; ordering
is before the auth seam and before the body read.** Unlike the security headers
(0164 D3, default-on), body limits are **opt-in** — the CORS posture (0159 D8): a
route with neither a `@limit` nor a service `limits { }` has **no cap** and emits
**byte-for-byte unchanged** output. So this increment is byte-inert for any service
that does not adopt it — only capped routes' fixtures change. The synthesised `413`
is passed through the same `applyCors`/`applySecurityHeaders` stamping as every
other response, so a cross-origin caller can read it. Ordering: the
`Content-Length` check runs **before the body is read** and **before the
`by`/Bearer auth seam** (like the method `405`, 0162 D4 — a size rejection is a
boundary concern, decided without a principal), then the stamping wraps the result.

**D6 — Additive; grammar lenient, checker strict; lowered only in the Workers
entry.** The grammar accepts any `name: value` field in `limits { }` and any
`@name(args)` before a handler; the closed field/arg set (`maxBody` only), the
positive-`Int` rule, the `from http`-only rule (`bynk.http.limits_not_http`), and
the body-taking-method rule are enforced by the checker (`bynk.http.limits_*` /
`bynk.http.limit_*`), following the ADR 0111 precedent. As with CORS (0159 D9) and
the security headers (0164 D8), the enforcement lives in `workers_entry.rs` — there
is no separate bundle-mode HTTP router — exercised end to end through the Workers
`fetch` the integration harness drives in-process.

## Consequences

- A `from http` service gains a `limits { maxBody }` section and a `@limit(maxBody)`
  handler annotation; a body-taking route with an effective cap answers an over-cap
  request (`Content-Length` > cap) with a synthesised `413 PayloadTooLarge` before
  the body is read, CORS/security-stamped.
- The closed `HttpResult` registry
  (`HttpResult`/`HTTP_VARIANTS`/`HTTP_STATUS`/`httpResultToResponse`) is
  **byte-for-byte unchanged** — the `413` reuses an existing status, and the check
  is inline router behaviour, not a helper on the result path.
- Opt-in and inert: a service that adopts no cap emits byte-for-byte identical
  output, so only capped routes' `expected/workers/**` fixtures change (contrast the
  default-on security headers, 0164 D4).
- The check keys on `Content-Length`, so it is a fast-reject paired with the
  platform cap, not a hard guarantee (D4).
- `@limit` is the second handler-position annotation (after `@cache`, 0163),
  reusing the shared `@`-before-`on` parse; hover/completion track the new section
  and annotation (ADR 0156).
- Named follow-ons: a byte `Size` literal (`1.mb`); a streamed-read cap that aborts
  mid-body (the hard guarantee); a per-content-type limit (a different ceiling per
  declared body type); and write-path **idempotency** (store-and-replay for unsafe
  methods, the distinct track flagged in 0163) — a natural neighbour of the
  write-path concerns this increment opens.
