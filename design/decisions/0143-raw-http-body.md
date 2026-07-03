# 0143 — a raw HTTP body: the `Raw` payload shape and a `Raw` (200) variant carrying `(body: Bytes, contentType: String)`, lowered to a response with an author-declared `content-type`, no codec

- **Status:** Accepted (v0.111; 2026-07-01)
- **Provenance:** the v0.111 raw-HTTP-body increment — a single-increment language change adding one `HttpResult` payload shape and one variant, in the `Streaming` idiom. The service-tier boundary call: Bynk handlers return typed values (JSON by default), and non-JSON service bodies are served as an explicit, named raw body, not by a templating/view layer.
- **Realises:** the non-JSON service body. A handler can now serve `robots.txt`, `sitemap.xml`, `.well-known` documents, RSS/Atom feeds, a CSV download, or a QR-code PNG as an explicit `Raw(body, contentType)` — closing the "no way to return non-JSON without a template layer" gap, without adding a template layer.
- **Relates:** ADR 0126 (the `HttpResult` RFC 9110 status vocabulary and the payload-shape registry this extends — exactly as 0126 added `Location`); ADR 0129 (`Streamed`/`Streaming` — the sibling body-carrying shape added by one row + three exhaustive arms + one runtime case, and the 200-only precedent this reuses); ADR 0142 (`Bytes` — the octet sequence this body carries; D8's "typed paths round-trip a `Bytes` fine", of which the response body is one).

## Context

`HttpResult` is a closed registry of status-bearing variants over a closed set of
payload **shapes** (`None`/`Value`/`Message`/`Location`/`Streamed`). Adding a
variant is a single `HTTP_VARIANTS` row that the resolver, the LSP completion
(sourced from the registry), and the emitter's generic variant lowering
(`HttpResult.{name}(args)`) pick up for free. `Streamed` (ADR 0129) proved the
pattern for a *new shape*: one row + three exhaustive arms (construction-check,
the `variants_of` binding, the runtime status map) + one runtime case.

Handlers return typed values, serialised to JSON. There was no way to serve a
body that is *not* JSON. The only escape would have been a templating/view layer —
which Bynk deliberately does not have, because presentation is the frontend tier
(Cloudflare Pages), not the service tier. With `Bytes` shipped (ADR 0142), the
missing piece was a payload shape that carries raw octets and a `content-type` and
bypasses the JSON serialiser.

## Decisions

**D1 — One shape `(body: Bytes, contentType: String)`, binary-first, realised by a
single `Raw` variant.** `HttpVariantPayload` gains a `Raw` arm; `HTTP_VARIANTS`
gains one 200 row. Text bodies go through `Bytes.fromUtf8`, binary bodies
(a PNG) flow in directly. A single uniform shape matches the
one-variant-one-shape model, keeps the runtime a one-liner, and forces the charset
to be an explicit author decision rather than a runtime assumption. The cost — the
common text case wraps in `Bytes.fromUtf8` — is one documented call, and honest.
One shape covers both text and binary, so **XML needs no new variant**; it is
`Raw` today. A *typed* XML codec (an XML analogue of the `Value`→JSON path) is a
separate, larger decision, out of scope. An ergonomic `Text(String, …)` sugar
variant is a named follow-on if the wrapping friction proves out.

**D2 — 200-only, mirroring `Streaming`.** Keeping `Raw` at a single status
preserves the one-variant-one-status property that gives the registry its
single-source-of-truth and completion-for-free behaviour; letting `Raw` carry a
status *value* would break it. It also aligns with the tier boundary: the
genuinely service-tier raw bodies (`robots.txt`, `sitemap.xml`, `.well-known`,
feeds, QR PNG, CSV download) are overwhelmingly 200, whereas a custom-status raw
body — a `404` with an HTML error page — is the presentation concern this
increment deliberately excludes. A `Raw` branch and a pre-body ordinary variant
share `HttpResult[()]`, so they coexist in one handler. Non-200 raw bodies are the
**re-openable boundary** (a later `Raw`-family row per status, or a
status-carrying variant if per-status rows prove silly) — noted, not built.

**D3 — `contentType` is an opaque `String`, unvalidated in v1.** Accept any
`String`, exactly as `Message`/`Location` accept any `String`; do not validate
against a media-type registry. This keeps content-type validation out of the
checker — the construction arm still does the genuinely-new two-argument shape
work, but adds no media-type logic on top. A refined `MediaType` (a refined
`String`) is a named follow-on and the place to later enforce the
`fromUtf8`↔`charset=utf-8` correspondence.

**D4 — This is *not* the `workers`-wire `Bytes` boundary.** ADR 0142 D8 diagnoses
a *bare* `Bytes` in a `workers` wire signature (`bynk.types.bytes_at_workers_boundary`)
because that erased cross-context hop does not base64-encode it. The HTTP response
body is a **different** boundary: the runtime writes the `Uint8Array` straight into
`Response`, with no cross-context erasure — so `Raw(Bytes, …)` is one of the typed
paths that round-trip a `Bytes` fine. No new diagnostic, and no interaction with
the workers-wire rule (which lives in the emitter's wire-signature validation, not
in variant-argument checking).

**D5 — Additive; minor bump v0.111.** No variant or shape is renamed or removed;
this is a pure surface widening. `Raw` is the project's **first two-argument**
payload shape and its **first two-field** `variants_of` binding — the real work of
the increment is the construction-check arm's two independent type checks (arg0
against `Bytes`, arg1 against `String`). Construction reuses the existing
arity/mismatch diagnostic **codes** (`bynk.types.variant_arity` /
`bynk.types.argument_mismatch`), not their logic; `Raw` needs no bespoke
diagnostic. The `variants_of` binding is on no reachable path — an `HttpResult` is
constructed in handler position and never scrutinised — so it exists for Rust
exhaustiveness only; construction and runtime fixtures cover the shape end to end.

**D6 — The variant is named `Raw`.** Chosen over `Content`/`Body`: it warns the
reader that the typed-wire guarantee is deliberately off for this body — the author
owns the encoding, no codec runs. The runtime writes the `Uint8Array` into the
`Response` under `result.contentType`, bypassing `serialiseValue` entirely; that
bypass is the point of the shape.

## Consequences

- Non-JSON service bodies are expressible without a template layer; HTML
  *rendering* remains explicitly out of scope (the frontend tier).
- The runtime `http.ts` gains the union member, constructor factory, `HTTP_STATUS`
  row, and switch case; the shipped `runtime.ts` is regenerated by the existing
  bundler, guarded by the drift check. No per-variant emitter edit — variant
  construction lowers generically against the registry.
- Charset foot-gun: `Raw(Bytes.fromUtf8(s), "…; charset=…")` is UTF-8, so a
  content-type claiming another charset would mislead. v1 trusts the author (like
  `Message`); the refined `MediaType` (D3) is where enforcement can later live.
