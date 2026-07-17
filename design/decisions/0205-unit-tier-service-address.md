# 0205 — A test `case` drives an http/cron/queue handler at the unit tier by address, with a call-site `by <Actor>(<identity>)`

- **Status:** Accepted (v0.185)
- **Provenance:** Slice A of the testing-the-boundary track (spine [#656](https://github.com/accuser/bynk/issues/656)), proposed and `accepted` as [#664](https://github.com/accuser/bynk/issues/664). Builds on Slice 0 ([[0203]]), which made the checker resolve a test-body `svc.call(...)` against its handler.
- **Relates:** [[0203]] (the `on call` resolution this extends to http/cron/queue), [[0153]] (the tier dial — this is the `unit` tier), [[0148]] (`Val[T]` and the test-only brand cast the identity reuses), [[0081]] (the sealed identity the `by` clause supplies at `unit` — see the threat note), [[0147]] (structural test-ness — the surface is stripped from the build).

## Context

Slice 0 made `svc.call(args)` resolve its `on call` handler. But a `from http` / `cron` / `queue` service has no `on call` handler, so its routes/schedules/messages were unaddressable from a test — `api.GET("/todos")` passed `bynkc check` and crashed at runtime (`TypeError: api.GET is not a function`), the twin of the #654 defect Slice 0 fixed for `.call`. The track's whole thesis (a `case` should drive the boundary the way the boundary's own claims are made) needs the http surface, and cron/queue had **never been executed by any test, including the compiler's own**.

## Decision

**A test `case` addresses a service handler by its natural surface, and names the principal it acts as with a call-site `by` clause.**

- **Address (D1).** `svc.<VERB>("/path", …)` for an http route, `svc.schedule("<expr>", …)` for a cron handler, `svc.message(msg)` for a queue handler, `svc.call(args)` for RPC. The leading string is the route pattern / schedule — a compile-time-resolved *name*, matched against a declared handler (`bynk.test.service_unknown_route` / `bynk.test.service_bad_address` when it doesn't); the remaining arguments are the handler's positional params, arity- and type-checked.

- **Principal (D2).** `let r <- <address> by <Actor>(<identity>)` — a new call-site clause on `effect_let_stmt`, distinct from the handler `by_clause` (which binds an actor and admits a sum but carries no identity value). `by User("bob")` for an identity-carrying actor; `by Visitor` (no argument) for a unit-identity actor; cron/queue's internal actors need no `by`. The actor resolves against the target's `actor` declarations or the prelude actors; a Declared identity requires a value **typed against the actor's identity type** (so `by User("")` fails `UserId`'s `NonEmpty` refinement), and a unit-identity actor rejects one.

- **Identity is a test-only brand cast (D3).** `by User("bob")` lowers to `{ ...deps, identity: ("bob" as any) }` — the same brand the agent key and `Val[T]` ([[0148]]) already use. No JWT, no verification: at `unit` the identity is *given*, which is what the tier is for. This also supplies the `deps.identity` a `by c: Caller` handler reads (#655's test-side gap).

- **No `deps.env` bridge (D4).** The proposal called the `deps.env` ↔ `makeTestState` wiring the "load-bearing delta". **It is not needed.** The unit runner emits in bundle mode, where an http handler's agent construction already resolves the in-memory `StateRegistry` — `__makeTodos(deps.identity)`, with `env` optional and unused. A unit address call is simply `api.http_GET_todos({ ...deps, identity })`; the handler exists and its agent backing is the registry. The proposal (and the exploration) were reading Workers-mode emission, which is why the bridge looked necessary.

- **The `consumes`/`symbols.rs` widening stays dropped (D5).** [[0203]] dropped it as having no consumer. Slice A confirms `unit` doesn't need it either: an address call lowers to a *direct* `handlers.<svc>.<key>(...)` invocation, never the cross-context `callService` path.

## Consequences

- Every example's `from http` / `cron` / `queue` service is testable at `unit`, and `scheduled` / `queue` handlers execute under a test for the first time.
- `HandlerKind::Http { method, path }` keys are a pure function of verb + path (`http_handler_method_name`); cron/queue keys are position-indexed among same-kind handlers, recovered by walking the declared handlers.
- The grammar gains `call_site_actor` (Rust parser, tree-sitter, formatter); five diagnostics (`service_bad_address`, `service_unknown_route`, `unknown_actor`, `actor_identity_required`, `actor_no_identity`).
- No new runtime, no signer, no `fetch` — `unit` calls the handler in-process directly. The `system` tier (a real `fetch` with a verified credential) is a later slice.

## What this does NOT do — the #655 finding

**#655 is not closed by this slice, and its root differs from the proposal's diagnosis.** The proposal expected the call-site `by` to close #655 (a `by c: Caller` handler emitting `deps` without `identity` → tsc TS2345). The test side is now fixed — a case supplies the identity. But the tsc failure is in the emitted **`makeSurface`** (the cross-context *deploy* surface, `emit_make_surface`): for a `by c: Caller` on-call handler it emits `svc.call(args, deps)` where the context's `<Ctx>Deps` carries no identity. That is a distinct emitter concern from the test surface — the cross-context caller-identity threading — and it affects only `on call` services with a Caller binder (http/cron/queue skip `makeSurface`, having no call handler). #655 stays open with this precise diagnosis; its fix belongs to the cross-context surface, not this slice.

## Re-openable

- Full `unit` auto-stubbing of collaborators (the named follow-on from [[0153]] D8) is unchanged by this slice.
- Whether a `system`-tier address should reuse this exact surface (a real `fetch`, a signed credential) is the next slice's decision.
