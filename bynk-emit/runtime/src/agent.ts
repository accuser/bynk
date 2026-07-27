import type { DurableObjectState } from "./storage.ts";
import { makeTestState } from "./storage.ts";

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

// Workers-mode agent method call: route through the DO stub's `fetch` under
// the `/_bynk/agent/<method>` wire protocol.
export async function callDurableObjectMethod<R>(
  stub: DurableObjectStub,
  method: string,
  args: unknown[],
  deps: unknown,
): Promise<R> {
  const response = await stub.fetch(`https://_bynk/_bynk/agent/${method}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ args, deps }),
  });
  if (!response.ok) throw new Error(await response.text());
  return (await response.json()) as R;
}

// Workers-mode agent: a typed proxy over the DO stub. Each method access
// returns a function that splits its final argument off as `deps` and routes
// the rest as method args through `callDurableObjectMethod`. The `as C` cast
// gives call sites the agent's real method signatures, so user code reads
// identically to bundle mode.
export function makeWorkersAgent<C>(binding: DurableObjectNamespace, key: unknown): C {
  const stub = binding.get(binding.idFromName(serialiseAgentKey(key)));
  const proxy = new Proxy(
    {},
    {
      get(_target, prop: string | symbol) {
        if (typeof prop !== "string") return undefined;
        return (...callArgs: unknown[]) => {
          const deps = callArgs.length > 0 ? callArgs[callArgs.length - 1] : {};
          const args = callArgs.length > 0 ? callArgs.slice(0, -1) : [];
          return callDurableObjectMethod(stub, prop, args, deps);
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
): C {
  if (binding !== undefined) {
    return makeWorkersAgent<C>(binding, key);
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
  events: Array<{ type: string; payload: unknown }>,
): Promise<void> {
  const stub = binding.get(binding.idFromName("singleton"));
  const response = await stub.fetch("https://_bynk/_bynk/fanout", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ events }),
  });
  if (!response.ok) {
    console.error("EventsFanout dispatch failed", { status: response.status });
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
