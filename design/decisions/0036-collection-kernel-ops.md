# 0036 — The collection kernel: `prepend` as the builder, `fold` + `foldEff` as iteration

- **Status:** Accepted (v0.20b)
- **Spec:** §5.10, §7.3.7

## Context
Bynk has no loops, so the kernel must supply iteration *and* construction:
a `fold` over `length`/`get` alone can read but not write — the combinators
cannot build their results without a builder primitive. The rejected
alternative (expose `head`/`tail`, let the stdlib recurse) invites the
unbounded user recursion the no-loops stance exists to avoid.

## Decision
**`List` kernel** = the `[…]` literal, `List.empty()`, `length()`,
`get(i) -> Option[T]`, **`prepend(x)`** (cons) as the builder, and two
folds: **`fold(init, (Acc, T) -> Acc) -> Acc`** (pure) and
**`foldEff(init, (Acc, T) -> Effect[Acc]) -> Effect[Acc]`** (sequential;
an effect operation under 0031's confinement — calling it in a pure context
is `bynk.effect.fn_value_in_pure_context`). **`Map` kernel** =
`Map.empty()`, `insert(k, v)`, `get(k) -> Option[V]`, `keys() -> List[K]`,
`length()`.

Order-preserving combinators build with `fold` + `prepend` + a derived
`reverse` — never `[...acc, x]` append. Both folds emit as a single loop
(no user recursion); the emitter may use local mutation inside that loop.
**`Map.fromList` is dropped** (was proposed as derived): Bynk has no pair
type to spell its argument with — maps build via `Map.empty()` + `insert`
folds until tuples or generic records exist.

## Consequences
Every shipped combinator derives from the kernel. Known costs, recorded
deliberately: over the array lowering `prepend` is an O(n) copy, so a
Bynk-written `map` is O(n²) in the worst case — acceptable for the
boundary-sized lists this increment targets, and fixable later inside the
kernel's emission without any language change. `fold` cannot short-circuit,
so `find`/`any`/`all` scan the whole list — semantically invisible for pure
code; do not "fix" it into a compiler primitive without a decision record.
