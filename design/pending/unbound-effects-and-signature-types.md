---
level: minor
changelog: "An `Effect` value in an effectful body must now be awaited in order: bound with `<-`, sequenced with `do`, returned, or passed to a parameter that takes it (#1658). Building one in a value position is the new error `bynk.effect.unbound_effect`. That covers a plain `let` (`let e = Counter(\"k\").bump()`, `let _ = …`, an `Effect`-annotated `let`), a list element, and the payload of `Some`/`Ok`/`Err`, a variant or a record field. Effectful calls translate to eager promises, so such a call still ran, unawaited and racing the statements after it, with no diagnostic. Separately, a type named in a service or agent handler signature, a `from websocket`/`from events` service header, an agent's key or `store` field type, a capability or provider operation signature, or an actor's `identity` must now resolve, as in a `fn` signature (#1679). `on call(v: Bogus)` and `Effect[Unit]` (Bynk's unit is `()`) were accepted, and the emitter wrote a `/* unknown */` placeholder; they are now `bynk.resolve.unknown_type`."
---

## ADR: unbound-effect-is-an-error
title: An `Effect` value built in an effectful body and not awaited is an error
summary: A plain let, list element or constructor payload of Effect type is bynk.effect.unbound_effect; bind, do, return or pass it instead

**Context.** The emitter translates an effectful call to an eager `Promise`: the
call starts when the expression is evaluated, not when it is awaited. In an
effectful body, `let e = Counter("k").bump()` (a plain `let`, not `<-`) therefore
still performed the write, unawaited, so it raced later reads. The same held for
`let _ = …`, `[Logger.get(), Logger.get()]` and `Some(Logger.get())`. No
diagnostic fired. These forms also bypassed the `do`/`~>` unit gates and
`waitUntil`. The spec already said the eager translation makes an un-bound
effectful call observable, but only pure contexts were protected (an effectful
call there is an error). A scan of the 179 `let` sites in fixtures and examples
found no legitimate unbound use (track doc, S9).

**Decision.**
1. **(A) An error, not a warning:** `bynk.effect.unbound_effect`. Once started
   eagerly, the value cannot be used correctly; awaiting it later does not undo
   the reordering. This matches `bind_in_pure_context`.
2. **Where it fires.** In an effectful body, an expression of type `Effect[_]`
   in one of these value positions:
   - the right-hand side of a plain `let`, including `let _ = …` and an
     `Effect`-annotated `let`;
   - a list literal's element. No API takes `List[Effect[T]]`, and a list's
     element type is inferred from its elements, so it cannot vouch for them;
   - the payload of `Some`/`Ok`/`Err`, a sum-variant constructor in either
     spelling (`Loaded(x)` or the qualified `ApiResult.Loaded(x)`), or a record
     field (construction or spread override).
3. **Where it doesn't.** The legal uses:
   - the right-hand side of `<-`, the operand of `do`, and a block's tail;
   - a call argument, where the parameter decides. A parameter typed
     `Effect[T]` binds it in the callee: the higher-order use the issue keeps
     legal.

   The check doesn't descend into call arguments, receivers, lambdas or nested
   blocks, which are checked as blocks themselves. Pure bodies are untouched,
   since effectful calls are already rejected there.
4. **(B) Emission is unchanged.** Rejecting at check time removes every
   observable case without changing the eager-`Promise` translation.

**Consequences.** Every effectful call in an effectful body is now awaited in
source order, except a `~>` send, which is deliberately fire-and-forget. Newly
rejected programs were already racing. The positive corpus compiles unchanged.

Proved by:
- eight negative fixtures: plain `let`, `let _ =`, annotated `let`, list,
  `Some`, record field, and a variant payload in both spellings;
- the positive fixture `1658_effect_values_bound`, covering `<-`, `do`, the
  tail, and an `Effect` passed to a parameter typed `Effect[Int]` that binds
  it.
