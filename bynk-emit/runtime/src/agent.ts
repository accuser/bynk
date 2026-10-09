import type { DurableObjectState } from "./storage.ts";
import { makeTestState } from "./storage.ts";
import type { BoundaryError, JsonValue } from "./boundary.ts";
import { boundaryError } from "./boundary.ts";
import type { Result } from "./result.ts";
import { Ok } from "./result.ts";

// v0.9.2: agent instantiation + per-key state lifecycle.
//
// An agent keyed by some value lowers to a *lookup-or-create* of the durable
// state for that key. In bundle mode the per-agent `StateRegistry` holds an
// in-memory `DurableObjectState` per serialised key (same key → same state
// within a session; reset per test). In workers mode the agent is a Durable
// Object: `makeWorkersAgent` returns a typed proxy over the DO stub whose
// method calls route through `callDurableObjectMethod`. `makeAgent` picks the
// path: a present binding means workers, an absent one means bundle.

// A minimal structural view of the Cloudflare Durable Object namespace/stub
// surface. The real runtime is richer but structurally compatible.
export interface DurableObjectStub {
  fetch(input: string, init?: unknown): Promise<Response>;
}

export interface DurableObjectNamespace {
  idFromName(name: string): unknown;
  get(id: unknown): DurableObjectStub;
}

// Serialise an agent key to a stable string. Two semantically-equal keys must
// serialise identically: a string is itself, a primitive is its JSON form, a
// record is canonical JSON with sorted fields.
export function serialiseAgentKey(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean" || value === null) {
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return JSON.stringify(value.map((v) => serialiseAgentKey(v)));
  }
  if (typeof value === "object") {
    const obj = value as { [k: string]: unknown };
    const keys = Object.keys(obj).sort();
    return JSON.stringify(keys.map((k) => [k, serialiseAgentKey(obj[k])]));
  }
  return JSON.stringify(value);
}

// Per-agent registry: serialised key → in-memory durable state. Used in bundle
// mode (and in `bynkc test`). `reset()` clears every state so a fresh test
// sees a clean slate.
export class StateRegistry<K> {
  private states = new Map<string, DurableObjectState>();

  getOrCreate(key: K): DurableObjectState {
    const sk = serialiseAgentKey(key);
    let state = this.states.get(sk);
    if (state === undefined) {
      state = makeTestState(sk);
      this.states.set(sk, state);
    }
    return state;
  }

  reset(): void {
    this.states.clear();
  }
}

// #1678 (runtime-semantics track S11): on the workers target an agent call
// crosses a Durable Object `fetch`, so its arguments and result are on a wire.
// They go through the boundary codec, the same `serialise_*`/`deserialise_*`
// helpers a cross-context call uses, so a value whose in-memory shape is not
// JSON-faithful (`Bytes`, a value `Map`, a non-finite `Float`) survives the
// trip. The emitter generates one `AgentWire` table per agent: for each handler,
// a codec per parameter and one for the result. Both ends read the same table:
// the caller's proxy encodes arguments and decodes the result, and the DO's
// `fetch` decodes arguments and encodes the result.

// One wire position's codec. `enc` takes the position's in-memory value; it is
// typed `never` so any concrete serialiser (`(value: Blob) => JsonValue`) fits.
export interface WireCodec {
  readonly enc: (value: never) => JsonValue;
  readonly dec: (json: JsonValue, path?: string) => Result<unknown, BoundaryError>;
}

export interface AgentWireMethod {
  readonly args: readonly WireCodec[];
  readonly result: WireCodec;
}

export type AgentWire = Readonly<Record<string, AgentWireMethod>>;

// A position whose type has no wire form (a held `Connection`, a function, a
// `Stream`, a `Query`) passes through as it did before #1678.
export const AGENT_WIRE_PASS: WireCodec = {
  enc: (value: never) => value as JsonValue,
  dec: (json: JsonValue) => Ok(json),
};

function agentWireMethod(wire: AgentWire, method: string): AgentWireMethod {
  // `hasOwn`: a method name comes off the request path.
  if (!Object.hasOwn(wire, method)) {
    throw new Error(`agent method \`${method}\` has no wire codec`);
  }
  return wire[method];
}

// The DO side: decode a call's arguments. A decode failure is an internal
// fault (the caller is the program itself, not an untrusted client), so it
// throws, and the DO's `fetch` fails with a 500 the caller rethrows.
export function decodeAgentArgs(wire: AgentWire, method: string, args: unknown[]): unknown[] {
  const m = agentWireMethod(wire, method);
  return args.map((a, i) => {
    const r = (m.args[i] ?? AGENT_WIRE_PASS).dec(a as JsonValue, `$.args[${i}]`);
    if (r.tag === "Err") throw boundaryError(r.error);
    return r.value;
  });
}

// #1818: the DO side of an agent that reads its own key (`self.<key>`). A
// Durable Object knows only its id, not the key it was addressed by, so the
// caller's proxy sends the key with each call, encoded by the key type's codec,
// and the DO decodes it here.
export function decodeAgentKey(codec: WireCodec, json: unknown): unknown {
  const r = codec.dec(json as JsonValue, "$.key");
  if (r.tag === "Err") throw boundaryError(r.error);
  return r.value;
}

// The DO side: encode a call's result.
export function encodeAgentResult(wire: AgentWire, method: string, result: unknown): JsonValue {
  return agentWireMethod(wire, method).result.enc(result as never);
}

// Workers-mode agent method call: route through the DO stub's `fetch` under
// the `/_bynk/agent/<method>` wire protocol. `args` are already encoded, and
// the result comes back as raw JSON for the caller to decode.
export async function callDurableObjectMethod(
  stub: DurableObjectStub,
  method: string,
  args: unknown[],
  deps: unknown,
  key?: JsonValue,
): Promise<JsonValue> {
  const response = await stub.fetch(`https://_bynk/_bynk/agent/${method}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    // #1818: the key rides along only for an agent that reads it.
    body: JSON.stringify(key === undefined ? { args, deps } : { args, deps, key }),
  });
  if (!response.ok) throw new Error(await response.text());
  return (await response.json()) as JsonValue;
}

// Workers-mode agent: a typed proxy over the DO stub. Each method access
// returns a function that splits its final argument off as `deps` and routes
// the rest as method args through `callDurableObjectMethod`. The `as C` cast
// gives call sites the agent's real method signatures, so user code reads
// identically to bundle mode.
export function makeWorkersAgent<C>(
  binding: DurableObjectNamespace,
  key: unknown,
  wire?: AgentWire,
  keyCodec?: WireCodec,
): C {
  const stub = binding.get(binding.idFromName(serialiseAgentKey(key)));
  const sentKey = keyCodec === undefined ? undefined : keyCodec.enc(key as never);
  const proxy = new Proxy(
    {},
    {
      get(_target, prop: string | symbol) {
        if (typeof prop !== "string") return undefined;
        return async (...callArgs: unknown[]) => {
          const deps = callArgs.length > 0 ? callArgs[callArgs.length - 1] : {};
          const args = callArgs.length > 0 ? callArgs.slice(0, -1) : [];
          if (wire === undefined) return callDurableObjectMethod(stub, prop, args, deps, sentKey);
          const m = agentWireMethod(wire, prop);
          const encoded = args.map((a, i) => (m.args[i] ?? AGENT_WIRE_PASS).enc(a as never));
          const json = await callDurableObjectMethod(stub, prop, encoded, deps, sentKey);
          const r = m.result.dec(json);
          if (r.tag === "Err") throw boundaryError(r.error);
          return r.value;
        };
      },
    },
  );
  return proxy as C;
}

// Single agent-construction helper. A present DO binding selects the workers
// path; otherwise the bundle registry path. Call sites are identical across
// targets.
export function makeAgent<C>(
  registry: StateRegistry<unknown>,
  binding: DurableObjectNamespace | undefined,
  key: unknown,
  constructBundle: (state: DurableObjectState) => C,
  wire?: AgentWire,
  keyCodec?: WireCodec,
): C {
  if (binding !== undefined) {
    return makeWorkersAgent<C>(binding, key, wire, keyCodec);
  }
  const state = registry.getOrCreate(key);
  return constructBundle(state);
}

// Events track, slice 0 (spine #936, ADR 0284): a publishing context's
// `deps.__eventsDispatch` hands its release-at-commit event batch to this
// context's own fan-out Durable Object (one namespace per publishing
// context — `idFromName` is passed a fixed key since the DO instance itself,
// not the id, is what scopes the fan-out to this publisher and gives it
// single-threaded per-publisher ordering). A non-`ok` response is logged, not
// thrown: the publishing handler already committed by the time this runs
// (release-at-commit), so a fan-out transport failure must not surface as a
// failure of the handler that emitted.
export async function dispatchToEventsFanout(
  binding: DurableObjectNamespace,
  events: Array<{
    type: string;
    payload: unknown;
    envelope: { eventId: string; publisherId: string; emittedAt: number; schemaVersion: number };
  }>,
): Promise<void> {
  // The whole round trip is wrapped, not just the status check: a rejected
  // `stub.fetch` (a network error, the DO throwing) is exactly as much a
  // fan-out transport failure as a non-`ok` response, and by this point
  // `commitState` has already succeeded — letting a rejection propagate
  // would surface a transport problem as a failure of a handler that, from
  // the caller's perspective, already completed.
  try {
    const stub = binding.get(binding.idFromName("singleton"));
    const response = await stub.fetch("https://_bynk/_bynk/fanout", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ events }),
    });
    if (!response.ok) {
      console.error("EventsFanout dispatch failed", { status: response.status });
    }
  } catch (e) {
    console.error("EventsFanout dispatch failed", { error: String(e) });
  }
}

// v0.16: an in-process Durable-Object namespace for multi-Worker integration
// tests. `construct` builds the emitted DO class from a fresh in-memory state;
// one instance is kept per key (so state accumulates within a test case). The
// returned stub bridges the stub-side `fetch(url, init)` the agent runtime
// speaks to the DO class's server-side `fetch(request)`.
export function makeIntegrationDoNamespace(
  construct: (state: DurableObjectState) => { fetch(request: Request): Promise<Response> },
): DurableObjectNamespace {
  const instances = new Map<string, { fetch(request: Request): Promise<Response> }>();
  return {
    idFromName(name: string): unknown {
      return name;
    },
    get(id: unknown): DurableObjectStub {
      const k = String(id);
      let inst = instances.get(k);
      if (inst === undefined) {
        inst = construct(makeTestState(k));
        instances.set(k, inst);
      }
      const target = inst;
      return {
        fetch: (input: string, init?: unknown): Promise<Response> =>
          target.fetch(new Request(input, init as RequestInit)),
      };
    },
  };
}
