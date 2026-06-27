# Agent capability encapsulation — the deps-split and a per-agent composition root

- **Status:** Draft (settling). Direction not yet merged; no slice authorised. Promoted
  from the v0.99 proposal's **[DECISION B]/[DECISION G]** after the implementation spike
  (below) showed the deps-split pulls in a composition root — the exact condition
  [DECISION G] named as the trigger to promote it to a track.
- **Realises / supersedes:** the cross-boundary half of the *"Inference of `given`"* question
  (`bynk-type-system.md` §2.6.4), answered by **encapsulation, not propagation**: an agent
  method owns and provisions its own capabilities; a call across the agent boundary forwards
  nothing. Mirrors the cross-context service boundary (ADR **0008** local instantiation, ADR
  **0092** caller-value boundary). The originating soundness bug is reproduced below.
- **Posture:** Feature track per [ADR 0076](../decisions/0076-feature-track-posture.md). It
  qualifies on two axes: the **surface is not yet settled** (how an agent obtains its
  capabilities in a *library*-shaped bundle is an open architectural call — see §3), and it is
  a **soundness boundary** (the bug is emitted TypeScript `tsc` rejects; a wrong shape is
  unsound by construction). It is narrower than the storage/actor tracks — likely two slices —
  but the load-bearing call (the bundle composition root) is hard to reverse, which is why it
  is settled here rather than cut straight as a proposal.
- **What already landed (v0.99, slice 0):** the **discoverability** half of the v0.99 proposal
  shipped independently — the requirement-provenance ledger and the materializable ghost
  `given` inlay hint (proposal [DECISION C]/[D]/[E]), plus the **[DECISION H]** rule rejecting
  `by` on an agent `on call` handler (`bynk.actor.by_on_agent`). Those touch the checker and
  LSP, **not** the composition root, so they were severable from this track and are done. This
  track is the remaining **correctness** half.

## 1. The bug, reproduced

A storage `Cache`/`Log` op reads the clock for TTL eviction / timestamping, so the agent
handler that runs it must declare `given Clock`. In the bundle emitter that requirement leaks
across the agent boundary. Minimal repro (compiled `--target bundle`):

```bynk
context commerce
capability Clock { fn now() -> Effect[Int] }
provides Clock = SystemClock { fn now() -> Effect[Int] { 0 } }

agent Sessions {
  key id: String
  store live: Cache[String, Int] @ttl(5.minutes)
  on call put(token: String, userId: Int) -> Effect[()] given Clock {
    let _ <- live.put(token, userId)
    Effect.pure(())
  }
}

service api from http {
  on POST("/put") by v: Visitor (body: PutReq) -> Effect[HttpResult[String]] {
    let _ <- Sessions(body.token).put(body.token, body.userId)
    Ok("ok")
  }
}
type PutReq = { token: String, userId: Int }
```

Emitted TS (elided): the agent method takes the requirement, the caller forwards its own deps:

```ts
async put(token: string, userId: number, deps: { Clock: Clock }): Promise<void> { … deps.Clock.now() … }
//                                        ^^^^^^^^^^^^^^^^^^^^^ agent requires Clock
async http_POST_put(body: PutReq, deps: {}): Promise<HttpResult<string>> {
  const __r0 = await __makeSessions(body.token).put(body.token, body.userId, deps);
//                                                                            ^^^^ caller forwards {}
}
```

`tsc --strict` rejects it:

```
commerce.ts(75,80): error TS2345: Argument of type '{}' is not assignable to parameter of
  type '{ Clock: Clock; }'. Property 'Clock' is missing in type '{}' but required in type '{ Clock: Clock; }'.
```

The caller-forwarding is one localized site — `bynk-emit/src/emitter/lower.rs:1083` pushes
`"deps"` onto every agent method call (both targets).

## 2. Spike findings — correcting the proposal's two false premises

The v0.99 proposal's spike asserted the change was "feasible, bounded, not structural." The
implementation spike against `main` overturns the two claims it rested on:

1. **"The workers target already provisions per-agent providers (reference implementation)."**
   *It does not.* `lower.rs:1083` forwards the caller's `deps` for **both** targets, and the
   workers Durable-Object dispatch (`emit.rs`, the `fetch` handler) unpacks `{ args, deps }`
   straight from the request and calls `method(...args, deps)`. **Neither target
   self-provisions an agent's capabilities today** — both thread the caller's deps. There is no
   reference implementation to mirror; this track must *build* the self-provisioning model.

2. **"The bundle can give an agent its own capabilities" (implied by `compose → __makeAgent →
   new Agent(state, caps)`).** *Not cleanly.* The bundle is a **library**: capability
   implementations are host-injected through the per-call `deps` parameter, and the bundle
   runtime carries **no concrete firstparty implementations** (`Clock`/`Random`/`Fetch`/
   `Logger`/`Secrets`/`Kv` are all supplied by the host; `makeTestDeps` emits `undefined as
   unknown as Cap` for an un-mocked firstparty cap). For an agent to *own* a capability, the
   host's implementation must reach the agent at a site that is **not** the calling handler's
   narrowed deps — i.e. a **module-level composition root** the bundle does not have today
   (agents are constructed lazily inside handler bodies; there is no global init).

So the deps-split is not "stop pushing `deps`": it is "introduce the place an agent's
capabilities are composed, distinct from any caller." That is structural, and is the call to
settle below.

## 3. The open architectural call — how an agent obtains its capabilities in a library bundle

The workers target has a natural answer (the agent is a real execution unit — a Durable Object
with `env` and platform access — so it composes its own providers at construction, mirroring
the existing provider-body `this.deps` lowering). The **bundle/test** target is the open one,
because there is no execution unit and no platform: capabilities are values the host owns.

**[DECISION B1 — open] Where the bundle composes an agent's capabilities.** Candidates:

- **B1a — module-level capability holder, configured by `compose`.** Emit a module-level object
  the host populates once (via `compose`/an init the host calls before agents are used); the
  agent factory `__make<Agent>(key)` reads the agent's `given` subset from it and injects via
  constructor. Faithful to "caller forwards nothing." **Cost:** the bundle gains a *required
  init step* — a public-shape change (today the library threads deps per call with no global
  state); and it changes **agent-capability test mocking** (a test mocking an agent's `Clock`
  must configure the holder, not pass it through the caller — `makeTestDeps` semantics shift).
- **B1b — agent factory self-resolves from declared providers.** `__make<Agent>(key)` builds
  caps from the emitted `*Provider` constants for each `given` cap. **Cost:** works only for
  capabilities with a `provides`; firstparty platform caps (no provider) have no bundle
  implementation, so this is incomplete exactly for the `Cache → Clock` case that motivates the
  track. Rejected unless paired with a firstparty-impl story.
- **B1c — thread the composition-root deps (not the handler's) to the agent.** Keep passing caps
  to the agent, but source them from the full composed set in scope at the boundary, widened
  past the calling handler's `given`. **Cost:** bends "caller forwards nothing" in
  implementation (the caller stays capability-free *in source*, which may be enough); needs the
  handler to carry the full caps in a scope its `given` does not name.

*Recommendation: settle B1a*, with an ADR recording the bundle init contract and the
test-mocking migration. It is the only candidate that is both complete (covers firstparty caps)
and faithful (the caller is capability-free in source **and** in emit). B1c is the fallback if
the required-init public-shape change proves unacceptable.

## 4. The deps-split taxonomy (unchanged from the proposal — the normative split)

What is **agent-owned** (composed at the agent, never forwarded) vs what **must cross**:

1. **Agent-owned** — capabilities (`given`, read via `this.deps`) and `env` (DO-namespace
   bindings; sourced at the agent's own construction).
2. **Execution context** — `__exec` (`waitUntil`). On workers the agent is its own DO →
   agent-local, nothing crosses. On **bundle/test** there is one shared request context →
   `__exec` is the **one must-cross item**, threaded as a small `exec`/`RuntimeContext` argument
   distinct from both `this.deps` and the payload, gated on the agent body using `~>`
   (`block_uses_send`). This target-dependent asymmetry is the spot the implementation must
   encode explicitly.
3. **Service-boundary-only** — `identity`/`who` (now enforced absent by **[DECISION H]**, landed
   in v0.99 slice 0: `by` on an agent handler is `bynk.actor.by_on_agent`) and `surface`
   (agents invoke agents via the factory, not the surface). Neither crosses.

Request payload crosses as the method's explicit parameters — unchanged.

## 5. Slice decomposition

- **Slice 1 — the bundle composition root + deps-split.** Settle [DECISION B1]; emit the
  composition site; constructor-inject `this.deps`; drop the method's capability `deps` param;
  point the body at `this.deps`; stop forwarding at `lower.rs:1083`; thread `__exec` separately,
  gated on `~>`. Land behind an end-to-end `tsc_verify`-style case proving the §1 repro
  type-checks. Update `makeTestDeps`/agent-cap mocking per [DECISION B1].
- **Slice 2 — workers parity.** The agent DO composes its own providers at construction
  (`this.deps`, `env`, agent-local `__exec`); the DO dispatch stops unpacking caller `deps`.
  Behavioural test on `workerd`/the test runner.

## 6. Front-loaded ADRs

- **The agent capability boundary** — an agent owns its capabilities; a call across the agent
  boundary forwards nothing; the deps-split taxonomy (§4) as the normative split; the
  [DECISION H] `by`-on-agent rule recorded as a *rule*. (Last landed ADR is 0126; this and the
  in-browser/websocket reservations push the number — confirm at authoring time.)
- **The bundle agent-composition contract** ([DECISION B1]) — where a library bundle composes an
  agent's capabilities, the required-init public-shape change, and the agent-cap test-mocking
  migration.

## 7. Done when

- An agent method owns its capabilities; a capability-free service/cron/queue handler can call
  it and the **bundle** output type-checks (the §1 repro is green under `tsc --strict`); no
  forwarded-deps leak.
- Workers parity: the agent DO self-composes; the dispatch forwards no caller deps.
- The deps-split taxonomy holds by construction; `__exec` is the only must-cross item on bundle
  and crosses only under `~>`.
- ADRs written; spec/book capability chapter gains the boundary rule (contrasted with the
  cross-context boundary it mirrors). **On retire:** remove this doc.
