# 0428 — Emitted names cannot collide with Bynk names

- **Status:** Accepted (v0.299)

**Context.** Before #1653, Bynk names went into TypeScript verbatim, in the
same module scope as everything the emitter adds: the in-memory discriminant
`tag`, runtime imports, codec helpers and host globals. Each program below
passed `bynkc check` and then failed `tsc --strict`:

- a payload field named `tag` or `kind`;
- a user type named `Record`, `Error`, `JSON` or `Number`, or `JsonValue` /
  `BoundaryError`;
- a function named `serialise_P` beside a type `P`;
- `if o is Some(o)` and `match o { Some(o) => … }`;
- a `let` re-binding a parameter.

The issue's Decision A preferred mangling at the emitter over reserving names
in the language. Measurement showed that renaming colliding user names would
also have to rewrite every reference, import and export to them. Qualifying the
emitter's own references instead leaves every user-visible name alone.

**Decision.**

1. **Host globals** are written as `globalThis.<name>` in every emitted module
   (`globalThis.JSON.stringify`, `new globalThis.Error(…)`,
   `globalThis.Promise<T>`, `new globalThis.Set(…)`). `String` is the one
   global left bare: it is a Bynk keyword, so nothing can declare it.
   Protecting `globalThis` itself:
   - a parameter or local of that name is renamed to `__id_globalThis` by the
     existing reserved-word mechanism (`ts_ident`), as is a constructor
     parameter for a payload field of that name;
   - a module-scope declaration of that name (a type, function, agent,
     provider, capability, service, actor or event) is rejected
     (`bynk.resolve.reserved_host_name`): several of these are emitted under
     their own name without passing through `ts_ident`.

   A capability's injection token is typed `symbol` rather than
   `unique symbol`, because TypeScript only gives a bare `Symbol(…)` call a
   unique type.
2. **Runtime names.**
   - The runtime exports each name an emitted module imports under a `__`
     alias as well (`__JsonValue`, `__matchPath`; `runtime/src/aliases.ts`),
     and emitted code imports only the alias.
   - The codec helpers are `__serialise_<T>` / `__deserialise_<T>`.
   - A Bynk identifier cannot begin with `_`, so a program cannot spell any of
     these names.
   - Names the language already reserves (`Ok`, `Result`, `HttpResult`, …) are
     unchanged.
3. **Shadowing.** Parameters are registered as bindings of the body's scope, and
   a pattern binding (`match` arm or `is`) gets a fresh emitted name when it
   re-binds a name already in scope, exactly as a `let` re-binding does.
4. **Payload fields.**
   - A payload field named `tag` is emitted as the property `$tag`. Its wire key
     stays `tag`, and the codec maps between the two.
   - The variant constructor binds each field as a parameter, which passes
     through `ts_ident`, so a field named like a reserved word (`class`,
     `arguments`, `deps`) keeps its property and wire name.
   - On the wire a variant is a flat `{ "kind": "<Variant>", … }` object, so a
     payload field named `kind` cannot keep its name. It is rejected
     (`bynk.resolve.reserved_payload_field`) rather than given a wire key that
     differs from the field.
5. **Drift guard.** `bynkc/tests/host_globals_qualified.rs` fails if a blessed
   unit module references a host global without `globalThis.`.

**Consequences.**
- Every golden output changes mechanically (`Promise<…>` → `globalThis.Promise<…>`,
  `JsonValue` → `__JsonValue`, `serialise_T` → `__serialise_T`). Programs that
  compiled before compile to the same behaviour.
- Two kinds of program are newly rejected: a payload field named `kind`, and a
  declaration named `globalThis`.
- Not covered: generated names derived from user names, such as `<Cap>Token`,
  `with<Arg>`, `Message` and `LocaleTag` in messages bundles. These are tracked
  in #1697.

Proved by:
- the behavioural fixture `1653_identifier_hygiene`, which declares a user type
  for each host global and several runtime names, functions named `console`,
  `crypto`, `matchPath`, `callService`, `serialise_Labelled`, `Set` and `Map`
  beside code that builds a JS `Set`/`Map`, a `tag` payload and payload fields
  named `class`/`arguments`/`deps` round-tripped through JSON, every shadowing
  form, `Int` division, a `Bytes` value, and an agent with store `Map`/`Set`.
  It passes `tsc --strict` on both targets;
- negatives `1653_payload_field_named_kind`, `1653_type_named_global_this` and
  `1653_agent_named_global_this`;
- the drift guard.
