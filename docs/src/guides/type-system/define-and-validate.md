# Define a refined type and validate untrusted input

**Goal:** constrain a value with a predicate, then safely admit a value that is
not known until runtime.

## Define the type

Write a base type followed by `where` and one or more predicates (combine them
with `and`):

```bynk
commons signup {
  type Age = Int where InRange(0, 150)
  type Username = String where MinLength(3) and MaxLength(20)
}
```

For the full predicate list, see the
[refined-type reference](../../reference/refined-types.md).

The base may be `Int`, `Float`, or `String`. `Float` is a distinct primitive —
it never coerces to or from `Int` — and takes float bounds:

```bynk
commons pricing {
  type Ratio = Float where InRange(0.0, 1.0)
  type Price = Float where Positive
}
```

## Validate with `.of`

Untrusted input must be checked at runtime. Every refined type has an `.of`
constructor that returns a `Result`:

```bynk
fn parseAge(raw: Int) -> Result[Age, ValidationError] {
  Age.of(raw)
}
```

`Age.of(raw)` returns `Ok(age)` if `raw` satisfies the predicate, or
`Err(validationError)` otherwise.

## Handle the `Result`

Propagate the error with `?` inside a function that returns a `Result`:

```bynk
fn register(name: String) -> Result[Username, ValidationError] {
  let u = Username.of(name)?
  Ok(u)
}
```

…or branch on both cases with `match`:

```bynk
fn label(raw: Int) -> String {
  match Age.of(raw) {
    Ok(a) => "valid age"
    Err(e) => "invalid age"
  }
}
```

## Related

- To admit a value you *do* know at compile time, see
  [Use a literal where a refined type is expected](use-a-literal.md).
- Reference: [refined-type API](../../reference/refined-types.md).
- Why it works this way: [The type-system philosophy](philosophy.md).
