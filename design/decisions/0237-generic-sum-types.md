# 0237 — Generic sum types — rigid-var variant payloads, erased to a TS discriminated union, boundary via monomorphised codecs

- **Status:** Accepted (v0.212)

**Context.** Generic *record* types shipped in v0.157 (ADR 0183, #546) and became
boundary-serialisable through monomorphised per-instantiation codecs in v0.174
(ADR 0197, #592). Both increments deferred **generic sums** — a `type ApiResult[T]`
tagged union — even though the built-in `Result`/`Option`/`HttpResult` are exactly
that shape, and an author has no way to express a `Result`-like envelope, a
`Tree[T]`, or a generic state machine of their own. The type parameters already
parse on any `type` body (the grammar shares the `[…]` prefix); the restriction to
records was purely semantic (`bynk.generics.generic_non_record`).

The type system needed no new representation: `Ty::Named { kind, args }` already
carries applied arguments for every kind, and substitution / unification /
`compatible` / display already recurse into `args` kind-agnostically. The work is
to make the two sites that resolve a variant's payload — `variants_of` (feeding
`match` and exhaustiveness) and `check_variant_construction` — substitution-aware,
and to give the emitter a sum analogue of the #592 record codec machinery.

**Decision.** A `type` declaration may carry `[A, B]` type parameters when its body
is a **record or a sum**; a refined or opaque body may not (its base is a fixed
primitive). Inside a generic sum's variant payloads the parameters resolve as rigid
type variables, exactly as inside a generic record's fields.

- **Construction is argument-directed** (as generic records are, ADR 0183 Decision
  E; and generic calls, ADR 0029). `Loaded(user)` infers `T = User` by unifying the
  payload's declared type against the argument's. A variant that cannot determine a
  parameter — a payload-less variant (`Empty`), or one whose payload never mentions
  it (`Failed(message: String)`) — grounds it from the surrounding binding's
  expected type (`let r: ApiResult[User] = Failed("...")`); an undetermined
  parameter is `bynk.generics.uninferable_type_arg`. The nullary case reuses the
  same valve at both spellings (`Empty` and `T.Empty`).
- **`match` substitutes** the instantiation's arguments into each arm's payload
  binding (`Loaded(v)` binds `v : User` over `ApiResult[User]`), through the same
  `instantiate_field_ty` the generic record uses; exhaustiveness and arm-join are
  unchanged.
- **Emission is erased** TypeScript generics, not monomorphised copies (ADR 0183
  Decision B): one `export type ApiResult<T>` discriminated union, each payload
  variant a generic-arrow constructor (`Loaded: <T>(value: T): ApiResult<T> => …`)
  and each payload-less variant a constant cast to the all-`never` instantiation
  (`Empty: { tag: "Empty" } as Tree<never>`), which is assignable to every `Tree<X>`
  because the nullary arm names no parameter. A non-generic sum emits byte-identical
  output.
- **The boundary reuses the #592 model.** A generic-sum instantiation is
  serialisable exactly when its type arguments are: the emitter generates a
  monomorphised codec per instantiation (`serialise_ApiResult_User` /
  `deserialise_ApiResult_User`, wire discriminant `kind`, in-memory `tag`),
  specialised to the concrete arguments and delegating to their codecs — the sum
  analogue of `RecordInst`, sharing the substitution (`sum_inst_variants`), the
  boundary-type collection, and the recursion guard. A recursive generic sum has no
  finite codec set and is rejected at a boundary with
  `bynk.generics.recursive_generic_at_boundary`.
- **Two capabilities stay deferred.** Methods on a generic type still require
  generic methods (`bynk.generics.method_on_generic_type`), and a generic sum may
  not carry an `embeds` clause — folding another sum's variants in by name does not
  compose cleanly with per-parameter substitution — rejected with the new
  `bynk.generics.generic_sum_embeds`.

**Consequences.** Authors can now name a generic tagged union and send it across a
boundary, closing the last data-shape gap the #546 review opened. Nothing new is
added to the type representation; `variants_of` and `check_variant_construction`
gain the substitution the record path already had, and the emitter gains a
`SumInst` mirroring `RecordInst`. `bynk.generics.generic_non_record` survives as the
error for a generic refined/opaque body, with a message that no longer names sums.
Generic methods, bounded parameters, higher-kinded parameters, and generic-sum
`embeds` remain reachable and unshipped.
