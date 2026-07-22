# 0220 — Enforce held-resource linearity in fn and method bodies

- **Status:** Accepted (v0.199)

**Context.** The held-resource linearity pass (ADR 0130, §2.9) enforces
single-owner discipline and mandatory disposal for held values like
`Connection[F]`. It was invoked from exactly one site — `check_handler_body` —
so it ran over handler bodies alone. `check_fn` (the checker entry for top-level
functions and methods) never ran it. The caller side already treats passing a
held value into a function as a transfer that discharges the caller's disposal
obligation (`use_held` marks the argument consumed), so the whole program type-
checks while the callee does whatever it likes with the value. A
`fn swallow(c: Connection[F]) -> Effect[()] { Effect.pure(()) }` leaks the
connection and a `fn dbl(c) { c.close(); c.close() }` double-closes it, and
neither produced a diagnostic anywhere — held-resource safety was silently
unenforced outside handlers (#718).

**Decision.** Run `linearity::check` over `fn`/method bodies too, from
`check_fn`, exactly as `check_handler_body` already does — after `type_of_block`
has populated `expr_types`, seeding each held parameter as *owned*. No parameter
is borrowed in this context (the borrowed case is a handler's framework-supplied
firing connection), so the borrowed set is empty and every held parameter
carries the full disposal obligation at scope exit. This is consistent with the
transfer-on-call semantics the caller side already assumes: ownership passes to
the callee, so the callee is the one held accountable for disposing it.

**Consequences.** A function or method that receives a held value must now
dispose it (close it, store it, or transfer it onward) on every path, or it
reports `bynk.held.leak`; a consuming operation applied twice reports
`bynk.held.use_after_consume`; branch divergence and borrow violations are
diagnosed identically to handler bodies. Programs that leaked or misused a held
value inside a `fn`/method are now rejected where they previously compiled,
hence a language increment. The shared `bynk.held.leak` message no longer names
"the handler" specifically, since it now fires for functions too.
