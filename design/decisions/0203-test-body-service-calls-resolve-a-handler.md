# 0203 — A test-body `svc.call(args)` resolves the service's `on call` handler, checked for existence, arity, and argument types

- **Status:** Accepted (v0.180)
- **Provenance:** Slice 0 of the testing-the-boundary track (spine [#656](https://github.com/accuser/bynk/issues/656)), proposed and `accepted` as [#662](https://github.com/accuser/bynk/issues/662). It closes the defect [#654](https://github.com/accuser/bynk/issues/654): `<service>.call(…)` in a test body was accepted for *any* service in scope — including a `from http`/`cron`/`queue` service with no `on call` handler — and crashed at runtime.
- **Relates:** [[0147]] (structural test-ness — a `suite` is checked wherever it lives, via `compile_project`, which is why this fires in `bynkc check`), [[0146]] (`suite`/`case`), and #504 (handler bodies rejecting unknown names / wrong arity — the same bar this brings the test-body service branch up to).

## Context

Since v0.25 a test `case` may invoke the target unit's service as `svc.call(args)`. The checker's branch for this matched the *method name* literally — `method.name == "call"` and the receiver being a service in scope — and then typed the arguments loosely (the runner recovers `Result`/`Effect` outcomes at runtime) and returned. It never resolved an `on call` handler on the named service.

The consequence (#654): a `from http` service such as `examples/todo`'s `api` *is* in the target's service set, so `api.call(1, 2, 3)` satisfied the branch. `bynkc check` exited 0; the emitter lowered `api.call(1, 2, 3, deps)` verbatim; and at runtime the emitted http-service object — which only carries `http_POST_todos`-style keys — had no `call` member, so `bynkc test` died with `TypeError: api.call is not a function`. No existence check, no arity check, no argument-type check.

This is the same bug class #504 fixed for handler bodies: the checker accepting a call it cannot honour, deferring the failure to emitted TypeScript or the runtime.

## Decision

**A test-body `svc.call(args)` resolves the service's `on call` handler and is checked against it.** The branch now carries, per target service, the service's protocol and its `on call` handler signature (params + span). Given `svc.call(args)`:

- **No `on call` handler** (a `from http`/`cron`/`queue` service) → `bynk.test.service_no_call_handler`, naming the service and its protocol: *"`api` is a `from http` service and has no `on call` handler to invoke."* The service exists — degrading to "unknown identifier" would be a worse message — so the diagnostic points at the service, and its note names the one shape addressable today.
- **Wrong arity** → `bynk.test.service_call_arity`, labelled at the handler declaration.
- **Mismatched argument type** → the existing `bynk.types.argument_mismatch`, reused rather than reinvented.

The outcome *type* stays loose — the runner still recovers the `Result`/`Effect` shape at runtime, unchanged — and the binding edge is still recorded so test-file references index. What changes is that the call must name a handler that exists and be shaped to fit it.

### What this deliberately does not do

- **It does not make a non-`call` service addressable.** `api.GET("/todos")` remains unresolved — the http/cron/queue *address* surface is a later slice of the track. This slice only stops `.call` from lying.
- **It does not widen the cross-context path.** The consumed-context service resolution (`symbols.rs`) stays `on call`-only, which is correct — cross-context calls are call-only by construction. (A later slice's analysis confirmed the "widen `symbols.rs`" idea from the track's candidate decomposition has no consumer; it is not done here or later.)

## Consequences

- `bynkc check` catches `#654`'s reproduction — a green check once more means the emitted TypeScript runs. A correct `svc.call(args)` checks, lowers, and runs exactly as before.
- The checker's `test_services` grows from a set of names to a map carrying each service's `on call` signature, built by the project test pass from the target's declarations (the same information the cross-context path already models as `CrossContextService`).
- Two new diagnostics, `bynk.test.service_call_arity` and `bynk.test.service_no_call_handler`; argument-type mismatches reuse `bynk.types.argument_mismatch`.
- **Re-openable:** whether a resolved `svc.call` should return the handler's *typed* result rather than staying loose — deferred, since the runner's outcome recovery is unaffected and typing the result is a separate, larger change.
