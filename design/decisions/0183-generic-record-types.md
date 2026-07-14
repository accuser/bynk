# 0183 — Generic record types: monomorphised in the checker, erased to TypeScript, non-boundary in v1

- **Status:** Accepted (v0.157)
- **Provenance:** design-review finding #546 (Bynk Language Design Review
  2026-07-05, §8 Language P1 #7) — "Generics are function-only with no bounds;
  users cannot declare `type Paginated[T]`. … API envelopes are the most common
  real pressure for a JSON-API language." **Closes #546.**
- **Realises:** a `type` declaration may carry `[A, B]` type parameters when its
  body is a **record** — `type Paginated[T] = { items: List[T], cursor:
  Option[String] }`. A parameter is an unconstrained, bound-free name scoped to
  the declaration, resolved as a rigid type variable inside the field types. A
  reference applies the type to concrete arguments (`Paginated[User]`); field
  access and construction substitute those arguments. Emission is erased TS
  generics — `export interface Paginated<T>` — exactly as generic functions
  erase to `function f<A>(…)`. Generic records are usable for internal values
  (construction, field access, `let`/parameter/return types, passing between
  helpers); they are **non-boundary** in v1.
- **Relates:** ADR 0028 (generics are function-only, no bounds — this is the
  narrowest lift of its "generic type declarations stay rejected" restriction;
  `TypeParam` was already struct-shaped for exactly this), ADR 0029
  (argument-directed inference — construction reuses the same unify/substitute
  machinery), and the built-in generics `List`/`Map` (which already
  monomorphise their JSON codecs per instantiation).

## Context

The review's premise is correct: capabilities abstract effects, but nothing
lets an author abstract *data* shape, and the recurring pressure — the API
envelope (`Paginated[T]`, `Page[T]`, a `Result`-like wrapper) — has no
expression. ADR 0028 deliberately left the door open: type parameters are a
struct (`TypeParam`), emission is erased, and "nothing ships that would have to
be unshipped." This increment walks through that door for **record** bodies.

The one genuinely large sub-problem is the boundary. Bynk treats every record as
serialisable data: a record field that cannot serialise (a function type) is
rejected at declaration, regardless of whether the type ever reaches a wire.
Making a generic record a first-class boundary payload would require
**monomorphised codecs** — a `serialise_Paginated_User` generated per
instantiation, parameterised by the element codec — an extension of the existing
`GenericInst` machinery that is a substantial piece in its own right.

## Decisions

**[DECISION A] Only a record body may be generic (Recommended: yes).** Type
parameters on a refined, opaque, or sum body are rejected
(`bynk.generics.generic_non_record`). Generic sums are a natural follow-on but
carry their own construction/exhaustiveness questions; refined/opaque generics
have no coherent meaning (their base is a fixed primitive). Records are the
shape the review asked for and the narrowest coherent lift.

**[DECISION B] Emission is erased TS generics, not monomorphised copies
(Recommended: erased).** `type Paginated[T]` emits one `interface Paginated<T>`;
a reference emits `Paginated<User>`. The checker *substitutes* type arguments
(so field access and construction see concrete types — "monomorphised" in the
checker sense), but no per-instantiation TypeScript is generated. This matches
generic-function erasure and keeps the emitted surface small; `tsc` enforces the
arguments.

**[DECISION C] Generic records are non-boundary in v1 (Recommended: yes).** A
generic record *instantiation* is rejected in any serialised position — a record
field, sum payload, service/agent handler signature, agent store, or
`Json.encode`/`decode` target — with `bynk.generics.generic_record_at_boundary`
(and `bynk.types.json_uncodable` for the codec statics), mirroring how `Fn` /
`Query` / `Stream` are confined. This defers monomorphised codecs to a later
increment. A useful consequence: because a generic record can never be a field
of any record, it can never be a field of *itself* — recursion is precluded
structurally, with no separate rule needed.

**[DECISION D] Methods on generic types are rejected (Recommended: yes).**
Attaching `fn Box.foo(…)` to a generic `Box[T]` would require generic methods
(ADR 0028 keeps those additive) and would emit an under-applied `self: Box`
signature. Rejected with `bynk.generics.method_on_generic_type`; use a free
function taking the generic value instead.

**[DECISION E] Construction infers type arguments; the binding's type is the
pressure valve (Recommended: yes).** `Paginated { items: users, cursor: None }`
infers `T` from the field values by the same argument-directed unification as a
generic call (records have no function-typed fields, so there is no
lambda-ordering subtlety). When a field cannot determine a parameter — an empty
`items` list — the expected type of the surrounding binding grounds it (`let p:
Paginated[String] = …`). An undetermined parameter is
`bynk.generics.uninferable_type_arg`. There is no explicit `Name[T] { … }`
construction form in v1 (additive later, as the bare-generic-value form was in
ADR 0029).

## Consequences

- `TypeDecl` gains `type_params`; `TypeRef` gains `App { name, args }`; the
  checker's `Ty::Named` gains `args`, and substitution / unification /
  compatibility / display recurse into it. `compatible` treats the arguments
  covariantly (records are `readonly`, like `List`).
- Bounds, generic sums, higher-kinded parameters, boundary codecs, generic
  methods, and recursive generic types all stay reachable and unshipped — the
  ADR 0028 property is preserved.
- The narrowest lift that answers the review: authors can name the envelope; the
  boundary story is a separate, well-scoped follow-on.
