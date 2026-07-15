# 0196 — Generic records at the boundary: monomorphised per-instantiation JSON codecs

- **Status:** Accepted (v0.173)
- **Provenance:** issue #592 (follow-up to the generic-record-types increment,
  v0.157). ADR 0183 shipped `type Paginated[T] = { … }` but made a generic-record
  instantiation **non-boundary** (Decision C): rejected in every serialised
  position with `bynk.generics.generic_record_at_boundary` /
  `bynk.types.json_uncodable`. The flagship motivation for #546 — an API envelope
  like `Paginated[T]` returned as an HTTP response body — therefore did not work.
  **Closes #592.**
- **Realises:** a generic-record instantiation is boundary-serialisable exactly
  when its type arguments are. `Paginated[User]` at a boundary emits a
  **monomorphised codec** `serialise_Paginated_User` / `deserialise_Paginated_User`
  — the declared field types with the type parameters substituted by the concrete
  arguments — extending the existing `GenericInst` machinery that already
  monomorphises `serialise_List_User`, `serialise_Map_String_Int`, ….
- **Supersedes:** ADR 0183 Decision C (generic records non-boundary in v1). The
  rest of ADR 0183 stands — the type is still monomorphised in the checker and
  erased to a TS generic interface; only the boundary confinement is lifted.
- **Relates:** the built-in generics `List`/`Map`/`Result`/`Option` (which already
  monomorphise their JSON codecs per instantiation — this is the same mechanism
  applied to user generics), ADR 0030 (function types non-boundary — the confined
  family a non-serialisable *argument* still belongs to), and ADR 0184 (`MapEntry`,
  a compiler-known generic record — see Decision D).

## Context

ADR 0183 identified the boundary as "the one genuinely large sub-problem" and
deferred it: making a generic record a first-class boundary payload requires
**monomorphised codecs** — a `serialise_Paginated_User` generated per
instantiation, parameterised by the element codec. That deferral is the only
thing standing between "authors can name the envelope" and "authors can send it."
Bynk treats every record as serialisable data, so the gap is felt immediately:
the moment an author writes `on get "/" () -> Paginated[User]` or
`Json.encode(page)`, the compiler rejects it.

The machinery to close the gap already exists for the built-in generics. The
emitter's `GenericInst` enum (`bynk-emit/src/emitter/serialisation.rs`) carries
`ResultInst` / `OptionInst` / `ListInst` / `MapInst` and emits a specialised
helper per instantiation, keyed by a mangled name (`List_User`,
`Map_String_Int`). A generic *record* is the same shape: an application of a
declared type to concrete arguments, whose codec is the declared body with the
parameters substituted. The lift is to add a `RecordInst` variant and a
TypeRef-level substitution of the declared field types.

## Decisions

**[DECISION A] Monomorphise a codec per instantiation; keep the interface erased
(Recommended: yes).** `Paginated[User]` at a boundary emits
`serialise_Paginated_User(value: Paginated<User>)` /
`deserialise_Paginated_User(…): Result<Paginated<User>, BoundaryError>`, its
fields specialised to `User` and delegating to their own codecs
(`serialise_List_User`, `serialise_Option_String`). The emitted TypeScript
*interface* stays the erased `Paginated<T>` (ADR 0183 Decision B unchanged) — only
the codec is per-instantiation, exactly as `List`/`Map`/`Result` already erase
their type but monomorphise their codec. This is the narrowest, most consistent
extension: no new runtime, no reflection, `tsc --strict`-clean by construction.

**[DECISION B] A generic record is serialisable iff its arguments are; the error
for a non-serialisable argument stays that argument's own boundary error
(Recommended: yes).** The boundary rule stops rejecting the application and
instead looks *through* it into the type arguments. `Paginated[User]` is
admitted; `Paginated[Int -> Int]` is rejected at the function argument with
`bynk.types.function_at_boundary`, and `Json.decode[Box[Query[Int]]]` with
`bynk.types.json_uncodable` — the same errors those types draw anywhere else. The
`json_codable` predicate and the `reject_fn_types` structural walk both recurse
into the arguments (mirroring how they already recurse into `List`/`Map`
elements). A generic record whose *field* (not argument) is non-serialisable is
still rejected at its declaration, unchanged. The blanket
`bynk.generics.generic_record_at_boundary` rejection is removed from the
structural and codec-static paths; the code survives only on the `Val[…]`
value-fabrication guard (Decision E).

**[DECISION C] A recursive generic record is rejected at the boundary, with its
own diagnostic (Recommended: a dedicated boundary guard).** ADR 0183 Decision C
leaned on non-boundary to make a generic record structurally unable to contain
itself. With that lifted, a generic record *can* be a record field, so
self-containment becomes expressible — and a recursive generic record has **no
finite set of monomorphised codecs**: uniform recursion (`type Node[T] = { next:
Option[Node[T]] }`) would need a self-referential codec chain the
per-instantiation model does not generate, and polymorphic recursion (`type
Weird[T] = { next: Option[Weird[List[T]]] }`) an unbounded set of instantiations
(`Weird_Int`, `Weird_List_Int`, … each distinct, so no dedup terminates). The
resolver's `bynk.resolve.recursive_record_field` guard does **not** cover these:
it treats only a *direct* `Named`/`App` head as a containment edge, and
deliberately not a reference through an `Option`/`List` wrapper (whose
empty/`None` inhabitant breaks the cycle for a *non-generic* record, which does
serialise — its single codec is self-referential and terminates on the data). So
a new boundary guard is added: `bynk.generics.recursive_generic_at_boundary`
fires (in both the structural boundary pass and the `Json` codec statics) when a
generic-record instantiation transitively contains itself, following every
wrapper, sum payload, and generic argument. Uniform-recursive generic records at
the boundary are a coherent future lift; polymorphic recursion is not
monomorphisable at all. The emit-side codec walks additionally short-circuit a
recursive generic as defence in depth, so a bypass could never fail to
terminate.

Because a **record field is itself a boundary position**, the self-reference
lives in a field and so the guard fires at the *declaration* — a recursive
generic record is therefore not merely non-boundary but **undeclarable**, exactly
as it already was under ADR 0183 (which rejected the same declaration under
`generic_record_at_boundary`). That is a deliberate consequence, not a
regression: only the diagnostic changes, from the blanket generic-record message
to the specific recursion one. A *non-generic* recursive record is unaffected —
its single self-referential codec terminates on the data, so it declares and
serialises as before.

**[DECISION D] `MapEntry` stays non-boundary in this increment (Recommended:
defer).** `MapEntry[K, V]` (ADR 0184) is a compiler-known generic record with no
user `TypeDecl`, so the general user-generic path — which substitutes a
declaration's fields — does not reach it, and it is not writable in a type
annotation. Making `MapEntry` cross a boundary directly and become annotatable is
a distinct surface change tracked by #595; it is out of scope here. The codec
machinery this increment adds is what that follow-up will build on.

**[DECISION E] `Val[…]` value fabrication for generic types stays rejected
(Recommended: yes).** Fabricating an arbitrary property-test value of a generic
type is a separate capability (per-instantiation *value* generation, not codec
generation) that this increment does not wire; `Val[Paginated[…]]` keeps its
`bynk.generics.generic_record_at_boundary` rejection. Serialising a value an
author already holds and fabricating one from nothing are independent problems.

## Consequences

- `GenericInst` gains a `RecordInst { name, args }` variant; the emitter gains a
  `TypeRef`-level `subst_type_ref` (substitute type parameters by concrete
  arguments) and `record_inst_fields` (the concrete field list for an
  instantiation). The codec-collection walks (`collect_type_names`,
  `walk_generic_inst`) thread the type-declaration table so they can expand a
  generic record's substituted fields, reaching the named and generic helpers its
  codec calls.
- `json_codable` (checker) and `reject_fn_types` (emit-side boundary pass) recurse
  into a generic record's arguments instead of rejecting the application, and both
  gain a `generic_record_is_recursive` check (a shared AST-graph reachability
  helper in `bynk-syntax`) that fires `recursive_generic_at_boundary`.
  `ty_to_type_ref` round-trips a `Ty::Named { args }` back to a `TypeRef::App` so
  the `Json.encode`/`decode` codec closure reaches the monomorphised helper.
- The confined family (`Fn` / `Query` / `Stream` / `Connection`), generic sums,
  generic methods, bounds, and higher-kinded parameters all stay reachable and
  unshipped — the ADR 0028 property is preserved.
- The API envelope the review asked for now works end to end: authors can name the
  envelope *and* send it.
