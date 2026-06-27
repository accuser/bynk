import { test } from "node:test";
import assert from "node:assert/strict";
import { HttpResult, matchPath, httpResultToResponse } from "../src/http.ts";
import type { JsonValue } from "../src/boundary.ts";

test("matchPath: captures and decodes params", () => {
  assert.deepEqual(matchPath("/orders/:id", "/orders/abc%20123"), { params: { id: "abc 123" } });
  assert.deepEqual(matchPath("/a/:x/b/:y", "/a/1/b/2"), { params: { x: "1", y: "2" } });
});

test("matchPath: returns null on literal or length mismatch", () => {
  assert.equal(matchPath("/orders/:id", "/customers/1"), null);
  assert.equal(matchPath("/orders/:id", "/orders/1/extra"), null);
});

const id = (v: number): JsonValue => v;

test("httpResultToResponse: status codes map per variant", async () => {
  // 2xx success.
  assert.equal(httpResultToResponse(HttpResult.Ok(1), id).status, 200);
  assert.equal(httpResultToResponse(HttpResult.Created(1), id).status, 201);
  assert.equal(httpResultToResponse(HttpResult.Accepted(1), id).status, 202);
  assert.equal(httpResultToResponse(HttpResult.NoContent, id).status, 204);
  // 3xx redirection.
  assert.equal(httpResultToResponse(HttpResult.MovedPermanently("/x"), id).status, 301);
  assert.equal(httpResultToResponse(HttpResult.Found("/x"), id).status, 302);
  assert.equal(httpResultToResponse(HttpResult.SeeOther("/x"), id).status, 303);
  assert.equal(httpResultToResponse(HttpResult.TemporaryRedirect("/x"), id).status, 307);
  assert.equal(httpResultToResponse(HttpResult.PermanentRedirect("/x"), id).status, 308);
  // 4xx client error.
  assert.equal(httpResultToResponse(HttpResult.BadRequest("b"), id).status, 400);
  assert.equal(httpResultToResponse(HttpResult.Unauthorized, id).status, 401);
  assert.equal(httpResultToResponse(HttpResult.Forbidden, id).status, 403);
  assert.equal(httpResultToResponse(HttpResult.NotFound, id).status, 404);
  assert.equal(httpResultToResponse(HttpResult.MethodNotAllowed, id).status, 405);
  assert.equal(httpResultToResponse(HttpResult.NotAcceptable, id).status, 406);
  assert.equal(httpResultToResponse(HttpResult.RequestTimeout, id).status, 408);
  assert.equal(httpResultToResponse(HttpResult.Conflict("c"), id).status, 409);
  assert.equal(httpResultToResponse(HttpResult.Gone, id).status, 410);
  assert.equal(httpResultToResponse(HttpResult.LengthRequired, id).status, 411);
  assert.equal(httpResultToResponse(HttpResult.PayloadTooLarge("p"), id).status, 413);
  assert.equal(httpResultToResponse(HttpResult.UnsupportedMediaType("m"), id).status, 415);
  assert.equal(httpResultToResponse(HttpResult.UnprocessableEntity("u"), id).status, 422);
  assert.equal(httpResultToResponse(HttpResult.TooManyRequests("t"), id).status, 429);
  assert.equal(httpResultToResponse(HttpResult.UnavailableForLegalReasons("l"), id).status, 451);
  // 5xx server error.
  assert.equal(httpResultToResponse(HttpResult.ServerError("s"), id).status, 500);
  assert.equal(httpResultToResponse(HttpResult.NotImplemented("n"), id).status, 501);
  assert.equal(httpResultToResponse(HttpResult.BadGateway("g"), id).status, 502);
  assert.equal(httpResultToResponse(HttpResult.ServiceUnavailable("s"), id).status, 503);
  assert.equal(httpResultToResponse(HttpResult.GatewayTimeout("g"), id).status, 504);
});

test("httpResultToResponse: redirects carry a Location header and no body", async () => {
  const res = httpResultToResponse(HttpResult.Found("https://bynk.dev/target"), id);
  assert.equal(res.status, 302);
  assert.equal(res.headers.get("location"), "https://bynk.dev/target");
  assert.equal(await res.text(), "");
});

test("httpResultToResponse: TooManyRequests carries an { error } body", async () => {
  const res = httpResultToResponse(HttpResult.TooManyRequests("slow down"), id);
  assert.equal(res.status, 429);
  assert.deepEqual(await res.json(), { error: "slow down" });
});

test("httpResultToResponse: Ok carries the serialised value; NoContent is empty", async () => {
  const ok = httpResultToResponse(HttpResult.Ok(42), id);
  assert.equal(await ok.json(), 42);
  const empty = httpResultToResponse(HttpResult.NoContent, id);
  assert.equal(await empty.text(), "");
});

test("httpResultToResponse: error variants carry an { error } body", async () => {
  const res = httpResultToResponse(HttpResult.BadRequest("bad input"), id);
  assert.deepEqual(await res.json(), { error: "bad input" });
});
