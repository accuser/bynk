# 0228 — Report unresolvable types at expression-position resolution sites

- **Status:** Accepted (v0.206)

**Context.** The resolver's reference walk validates every type reference in
declarations and `fn`/method bodies, but it deliberately does not descend into
*handler* bodies (service/agent/actor/test) — those are checked only by the type
checker, which runs as a later phase. Three checker sites resolved a type
reference with the fallible `resolve_type_ref(_in)` and, on `None`, propagated
the `None` with no diagnostic:

- an explicit call type argument — `identity[Missing](5)` (`check_generic_call`);
- a `Json.decode[T]` target type — `Json.decode[Typo]("{}")`;
- a lambda parameter annotation — `(x: Nope) => x` (`check_lambda`).

In a handler body all three compiled clean — `bynkc check` exited 0 with zero
diagnostics, or the binding vanished and a downstream use reported a misleading
"unknown name". The explicit-type-argument case was silent even in a `fn` body:
the resolver's `ExprKind::Call` arm destructured `Call { name, args, .. }`,
dropping `type_args` entirely, so it validated them nowhere. This is the same
class of gap as service record field-set validation (#711): a validation that
lived only on a path handler bodies bypass.

**Decision.** Emit `bynk.resolve.unknown_type` on the `?`-failure path at each of
the three checker sites, pointing at the exact offending name where one can be
identified. A shared `resolve_expr_type_ref` helper resolves the reference,
records its symbol references for the IDE on success, and reports on failure.
Additionally, the resolver now walks `Call::type_args`, so `fn`/method bodies are
covered there by the resolver rather than the checker backstop. Because the
compile pipeline sequences `resolve(..)?` before `check(..)`, the checker runs
only when resolution reports nothing, so a `fn`-body case reported by the
resolver never double-reports through the checker.

**Consequences.** A previously-accepted (silently mis-compiled) program with an
unresolvable type in one of these positions is now rejected with a precise
diagnostic — a language increment. The checker is the sole guard for handler
bodies (which the resolver never visits) and a backstop for `fn` bodies (which
the resolver now covers for call type args). Lambda parameter annotations that
name an enclosing type parameter resolve consistently with how the resolver
already treats them, since the shared helper resolves with the enclosing type
parameters in scope.
