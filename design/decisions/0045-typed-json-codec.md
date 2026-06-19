# 0045 — The typed JSON codec: compiler-backed, no untyped `Json`

- **Status:** Accepted (v0.22b)
- **Spec:** §4.6.8, §5.2, §7.3.9

## Context
Programs need JSON at the edges Bynk doesn't own (request bodies built by
hand, stored blobs, third-party payloads). An untyped `Json` value type
with navigation is against the grain of a language whose whole posture is
typed boundaries; the boundary codec machinery (`serialise_<T>` /
`deserialise_<T>`) already exists per type.

## Decision
`Json.encode(v) -> String` and `Json.decode[T](s) -> Result[T, JsonError]`
as **compiler-backed statics** on a built-in `Json` module: `encode`
dispatches to `serialise_<typeof v>` + `JSON.stringify`; `decode[T]` to
`JSON.parse` + `deserialise_<T>`. It must be compiler-known — an erased
generic function cannot conjure the per-type deserialiser. No untyped
`Json` value ships.

- **Domain**: any boundary-legal shape (bases, named types, the built-in
  containers over them — `Json.decode[List[Order]]` works); functions,
  effects, `HttpResult`, the error builtins, and type variables are
  `bynk.types.json_uncodable`.
- **`decode[T]` forces type application on qualified statics** — the
  v0.20b open item (0039). `MethodCall` gains `type_args` (the proposal
  guessed `ConstructorCall`; the parser never builds that node — statics
  are `MethodCall`s with an identifier receiver), under the same
  same-line-`[` rule. Only `Json.decode` consumes them; elsewhere
  `bynk.generics.type_arg_mismatch`. `T` may also be inferred from an
  expected `Result[T, JsonError]`; with neither,
  `bynk.generics.uninferable_type_arg`.
- **`encode` is not total** (0040): serialising a value containing a
  non-finite `Float` throws (a contract violation) — documented
  normatively, not `Result`-ified, since the program itself created that
  state and it matches the boundary posture.
- **Helpers are emitted module-locally** into each module that calls the
  codec (bundle modules previously had no serialisation machinery at
  all), reusing the boundary-closure collectors and deduping against
  workers boundary helpers. Codec runtime imports are conditional, so
  non-codec modules emit byte-identically.

## Consequences
JSON handling is typed end to end; there is no partially-typed escape
hatch to grow around. The first generic-ish type application on a static
exists, with the user-methods generalisation still cleanly deferred.
