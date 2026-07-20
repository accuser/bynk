---
level: patch
changelog: A `system`-tier case can mix a typed argument with `Wire(...)` in the same http address call
---

## ADR: system-tier-wire-mixed-args
title: A `system`-tier case mixes a typed argument with `Wire(...)` in one http address call
summary: The raw driver serialises a typed slot at the call site instead of forwarding it as-is

**Context.** ADR 0210 (system-tier-wire-rejection) gave a `system` case a raw driver (`__sysdrive_raw_…`) for a `Wire(<String>)` call — every slot forwarded as a `string` (path params into the URL, the body sent verbatim). The checker already validates a mixed call arg-by-arg (a `Wire` arg as `String`, a non-`Wire` arg against the handler's declared type — `check_address_args` in `bynk-check/src/checker/calls.rs`), so `api.PUT("/flags/:name", Wire(""), Flag { enabled: true })` compiled, but the emitter did not: `lower_expr` on a non-`Wire` arg produced its native TypeScript value (an object literal, a number, …), which the raw driver's uniformly-`string`-typed slots reject — a `TS2345` for a body, silent wrong-shape data for a path param. Noted as a deferred follow-on (#708).

**Decision.**

- **D1 — a non-`Wire` arg in a raw call converts to the string the raw driver's slot expects, at the call site.** `lower_method_call` now threads two lookups into `LowerCtx` from the emitted per-route driver metadata: `system_http_route_body` (route → the body param's positional index and declared type, if any) and `system_http_type_ns` (the target's type namespace). For a raw call (`has_wire`), each non-`Wire` positional arg is converted: the body-position arg serialises through the same wire codec the typed driver uses (`JSON.stringify(serialise_expr_via(...))`) so a hand-typed body matches a `Wire`d one byte-for-byte; any other (path) arg just coerces via `String(...)` — matching what the URL template substitution already does implicitly. A `Wire(inner)` arg is untouched (already lowers to its raw inner string).
- **D2 — the typed value casts through `any` before serialising.** A named-type literal (a record) is unbranded in test-scaffold code — the Bynk checker already validated it against the handler's declared type (Slice A), so branding is immaterial here — the same reason `driver_param_ty` types a named **driver** param `any` rather than the branded type. The call-site conversion casts the lowered value the same way (`(<lowered> as any)`) before handing it to `serialise_expr_via`, so `serialise_<Name>` (which expects the branded shape) accepts the plain object literal.

**Consequences.**

- Fixture `385_system_wire_rejection` gains a `PUT("/cart/:sku") (sku: Sku, body: Item)` route and three cases: a raw (`Wire`) path with a typed body (valid → `Handled`, and an empty `sku` → `Rejected(RefinementViolation(_))`), and a typed path with a raw (`Wire`) body (`Handled`). All three drive the real wire and type-check under `tsc --strict`.
- The conversion only applies to the raw driver (`has_wire`); the typed and no-auth drivers are unchanged — a call with no `Wire` arg keeps lowering exactly as before.
- **Still not in scope:** a `Wire` argument in a path-param position that the router treats as a compound (non-string) segment — path params are string URL segments by construction, so this does not arise; a call mixing `Wire` with `by Nobody` still routes to the (typed) no-auth driver unconverted, an existing gap this ADR does not touch.
