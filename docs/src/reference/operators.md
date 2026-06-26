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
| `is` | binary | sum value + pattern | `Bool` | variant test, may bind ([guide](../guides/type-system/narrow-with-is.md)) |
| `?` | postfix | `Result` | unwraps `Ok` | propagates `Err`; only in a `Result`-returning fn |
| `<-` | bind | `Effect[T]` | `T` | sequences an effect in a `let` |

There is no string concatenation operator: `+` requires `Int` operands.

## Precedence

Lowest to highest:

1. `assert` (in expression position)
2. `||`
3. `&&`
4. `==` `!=` `is` *(non-associative — no chaining)*
5. `<` `<=` `>` `>=`
6. `+` `-`
7. `*` `/`
8. unary `-` `!`
9. postfix: `?`, `.field`, `.method(…)`, calls

So `assert x == 1` parses as `assert (x == 1)`, and `a + b * c` as `a + (b * c)`.

## Other expression forms

| Form | Meaning |
|---|---|
| `if c { … } else { … }` | conditional expression; branches share a type |
| `match e { … }` | exhaustive pattern match ([types](types.md#matching)) |
| `let x = e` / `let x: T = e` | binding; `let x <- e` binds an effect |
| `T.of(…)` / `T.unsafe(…)` | refined/opaque construction ([refined types](refined-types.md)) |
| `name := e` | write a `store Cell` field; committed at handler end ([agents](agents.md)) |

## Built-in types

`Int`, `String`, `Bool`, the unit `()`, and the generics `Result[T, E]`,
`Option[T]`, `Effect[T]`, `HttpResult[T]`. See the
[type system reference](types.md).
