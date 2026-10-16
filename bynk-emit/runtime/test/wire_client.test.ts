// I4 (#1970): @bynk/wire-client, the runtime half of a generated TypeScript
// client, against the callee's own verifier. The package is bundled from this
// runtime's modules, so these tests pin the property that makes that worth
// doing: a foreign caller's request, built by the package, is the request the
// callee's `verifyCall` admits as that foreign caller.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  __callService,
  callErrorOf,
  Ok,
  Err,
  type Result,
  type WireVersion,
  type __BoundaryError,
  type __JsonValue,
  type __ServiceBinding,
} from "../../wire-client/index.ts";
import { verifyCall, WIRE_VERSION } from "../src/boundary.ts";

// The callee, `shop.orders`, admits the foreign caller `Legacy` under the
// secret it holds as `LEGACY_CALL_KEY`; `Legacy`'s Worker holds the same value
// as `BYNK_CALL_TO_SHOP__ORDERS`, the name `bynk deploy`'s hand-off sets.
const FOREIGN = { Legacy: "LEGACY_CALL_KEY" };
const CALLEE_ENV = { LEGACY_CALL_KEY: "k3y" };
const CALLER_ENV = { BYNK_CALL_TO_SHOP__ORDERS: "k3y" };

const decodeString = (json: __JsonValue): Result<string, __BoundaryError> =>
  typeof json === "string" ? Ok(json) : Err({ kind: "StructuralMismatch", path: "$", expected: "string", actual: typeof json });

// A binding to a callee that verifies like the emitted dispatch's first step,
// then answers with the verified caller and body.
function callee(env: unknown): __ServiceBinding {
  return {
    fetch: async (request: Request) => {
      const verified = await verifyCall(request, env, "shop.orders", FOREIGN);
      if (typeof verified !== "string") return verified;
      return new Response(JSON.stringify(`${request.headers.get("X-Bynk-Caller")}:${verified}`));
    },
  };
}

test("wire-client: a foreign caller's call verifies at the callee as that caller", async () => {
  const r = await __callService(callee(CALLEE_ENV), "status", "o-1", decodeString, "Legacy", "226b9939f85e69ea", CALLER_ENV, "shop.orders");
  assert.equal(r, 'Legacy:"o-1"');
});

test("wire-client: a caller without the secret cannot send", async () => {
  await assert.rejects(
    () => __callService(callee(CALLEE_ENV), "status", "o-1", decodeString, "Legacy", "", {}, "shop.orders"),
    /BYNK_CALL_TO_SHOP__ORDERS is not set/,
  );
});

test("wire-client: a wrong secret is refused with 401, surfaced as Transport", async () => {
  const wrong = { BYNK_CALL_TO_SHOP__ORDERS: "not-it" };
  const e = await __callService(callee(CALLEE_ENV), "status", "o-1", decodeString, "Legacy", "", wrong, "shop.orders").then(
    () => assert.fail("expected a refusal"),
    (e: unknown) => e,
  );
  assert.deepEqual(callErrorOf(e), { kind: "Transport", status: 401, details: "Unauthorized" });
});

test("wire-client: a 409 surfaces as ContractMismatch", async () => {
  const skewed: __ServiceBinding = {
    fetch: async () =>
      new Response(JSON.stringify({ kind: "ContractMismatch", service: "status", expected: "aaaa", actual: "bbbb" }), { status: 409 }),
  };
  const logged = console.error;
  console.error = () => {};
  try {
    const e = await __callService(skewed, "status", "o-1", decodeString, "Legacy", "bbbb", CALLER_ENV, "shop.orders").then(
      () => assert.fail("expected a refusal"),
      (e: unknown) => e,
    );
    assert.equal(callErrorOf(e)?.kind, "ContractMismatch");
  } finally {
    console.error = logged;
  }
});

test("wire-client: callErrorOf ignores anything that is not a call error", () => {
  assert.equal(callErrorOf(new Error("plain")), undefined);
  assert.equal(callErrorOf(undefined), undefined);
  assert.equal(callErrorOf("x"), undefined);
});

test("wire-client: speaks the runtime's wire version", () => {
  const v: WireVersion = "1";
  assert.equal(v, WIRE_VERSION);
});
