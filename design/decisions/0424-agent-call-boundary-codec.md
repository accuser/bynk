# 0424 — A workers agent call encodes its arguments and result with the boundary codec

- **Status:** Accepted (v0.294)

**Context.** On the `workers` target an agent is a Durable Object, and a call
`Agent(key).m(args)` goes through `makeWorkersAgent`'s proxy, which POSTs
`{ args, deps }` to `/_bynk/agent/<m>` on the stub. The generated agent class's
`fetch` read the body with `request.json()`, called the handler, and returned
`JSON.stringify(result ?? null)`. The caller read it with `response.json() as R`.
No codec ran on either side, so any value whose in-memory shape is not
JSON-faithful was corrupted. Reproduced on workerd 1.20260930: `Bytes`, a value
`Map`, a record holding `Bytes`, `List[Bytes]` and a generic `Box[Bytes]` all came
back unequal, and a handler computing `n + b.length()` returned `null` (`NaN`).
Enums and `Option`s survived only because their in-memory form happens to be
JSON-shaped. Every other boundary (HTTP, cross-context calls, agent state since
#1649) already used the codec. The call site cannot encode by type: it is the
same code on both targets (`makeAgent` picks the path at runtime), and on
`bundle` the call is an in-process method call on in-memory values.

**Decision.**
1. On `workers`, each agent emits `const __<Agent>Wire: AgentWire`. It maps each
   handler's method name to `{ args: [WireCodec…], result: WireCodec }`, where a
   `WireCodec` is `{ enc, dec }`, the boundary codec's `serialise_ref_via` and
   `deserialise_ref_via` for the position's type (with the handler's `Effect`
   peeled).
2. `__make<Agent>` passes the table to `makeAgent`, which passes it to
   `makeWorkersAgent`. The proxy encodes each argument and decodes the result;
   `callDurableObjectMethod` now returns raw JSON. The class's `fetch` decodes
   the arguments (`decodeAgentArgs`) and encodes the result
   (`encodeAgentResult`). Both ends read one table, so they cannot disagree.
3. The codec closure includes every agent handler's parameter and return types
   on `workers` (`agent_call_boundary_roots`, a new `extra_roots` argument to
   `collect_boundary_types` and `collect_generic_instantiations`), so their
   helpers are emitted in the context module, which is where both the class and
   every caller live.
4. A decode failure is an **internal fault**, not a `400`: the caller is the
   program itself. On the DO side `decodeAgentArgs` throws a boundary error, and
   `fetch` fails with a 500 that the caller rethrows. On the caller side the
   proxy throws `boundaryError`, as `callService` does. A method absent from
   the table also throws; the lookup is own-property, because the name comes off
   the request path.
5. A position whose type has no wire form passes through unencoded
   (`AGENT_WIRE_PASS`), preserving today's behaviour. That means a held
   `Connection`, a function, a `Stream`, a `Query` or a `History`, anywhere in
   the type. Every other type has a codec, including a generic application.
6. `bundle` is unchanged: no table, and `makeAgent`'s `wire` parameter is
   optional.

The `deps` half of the body is unchanged. It is still JSON, and the DO rebuilds
capability providers and the events dispatcher in-process (#527).

**Consequences.** An agent behaves the same on both targets for every
codec-able value, closing the gap #1678 named.
- **Cost:** each call gains one serialiser walk per argument and result, the
  same order of work as the `JSON.stringify` it already did.
- **Goldens:** every workers golden with an agent gains its table and the
  `fetch` codec calls; nothing else moves.
- **Pass-through positions** keep their pre-#1678 behaviour. Whether a held
  `Connection` should ever cross an agent call on `workers` is a separate
  question; this decision does not change it.

Proved by:
- `bynkc/tests/workers_runtime_smoke.rs::agent_calls_use_the_boundary_codec_on_workerd`
  on real workerd. It round-trips `Bytes`, an enum, an `Option`, a value `Map`,
  a record holding `Bytes`, `List[Bytes]`, a generic `Box[Bytes]` and a
  two-argument handler. It failed on every non-JSON-faithful shape before.
- `bynk-emit/runtime/test/agent.test.ts`: proxy encoding and decoding, a
  decode failure on each side, an absent or inherited method name, and
  pass-through.
