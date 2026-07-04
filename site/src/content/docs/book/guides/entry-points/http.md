---
title: "Handle an HTTP request and shape an `HttpResult`"
---
**Goal:** answer an HTTP request, reading path parameters and a request body, and
return the right status.

Handlers go in a `service` inside a `context`. Each handler names a verb, a route,
its parameters, and returns `Effect[HttpResult[T]]`.

## A handler with no input

```bynk
context notes

service api from http {
  on GET("/ping") by Visitor () -> Effect[HttpResult[String]] {
    Ok("pong")
  }
}
```

`Ok("pong")` is the `HttpResult` variant for `200 OK`.

## Read a path parameter

A `:name` segment in the route becomes a parameter of the same name:

```bynk
  on GET("/notes/:id") by Visitor (id: String) -> Effect[HttpResult[String]] {
    NotFound
  }
```

## Accept a request body

A `body` parameter is parsed and validated from the request's JSON before the
handler runs — an invalid body is rejected with `400` at the boundary:

```bynk
type NewNote = { title: String }

service api from http {
  on POST("/notes") by Visitor (body: NewNote) -> Effect[HttpResult[NewNote]] {
    Created(body)
  }
}
```

## Choose the right status

Return the `HttpResult` variant matching the outcome — `Ok` (200),
`Created` (201), `Accepted` (202), `NoContent` (204), a redirect such as
`Found(url)` (302) or `SeeOther(url)` (303), `BadRequest(msg)` (400),
`Unauthorized` (401), `Forbidden` (403), `NotFound` (404), `Conflict(msg)` (409),
`UnprocessableEntity(msg)` (422), `TooManyRequests(msg)` (429),
`ServerError(msg)` (500), `ServiceUnavailable(msg)` (503). See the
[HTTP reference](/book/reference/http/) for the full table. Map domain errors
to statuses with `match`:

```bynk
fn handle(ok: Bool) -> HttpResult[String] {
  if ok {
    Ok("done")
  } else {
    BadRequest("bad input")
  }
}
```

## Stream an incremental response

To send a response as it is produced — a progress feed, a token relay, a live
tick — return `Streaming(stream)` instead of a buffered `Ok(value)`. It carries a
[`Stream[String]`](/book/reference/types/#stream) and emits it as Server-Sent
Events (`text/event-stream`); each stream element is one `data:` event.

```bynk
context feed

service Feed from http {
  on GET("/ticks") by Visitor () -> Effect[HttpResult[()]] {
    Streaming(Stream.of(["tick-1", "tick-2", "tick-3"]).take(3))
  }
}
```

A streamed response returns `Effect[HttpResult[()]]` — there is no JSON body
value, so the parameter is `()`. Because the status is sent before the first
chunk, streaming is **200-only**: decide any failure *before* you start, and
return an ordinary variant instead — it shares `HttpResult[()]`, so both live in
one handler:

```bynk
on GET("/feed/:mode") by Visitor (mode: String) -> Effect[HttpResult[()]] {
  if mode == "live" {
    Streaming(Stream.of(["a", "b", "c"]).take(2))
  } else {
    NotFound
  }
}
```

See [HTTP → Streamed responses](/book/reference/http/#streamed-responses) for
the framing rules and the mid-stream-error pattern.

## Return a non-JSON body

Handlers return typed values, serialised as JSON. To serve something that is
*not* JSON — `robots.txt`, `sitemap.xml`, an RSS feed, a CSV download, a QR-code
PNG — return `Raw(body, contentType)`. It writes a raw
[`Bytes`](/book/reference/types/#bytes) body straight into the response under the
`content-type` you declare, with **no codec** in between.

`Bytes` is binary-first: a PNG flows in directly, and text goes through
`Bytes.fromUtf8`, which makes the charset explicit — the body is UTF-8, so pair
it with a matching `content-type`.

```bynk
context site

service Site from http {
  on GET("/sitemap.xml") by Visitor () -> Effect[HttpResult[()]] {
    let xml = "<?xml version=\"1.0\"?><urlset></urlset>"
    Raw(Bytes.fromUtf8(xml), "application/xml")
  }
}
```

Like `Streaming`, `Raw` returns `Effect[HttpResult[()]]` and is **200-only** —
it is for service-tier bodies, which are almost always `200`.

**Why can't I just return HTML?** Because rendering is the frontend tier's job,
not the service's. Bynk serves *bytes with a content-type*; it deliberately has
no HTML template layer. A page — including a styled `404` — belongs in the
frontend (Cloudflare Pages), not in a handler. See
[HTTP → Raw responses](/book/reference/http/#raw-responses) for the full rules.

## Call it from a browser (CORS)

By default a `from http` service is **same-origin**: a browser page on a
*different* origin cannot read its responses, because the response carries no
`Access-Control-*` headers and there is no `OPTIONS` handler for the browser's
preflight. That is the safe default — you opt a service in, per origin.

To make a service cross-origin callable, add a `cors { }` policy at the top of the
service body and list the origins that may call it:

```bynk
context api

service api from http {
  cors {
    origins: ["https://app.example.com"],
  }

  on GET("/items/:id") by v: Visitor (id: String) -> Effect[HttpResult[String]] {
    Ok(id)
  }
}
```

That is all it takes. From the policy the compiler:

- answers the browser's **preflight** — an `OPTIONS` to any of the service's
  routes returns `204` with the `Access-Control-*` headers. The preflight is
  answered *before* the handler's `by`/Bearer authentication, because a browser
  sends it with no credentials attached — so a preflight is never rejected by an
  actor check; and
- **stamps `Access-Control-Allow-Origin`** on every response the service returns.

The allowed **methods** are taken from the routes you already declared — you never
restate them. Allowed **headers** default to `content-type` (plus `Authorization`
when the service authenticates with a Bearer actor). Add `credentials: true` to
allow cookies / `Authorization`, and `maxAge: 1.hours` to let the browser cache
the preflight. A request from an origin *not* on the list simply gets no grant, so
the browser blocks it — the same fail-closed posture as authentication.

> **Wildcard + credentials is a compile error.** `origins: ["*"]` opens the service
> to any origin, but you cannot combine it with `credentials: true` — the browser
> rejects that pair at runtime, so Bynk rejects it at compile time. With
> credentials, name the exact origins.

See [HTTP → CORS](/book/reference/http/#cors) for the full field table and matching
rules.

## Cache a read endpoint

A read endpoint that returns the same bytes to repeated callers is wasting
bandwidth. Two things fix that, and Bynk splits them by who knows what.

**Revalidation is automatic.** Every `GET` that returns `Ok` already carries a weak
`ETag` over its body — you write nothing. A browser that re-requests with the
`ETag` it saved gets a `304 Not Modified` with an empty body instead of the whole
payload:

```bynk
on GET("/links/:code") by v: Visitor (code: String) -> Effect[HttpResult[String]] {
  Ok(code)
}
```

`GET /links/abc` → `200` + `ETag: W/"…"`. The next `GET /links/abc` with
`If-None-Match: W/"…"` → `304`, empty body. Nothing to configure.

**Freshness you declare.** Only you know whether a client may serve a response
*without* checking back, and for how long. Say so with `@cache`, written just above
the handler:

```bynk
@cache(maxAge: 5.minutes)
on GET("/links/:code") by v: Visitor (code: String) -> Effect[HttpResult[String]] {
  Ok(code)
}
```

That adds `Cache-Control: private, max-age=300`. Reach for `scope: public` only
when a **shared** cache or CDN should store the response too:

```bynk
@cache(maxAge: 1.hours, scope: public)
on GET("/config") by v: Visitor () -> Effect[HttpResult[String]] {
  Ok("…")
}
```

The default is `private` — a per-user cache, never a shared one — because that is
the safe choice for anything that might vary by caller. Durations are plural
(`5.minutes`, `1.hours`), and `@cache` is only valid on a `GET` returning `Ok`;
the compiler flags a singular unit or a misplaced annotation.

See [HTTP → Caching](/book/reference/http/#caching) for the eligibility rules and
the full field table.

## Build and run

HTTP services compile to a Cloudflare Worker with `--target workers`. See
[Target Cloudflare Workers](/book/guides/projects-build-and-deployment/cloudflare-workers/).

## Related

- Tutorial: [Build a small HTTP service](/book/tutorials/02-http-service/).
- Reference: [HTTP](/book/reference/http/) — the complete variant/status table.
