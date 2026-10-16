# 0481 — A case's service call is typed by its handler, and a call past the handler's types by the wire

- **Status:** Accepted (v0.336)

- **Relates:** [[0203]] (the loose outcome, re-openable), [[0476]] (`svc.call` typed by its handler, http/cron/queue left open), #1931 (fixed payloads typed), #1934.

**Context.** ADR 0203 resolved a test body's `svc.call(args)` against the
service's `on call` handler, but left the outcome's *type* loose: "the runner
recovers `Result`/`Effect` at runtime". It marked typing it as re-openable.
Later slices added the http, cron and queue address forms (`svc.GET("/x")`,
`svc.schedule("…")`, `svc.message(m)`) and kept the outcome loose.

ADR 0476 (#1979) typed `svc.call(…)` by its handler and left the other
addresses loose, "its outcome's shape differs by tier and by `Wire`, so a
single static type would be wrong somewhere". It gave
`bynk.test.untyped_outcome` a second use: comparing such an outcome with `==`
against a bare constructor.

A loose outcome cost more than it saved. A method on a value drawn from it had
no receiver type to lower, so it emitted `/* unknown */` and failed `tsc`
(#1931). #1931 narrowed the gap rather than closing it. It bound an outcome
*loose*: error-typed, with no error, tracked per scope frame and per `is` test.
It typed the payloads of fixed type (`Raw(body, contentType)`, message and
redirect variants) through `HttpResult[<error>]`. A method on anything else was
the new `bynk.test.untyped_outcome`. So a case still could not call a method on,
or get a type error about, a value of the handler's result type. A mistyped
rejection kind (`Rejected(Refinment(_))`) was a case that silently never
matched.

A prototype typed every matched address as its handler's declared return type.
At the unit tier that was right. At the `system` tier it broke
`385_system_wire_rejection`. There, a call can observe the *boundary* as well as
the handler. The raw driver (`Wire(…)`), the no-credential driver (`by Nobody`)
and the wrong-method driver decode an `HttpOutcome` (`Rejected(…)` or
`Handled(httpResult)`), not the handler's `HttpResult[T]`.

**Decision.** A test-service address has the type of what it yields.

- **A resolved handler yields its declared return type.** As ADR 0476 decided
  for `svc.call(…)`, so for every address: `svc.call(…)`, a
  matched `svc.<VERB>("/path", …)`, `svc.schedule("…")` and `svc.message(m)`
  have the handler's return type, an `Effect[T]` (a service handler must return
  one, `bynk.service.return_not_effect`), so `let r <-` binds `T`. This holds at
  every tier. A typed `system`-tier call goes through the serialising driver,
  which decodes the handler's `HttpResult[T]` from the response, the same type.
- **A call past the handler's types yields `HttpOutcome[T]`.** This covers an
  http address that carries a `Wire(…)` argument, is driven `by Nobody`, or names
  a method the path declares no handler for. Its type is
  `Effect[HttpOutcome[T]]`, where the handler returns `Effect[HttpResult[T]]`.
  These are exactly the conditions on which the emitter picks the raw, no-auth
  or wrong-method driver, so the type and the decoded shape cannot disagree. A
  wrong method reaches no handler, so its `T` is `()`. `by Nobody` lives on the
  statement, not the call, so the effect-let applies it after typing the call.
- **`HttpOutcome[T]` and `BoundaryRejection` are built-in sums**, alongside
  `HttpResult[T]` and `QueueResult`:

  ```text
  HttpOutcome[T]    = Rejected(value: BoundaryRejection) | Handled(value: HttpResult[T])
  BoundaryRejection = RefinementViolation(detail: String) | MalformedJson(detail: String)
                    | StructuralMismatch(detail: String) | Unauthorized | MethodNotAllowed
  ```

  The first three kinds are the router's `BoundaryError` kinds. Their bodies
  differ (`details`; `path`/`expected`/`actual`; `path`/`violation`), so the
  rejection carries one uniform `detail: String` instead: the router's one-line
  description, which `responseToHttpOutcome` now computes. That keeps the
  one-field arity the documented patterns already use (`RefinementViolation(_)`).
  It does not commit the language to the router's error records. `Unauthorized`
  is the auth seam's `401` to a request with no credential. `MethodNotAllowed`
  is the router's `405` fall-through.

  Neither type can be named or constructed in source: they are
  scrutinised only, with `is` and `match`. They exist only as what a case's service
  address yields, so a type annotation has nothing to name. Nameability can be
  added later without changing what any case means.
- **The outcome type is tier-independent.** `Wire` and `by Nobody` are already
  `system`-only (`bynk.test.wire_needs_system`,
  `bynk.test.credential_needs_system`), so the checker needs no tier to type
  them. Those gates are what make the type meaningful. A wrong-method call is
  not tier-gated, and types as `HttpOutcome[()]` at every tier.
- **An asset fetch yields `Effect[HttpResult[()]]`.** The platform answers
  `Raw(body, contentType)` or `NotFound`, and no handler's `T` is involved.
- **Cron and queue are typed, not left untyped.** A cron handler returns its
  declared type, usually `Effect[Result[(), String]]`. A queue handler returns
  `Effect[QueueResult]`. `QueueResult` is now matchable (`Ack`, `Retry(reason:
  String)`) wherever a value of it is scrutinised. It was construct-only, so
  `expect r is Ack` had worked only because the outcome was loose.
- **`bynk.test.untyped_outcome` is retired.** No address leaves its outcome
  untyped, so neither of its uses (a method on part of an outcome, ADR 0476's
  `==` against a bare constructor) remains: `expect r == Ok("hi")` on an http
  address now types the `Ok` from `r`. #1931's loose-binding machinery
  (loose bindings, per-`is`-test adoption, loose match arms) is removed with it.
  The emitter's `/* unknown */` receiver placeholder stays reachable only for a
  program that failed checking, as before.

**Consequences.**

- A case can call a method on, and get type errors about, any value its
  service call yields: `Ok(v) => v.length()`, `Handled(Created(item)) =>
  item.sku`, `Retry(why) => why.length()`. A mistyped rejection kind, a
  variant the outcome lacks (`expect r is Rejected(_)` on a typed call), or a
  wrong arity is a compile error, not a case that silently fails.
- Typed values lower by their types. A primitive-typed binding compared with
  `==` lowers to host `===` instead of the structural `__bynkEq` walker, as it
  does everywhere else. For `svc.call` outcomes ADR 0476 already moved those
  goldens; the http, cron and queue fixtures on `main` compare no primitive
  outcome, so this increment moves no other test module.
- The runtime's `Rejected` value gains `detail`, a string field. A case that
  never binds it is unaffected.
- A wrong-method call at the unit or integration tier types, but its lowering
  (a direct call on a handler key that does not exist) does not run. That is a
  pre-existing gap in the wrong-method driver, which only `system` wires, and
  is tracked separately.
