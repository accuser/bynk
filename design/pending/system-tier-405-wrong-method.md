---
level: minor
changelog: A `system`-tier test case drives an existing http path with a method it declares no handler for and observes the router's `405` fall-through as `Rejected(MethodNotAllowed)` (#707)
---

## ADR: system-tier-405-wrong-method

title: A `system`-tier case addresses a route with the wrong method to test the `405` fall-through
summary: Addressing a declared path with an undeclared method is no longer `service_unknown_route`; it drives the router's method fall-through through a generic no-handler driver and yields `Rejected(MethodNotAllowed)`, completing the boundary-rejection surface (`400`/`401`/`405`). A genuinely unknown *path* is still rejected.

**Context.** ADR 0210 (system-tier-wire-rejection) and its follow-ons gave a `system` case the outcome sum `Rejected(detail) | Handled(HttpResult[T])` and drove the boundary's `400` (raw `Wire`, #704) and `401` (`by Nobody`, #706) rejections. The `405` fall-through — a live path reached under a method it has no handler for — stayed unreachable (#707): the test-body http address required the `(method, path)` to match a declared route **exactly** (`bynk.test.service_unknown_route`), so `api.DELETE("/cart")` on a POST-only route was a compile error. ADR 0210 DECISION C sketched the answer (assert the `405` in the `HttpResult` vocabulary) but deferred it.

**Decision.**

- **D1 — addressing a declared path with an undeclared method is a wrong-method `405` test, not an error.** The checker's http-address resolution splits the old exact-match failure in two: if the `(method, path)` has no handler but the **path** is declared for some other method, the call is allowed — it drives the `405` fall-through. Only a path declared for **no** method is `bynk.test.service_unknown_route` (message reworded to name the path, with a note pointing at the wrong-method form). No handler runs, so the call takes no arguments and no `by` clause; the outcome is loose.

- **D2 — a `405` is `Rejected(MethodNotAllowed)`, superseding DECISION C.** The `405` joins the `400`/`401` rejections rather than the `HttpResult` vocabulary: it means *no handler ran* (the router synthesised the response before dispatch, and no handler can return it), which is the definition of `Rejected`. A generic per-service **no-handler driver** (`__sysdrive_wrongmethod_<svc>(method, path)`, test-output only) sends the arbitrary `(method, path)` and decodes through `responseToHttpOutcome`, which now maps a `405` to `Rejected(MethodNotAllowed)` unambiguously (a `405` is never handler-produced, so this is safe for the shared decoder). The lowering routes a call whose `(method, path)` is not a declared route — carried into the emitter as a new `system_http_routes` set — to that driver. `expect r is Rejected(MethodNotAllowed)` discriminates the inner kind via the nested-`is` from ADR 0211 (#705).

**Consequences.**

- The boundary-rejection surface is complete for a `system` case: `400` (`Wire`), `401` (`by Nobody`), `405` (wrong method). Fixture 385 drives `api.DELETE("/cart")` → `Rejected(MethodNotAllowed)` over the real wire; the golden carries the generic driver and the two-level `r.tag === "Rejected" && r.value.tag === "MethodNotAllowed"` test. Behaviourally verified: a `405` decodes to `Rejected(MethodNotAllowed)`, a genuine `404` decodes to `Handled(NotFound)` (not a false `MethodNotAllowed`).
- `responseToHttpOutcome` gains a `405` case; the wrong-method driver's payload deserialiser is unused (the `405` takes the `Rejected` arm), so it is a trivial inline `Ok` lambda. Emitted only into `out/tests/`; the deployable worker is untouched.
- The distinction between a wrong method and an absent path is real coverage: fixture 385 (wrong method on a declared path → allowed) and the existing negative `379_test_unknown_route` (`api.GET("/nope")`, an absent path → `service_unknown_route`) both hold.
- **Not in scope:** the bare-`OPTIONS` `204` discovery answer (a `2xx`, not a rejection — it would decode as `Handled`); a wrong method against a *parameterised* path uses the pattern string verbatim as the URL (the `405` fall-through matches on the path pattern, so the param values are immaterial), so no concrete-path substitution is added.
