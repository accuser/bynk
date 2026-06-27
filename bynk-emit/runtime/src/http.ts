import type { JsonValue } from "./boundary.ts";

// v0.9: HttpResult — the built-in HTTP-result sum.

export type HttpResult<T> =
  // 2xx success — carries the serialised value.
  | { readonly tag: "Ok"; readonly value: T }
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
