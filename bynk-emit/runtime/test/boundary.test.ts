import { test } from "node:test";
import assert from "node:assert/strict";
import { callService, deserialiseEventEnvelope, type BoundaryError, type ServiceBinding } from "../src/boundary.ts";
import { Ok, Err, type Result } from "../src/result.ts";

function bindingReturning(body: unknown, init?: ResponseInit): ServiceBinding {
  return { fetch: async () => new Response(JSON.stringify(body), init) };
}

// A deserialiser that expects { ok: T } | { err: E }.
const deser =
  <T, E>() =>
  (json: unknown): Result<Result<T, E>, BoundaryError> => {
    const j = json as { ok?: T; err?: E };
    if (j && "ok" in j) return Ok(Ok(j.ok as T));
    if (j && "err" in j) return Ok(Err(j.err as E));
    return Err({ kind: "MalformedJson", details: "shape" });
  };

test("callService: unwraps a successful inner Ok", async () => {
  const r = await callService(bindingReturning({ ok: 7 }), "svc", null, deser<number, string>());
  assert.deepEqual(r, { tag: "Ok", value: 7 });
});

test("callService: returns the inner Err to the caller", async () => {
  const r = await callService(bindingReturning({ err: "nope" }), "svc", null, deser<number, string>());
  assert.deepEqual(r, { tag: "Err", error: "nope" });
});

test("callService: non-2xx response throws a Transport BoundaryError", async () => {
  const binding = bindingReturning({}, { status: 503 });
  await assert.rejects(
    () => callService(binding, "svc", null, deser<number, string>()),
    (e: Error) => (e as any).boundaryError.kind === "Transport" && (e as any).boundaryError.status === 503,
  );
});

test("callService: invalid JSON body throws MalformedJson", async () => {
  const binding: ServiceBinding = { fetch: async () => new Response("not json", { status: 200 }) };
  await assert.rejects(
    () => callService(binding, "svc", null, deser<number, string>()),
    (e: Error) => (e as any).boundaryError.kind === "MalformedJson",
  );
});

test("callService: a deserialiser BoundaryError is thrown", async () => {
  const binding = bindingReturning({ unexpected: true });
  await assert.rejects(
    () => callService(binding, "svc", null, deser<number, string>()),
    (e: Error) => (e as any).boundaryError.kind === "MalformedJson",
  );
});

test("callService: stamps the caller context header", async () => {
  let seen: string | null = null;
  const binding: ServiceBinding = {
    fetch: async (req) => {
      seen = req.headers.get("X-Bynk-Caller");
      return new Response(JSON.stringify({ ok: 1 }));
    },
  };
  await callService(binding, "svc", null, deser<number, string>(), "ctx.Caller");
  assert.equal(seen, "ctx.Caller");
});

// v0.177 (#643): the contract seam.

test("callService: a 409 ContractMismatch surfaces as the named error", async () => {
  const binding = bindingReturning(
    {
      kind: "ContractMismatch",
      service: "whoami",
      expected: "317bdd3de84d2176",
      actual: "0000000000000000",
    },
    { status: 409 },
  );
  await assert.rejects(
    () => callService(binding, "whoami", null, deser<string, string>(), "app.a", "0000000000000000"),
    (e: Error) => {
      const be = (e as { boundaryError?: { kind: string; service?: string } }).boundaryError;
      assert.equal(be?.kind, "ContractMismatch");
      assert.equal(be?.service, "whoami");
      return true;
    },
  );
});

// The body stream is consumed on first read, so reading it twice throws
// `TypeError: Body is unusable`. A 409 that is *not* ours must still produce a
// `Transport` error naming the status — replacing that with an opaque TypeError
// from inside the runtime would bury exactly the diagnosis this seam exists to
// give.
test("callService: a 409 that is not a ContractMismatch stays a Transport error", async () => {
  const binding = bindingReturning({ kind: "SomethingElse" }, { status: 409 });
  await assert.rejects(
    () => callService(binding, "whoami", null, deser<string, string>(), "app.a", "h"),
    (e: Error) => {
      const be = (e as { boundaryError?: { kind: string; status?: number } }).boundaryError;
      assert.equal(be?.kind, "Transport", `got ${e.message}`);
      assert.equal(be?.status, 409);
      return true;
    },
  );
});

test("callService: a 409 with an unparseable body stays a Transport error", async () => {
  const binding: ServiceBinding = {
    fetch: async () => new Response("<html>gateway</html>", { status: 409 }),
  };
  await assert.rejects(
    () => callService(binding, "whoami", null, deser<string, string>(), "app.a", "h"),
    (e: Error) => {
      const be = (e as { boundaryError?: { kind: string; details?: string } }).boundaryError;
      assert.equal(be?.kind, "Transport", `got ${e.message}`);
      assert.match(be?.details ?? "", /gateway/);
      return true;
    },
  );
});

// #973: the Events boundary had no runtime validation at all — a malformed
// payload/envelope reached the subscriber's handler via a bare `as any` cast.
// `deserialiseEventEnvelope` is the envelope half of the fix (the payload
// half is an ordinary generated `deserialise_<Type>`, covered by
// `events_workers_wiring.rs`, not here).
const validEnvelope = {
  eventId: "e1",
  publisherId: "commerce.order",
  emittedAt: 1700000000000,
  schemaVersion: 1,
};

test("deserialiseEventEnvelope: accepts a well-formed envelope", () => {
  const r = deserialiseEventEnvelope(validEnvelope);
  assert.deepEqual(r, { tag: "Ok", value: validEnvelope });
});

test("deserialiseEventEnvelope: rejects a non-object", () => {
  const r = deserialiseEventEnvelope("nope");
  assert.equal(r.tag, "Err");
  assert.equal((r as { error: BoundaryError }).error.kind, "StructuralMismatch");
});

test("deserialiseEventEnvelope: rejects a missing eventId", () => {
  const { eventId: _drop, ...rest } = validEnvelope;
  const r = deserialiseEventEnvelope(rest);
  assert.equal(r.tag, "Err");
  const err = (r as { error: BoundaryError & { path: string } }).error;
  assert.equal(err.kind, "StructuralMismatch");
  assert.equal(err.path, "$.eventId");
});

test("deserialiseEventEnvelope: rejects a non-string publisherId", () => {
  const r = deserialiseEventEnvelope({ ...validEnvelope, publisherId: 7 });
  assert.equal(r.tag, "Err");
  assert.equal((r as { error: BoundaryError & { path: string } }).error.path, "$.publisherId");
});

test("deserialiseEventEnvelope: rejects a fractional emittedAt", () => {
  const r = deserialiseEventEnvelope({ ...validEnvelope, emittedAt: 1.5 });
  assert.equal(r.tag, "Err");
  const err = (r as { error: BoundaryError & { path: string; expected: string } }).error;
  assert.equal(err.path, "$.emittedAt");
  assert.equal(err.expected, "integer");
});

test("deserialiseEventEnvelope: rejects a missing schemaVersion", () => {
  const { schemaVersion: _drop, ...rest } = validEnvelope;
  const r = deserialiseEventEnvelope(rest);
  assert.equal(r.tag, "Err");
  assert.equal((r as { error: BoundaryError & { path: string } }).error.path, "$.schemaVersion");
});

test("deserialiseEventEnvelope: reports the custom path prefix on failure", () => {
  const r = deserialiseEventEnvelope("nope", "$.envelope");
  assert.equal((r as { error: BoundaryError & { path: string } }).error.path, "$.envelope");
});
