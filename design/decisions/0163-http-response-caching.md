# 0163 — Conditional caching for `from http` GET responses is a synthesised router behaviour (automatic weak `ETag` + `304`) plus one opt-in handler annotation (`@cache` freshness) — not a change to the `HttpResult` sum

- **Status:** Accepted (v0.140; 2026-07-04)
- **Provenance:** the v0.140 response-caching increment — an emitter + runtime change plus the first grammar/checker handler-position annotation, addressing #492. A `from http` `GET` response carried **no validator and no freshness signal**: `httpResultToResponse` built an `Ok` as a bare `200` + `content-type` (`bynk-emit/runtime/src/http.ts`), the only `Cache-Control` anywhere being the SSE stream's `no-cache`. So every re-fetch of an unchanged resource transferred the full body, and there was no way to declare a freshness window.
- **Realises:** the read half of HTTP caching a browser and CDN already know how to use — free bandwidth on unchanged `GET`s via a content `ETag` and conditional `304`, and a declared freshness window where the author chooses one.
- **Relates:** ADR 0159 (CORS — the "derive what the compiler can, declare what only the author can" split (D-C), the "behaviour layer *around* the closed result sum" posture (D8), and `applyCors`, the in-place `Response`-stamping this reuses); ADR 0162 (HTTP method correctness — the sibling synthesised **router** response `405`/`OPTIONS`/`HEAD`, and the `GET` dispatch return site this composes into); ADR 0126 (the `HttpResult` status vocabulary — D4 kept conditional `304` **out** of the sum, the decision this honours); ADR 0111 (the `@ttl`/`@indexed` store-field annotation grammar this reuses for a handler annotation); ADR 0143 (the `Raw` body, one of the excluded variant families; and the drift-guarded embedded runtime copy); ADR 0156 (the editor surface tracks the language — here a **net-new** `@cache` completion/hover/semantic-token surface, the first handler annotation).

## Context

Caching a read endpoint is wire behaviour of an HTTP service, and it splits along
exactly the line ADR 0159 drew for CORS. The **validator** — the `ETag` that
answers "has this changed?" — is fully *derivable*: it is a hash of the bytes the
service already produces, so by the CORS DECISION C discipline (the
enumeration/derivation is the source of truth, never restated by the author) the
compiler should synthesise it. **Freshness** — `max-age`, "is stale data
acceptable, and for how long?" — is the one thing the compiler *cannot* know; only
the author does. So the design writes itself: derive the `ETag` automatically, let
the author declare the staleness window, and nothing else.

And a `304 Not Modified` is not a value a handler returns. ADR 0126 D4 deliberately
kept conditional `304` out of the `HttpResult` vocabulary; it is a decision the
*router* makes by comparing the request's `If-None-Match` against the response's
validator. So, like the CORS preflight and the method-semantics `405`, the `304` is
a synthesised **router** response — this increment is a behaviour layer *around*
`httpResultToResponse`, not a change to the closed sum.

## Decisions

**D1 — Automatic conditional `GET`: a synthesised weak `ETag` + `If-None-Match` →
`304`, on by default, *not* an `HttpResult` variant.** The validator is derivable
(a hash of the serialised body), so per ADR 0159 D-C the compiler produces it
rather than have the author restate it; and ADR 0126 D4 already put conditional
`304` outside the result vocabulary, so this is a router response (like the CORS
preflight and the method-semantics `405`), not an `HttpResult.NotModified`.
*Consequence, stated plainly:* an `ETag` header is added to every eligible `GET`,
so those responses change — this is **not** byte-inert the way CORS is — but it is
safe (revalidation never serves stale data; a client that sends no `If-None-Match`
gets an identical body plus a header). The GET-returning fixtures regenerate,
reviewed as `ETag`-only deltas. *Alternative rejected:* folding the conditional
half into `@cache` (opt-in) — the bandwidth win is free and the risk is nil, so
gating it would waste it.

**D2 — "Eligible" = a `GET` whose returned variant is the JSON `Ok`; `Streaming`,
`Raw`, redirects, and errors are excluded.** `ETag`/`304` are for safe, idempotent
reads with a hashable body. A `Streaming` (SSE) body cannot be hashed without
buffering it (defeating the stream); a `Raw` body could be hashed but is deferred;
redirects and error variants have no representation to validate. Eligibility is a
**value-level** property — the variant a handler returns, not its
`Effect[HttpResult[T]]` return type, which does not distinguish the variants — so
the runtime attaches the `ETag` only when `result.tag === "Ok"`; the excluded
families self-select to no `ETag` and are never answered `304`. Because the return
type cannot express eligibility, a **static** `@cache`-on-a-streaming-`GET`
diagnostic would need a handler-body scan; it is deferred as a named follow-on
(the runtime behaviour is already safe — a harmless no-op for the stream).

**D3 — The validator is a **weak** `ETag` over the serialised bytes, from a fast
non-cryptographic hash.** `W/"<hash>"`, where `<hash>` is FNV-1a over the
serialised JSON string — **synchronous**, so `httpResultToResponse` stays sync (no
`await crypto.subtle`). Weak (not strong/byte-exact) is correct here: the guarantee
is *semantic* equivalence of the representation, which weak validators exist for,
and Bynk serves no byte-range requests (the one place strong validators are
required). *Alternative rejected for v1:* SHA-256 via WebCrypto (strong, async) —
it would infect the response path with `async` for no practical gain over a weak
validator, and the hash is a config-free swap behind the `weakETag` helper later if
ever needed. *Consequence:* a hash collision would produce a false `304`; over
small JSON bodies this is negligible, and it is a *weak* validator by contract.

**D4 — Freshness is opt-in via a **handler annotation** `@cache(maxAge: Duration,
scope: public | private)`; the conditional `ETag` is not.** Only the author knows
whether stale data is acceptable and for how long, so `max-age` is declared, not
derived. It is a **handler** annotation, not a service `cache { }` section, because
caching is *per route* — `GET /links/:code` and `GET /health` want different
windows — so the per-service shape `cors { }` uses is the wrong granularity; the
`@ttl`/`@indexed` precedent (ADR 0111) is exactly a per-declaration policy
annotation and reuses the existing `Annotation` AST. This introduces the **first
handler-position annotation** — the grammar accepts `@name(args)` immediately
before `on`, a dangling one (no following `on`) being a parse error
(`bynk.parse.dangling_handler_annotation`), an unknown name
`bynk.http.unknown_handler_annotation`, and `@cache` off a `GET`
`bynk.http.cache_on_non_get`. `scope` defaults to `private` (never let a *shared*
cache store by default — the safe default); with no `@cache`, no `Cache-Control` is
emitted (the response is still revalidatable via its `ETag`). *Considered and
rejected default:* emitting `Cache-Control: no-cache` on every `GET`
(store-but-revalidate) — it pairs neatly with the `ETag` but silently changes edge
behaviour, so it stays out.

**D5 — Ordering & composition.** The conditional check runs *after* the handler
(the body must exist to hash) and *replaces* the `200` with a `304` at the `GET`
dispatch return site; then `applyCors` stamps it — a cross-origin `304` must still
carry `Access-Control-Allow-Origin` or the browser cannot read it. The composition
is `applyCors(notModifiedIfMatch(applyCache(httpResultToResponse(result, ser, {
weakEtag: true }), maxAgeSecs, scope), request), cors, origin)` — CORS outermost,
the conditional replacing the built response, `applyCache` present only when the
handler carries `@cache`, and the whole ahead of the existing `HEAD` body strip
(ADR 0162 D3). Lowered only in the Workers entry (ADR 0159 D9): there is no separate
bundle-mode HTTP router, so the `ETag`/`304`/`Cache-Control` handling lives in
`workers_entry.rs` alone; the emitter-embedded runtime copy regenerates from the
runtime source (drift-checked, ADR 0143). *Honest limitation:* because the `ETag`
is a content hash, a `304` still runs the handler and serialises the body — it saves
**bandwidth, not server work**. A cheaper validator that short-circuits *before*
the handler (e.g. a `Last-Modified` from a store timestamp) is a named follow-on.

## Consequences

- The closed `HttpResult` registry (`HttpResult`/`HTTP_VARIANTS`/`HTTP_STATUS`/
  `httpResultToResponse`'s variant→status map) is **byte-for-byte unchanged**; the
  `304` and the `ETag`/`Cache-Control` headers are a behaviour layer around it.
- Every eligible `GET` output changes (adds `ETag`), so the GET-returning
  `expected/workers/**` fixtures regenerate as `ETag`-only deltas; non-`GET` and
  ineligible routes are byte-for-byte unchanged.
- The first handler-position annotation lands, with a **net-new** editor surface
  (completion, hover, `decorator` semantic tokens) — not the "unchanged" ADR 0156
  delta prior increments had. The parse position is shared with any future handler
  annotation (e.g. a body-size `@limit`).
- Named follow-ons: cheap before-handler validators (`Last-Modified` /
  `If-Modified-Since`); strong `ETag`s / byte-range support; a static
  `@cache`-on-unhashable-`GET` diagnostic; server-side response *memoisation*; and
  **idempotency** (store-and-replay for unsafe methods keyed on a client
  `Idempotency-Key`) — a distinct future track sharing this proposal's
  response-storage machinery but keyed on a client-supplied key, not a derived
  validator.
