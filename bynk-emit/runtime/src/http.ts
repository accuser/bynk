import type { JsonValue } from "./boundary.ts";

// v0.9: HttpResult — the built-in HTTP-result sum.

export type HttpResult<T> =
  // 2xx success — carries the serialised value.
  | { readonly tag: "Ok"; readonly value: T }
  // 2xx streamed body — SSE-framed (real-time track slice 1).
  | { readonly tag: "Streaming"; readonly stream: AsyncIterable<string> }
  // 2xx raw body — author-owned bytes with an explicit content-type (v0.111).
  | { readonly tag: "Raw"; readonly body: Uint8Array; readonly contentType: string }
  | { readonly tag: "Created"; readonly value: T }
  | { readonly tag: "Accepted"; readonly value: T }
  | { readonly tag: "NoContent" }
  // 3xx redirection — carries the target URL, emitted as a Location header.
  | { readonly tag: "MovedPermanently"; readonly location: string }
  | { readonly tag: "Found"; readonly location: string }
  | { readonly tag: "SeeOther"; readonly location: string }
  | { readonly tag: "TemporaryRedirect"; readonly location: string }
  | { readonly tag: "PermanentRedirect"; readonly location: string }
  // 4xx client error.
  | { readonly tag: "BadRequest"; readonly message: string }
  | { readonly tag: "Unauthorized" }
  | { readonly tag: "Forbidden" }
  | { readonly tag: "NotFound" }
  | { readonly tag: "MethodNotAllowed" }
  | { readonly tag: "NotAcceptable" }
  | { readonly tag: "RequestTimeout" }
  | { readonly tag: "Conflict"; readonly message: string }
  | { readonly tag: "Gone" }
  | { readonly tag: "LengthRequired" }
  | { readonly tag: "PayloadTooLarge"; readonly message: string }
  | { readonly tag: "UnsupportedMediaType"; readonly message: string }
  | { readonly tag: "UnprocessableEntity"; readonly message: string }
  | { readonly tag: "TooManyRequests"; readonly message: string }
  | { readonly tag: "UnavailableForLegalReasons"; readonly message: string }
  // 5xx server error.
  | { readonly tag: "ServerError"; readonly message: string }
  | { readonly tag: "NotImplemented"; readonly message: string }
  | { readonly tag: "BadGateway"; readonly message: string }
  | { readonly tag: "ServiceUnavailable"; readonly message: string }
  | { readonly tag: "GatewayTimeout"; readonly message: string };

export const HttpResult = {
  // 2xx success.
  Ok: <T>(value: T): HttpResult<T> => ({ tag: "Ok", value }),
  // 2xx streamed body — the argument is a stream of SSE event payloads.
  Streaming: (stream: AsyncIterable<string>): HttpResult<never> => ({ tag: "Streaming", stream }),
  // 2xx raw body — the arguments are the octets and their content-type.
  Raw: (body: Uint8Array, contentType: string): HttpResult<never> => ({ tag: "Raw", body, contentType }),
  Created: <T>(value: T): HttpResult<T> => ({ tag: "Created", value }),
  Accepted: <T>(value: T): HttpResult<T> => ({ tag: "Accepted", value }),
  NoContent: { tag: "NoContent" } as HttpResult<never>,
  // 3xx redirection — the argument is the target URL (Location header).
  MovedPermanently: (location: string): HttpResult<never> => ({ tag: "MovedPermanently", location }),
  Found: (location: string): HttpResult<never> => ({ tag: "Found", location }),
  SeeOther: (location: string): HttpResult<never> => ({ tag: "SeeOther", location }),
  TemporaryRedirect: (location: string): HttpResult<never> => ({ tag: "TemporaryRedirect", location }),
  PermanentRedirect: (location: string): HttpResult<never> => ({ tag: "PermanentRedirect", location }),
  // 4xx client error.
  BadRequest: (message: string): HttpResult<never> => ({ tag: "BadRequest", message }),
  Unauthorized: { tag: "Unauthorized" } as HttpResult<never>,
  Forbidden: { tag: "Forbidden" } as HttpResult<never>,
  NotFound: { tag: "NotFound" } as HttpResult<never>,
  MethodNotAllowed: { tag: "MethodNotAllowed" } as HttpResult<never>,
  NotAcceptable: { tag: "NotAcceptable" } as HttpResult<never>,
  RequestTimeout: { tag: "RequestTimeout" } as HttpResult<never>,
  Conflict: (message: string): HttpResult<never> => ({ tag: "Conflict", message }),
  Gone: { tag: "Gone" } as HttpResult<never>,
  LengthRequired: { tag: "LengthRequired" } as HttpResult<never>,
  PayloadTooLarge: (message: string): HttpResult<never> => ({ tag: "PayloadTooLarge", message }),
  UnsupportedMediaType: (message: string): HttpResult<never> => ({
    tag: "UnsupportedMediaType",
    message,
  }),
  UnprocessableEntity: (message: string): HttpResult<never> => ({
    tag: "UnprocessableEntity",
    message,
  }),
  TooManyRequests: (message: string): HttpResult<never> => ({ tag: "TooManyRequests", message }),
  UnavailableForLegalReasons: (message: string): HttpResult<never> => ({
    tag: "UnavailableForLegalReasons",
    message,
  }),
  // 5xx server error.
  ServerError: (message: string): HttpResult<never> => ({ tag: "ServerError", message }),
  NotImplemented: (message: string): HttpResult<never> => ({ tag: "NotImplemented", message }),
  BadGateway: (message: string): HttpResult<never> => ({ tag: "BadGateway", message }),
  ServiceUnavailable: (message: string): HttpResult<never> => ({ tag: "ServiceUnavailable", message }),
  GatewayTimeout: (message: string): HttpResult<never> => ({ tag: "GatewayTimeout", message }),
};

// Match a path pattern (e.g., "/orders/:id") against a request path.
// Returns the captured parameter map, or null on no match.
export function matchPath(
  pattern: string,
  path: string,
): { params: Record<string, string> } | null {
  const patternSegments = pattern.split("/").filter(Boolean);
  const pathSegments = path.split("/").filter(Boolean);
  if (patternSegments.length !== pathSegments.length) return null;
  const params: Record<string, string> = {};
  for (let i = 0; i < patternSegments.length; i++) {
    const p = patternSegments[i];
    if (p.startsWith(":")) {
      params[p.slice(1)] = decodeURIComponent(pathSegments[i]);
    } else if (p !== pathSegments[i]) {
      return null;
    }
  }
  return { params };
}

// The HTTP status code each HttpResult variant maps to. Kept in sync with
// HTTP_VARIANTS in bynk-syntax/src/ast.rs (the compiler-side source of truth).
const HTTP_STATUS: Record<HttpResult<unknown>["tag"], number> = {
  Ok: 200,
  Streaming: 200,
  Raw: 200,
  Created: 201,
  Accepted: 202,
  NoContent: 204,
  MovedPermanently: 301,
  Found: 302,
  SeeOther: 303,
  TemporaryRedirect: 307,
  PermanentRedirect: 308,
  BadRequest: 400,
  Unauthorized: 401,
  Forbidden: 403,
  NotFound: 404,
  MethodNotAllowed: 405,
  NotAcceptable: 406,
  RequestTimeout: 408,
  Conflict: 409,
  Gone: 410,
  LengthRequired: 411,
  PayloadTooLarge: 413,
  UnsupportedMediaType: 415,
  UnprocessableEntity: 422,
  TooManyRequests: 429,
  UnavailableForLegalReasons: 451,
  ServerError: 500,
  NotImplemented: 501,
  BadGateway: 502,
  ServiceUnavailable: 503,
  GatewayTimeout: 504,
};

// Serialise an HttpResult<T> to a Response. The variant determines the HTTP
// status code; success variants carry the value as JSON, redirects emit a
// Location header, error variants carry an `{ error }` body, and the remaining
// statuses are bodyless.
// Frame a stream of event payloads as an SSE (`text/event-stream`) Response.
// Each stream element is one SSE event; a multi-line element becomes multiple
// `data:` lines, terminated by a blank line. The body is a ReadableStream, so
// this is a Web standard that runs unchanged on Workers and Node.
function sseResponse(stream: AsyncIterable<string>): Response {
  const encoder = new TextEncoder();
  // v0.139 (ADR 0162): a `pull` source with `highWaterMark: 0` makes the body
  // lazy — the source stream is advanced only when a reader pulls, never
  // eagerly at construction. So a `Streaming` `GET` answered as `HEAD` (whose
  // `Response` body is discarded unread) never advances the source at all, and a
  // real `GET` streams under the consumer's backpressure rather than buffering
  // the whole stream up front. `cancel` returns the iterator so an abandoned
  // read releases the source promptly.
  const iterator = stream[Symbol.asyncIterator]();
  const body = new ReadableStream<Uint8Array>(
    {
      async pull(controller) {
        const { value, done } = await iterator.next();
        if (done) {
          controller.close();
          return;
        }
        for (const line of value.split("\n")) {
          controller.enqueue(encoder.encode(`data: ${line}\n`));
        }
        controller.enqueue(encoder.encode("\n"));
      },
      async cancel(reason) {
        await iterator.return?.(reason);
      },
    },
    { highWaterMark: 0 },
  );
  return new Response(body, {
    status: 200,
    headers: { "content-type": "text/event-stream", "cache-control": "no-cache" },
  });
}

export function httpResultToResponse<T>(
  result: HttpResult<T>,
  serialiseValue: (v: T) => JsonValue,
): Response {
  const status = HTTP_STATUS[result.tag];
  switch (result.tag) {
    // 2xx with a body — the serialised value as JSON.
    case "Ok":
    case "Created":
    case "Accepted":
      return new Response(JSON.stringify(serialiseValue(result.value)), {
        status,
        headers: { "content-type": "application/json" },
      });
    // 200 with a streamed body — each stream element is one SSE event.
    case "Streaming":
      return sseResponse(result.stream);
    // 200 with a raw body — author-owned bytes with an explicit content-type.
    // No codec runs; the Uint8Array is written straight into the Response. The
    // `as BodyInit` cast satisfies TS 5.7, whose `Uint8Array<ArrayBufferLike>`
    // no longer matches DOM's `BufferSource` (it excludes SharedArrayBuffer) —
    // a bare Uint8Array is a valid body at runtime on both Workers and Node.
    case "Raw":
      return new Response(result.body as BodyInit, {
        status: 200,
        headers: { "content-type": result.contentType },
      });
    // 3xx — bodyless, with the target URL in the Location header.
    case "MovedPermanently":
    case "Found":
    case "SeeOther":
    case "TemporaryRedirect":
    case "PermanentRedirect":
      return new Response(null, { status, headers: { location: result.location } });
    // Error variants carrying an explanatory message — `{ error }` JSON body.
    case "BadRequest":
    case "Conflict":
    case "PayloadTooLarge":
    case "UnsupportedMediaType":
    case "UnprocessableEntity":
    case "TooManyRequests":
    case "UnavailableForLegalReasons":
    case "ServerError":
    case "NotImplemented":
    case "BadGateway":
    case "ServiceUnavailable":
    case "GatewayTimeout":
      return new Response(JSON.stringify({ error: result.message }), {
        status,
        headers: { "content-type": "application/json" },
      });
    // Self-describing statuses — bodyless.
    case "NoContent":
    case "Unauthorized":
    case "Forbidden":
    case "NotFound":
    case "MethodNotAllowed":
    case "NotAcceptable":
    case "RequestTimeout":
    case "Gone":
    case "LengthRequired":
      return new Response(null, { status });
  }
}

// v0.139 (ADR 0162): rebuild a `GET` response as its `HEAD` answer — identical
// status and headers, empty body (RFC 9110 §9.3.2). `Response.body` is not
// re-read: passing `null` discards the original body without consuming it, so a
// `Streaming` (`SSE`) `GET` answered as `HEAD` returns the stream's headers while
// its `ReadableStream` is never started or drained. `content-length` is omitted
// (the body is never materialised — permitted, §9.3.2 "MAY").
export function headResponse(response: Response): Response {
  return new Response(null, { status: response.status, headers: response.headers });
}

// v0.131 (ADR 0159): the cross-origin (CORS) policy a `from http` service carries
// via its `cors { }` section. The compiler synthesises one of these per
// CORS-enabled service and threads it into the entry router. `allowMethods` is
// derived from the service's routes at emit time (never restated by the author);
// `allowHeaders` carries the resolved list (the `content-type`/`Authorization`
// default, or the author's override). `maxAgeSecs` is the `Access-Control-Max-Age`
// in whole seconds, absent when the author gave no `maxAge`.
export interface CorsPolicy {
  readonly origins: readonly string[];
  readonly allowMethods: readonly string[];
  readonly allowHeaders: readonly string[];
  readonly credentials: boolean;
  readonly maxAgeSecs: number | null;
}

// Resolve the `Access-Control-Allow-Origin` value for a request, given the
// policy and the request's `Origin` header. A wildcard policy (`["*"]`) answers
// every origin with `*` (and needs no `Vary`, since the response does not depend
// on the request origin). A concrete allowlist **reflects** the request's origin
// when it matches — never echoes an unvalidated value — and returns `null` on no
// match, so the caller omits the header and the browser fails the request closed.
function corsResolveOrigin(policy: CorsPolicy, requestOrigin: string | null): string | null {
  if (policy.origins.length === 1 && policy.origins[0] === "*") return "*";
  if (requestOrigin !== null && policy.origins.includes(requestOrigin)) return requestOrigin;
  return null;
}

// Stamp the CORS response headers onto an already-built `Response`, in place.
// `Response.headers` is mutable, so this works uniformly across every
// `httpResultToResponse` shape — JSON, `Raw` bytes, a redirect, an error body,
// or an SSE `ReadableStream` — without reconstructing the response or touching
// its body. When the origin does not match the allowlist, no ACAO is added (the
// browser blocks the read); a reflected origin also gets `Vary: Origin` so a
// shared cache never serves one origin's grant to another.
export function applyCors<R extends Response>(
  response: R,
  policy: CorsPolicy,
  requestOrigin: string | null,
): R {
  const allowOrigin = corsResolveOrigin(policy, requestOrigin);
  if (allowOrigin === null) return response;
  response.headers.set("access-control-allow-origin", allowOrigin);
  if (allowOrigin !== "*") response.headers.append("vary", "Origin");
  if (policy.credentials) response.headers.set("access-control-allow-credentials", "true");
  return response;
}

// Build the `204 No Content` preflight response for an `OPTIONS` request against
// a CORS-enabled route. Answered by the entry router *before* the auth seam — a
// preflight is credential-less by spec, so it must not be rejected by a `by`
// actor / Bearer check. A non-allowlisted origin still gets a bodyless `204`, but
// without the `Access-Control-*` grant, so the browser's preflight check fails
// closed.
export function corsPreflightResponse(policy: CorsPolicy, requestOrigin: string | null): Response {
  const headers = new Headers();
  const allowOrigin = corsResolveOrigin(policy, requestOrigin);
  if (allowOrigin !== null) {
    headers.set("access-control-allow-origin", allowOrigin);
    if (allowOrigin !== "*") headers.append("vary", "Origin");
    if (policy.credentials) headers.set("access-control-allow-credentials", "true");
    headers.set("access-control-allow-methods", policy.allowMethods.join(", "));
    headers.set("access-control-allow-headers", policy.allowHeaders.join(", "));
    if (policy.maxAgeSecs !== null) {
      headers.set("access-control-max-age", String(policy.maxAgeSecs));
    }
  }
  return new Response(null, { status: 204, headers });
}
