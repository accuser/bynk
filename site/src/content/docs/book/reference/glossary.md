---
title: Glossary
---
Terse, exact definitions of Bynk's load-bearing terms. Each links to its fuller
reference page where one exists.

### agent {#term-agent}

A keyed, stateful entity whose state lives in `store` fields and changes only
through writes inside its handlers. See [Agents](/book/reference/agents/).

### context {#term-context}

A bounded context: an isolated module with its own services, agents, and provided
capabilities, reachable only across an explicit boundary.

### service {#term-service}

A named group of handlers (`on call`, `from http`, `from cron`, `from queue`) declared
inside a context. See [HTTP](/book/reference/http/).

### capability {#term-capability}

An interface of effectful operations a context may depend on. See
[Capabilities & providers](/book/reference/capabilities/).

### provider {#term-provider}

A `provides` block implementing a capability, optionally `given` other
capabilities it uses. See [Capabilities & providers](/book/reference/capabilities/).

### commit {#term-commit}

The atomic persistence of a handler's `store` writes. A handler's `:=` writes are
staged and committed together when it returns, after invariants are checked; a
faulting handler commits nothing. See [Agents](/book/reference/agents/).

### branded type {#term-branded-type}

A compiled type carrying a unique tag so values of distinct Bynk types cannot be
interchanged in the emitted TypeScript; how opaque types stay distinct. See
[Emission](/docs/emission/).

### opaque type {#term-opaque-type}

A type whose representation is hidden outside its defining module; constructed and
inspected only through its API. See [Type system](/book/reference/types/).

### refined type {#term-refined-type}

A base or named type narrowed by a `where` predicate, e.g. `Int where Positive`.
See [Refined-type API](/book/reference/refined-types/).

### sum type {#term-sum-type}

A tagged union of variants, each optionally carrying a payload. See
[Type system](/book/reference/types/).

### record {#term-record}

A product type of named fields, each with a type and optional default. See
[Type system](/book/reference/types/).

### enum {#term-enum}

A sum type whose variants all carry no payload. See [Type system](/book/reference/types/).

### refinement predicate {#term-refinement-predicate}

A built-in constraint used in a `where` clause (`Positive`, `NonNegative`,
`InRange`, `Matches`, `MinLength`, …). See [Refined-type API](/book/reference/refined-types/).

### admission {#term-admission}

The compile-time rule by which a literal that provably satisfies a refined type's
predicate is accepted directly (lowering to `.unsafe`), with no `Result`. See
[Refined-type API](/book/reference/refined-types/).

### `.of` / `.unsafe` {#term-of-unsafe}

Constructors for a refined type: `.of` validates at run time and returns a
`Result`; `.unsafe` constructs without a check. See
[Refined-type API](/book/reference/refined-types/).

### zeroable {#term-zeroable}

A type with a defined implicit zero value, letting an agent state field omit an
initialiser. See [Agents](/book/reference/agents/).

### `Effect` {#term-effect}

The type of a computation that performs effects; produced by handlers and
capability operations and sequenced with `<-`.

### `Result` / `Option` {#term-result-option}

Errors-as-values types: `Result` is `Ok` or `Err`; `Option` is `Some` or `None`.
See [Type system](/book/reference/types/).

### `Duration` {#term-duration}

A base type for a span of time, written as a unit literal (`5.minutes`); composes
with `Instant`. See [Type system](/book/reference/types/#duration).

### `Instant` {#term-instant}

A base type for an absolute point in time, minted by `Clock.now()`; no literal,
orderable but not numeric, advanced by a `Duration`. See
[Type system](/book/reference/types/#instant).

### `Query` {#term-query}

A lazy read over a `store`'s storage, carrying the same combinator vocabulary as
the eager `List` methods but dispatched by receiver provenance; non-storable and
non-boundary. See [Type system](/book/reference/types/#query).

### `Val[T]` {#term-val}

A test-only expression that fabricates a valid inhabitant of type `T` drawn from
its refinement domain, optionally pinned to a chosen value (`Val[T](v)`,
refinement-checked at compile time). Replaces the retired `Mock[T]` (v0.114). See
[Testing](/book/reference/testing/).

### `property` {#term-property}

A generative test, the sibling of `case` in a `suite`: `for all x: T` binds `x` to
a *generated* inhabitant of `T` and the body's `expect`s must hold across many. On
failure it reports a shrunk counterexample and a seed to reproduce. See
[Testing](/book/reference/testing/).

### project vs legacy mode {#term-project-vs-legacy-mode}

*Project mode* is a `bynk.toml`-driven directory layout (a `src`/`tests` split,
`bynkc test`); *legacy mode* compiles a single `.bynk` file as a standalone unit,
with no manifest. See [`bynk.toml` manifest](/docs/manifest/).
