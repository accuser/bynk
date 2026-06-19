# The type system

Bynk makes illegal states unrepresentable: refined and opaque types, sums and
records, and `Result`/`Option` errors-as-values, all checked before emission.

**Understand**
- [The type-system philosophy](philosophy.md)
- [The refined-literal admission model](refined-literal-admission.md)

**Do**
- [Define and validate untrusted input](define-and-validate.md)
- [Decode untrusted JSON into a typed value](decode-json.md)
- [Use a literal where a refined type is expected](use-a-literal.md)
- [Define sum, record, and opaque types](define-types.md)
- [Work with `Result` and optional values](result-and-optionals.md)
- [Pattern-match with `match`](match.md)
- [Narrow and bind with `is`](narrow-with-is.md)

**See also:** [Reference — Type system](../../reference/types.md),
[Refined-type API](../../reference/refined-types.md),
[Spec §6 — The type system](../../spec/type-system.md).
