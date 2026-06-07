# Glossary

Terse, exact definitions of Karn's load-bearing terms. Each links to its fuller
reference page where one exists.

- **agent** — A keyed, stateful entity whose state changes only through `commit`
  inside its handlers. See [Agents](agents.md).
- **context** — A bounded context: an isolated module with its own services,
  agents, and provided capabilities, reachable only across an explicit boundary.
- **service** — A named group of handlers (`on call`, `on http`, `on cron`,
  `on queue`) declared inside a context. See [HTTP](http.md).
- **capability** — An interface of effectful operations a context may depend on.
  See [Capabilities & providers](capabilities.md).
- **provider** — A `provides` block implementing a capability, optionally `given`
  other capabilities it uses. See [Capabilities & providers](capabilities.md).
- **commit** — The statement that writes an agent's next state; valid only in an
  agent handler. See [Agents](agents.md).
- **branded type** — A compiled type carrying a unique tag so values of distinct
  Karn types cannot be interchanged in the emitted TypeScript; how opaque types
  stay distinct. See [Emission](emission.md).
- **opaque type** — A type whose representation is hidden outside its defining
  module; constructed and inspected only through its API. See
  [Type system](types.md).
- **refined type** — A base or named type narrowed by a `where` predicate, e.g.
  `Int where Positive`. See [Refined-type API](refined-types.md).
- **sum type** — A tagged union of variants, each optionally carrying a payload.
  See [Type system](types.md).
- **record** — A product type of named fields, each with a type and optional
  default. See [Type system](types.md).
- **enum** — A sum type whose variants all carry no payload. See
  [Type system](types.md).
- **refinement predicate** — A built-in constraint used in a `where` clause
  (`Positive`, `NonNegative`, `InRange`, `Matches`, `MinLength`, …). See
  [Refined-type API](refined-types.md).
- **admission** — The compile-time rule by which a literal that provably
  satisfies a refined type's predicate is accepted directly (lowering to
  `.unsafe`), with no `Result`. See [Refined-type API](refined-types.md).
- **`.of` / `.unsafe`** — Constructors for a refined type: `.of` validates at run
  time and returns a `Result`; `.unsafe` constructs without a check. See
  [Refined-type API](refined-types.md).
- **zeroable** — A type with a defined implicit zero value, letting an agent
  state field omit an initialiser. See [Agents](agents.md).
- **`Effect`** — The type of a computation that performs effects; produced by
  handlers and capability operations and sequenced with `<-`.
- **`Result` / `Option`** — Errors-as-values types: `Result` is `Ok` or `Err`;
  `Option` is `Some` or `None`. See [Type system](types.md).
- **`Mock[T]`** — A test-only expression that fabricates a value of type `T`,
  optionally pinned to a chosen value. See [Testing](testing.md).
- **project vs legacy mode** — *Project mode* is a `karn.toml`-driven directory
  layout (a `src`/`tests` split, `karnc test`); *legacy mode* compiles a single
  `.karn` file as a standalone unit, with no manifest. See
  [`karn.toml` manifest](manifest.md).
