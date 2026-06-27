# 0126 — `HttpResult` tracks the RFC 9110 status vocabulary, and redirects gain a `Location` payload shape

- **Status:** Accepted (HTTP status-vocabulary increment; 2026-06-27).
- **Spec:** `runtime-library.md` §7.4.3 (`HttpResult`), `reference/http.md` (the variant table), `static-semantics.md` §5.7 (handlers).
- **Relates:** [ADR 0078](0078-queueresult-typed-verdict.md) (the sibling built-in typed-verdict sum — a closed, registry-driven variant set the runtime maps to a wire outcome, the same shape this widens); [ADR 0093](0093-completion-surface-contract.md) (the "a new built-in variant surfaces in completion for free" property this preserves).

## Context

`HttpResult[T]` shipped in v0.9 as a closed, curated sum of **ten** variants —
`Ok`/`Created`/`NoContent` and a handful of 4xx/5xx outcomes — over three
payload shapes: the value `T` as JSON (`Value`), an explanatory `String` as an
`{ "error": … }` body (`Message`), or no body (`None`). The registry lives in one
place (`HTTP_VARIANTS` in `bynk-syntax`); the checker, resolver, LSP completion,
and emitter read it by name, so the curated set was the only enumeration.

Two shipped examples ran into its ceiling, each with a documented caveat:

- **`rate-limiter`** wanted **429 Too Many Requests** but had to report the
  verdict in a `200` body — the status vocabulary had no deny code.
- **`link-shortener`** wanted a **302** redirect but had to return the target URL
  as JSON — there was no variant, and no payload shape, for a redirect (whose
  outcome is a `Location` header, not a body).

The gap was a vocabulary gap, not a structural one: the type, the lowering, and
the runtime mapping were sound; they simply did not span the status codes
handlers routinely return.

## Decisions

- **D1 — widen the registry to the common RFC 9110 vocabulary.** `HTTP_VARIANTS`
  grows from 10 to **29**: `Accepted` (202); the redirects `MovedPermanently`
  (301), `Found` (302), `SeeOther` (303), `TemporaryRedirect` (307),
  `PermanentRedirect` (308); the 4xx additions `MethodNotAllowed` (405),
  `NotAcceptable` (406), `RequestTimeout` (408), `Gone` (410), `LengthRequired`
  (411), `PayloadTooLarge` (413), `UnsupportedMediaType` (415), `TooManyRequests`
  (429), `UnavailableForLegalReasons` (451); and the 5xx siblings
  `NotImplemented` (501), `BadGateway` (502), `ServiceUnavailable` (503),
  `GatewayTimeout` (504). The set is **curated, not the full IANA registry** —
  the common, handler-authored statuses. 1xx informational, conditional `304`,
  and codes a handler never mints itself stay out (D4 of the boundary below).

- **D2 — redirects need a fourth payload shape, `Location`.** A redirect's
  outcome is a target URL in a `Location` header over an **empty** body — neither
  the JSON value of `Value` nor the `{ error }` body of `Message`. So
  `HttpVariantPayload` gains `Location`, carrying a `String`: the checker
  validates that argument exactly as `Message` does (a single `String`), the
  runtime emits `new Response(null, { status, headers: { location } })`, and
  pattern-matching binds it as `location: String`. The five 3xx variants share
  this shape.

- **D3 — the registry stays the single source of truth.** Adding a variant is a
  single `HTTP_VARIANTS` row: the resolver (name lookup), the LSP completion
  (ADR 0093, variants sourced from the registry), and the emitter's
  `HttpResult.<Variant>` lowering all pick it up with no per-variant edit. The
  **only** exhaustive matches are the payload-shape arms — construction-checking,
  pattern-binding, and the runtime's status map — which the new `Location` shape
  extends once each. The status→code mapping in the runtime is kept beside the
  registry as a comment cross-reference, not duplicated logic.

- **D4 — `Message` vs `None` is a per-status editorial call; the boundary is
  deliberate.** A status where an explanatory string helps the caller (the 4xx/5xx
  faults, and `429`) carries `Message`; a self-describing status (`405`/`406`/
  `408`/`410`/`411`, alongside the existing `401`/`403`/`404`) carries `None`.
  `202` carries the representation as `Value`, like `200`/`201`. A status outside
  the curated set has **no constructor** — a deliberate, re-openable boundary: the
  next example that needs one adds a registry row, exactly as this increment did.

- **D5 — additive, so no new diagnostics and no version bump.** No variant is
  renamed or removed (`ServerError` keeps its name, not `InternalServerError`), so
  this is a pure surface widening, not a breaking change. Construction reuses the
  existing `bynk.types.variant_arity` / `argument_mismatch` machinery; `Location`
  needs no bespoke error. A refined `Url` (a refined `String`) flows into a
  redirect argument by the existing refinement→base widening rule, so `Found(url)`
  and `Found(rawString)` both type.

## Consequences

- The two examples now use the real statuses: `rate-limiter` returns
  `429 TooManyRequests` on the deny path (the verdict body stays on the `200`
  allow path); `link-shortener` issues a `302 Found` with the target in
  `Location`. The "no redirect/429 yet" caveats are removed from both READMEs and
  the examples index.
- The curated-not-exhaustive stance is explicit: completeness is "what handlers
  return", not "every registered code". Widening later is a one-row edit, by
  design — the registry-driven consumers (D3) absorb it for free.
- The `Location` shape is the first non-body, header-carrying outcome in the sum;
  it sets the precedent for any future header-shaped status (e.g. an `Allow`
  header on `405`) to be a payload shape, not a special case in the runtime
  switch.
