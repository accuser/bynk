# 0176 — The `Effect[Result[T, E]]` combinators (`mapOk` / `mapErr` / `flatMapOk` / `flatMapErr`)

- **Status:** Accepted (v0.152)
- **Provenance:** design-review finding #543 (Bynk Language Design Review
  2026-07-05, §8 Language P1 #4), refs §2.8.3 — "the designed `Effect[Result]`
  combinators (`mapOk`, `flatMapErr`, design doc §2.8.3) never shipped, so
  recovery is bind-then-match." The combinators are **fully specified** in
  [`design/bynk-type-system.md`](../bynk-type-system.md) §2.8.3.
- **Realises:** four compiler-synthesised value methods on any concrete
  `Effect[Result[T, E]]` receiver —
  `mapOk(f: T -> U) : Effect[Result[U, E]]`,
  `mapErr(f: E -> F) : Effect[Result[T, F]]`,
  `flatMapOk(f: T -> Effect[Result[U, E]]) : Effect[Result[U, E]]`, and
  `flatMapErr(f: E -> Effect[Result[T, F]]) : Effect[Result[T, F]]` — so the
  success/error split of the universal cross-context shape is transformed in
  place, without an intervening `<-` peel and `match`.
- **Relates:** [ADR 0048](0048-combinators-as-kernel-methods.md) (combinators as
  kernel methods — the collision-free receiver-method form these join),
  [ADR 0174](0174-short-circuit-collect-iterators.md) /
  [ADR 0172](0172-list-collect-all-iterators.md) (the `Effect[Result]`-shaped
  `List` iterators whose `check_kernel_fn_arg` inference and inline-`async`-IIFE
  emission this mirrors), [ADR 0116](0116-query-vocabulary-and-ordering.md)
  (kernel-method direction), [ADR 0156](0156-editor-surface-tracks-language.md)
  (the editor surface tracks the language).

## Context

`Effect[Result[T, E]]` is the universal shape of a cross-context call in Bynk:
a storage read, a capability op, an agent call — each returns "an effect that
yields a fallible value." The design (§2.8.3) recognised that the *volume* of
this composition warrants language support beyond what `Effect` or `Result`
provide individually, and specified four methods for it. They had never shipped.

Without them, the two everyday moves — reshape the success/error value, or
recover from a specific error — forced a `<-` await into a local, then a
`match`, then a re-wrap, even when the intent was a one-liner. The design doc's
own worked line,
`let authId <- Payments.authorise(amount, user).mapErr(toBookingError)?`,
depends on `.mapErr` existing on the `Effect[Result]` value; it did not.

The machinery to type and emit these already exists. The checker infers a
function-argument's return exactly as `map`/`traverseTry` do
(`check_kernel_fn_arg`), and `Effect[T]` is `Promise<T>` at runtime, so each
method is the same inline `async` IIFE the `traverseTry` family
([ADR 0174](0174-short-circuit-collect-iterators.md)) already emits — awaiting
the receiver and rebuilding the `Result`. This is an additive kernel: a new
`Ty::Effect(Result)` receiver arm in the checker dispatch, the emitter dispatch,
and the LSP registry — the *first* Effect-receiver kernel methods, but built
entirely from existing parts.

## Decisions

**A — Ship all four methods, not only the two the finding names.** Finding #543
names `mapOk`/`flatMapErr`; §2.8.3 specifies four (`mapOk`, `mapErr`,
`flatMapOk`, `flatMapErr`). Realising the section by halves would leave `mapErr`
and `flatMapOk` as an odd gap on the same receiver — the set is small,
symmetric, and shares a single dispatch arm, so shipping it whole costs almost
nothing and leaves §2.8.3 fully realised. `mapOk`/`mapErr` cover the two
transformation directions; `flatMapOk` the common "continue on success with
another effectful-fallible step"; `flatMapErr` the recovery case.

**B — Compiler-synthesised sugar, emitted inline; the desugaring is the
semantics.** These are not stdlib functions or user definitions — the compiler
synthesises them for any concrete `Effect[Result[T, E]]`. Each desugars to a
predictable composition (`e.mapOk(f) ≡ e.map(r => r.map(f))`;
`e.flatMapErr(f) ≡ e.flatMap(r => match r { Ok(v) => Effect.pure(Ok(v)); Err(e) => f(e) })`),
and the emitter inlines exactly that as an `async` IIFE that `await`s the
receiver `Promise<Result<…>>` and rebuilds the transformed `Result` — no runtime
import, `tsc --strict`-clean, mirroring the `Result`-combinator lowering.

**C — Named methods, not implicit `.map` lifting.** The general rule "methods of
`T` lift onto `Effect[T]`" was considered and rejected in the design: both
`Effect` and `Result` have `.map`, and on `Effect[Result[T, E]]` "which `.map`?"
has no good answer. The four explicit names remove the ambiguity; `.map` and
`.flatMap` on the receiver stay **Effect's own**, operating on the whole
`Result`. Other `Effect`-of-X shapes (`Effect[Option[T]]`, `Effect[List[T]]`)
get **no** synthesised methods — the surface stays narrow; write
`e.map((r) => …)` explicitly.

**D — Not effectful-context-confined.** Unlike `List.forEach`/`traverseTry`,
which *run* an effectful function value and are therefore confined to effectful
bodies (`bynk.effect.fn_value_in_pure_context`), these **produce** an `Effect`
value and do not run it — the composite runs only when someone awaits it. So a
pure helper may reshape an `Effect[Result[…]]` it was handed and return it, and
no context gate applies. `flatMapOk`/`flatMapErr` require their function to
return `Effect[Result[…]]` and line up the untouched side (`flatMapOk` keeps the
receiver's error `E`; `flatMapErr` requires the recovery to produce the
receiver's success `T`) — a mismatch is `bynk.types.argument_mismatch`; a method
outside the four is `bynk.types.method_not_found`. No new diagnostic.

**E — The finding's other two recommendations are deferred as a coupled later
slice.** #543 also asks to extend `?`/early-return into `Effect[HttpResult[T]]`
handlers (with `Option`→`HttpResult` and `Result`→`HttpResult` lifts) and to add
a declared error embedding (`embeds PaymentError as Payment`) that `?` uses for
automatic conversion. Those two are **coupled** — the honest `Result`→`HttpResult`
lift needs an error-conversion mechanism, which is exactly what the `embeds`
clause provides — and together they constitute a distinct increment: a new
grammar keyword (lexer, parser, AST, tree-sitter, formatter, and the
grammar/keyword drift surface), a checker resolution pass, and an emitter
conversion. They earn their own proposal and ADR. This increment ships the one
recommendation with a settled in-repo spec (§2.8.3); **#543 stays open** for the
remaining slice.

## Consequences

- `Effect[Result[T, E]]` gains four combinators; §2.8.3 is fully realised. The
  bind-then-match tax on recovery and success/error reshaping is gone.
- The first Effect-receiver kernel methods exist, via a `Ty::Effect(Result)` arm
  in `check_kernel_method` dispatch (`bynk-check`), the emit dispatch
  (`bynk-emit`), and `methods_for` (`kernel_methods.rs`). Any other `Effect[_]`
  is untouched — it still has no methods.
- No new diagnostic, no grammar change, no runtime change. Net-new inline emit
  arms; every other kernel's lowering is byte-identical.
- The `?`-into-`Effect[HttpResult]` lifts and `embeds`-declared error embeddings
  from the same finding are **explicitly deferred** to a follow-on increment.

## Tooling (ADR 0156)

- **Hover / Completion / Signature help:** the four are added to a new
  `EFFECT_RESULT_METHODS` registry table, wired into `methods_for` for an
  `Effect[Result[_, _]]` receiver — the same source the LSP reads for the other
  kernels. The registry drift test (`kernel_registry_pins_dispatch`) gains an
  `Effect[Result[Int, String]]` receiver case, so the table cannot list a
  phantom method nor omit a real one.
- **Semantic tokens / Formatter:** unchanged — ordinary method names.

## Alternatives considered

- **Ship only `mapOk`/`flatMapErr` (the finding's literal ask).** Rejected —
  leaves `mapErr`/`flatMapOk` as an asymmetric gap on the same receiver for no
  saving (Decision A).
- **Implicit lifting of `Result` methods onto `Effect[Result]`.** Rejected in
  the design for the `.map` ambiguity (Decision C).
- **Synthesise the combinators for every `Effect`-of-X shape.** Rejected — it
  widens the surface and invites surprise; the explicit `e.map((r) => …)` escape
  hatch covers the rest.
