---
level: minor
probes: ts_writes
changelog: In a test case, a `svc.call(…)` outcome takes the handler's declared type, so `expect r == Ok("a")` checks; a `by c: Caller` handler called without `by Caller("…")` is `bynk.test.actor_identity_required`; and a handler that sends with `~>` runs at every tier, its sends awaited before the case's expectations
---

## ADR: test-call-outcome-takes-the-handlers-type
title: A test-body `svc.call(…)` outcome takes the handler's declared type
summary: `.call` outcomes are typed; http/cron/queue outcomes stay loose and reject `==` against a bare constructor

**Context.** ADR 0203 resolved a test-body `svc.call(args)` against the
service's `on call` handler but left the outcome loose, deferring whether it
should take the handler's typed result. The deferral leaked: in a case,
`expect r == Ok("a")` gave the `Ok` no operand type to take, so it fell back to
the case body's own implicit `Result[(), ValidationError]` and reported
`bynk.types.ok_value_mismatch` against a type the author never wrote (#1979).
The same comparison checks outside tests, where `r` has a type.

**Decision.** A `svc.call(…)` outcome takes the `on call` handler's declared
return type (`Effect[T]`, binding `T`; a bare `T` is lifted). An `Ok`/`Err`
operand of `==` is then typed from the other operand, as anywhere else, and
`==` lowers by that type. An http, cron, or queue address stays loose, as ADR
0203 and #1931 left it: its outcome's shape differs by tier and by `Wire`, so a
single static type would be wrong somewhere. Comparing such an outcome with
`==`/`!=` against a bare constructor is `bynk.test.untyped_outcome` with a
pointer to `is`, rather than borrowing the case's return type: the case cannot
tell whether the constructor is `Result.Ok` or `HttpResult.Ok`. A type the
test cannot resolve leaves the `.call` outcome loose, as before.

**Consequences.** Uses of a `.call` outcome are checked as in source code, so a
mistyped use is a `bynkc check` error rather than a runtime surprise. A
primitive outcome's `==` now lowers to `===` instead of the untyped structural
fallback (the moved fixture goldens). Typing http/cron/queue outcomes remains
open.

## ADR: caller-handler-needs-a-named-caller-in-tests
title: A test calling a `by c: Caller` handler names the calling context
summary: No default caller; the call is rejected at check without `by Caller("…")`

**Context.** An `on call` handler that binds its caller (`by c: Caller`) reads
`deps.identity`, the calling context's name. #680 added the call-site
`by Caller("…")`; a case that omitted it still passed `bynkc check` and then
failed `tsc` (#1978), which is what #655 set out to prevent. #655 also floated
a harness-supplied default caller.

**Decision.** `bynkc check` reports `bynk.test.actor_identity_required` on a
call to such a handler with no `by`, or with a bare `by Caller`, and asks for
`by Caller("…")`. There is no default: the identity is the name of whichever
context called, and a test has no calling context to default to, so any
invented name would be a guess the handler might branch on. A `by <Actor>` that
carries no identity (`by Visitor`) is `bynk.test.principal_identity_mismatch`.
Handlers that do not bind their caller are unaffected.

**Consequences.** The diagnostic the testing reference already named now fires
for this shape too, and its registry description widens to cover it.

## ADR: test-harness-settles-sends
title: The test harness supplies an execution context and settles `~>` sends
summary: A collecting `waitUntil` per case, awaited after each addressed call, at the unit, integration and system tiers

**Context.** A handler whose body uses `~>` hands the send's promise to
`deps.__exec.waitUntil`, which the Worker entry threads from the request's
`ExecutionContext`. The test harness supplied no `__exec`, so a case that
called such a route passed `bynkc check` and failed `tsc` (#1994); the system
tier's in-process Worker `fetch` likewise lacked its `ctx` argument.

**Decision.** For a target with a sending handler, `__makeTestDeps` carries an
`__exec` whose `waitUntil` collects promises, and each addressed service call
in a case is wrapped so the collected sends are awaited (and any sends they
start, until none remain) before the call's outcome is bound. A rejected send
does not fail the case, matching `waitUntil` on Workers. The system tier and
integration harness hand a sending participant's `fetch` the same double and
settle it before the response returns. Targets that never send emit unchanged.

**Consequences.** A case observes a send's effects, such as an agent it
updated, in the expectations that follow the call.
