---
level: minor
changelog: "`Fetch.send` sends extra request headers and returns the response's: `Request` and `Response` gain `headers: Map[String, String]` (a required field — existing literals add `headers: Map.empty()`), and `FetchError` gains `InvalidHeader` for a request whose headers conflict with a typed slot, name a header the platform owns or are not legal headers, or whose `contentType`/`authorization` value is not a legal header value (#1886)"
---

## ADR: fetch-headers
title: Fetch requests and responses carry a header map; conflicting and platform-owned headers are refused
summary: Request/Response gain headers: Map[String, String] (lowercased response keys); a typed-slot conflict or forbidden name is Err(InvalidHeader), unsent

**Context.** ADR 0022 shipped `bynk.Fetch` with a minimal typed core: a
`Request` whose only headers were the `contentType` and `authorization`
slots, the general header list deferred "until Bynk grows a sequence type".
Bynk has since grown `List` and `Map`, and the deferral now blocks real
services: `api.github.com` refuses a request without `User-Agent` (403), and
`Accept`, `If-None-Match`, API-version and vendor-key headers were all
unreachable (#1886). `Response` likewise exposed only status and body, so a
service could not read `ETag`, `Retry-After` or rate-limit headers.

**Decision.**

- **A map, not a list.** `Request` gains `headers: Map[String, String]`.
  A map states one value per name, which is what a request almost always
  wants and what `expect Fetch.send called once with
  req.headers.get("user-agent") == Some("…")` reads naturally against. A
  `List[Header]` would admit repeated names whose meaning (append or replace)
  the language would then have to define; the rare multi-valued request
  header is sent comma-joined, as HTTP allows.
- **Required, not defaulted.** Bynk record fields have no defaults, so the
  field is required and every existing `Request` literal adds
  `headers: Map.empty()` — a one-line, mechanical migration that keeps "this
  request sends no extra headers" explicit. `Map.empty()` takes its type from
  the field.
- **The typed slots stay, and own their headers.** `contentType` and
  `authorization` remain the typed slots. Header names are case-insensitive;
  a `headers` entry naming Content-Type or Authorization (in any case) while
  the matching slot is `Some` makes `Fetch.send` return `Err(InvalidHeader)`
  without sending, rather than let one silently win. While the slot is
  `None` there is no conflict, so `headers` may supply that header itself.
  A check in the type system was not possible: the map's keys are runtime
  values.
- **Platform-owned names are refused.** `host`, `content-length`,
  `connection`, `keep-alive`, `te`, `trailer`, `transfer-encoding`,
  `upgrade` and `expect` are refused the same way. These are the framing and
  hop-by-hop entries of the Fetch standard's forbidden request-header list:
  the host comes from the URL, the length and transfer coding from the body,
  and the rest are hop-by-hop headers the runtime manages per connection
  (Node's fetch rejects several outright). The list's browser-privacy
  entries (`cookie`, `origin`, `referer`, `dnt`, `sec-*`, …) stay settable,
  because a server-side Worker or Node service legitimately sends them. A
  duplicate name differing only in case, and a name or value that is not a
  legal header (the platform's `Headers` throws), are also `InvalidHeader`
  — never misreported as `Network`. So is a typed slot whose value is not a
  legal header value: before this change a `contentType` or `authorization`
  holding, say, a newline reached `fetch`, which threw, and surfaced as
  `Network`; it is now `InvalidHeader`, even with an empty `headers`.
- **`FetchError` gains `InvalidHeader`.** It is a distinct variant because it
  is a caller error that retrying cannot fix, unlike `Network`/`Timeout`. No
  exhaustive `match` on `FetchError` existed in the corpus, examples or docs,
  so adding the variant broke none.
- **`Response` gains `headers: Map[String, String]`, keys lowercased.** The
  platform's `Headers` iteration already yields lowercased names, so
  `res.headers.get("etag")` reads a header whatever case the server sent. A
  header the server repeated arrives as one value joined with ", ". The Fetch
  standard yields `set-cookie` once per value rather than joined, and
  runtimes have differed, so the bindings rebuild it from
  `Headers.getSetCookie()` where available and join it the same way — undici
  and workerd agree. For `set-cookie` the join is **lossy**: an `Expires`
  attribute holds a comma, so the joined value cannot be split back into its
  cookies reliably. A `List`-shaped slot for it is left to a later increment
  if a service needs one.
- **Where it lives.** The checks are in the `node` and `cloudflare` bindings'
  `FetchProvider` (kept textually identical); the `browser` binding still
  withholds `Fetch` (ADR 0138). Header values are not logged by any binding,
  so the `authorization` redaction question does not arise for `headers`.

**Consequences.** Services can call APIs that need any request header and
read the response's headers, and test doubles match on both. Every `Request`
and `Response` literal — in stubs, tests, examples and docs — names
`headers`. A future typed slot (say `userAgent`) would take ownership of its
header the same way the two existing slots do. ADR 0022's header deferral is
retired. The behaviour is pinned by `bynkc/tests/fetch_headers.rs`, which
drives both bindings under Node against a recording `fetch` stub.
