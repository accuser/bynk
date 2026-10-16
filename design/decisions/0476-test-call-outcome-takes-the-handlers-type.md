# 0476 — A test-body `svc.call(…)` outcome takes the handler's declared type

- **Status:** Accepted (v0.334)

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
