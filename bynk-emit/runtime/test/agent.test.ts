import { test } from "node:test";
import assert from "node:assert/strict";
import {
  serialiseAgentKey,
  StateRegistry,
  makeAgent,
  makeIntegrationDoNamespace,
  makeWorkersAgent,
  decodeAgentArgs,
  encodeAgentResult,
  AGENT_WIRE_PASS,
  type AgentWire,
  type DurableObjectNamespace,
} from "../src/agent.ts";
import type { BoundaryError, JsonValue } from "../src/boundary.ts";
import { Ok, Err } from "../src/result.ts";
import type { DurableObjectState } from "../src/storage.ts";

test("serialiseAgentKey: strings are identity, primitives are JSON", () => {
  assert.equal(serialiseAgentKey("abc"), "abc");
  assert.equal(serialiseAgentKey(42), "42");
  assert.equal(serialiseAgentKey(true), "true");
  assert.equal(serialiseAgentKey(null), "null");
});

test("serialiseAgentKey: records are canonical regardless of field order", () => {
  assert.equal(serialiseAgentKey({ a: 1, b: 2 }), serialiseAgentKey({ b: 2, a: 1 }));
  assert.notEqual(serialiseAgentKey({ a: 1 }), serialiseAgentKey({ a: 2 }));
});

test("serialiseAgentKey: arrays recurse positionally", () => {
  assert.equal(serialiseAgentKey([1, "x", { k: 1 }]), serialiseAgentKey([1, "x", { k: 1 }]));
  assert.notEqual(serialiseAgentKey([1, 2]), serialiseAgentKey([2, 1]));
});

test("StateRegistry: same key yields same state; reset clears", () => {
  const reg = new StateRegistry<{ id: number }>();
  const a = reg.getOrCreate({ id: 1 });
  const aAgain = reg.getOrCreate({ id: 1 });
  const b = reg.getOrCreate({ id: 2 });
  assert.equal(a, aAgain);
  assert.notEqual(a, b);
  reg.reset();
  assert.notEqual(reg.getOrCreate({ id: 1 }), a);
});

test("makeAgent: absent binding takes the bundle path", () => {
  const reg = new StateRegistry<string>();
  let built: DurableObjectState | null = null;
  const agent = makeAgent(reg, undefined, "k", (state) => {
    built = state;
    return { kind: "bundle" as const };
  });
  assert.equal(agent.kind, "bundle");
  assert.ok(built);
});

test("makeAgent: present binding takes the workers proxy path", async () => {
  // A namespace whose stub echoes the method name back as JSON.
  const ns: DurableObjectNamespace = {
    idFromName: (n) => n,
    get: () => ({
      fetch: async (input: string) =>
        new Response(JSON.stringify({ called: input }), {
          headers: { "content-type": "application/json" },
        }),
    }),
  };
  interface Counter {
    bump(n: number, deps: unknown): Promise<{ called: string }>;
  }
  const agent = makeAgent<Counter>(reg(), ns, "k", () => {
    throw new Error("should not construct bundle when binding present");
  });
  const res = await agent.bump(1, {});
  assert.match(res.called, /_bynk\/agent\/bump$/);
});

function reg(): StateRegistry<unknown> {
  return new StateRegistry<unknown>();
}

test("integration namespace + workers proxy: round-trips method calls and splits deps", async () => {
  // A tiny emitted-style DO: handles POST /_bynk/agent/<method> with {args, deps}.
  const ns = makeIntegrationDoNamespace((state: DurableObjectState) => ({
    async fetch(request: Request): Promise<Response> {
      const url = new URL(request.url);
      const method = url.pathname.split("/").pop();
      const { args, deps } = (await request.json()) as { args: unknown[]; deps: unknown };
      if (method === "add") {
        const prev = (await state.storage.get<number>("total")) ?? 0;
        const next = prev + (args[0] as number);
        await state.storage.put("total", next);
        return Response.json({ total: next, deps });
      }
      return new Response("no such method", { status: 404 });
    },
  }));

  interface Acc {
    add(n: number, deps: unknown): Promise<{ total: number; deps: unknown }>;
  }
  const agent = makeWorkersAgent<Acc>(ns, "acc-1");
  const r1 = await agent.add(3, { trace: "t1" });
  assert.deepEqual(r1, { total: 3, deps: { trace: "t1" } });
  // state accumulates per key within the namespace
  const r2 = await agent.add(4, { trace: "t2" });
  assert.equal(r2.total, 7);
  // a different key is isolated
  const other = makeWorkersAgent<Acc>(ns, "acc-2");
  assert.equal((await other.add(1, {})).total, 1);
});

// #1678: a wire table whose single method takes and returns bytes, encoded as a
// hex string so the test can see that both ends ran.
const hexWire: AgentWire = {
  echo: {
    args: [
      {
        enc: ((v: Uint8Array) => Buffer.from(v).toString("hex")) as (v: never) => JsonValue,
        dec: (j: JsonValue) =>
          typeof j === "string"
            ? Ok(new Uint8Array(Buffer.from(j, "hex")))
            : Err({ kind: "StructuralMismatch", path: "$", expected: "hex", actual: typeof j } as BoundaryError),
      },
    ],
    result: {
      enc: ((v: Uint8Array) => Buffer.from(v).toString("hex")) as (v: never) => JsonValue,
      dec: (j: JsonValue) =>
        typeof j === "string"
          ? Ok(new Uint8Array(Buffer.from(j, "hex")))
          : Err({ kind: "StructuralMismatch", path: "$", expected: "hex", actual: typeof j } as BoundaryError),
    },
  },
};

// An emitted-style DO that decodes and encodes through the same table.
function hexEchoNamespace(): DurableObjectNamespace {
  return makeIntegrationDoNamespace(() => ({
    async fetch(request: Request): Promise<Response> {
      const method = new URL(request.url).pathname.slice("/_bynk/agent/".length);
      const { args } = (await request.json()) as { args: unknown[]; deps: unknown };
      const decoded = decodeAgentArgs(hexWire, method, args);
      return Response.json(encodeAgentResult(hexWire, method, decoded[0]));
    },
  }));
}

test("#1678 workers proxy: arguments and result go through the wire table", async () => {
  interface Echo {
    echo(b: Uint8Array, deps: unknown): Promise<Uint8Array>;
  }
  const agent = makeWorkersAgent<Echo>(hexEchoNamespace(), "k", hexWire);
  const out = await agent.echo(new Uint8Array([1, 2, 255]), {});
  assert.ok(out instanceof Uint8Array, "the result is decoded back to bytes");
  assert.deepEqual([...out], [1, 2, 255]);
});

test("#1678 workers proxy: a result that fails to decode throws a boundary error", async () => {
  const ns: DurableObjectNamespace = {
    idFromName: (n) => n,
    get: () => ({ fetch: async () => Response.json(42) }),
  };
  interface Echo {
    echo(b: Uint8Array, deps: unknown): Promise<Uint8Array>;
  }
  const agent = makeWorkersAgent<Echo>(ns, "k", hexWire);
  await assert.rejects(agent.echo(new Uint8Array([1]), {}), /BoundaryError: StructuralMismatch/);
});

test("#1678 a method absent from the wire table throws, including an inherited name", async () => {
  assert.throws(() => decodeAgentArgs(hexWire, "missing", []), /has no wire codec/);
  assert.throws(() => decodeAgentArgs(hexWire, "constructor", []), /has no wire codec/);
  assert.throws(() => encodeAgentResult(hexWire, "toString", 1), /has no wire codec/);
  interface Odd {
    missing(deps: unknown): Promise<unknown>;
  }
  const agent = makeWorkersAgent<Odd>(hexEchoNamespace(), "k", hexWire);
  await assert.rejects(agent.missing({}), /has no wire codec/);
});

test("#1678 the DO side rejects an argument that fails to decode", () => {
  assert.throws(() => decodeAgentArgs(hexWire, "echo", [7]), /BoundaryError: StructuralMismatch/);
});

test("#1678 a pass-through position is carried as is", () => {
  const wire: AgentWire = { m: { args: [AGENT_WIRE_PASS], result: AGENT_WIRE_PASS } };
  assert.deepEqual(decodeAgentArgs(wire, "m", [{ id: "c1" }]), [{ id: "c1" }]);
  assert.deepEqual(encodeAgentResult(wire, "m", { id: "c1" }), { id: "c1" });
});
