# §6 The type system

This chapter defines Karn's types: what each kind is, how it is written, what its
values are, how a value is constructed, and what identity the type has. It is the
definitional layer. The rules a well-formed program must satisfy — type
compatibility, admission, exhaustiveness, and the rest — are stated in
[§5](static-semantics.md), which references this chapter.

## §6.1 Type kinds

A `type` declaration ([§4.2.1](syntactic-grammar.md#421-type_decl)) introduces a
named type in one of six kinds.

### §6.1.1 Base types

{{#grammar base_type}}

The primitive types are `Int`, `String`, and `Bool`. Their values are,
respectively, integer literals, string literals, and the two booleans `true` and
`false`. The **unit type** is written `()` and has the single value `()`. Base
types are the only types that may be refined or made opaque.

### §6.1.2 Refined types

A refined type is a base type narrowed by a `where` refinement
([§4.2.4](syntactic-grammar.md#424-refined_type)). Its values are exactly those
values of the base type that satisfy every predicate of the refinement (predicate
meanings: [§6.3](#63-refinement-predicates)). A refined type is a **distinct
named type**: it does not interchange with its base or with another refined type
over the same base. Values enter it by admission ([§6.4](#64-admission--construction)).

A `type` alias with no `where` clause — for example `type Id = Int` — is the
degenerate refined type: a distinct named type over its base, carrying the same
`.of` and `.unsafe` constructors, but admitting every value of the base.

### §6.1.3 Opaque types

{{#grammar opaque_type}}

An opaque type has **nominal identity over a base** type whose representation is
hidden outside the type's defining module. Its values are constructed only
through its API — `.of` or `.unsafe` ([§6.4](#64-admission--construction)) —
never with record syntax. The base value is recoverable only through `.raw`, and
only within the defining `commons`. An opaque type MAY also carry a `where`
refinement. Opaque types are **excluded** from literal admission.

### §6.1.4 Sum types

A sum type ([§4.2.7](syntactic-grammar.md#427-sum_type)) is a tagged union of one
or more variants; a variant MAY carry a named payload. A value is constructed by
naming a variant (with its payload arguments, if any) and is consumed by `match`
([§4.6.4](syntactic-grammar.md#464-match_expr)) or `is`
([§4.6.5](syntactic-grammar.md#465-is_expr)). A sum type is represented as a
discriminated union over a tag.

### §6.1.5 Record types

A record type ([§4.2.5](syntactic-grammar.md#425-record_type)) is a product of
named, immutable fields. A value is constructed by giving every field
([§4.6.11](syntactic-grammar.md#4611-record_construction)), read by field access
([§4.6.9](syntactic-grammar.md#469-field_access)), and updated by the spread form
([§4.6.13](syntactic-grammar.md#4613-record_spread)), which copies and overrides.
A record field MUST NOT directly have the record's own type.

### §6.1.6 Enum types

An enum ([§4.2.10](syntactic-grammar.md#4210-enum_type)) is sugar for a sum type
all of whose variants carry no payload. Its values are its constant names.

## §6.2 Built-in generic types

Four generic types are built in. Their runtime shapes are normatively the
[runtime-library contract (§7.4)](runtime-library.md); this section defines their
surface.

- **`Result[T, E]`** — either `Ok(T)` (success) or `Err(E)` (error). Errors are
  values; a fallible computation returns a `Result` rather than throwing.
- **`Option[T]`** — either `Some(T)` (a value) or `None` (absence).
- **`Effect[T]`** — an effectful computation yielding a `T`. It has no surface
  constructor other than `Effect.pure(x)`, which lifts a pure `x`; an `Effect` is
  sequenced with the `<-` bind ([§4.8.4](syntactic-grammar.md#484-effect_let_stmt)).
  Effects are how a program reaches the outside world ([§5.5](static-semantics.md#55-effects-capabilities--providers)).
- **`HttpResult[T]`** — an HTTP response, the return shape of `on http` handlers
  (see [§5.7](static-semantics.md#57-handlers)).

`ValidationError` ([§4.2.19](syntactic-grammar.md#4219-validation_error_type)) is
the error type produced by a refined or opaque `.of` constructor when validation
fails.

> [!NOTE]
> The `Ok` and `Err` constructors are shared between `Result` and `HttpResult`;
> where the target is ambiguous a program must qualify the constructor
> ([§5.2](static-semantics.md#52-well-typedness)). This note is informative.

## §6.3 Refinement predicates

A refinement ([§4.2.11](syntactic-grammar.md#4211-refinement)) is one or more
built-in predicates joined by `and`. Each predicate applies to a specific base
([§5.3](static-semantics.md#53-refinement--admission)).

{{#grammar predicate_name}}

**On `Int`:**

| Predicate | Holds when |
|---|---|
| `NonNegative` | value ≥ 0 |
| `Positive` | value > 0 |
| `InRange(lo, hi)` | lo ≤ value ≤ hi (inclusive) |

**On `String`:**

| Predicate | Holds when |
|---|---|
| `NonEmpty` | length ≥ 1 |
| `MinLength(n)` | length ≥ n |
| `MaxLength(n)` | length ≤ n |
| `Length(n)` | length = n |
| `Matches(regex)` | the whole string matches `regex`, anchored |

## §6.4 Admission & construction

A value enters a refined or opaque type in one of three ways:

- **`.of(v) -> Result[T, ValidationError]`** — checked construction. The
  predicates are tested **at run time**; the result is `Ok(T)` or
  `Err(ValidationError)`. This is the path for values not known until run time —
  input, parameters, computed values.
- **`.unsafe(v) -> T`** — unchecked construction. The value is taken to satisfy
  the type without a check. It is the deliberate escape hatch for a value already
  known to be valid.
- **Literal admission** — a compile-time literal written where a refined type is
  expected is checked **at compile time** and admitted directly, lowering to
  `.unsafe` with no `Result`. The admissible positions, and the failure when a
  literal violates the predicate, are specified in
  [§5.3](static-semantics.md#53-refinement--admission). Opaque types are excluded
  from literal admission.

The flow-sensitive counterpart to `.of` is the `is` narrowing
([§4.6.5](syntactic-grammar.md#465-is_expr)): in a guard, an identifier of the
base type is narrowed to the refined type without constructing a `Result`.

## §6.4a Function types (v0.20a)

`A -> B` is a type form: a value of it is a lambda, a named function used as
a value, or a function-typed parameter/local. It is **effectful** exactly
when `B` is `Effect[_]` — the same structural rule that classifies function
declarations, so the pure-vs-effectful (`map`-vs-`traverse`) distinction
needs no new machinery. Compatibility is **contravariant in parameters and
covariant in the return type**: `t` is usable where `u` is expected when each
of `u`'s parameter types is usable where `t`'s is, and `t`'s return is usable
where `u`'s is. This is the sound generalisation of Karn's only subtyping
(refined → base widening): the covariant-everywhere alternative would let
unvalidated base values flow into a refined-typed function body. **Function
types are admissible only in non-boundary positions** — fn/lambda parameters,
returns, and locals; everywhere a value would serialise, persist, or cross a
boundary they are rejected ([§5.8](static-semantics.md#58-boundaries--cross-context),
`karn.types.function_at_boundary`). A generic function's type parameters are
unconstrained type variables, instantiated per call site and erased at
emission; they never appear in a checked program's expression types outside
the generic function's own body.

## §6.5 Type compatibility & boundaries

**Within a context, type identity is nominal.** Two named types — refined,
opaque, or alias — are distinct even over the same base; an opaque type is
distinct from and hides its base. A value of one named type is not
interchangeable with another, nor with the bare base.

**Across a context boundary, compatibility is structural.** When a value passes
between contexts — a `consumes` call argument, or a service's return value — it
need not name the same type on both sides; it MUST be **structurally compatible**:
the same `commons`-derived type, or an identical record or sum shape. A value's
type is then **projected** into the receiving context's namespace: its structural
shape is preserved and its brand is changed to the receiver's. A mismatch is
`karn.boundary.structural_mismatch`. This projection is what lets a `commons`
type be shared across contexts without a shared nominal identity.

A context's `exports` clause controls what the boundary reveals: an
`exports transparent` type shares its structure with consumers — including
field-level construction — whereas an `exports opaque` type exposes only an
opaque handle — inspecting it from outside the owning context is rejected
(`karn.context.opaque_inspection`), as is constructing a context-owned type from
outside (`karn.context.external_construction`).

**An adapter's binding is a privileged constructor of its boundary types.** The
binding is host code: it sits outside Karn's static semantics, and only the
emitted TypeScript types constrain it. For *transparent* records this is no
privilege — any consumer may construct them. The privilege bites on the stricter
kinds: a **refined** type's predicate is invisible to the TypeScript checker, so
a binding MUST construct refined values through the emitted validating `.of`
constructor and handle its `Result` — a raw cast or `.unsafe` would mint a value
the rest of the program trusts as validated without running the predicate — and
an **opaque** type, ordinarily constructible only by its defining unit, may be
built by the binding under the same emitted-constructors convention
([§7.3.6](emission.md#736-adapters)). This requirement on bindings is a
convention enforced by review and the emitted constructors' shapes, not by a
`karn.*` diagnostic — the binding is, by design, beyond the compiler's reach.

At the type level, **purity is the absence of `Effect`**: an expression whose
type is an `Effect[T]` is effectful, and an effect may be performed only in an
effectful position. The well-formedness of this discipline is
[§5.5](static-semantics.md#55-effects-capabilities--providers).
