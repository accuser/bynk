# Operators & built-ins

## Operators

| Operator | Arity | Operands | Result | Notes |
|---|---|---|---|---|
| `+` `-` `*` `/` | binary | `Int` | `Int` | arithmetic; no `%` |
| `-` | unary | `Int` | `Int` | negation |
| `==` `!=` | binary | same type | `Bool` | non-associative |
| `<` `<=` `>` `>=` | binary | `Int` | `Bool` | comparison |
| `&&` `\|\|` | binary | `Bool` | `Bool` | logical |
| `!` | unary | `Bool` | `Bool` | logical not |
| `implies` | binary | `Bool` | `Bool` | logical implication (`P implies Q` ≡ `!P \|\| Q`, directional); lowest precedence; in invariant predicates ([invariants](agent-invariants.md)) |
| `is` | binary | sum value + pattern | `Bool` | variant test, may bind ([guide](../guides/type-system/narrow-with-is.md)) |
| `?` | postfix | `Result` | unwraps `Ok` | propagates `Err`; only in a `Result`-returning fn |
| `<-` | bind | `Effect[T]` | `T` | sequences an effect in a `let` |
| `~>` | prefix | `Effect[()]` | statement | asynchronous (fire-and-forget) send; the reply is discarded, so it is restricted to `Effect[()]` ([guide](../guides/effects-and-capabilities/synchronous-and-asynchronous-sends.md)) |

There is no string concatenation operator: `+` requires `Int` operands.

## Precedence

Lowest to highest:

1. `assert` (in expression position)
2. `implies` *(lowest-precedence operator; predicate language)*
3. `||`
4. `&&`
5. `==` `!=` `is` *(non-associative — no chaining)*
6. `<` `<=` `>` `>=`
7. `+` `-`
8. `*` `/`
9. unary `-` `!`
10. postfix: `?`, `.field`, `.method(…)`, calls

So `assert x == 1` parses as `assert (x == 1)`, and `a + b * c` as `a + (b * c)`.

## Duration & Instant arithmetic

`Duration` (a span) and `Instant` (an absolute point in time) compose through a
fixed operator surface; every other numeric mix between them — or with `Int` — is a
`bynk.types.no_numeric_coercion` error.

| Expression | Result | Notes |
|---|---|---|
| `Duration ± Duration` | `Duration` | subtraction is unclamped (may go negative) |
| `Duration * Int` / `Int * Duration` | `Duration` | scalar scaling |
| `Duration` vs `Duration` (`<` `<=` `>` `>=`) | `Bool` | comparison |
| `Instant ± Duration` | `Instant` | advance / retreat |
| `Instant - Instant` | `Duration` | the span between |
| `Instant` vs `Instant` (`<` `<=` `>` `>=`) | `Bool` | chronological ordering |

`Instant` is **orderable** (so `sortBy`/`min`/`max` key on it) but **not numeric**
(`sum`/`average` reject it). Timestamp math goes through `Instant` —
`Clock.now() + 5.minutes` is `Instant + Duration`. The earlier `Int + Duration ->
Int` clock-math coercion was **withdrawn** at v0.90; mixing `Instant` with `Int`
now errors. Convert explicitly with `d.toMillis()` / `Duration.millis(n)` and
`t.toEpochMillis()` / `Instant.fromEpochMillis(n)`. See [types](types.md#duration).

## Other expression forms

| Form | Meaning |
|---|---|
| `if c { … } else { … }` | conditional expression; branches share a type |
| `match e { … }` | exhaustive pattern match ([types](types.md#matching)) |
| `let x = e` / `let x: T = e` | binding; `let x <- e` binds an effect |
| `T.of(…)` / `T.unsafe(…)` | refined/opaque construction ([refined types](refined-types.md)) |
| `name := e` | write a `store Cell` field; committed at handler end ([agents](agents.md)) |

## Built-in types

`Int`, `Float`, `String`, `Bool`, `Duration`, `Instant`, the unit `()`, and the
generics `Result[T, E]`, `Option[T]`, `Effect[T]`, `HttpResult[T]`, `Stream[T]`,
`Query[T]`. See the [type system reference](types.md).
