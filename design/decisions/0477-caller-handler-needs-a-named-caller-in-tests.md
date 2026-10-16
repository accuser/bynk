# 0477 — A test calling a `by c: Caller` handler names the calling context

- **Status:** Accepted (v0.334)

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
