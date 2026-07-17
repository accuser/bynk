---
level: minor
changelog: A test case drives an http route at the system tier over a real fetch with a framework-signed credential; `system_needs_wire` relaxes to a serialisation edge
---

## ADR: system-tier-http-boundary

title: A `system`-tier case drives an http route over a real `fetch` with a framework-signed credential
summary: Promote a case to `as system` to enter the target's public route table through the deployable Worker's real `fetch`, verified by the real auth seam; `system_needs_wire` relaxes from a participant count to a serialisation edge

**Context.** Slice A (#664) let a `case` drive an http/cron/queue handler at the `unit` tier — the handler in-process, the identity *given*. The testing-the-boundary track's `system` tier is the next rung: the *whole deployable app*, wired as the TypeScript it ships as. Before this slice, `as system` entered only the **internal** `/_bynk/call/` Service-Binding door (`callService`), never the public route table — so an http route was unreachable at `system`, and a single-context http target was rejected by `system_needs_wire` (the rule counted `< 2` participants, a proxy for "nothing to serialise across" that was exact only when the sole edge was cross-context).

The tier ladder this settles: **integration** = wired Bynk (the app's Bynk implementation); **system** = wired TypeScript (the whole deployable app, in-process); **e2e** (Postman, external) = real auth ceremony against a deployed instance. So *proper auth* — real IdPs, expired/forged tokens, the credential dance — is e2e's job. The system tier needs only *an* authenticated user to exercise the deployable app; the developer never hand-crafts auth.

**Decision.**

- **D1 — `system_needs_wire` relaxes to a serialisation edge.** A `system` suite needs a real serialise → JSON → deserialise boundary — a consumed context, *or* an `http` service. The `< 2 participants` count becomes `< 2 participants && no http service`. This restores the rule's intent (the check's own comment: "nothing to serialise across"), widened by the public boundary the tier now reaches. Only `http` is admitted, matching what D2 *wires*: a `queue` service does serialise its message, but driving a queue over a real wire at `system` is not built this slice, so admitting it would let a queue-only target compile as `system` while `q.message(...)` fell through to the unit-tier direct call (no wire, unit semantics under a `system` label). `cron` never qualifies (`scheduled` serialises nothing). **Queue-at-system is a noted follow-on.**

- **D2 — an `as system` http address enters the public route via a real `worker.fetch`.** `api.POST("/todos", body)` at `system` lowers to a per-route **driver** that builds a concrete `Request` (substituted path, JSON body), drives the target Worker's public `fetch` — the same emitted Worker that deploys, unmodified — and decodes the `Response` back to `HttpResult[T]` via a new `responseToHttpResult` runtime helper (the inverse of `httpResultToResponse`). This is distinct from the cross-context `callService` path, which stays for `on call` edges. Promotion holds: the case body is byte-for-byte the unit body; only `as system` changes the lowering.

- **D3 — the framework signs the credential; the real seam verifies it.** `by User("bob")` supplies the JWT `sub`; a **test-only** HS256 signer, emitted into the test output (never the deployable app — #664's Q5), signs it against the actor's declared secret, which the harness sets in `process.env` so the real Bearer seam reads and verifies it. The developer writes no auth. Real-token edge cases (expired, forged, wrong-issuer) are e2e's job, out of scope here.

**Consequences.**

- A `system` test drives the whole deployable Worker end-to-end — routing, the real auth seam (passing), the handler, serialisation, the response — for a single-context http target as much as a wired multi-context one.
- `responseToHttpResult` joins the runtime as the inverse decoder (status → variant; value-bearing 2xx parse the body; errors recover `{ error }`; redirects the `Location`). It is the one net-new runtime helper; the signer and per-route drivers are emitted only into `out/tests/`, so the deployable Worker and `runtime.ts`'s crypto surface (verify-only) are untouched.
- A driver's named-type param is typed loosely (`any`) at the driver boundary, because the case passes a plain object literal the Bynk checker already validated against the handler (Slice A); a follow-on can thread the case body's `expr_types` into the system lowering to brand refined literals as the unit path does.
- **Not in scope:** driving a `queue` (or `cron`) over a real wire at `system` — only `http` is wired, and the gate admits only `http` to match (queue-at-system is a follow-on); `Signature`/`Oidc` schemes (Bearer only — the one with an inline signable secret); the `Wire(…)` raw-input rejection path (a later slice); and `#655`'s unrelated `makeSurface` root.
