---
level: minor
changelog: "Resolves #1889: the `List[T]` kernel gains `append(x: T) -> List[T]` and `concat(other: List[T]) -> List[T]`, both pure and non-mutating, so a `fold` or `foldEff` builds a list in order without `prepend` and `reverse`. They emit as the spreads `[...xs, x]` and `[...xs, ...ys]`, and like `prepend` they pass an expected list type down to their receiver, so `let xs: List[Int] = [].append(1)` infers. Supersedes ADR 0036's never-append clause."
---

## ADR: list-concat-append
title: The List kernel gains `append` and `concat`
summary: Order-preserving list building uses `append`/`concat`; supersedes ADR 0036's "never append" clause

**Context.** ADR 0036 made `prepend` the kernel's only list builder and said
order-preserving combinators build with `fold` + `prepend` + a derived
`reverse`, "never `[...acc, x]` append". The reason was minimalism: one builder
is enough to derive the rest. In practice the ordinary use of `fold`/`foldEff`
into a list is to accumulate in order, and #1889 found a real service
(`bynk-lang/status`) that had to `prepend` and then `sortBy` to restore the
order, which only worked because its inputs happened to sort the way they
were declared. There was also no direct way to join two lists: `a + b` is
arithmetic only, there is no list spread, and the workaround
`[a, b].flatMap((x) => x)` is not something anyone would think to write.

ADR 0036 also recorded that `prepend` is an O(n) copy over the array
lowering, so the cost argument for `prepend` over `append` does not hold:
both copy.

**Decision.** The `List[T]` kernel gains two methods, next to `prepend`:

- `append(x: T) -> List[T]`: a new list with `x` at the end.
- `concat(other: List[T]) -> List[T]`: a new list with the receiver's
  elements followed by `other`'s.

Both are pure and non-mutating, like the rest of the kernel. `append`'s item
checks against the receiver's element type, as `prepend`'s does. `concat`'s
argument checks against the receiver's `List[T]`, so an empty `[]` argument
takes its element type from the receiver, and lists of different element
types are a `bynk.types.type_mismatch`. Like `prepend` and `insert`, both
pass an expected list type down to their receiver, so
`let xs: List[Int] = [].append(1)` infers.

They lower to the spreads `[...xs, x]` and `[...xs, ...ys]`, with the
receiver evaluated before the argument, as written.

This supersedes ADR 0036's "never append" clause. The rest of ADR 0036
stands. There is no list spread syntax (`[...a, x]`), and `bynk.list` gains no
`concat` free function: its free functions are deprecated in favour of kernel
methods.

**Consequences.** A fold builds a list in order with `acc.append(x)`, and
joining lists is `a.concat(b)`. Both copy the receiver, so a fold that appends
is O(n²) in the worst case, the same cost ADR 0036 already accepted for
`prepend`. Completion, signature help and the `method_not_found` hint list
both methods, because they read the `LIST_METHODS` registry.
