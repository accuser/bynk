# 0251 — A `system`-tier case combines `Wire(...)` with a `by Nobody` http address call

- **Status:** Accepted (v0.218)

**Context.** ADR 0210 (system-tier-wire-rejection) gave a `Wire(...)`-carrying call a raw driver (`__sysdrive_raw_…`, every slot a `string`, decoding to an `HttpOutcome`). ADR 0212 (system-tier-no-credential) gave a `by Nobody` call a no-auth driver (`__sysdrive_noauth_…`, the *typed* driver's request minus the `Authorization` header). ADR 0249 (system-tier-wire-mixed-args) then let a raw call also carry a *typed* argument alongside a `Wire` one, converting it at the call site to the string the raw driver's slot expects.

None of these covered the fourth combination: `Wire(...)` together with `by Nobody` in the same call. `lower_method_call` (`bynk-emit/src/emitter/lower.rs`) picked the driver by checking `call_site_no_credential` before `has_wire`, so this combination always reached `__sysdrive_noauth`, which keeps the typed driver's native (non-`string`) slots. A `Wire(s)` arg still lowers to its raw inner string (`ExprKind::Wire` unwraps unconditionally), so that string landed straight into a typed slot — the same class of defect ADR 0249 fixed, for the no-auth driver instead of the raw one. ADR 0249 named this gap explicitly as out of scope (#821).

The checker already treats the combination as legal: `by Nobody`'s validation (`bynk-check/src/checker/calls.rs`) only checks that the target route is Bearer-secured and carries no identity argument, and a `Wire` arg's validation is independent of the call's credential. Rejecting the combination at check time would forbid a meaningful scenario a test author can already write elsewhere: driving an unauthenticated, unvalidated raw request and observing the seam reject the missing credential before the body is even parsed.

**Decision.**

- **D1 — a new driver, `__sysdrive_rawnoauth_…`, covers the intersection.** Emitted alongside the existing raw and no-auth drivers (`bynk-emit/src/project/tests_emit.rs`), under the same joint condition as its two parents: a route with at least one param (a `Wire`-eligible slot) *and* a Bearer-secured `by` clause. Every slot is a raw `string` (as `__sysdrive_raw`), the `Authorization` header is omitted (as `__sysdrive_noauth`), and the response decodes via `responseToUnauthOutcome` — which already delegates a non-`401` status to `responseToHttpOutcome`'s shape-based classification, so a malformed raw body would still classify correctly if the credential check somehow passed it through.
- **D2 — driver selection becomes a full 2×2 match on `(call_site_no_credential, has_wire)`.** `(true, true)` picks the new driver; the other three cells are unchanged. The mixed typed+`Wire` conversion added by ADR 0249 (the body-position arg serialises, other args coerce via `String(...)`) now triggers for either raw driver (`is_raw = driver == "__sysdrive_raw" || driver == "__sysdrive_rawnoauth"`), so a call combining all three axes (`Wire`, a typed arg, and `by Nobody`) converts correctly too.

**Consequences.**

- Fixture `385_system_wire_rejection` gains two cases: `Wire(...)` body with `by Nobody` (the two-axis combination), and a raw path segment plus a typed body with `by Nobody` (the three-axis combination D2 describes), both expecting `Rejected(Unauthorized)` — the seam rejects the missing credential regardless of the raw or serialised body underneath.
- The new driver is emitted per Bearer-secured, parameterised route unconditionally (mirroring its two parents), so unrelated fixtures with such routes (e.g. `379_system_http_boundary`) gain the emitted function even without a case exercising it.
- **Still not in scope:** nothing further — this closes the last of the four `(credential, wire)` combinations named across ADR 0210/0212/0249.
