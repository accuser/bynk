# 0037 — Collection call surface: built-in methods, qualified statics, free-function combinators

- **Status:** Accepted (v0.20b)
- **Spec:** §4.6.21a, §5.10, §8.4

## Context
Method resolution is declared-method (`has_self`) based, not universal
UFCS — and *declared* generic methods are deferred (0028) — so a
user-written generic combinator cannot be `xs.map(f)`.

## Decision
Three forms. (a) **Kernel receiver-ops are compiler built-in methods**
(`xs.fold`, `xs.foldEff`, `xs.prepend`, `xs.get`, `xs.length`; `m.insert`,
`m.get`, `m.keys`, `m.length`), dispatched on the receiver's checked type
before the declared-method path. They may be generic (`fold` is, in its
accumulator) because they are **compiler-known special forms typed directly
by the checker**, not declared methods — the generic-methods deferral bites
only on declared methods. (b) **Construction** is the `[…]` literal plus the
qualified statics `List.empty()` / `Map.empty()`, reusing the
`HttpResult.Ok` qualified-call shape. (c) **Combinators are Bynk-written
generic free functions** in `bynk.list`/`bynk.map` — `map(xs, f)`,
`filter(xs, p)`, `traverse(xs, f)`, … — brought in by `uses bynk.list`.
**No method chaining (`xs.map`) in v0.20b**; it arrives additively when
declared generic methods land. Free-name clash handling is the existing
`uses` behaviour.

`bynk.list`/`bynk.map` are the first **non-adapter synthetic units**: plain
first-party commons injected when `uses`-imported, flowing through the
ordinary commons pipeline (`bynk.map` itself `uses bynk.list`). A
`uses`-imported *function* imports as a plain value — the context type
rebrand (`__Commons<Name>`) applies to types only.

## Consequences
`insert`/`prepend` propagate an expected collection type down the receiver
chain, so `let m: Map[String, Int] = Map.empty().insert("a", 1)` infers.
The stdlib ships: `bynk.list` = `reverse`, `map`, `filter`, `find`, `any`,
`all`, `traverse`; `bynk.map` = `values`, `contains`, `getOr`.
