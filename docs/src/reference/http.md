# HTTP

HTTP handlers are declared in a `service` inside a `context`. See the
[grammar for `on http` handlers](grammar.md#rule-http_handler) for the production
and the diagnostics that govern it.

## Handler form

```karn
on http <METHOD> "<route>" (<params>) -> Effect[HttpResult[T]] {
  …
}
```

- **Methods:** `GET`, `POST`, `PUT`, `PATCH`, `DELETE`.
- **Route:** must start with `/`; a `:name` segment is a path parameter.
- **Parameters:** each parameter is either a path parameter (matching a `:name`
  segment) or the special `body` parameter. A path parameter's type must be
  constructible from a string (`karn.http.path_param_not_stringy`); `GET` and
  `DELETE` may not take a `body` (`karn.http.body_on_get_or_delete`).
- **Return type:** must be `Effect[HttpResult[T]]`
  (`karn.http.return_not_effect_http_result`).

> [!DANGER]
> The `/_karn/` route prefix is reserved for the runtime. Any route under it is
> rejected with `karn.http.reserved_prefix`.

A `body` parameter is parsed from the request JSON and validated before the
handler runs; an invalid body is rejected with `400` at the boundary.

## `HttpResult` variants

| Variant | Status | Payload |
|---|---|---|
| `Ok(value)` | 200 | the value, as JSON |
| `Created(value)` | 201 | the value, as JSON |
| `NoContent` | 204 | none |
| `BadRequest(message)` | 400 | message |
| `Unauthorized` | 401 | none |
| `Forbidden` | 403 | none |
| `NotFound` | 404 | none |
| `Conflict(message)` | 409 | message |
| `UnprocessableEntity(message)` | 422 | message |
| `ServerError(message)` | 500 | message |

> [!TIP]
> When `Ok`/`Err` could mean either `Result` or `HttpResult`, qualify the
> constructor (e.g. `HttpResult.Ok(…)`) to resolve
> `karn.types.ambiguous_constructor`.

## Request lifecycle

```mermaid
flowchart TD
  req["incoming request"] --> router["Worker fetch — index.ts router"]
  router --> match{"route matches?"}
  match -->|no| nf["404"]
  match -->|yes| params["bind :name path params"]
  params --> body{"body valid?"}
  body -->|no| bad["400 at the boundary"]
  body -->|yes| handler["handler runs — returns Effect"]
  handler --> result["HttpResult[T]"]
  result --> status["HTTP status + JSON body"]
```

*Validation happens once, at the edge; the handler only ever sees valid input.*

Text equivalent: the Worker's `fetch` entry point (`index.ts`) routes the request;
an unmatched route is a `404`. On a match, path parameters are bound and any
`body` is parsed and validated against its refined type — an invalid body is
rejected with `400` at the boundary, before the handler runs. The handler then
runs as an `Effect` and returns an `HttpResult[T]`, which is mapped to an HTTP
status and JSON body per the table above.

## Example

```karn
context notes

service api {
  on http GET "/ping" () -> Effect[HttpResult[String]] {
    Ok("pong")
  }

  on http GET "/notes/:id" (id: String) -> Effect[HttpResult[String]] {
    NotFound
  }
}
```

## Emission

`on http` services compile to a runnable Cloudflare Worker on the `--target
workers` target (`index.ts` router, `handlers.ts`, `compose.ts`,
`wrangler.toml`). See [emission](emission.md) and
[Target Cloudflare Workers](../how-to/projects/cloudflare-workers.md).
