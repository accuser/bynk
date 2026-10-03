---
level: minor
changelog: Every unreachable `match` arm is now an error, not only one after a leading wildcard (`Some(_)` then `Some(Red)`, a trailing `_` after every variant, an or-pattern overlap; guarded arms never cover) (#1656). User names can no longer collide with emitted names (#1653): host globals are reached as `globalThis.X`, runtime imports and codec helpers carry a `__` prefix (`__JsonValue`, `__serialise_T`), shadowing parameters and pattern bindings get fresh names, and a payload field `tag` becomes `$tag` in TypeScript. A payload field named `kind` (`bynk.resolve.reserved_payload_field`) and a type named `globalThis` (`bynk.resolve.reserved_host_name`) are rejected
---

## ADR: emitted-name-hygiene
title: Emitted names cannot collide with Bynk names
summary: Host globals via `globalThis`, runtime and helper names under `__`, fresh names for shadowing binders, `$tag`; `kind` and `globalThis` reserved

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
   `globalThis.Promise<T>`). Protecting `globalThis` itself:
   - a value binding of that name (parameter, local, function) is renamed to
     `__id_globalThis` by the existing reserved-word mechanism (`ts_ident`);
   - a payload field of that name becomes the property `$globalThis`;
   - a type of that name is rejected (`bynk.resolve.reserved_host_name`).

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
  type named `globalThis`.
- Not covered: generated names derived from user names, such as `<Cap>Token`,
  `with<Arg>`, `Message` and `LocaleTag` in messages bundles. These are tracked
  in #1697.

Proved by:
- the behavioural fixture `1653_identifier_hygiene`, which declares a user type
  for each host global and several runtime names, functions named `console`,
  `crypto`, `matchPath`, `callService` and `serialise_Labelled`, a `tag`
  payload round-tripped through JSON, every shadowing form, `Int` division, a
  `Bytes` value, and an agent with store `Map`/`Set`. It passes `tsc --strict`
  on both targets;
- negatives `1653_payload_field_named_kind` and `1653_type_named_global_this`;
- the drift guard.

## ADR: unreachable-match-arms
title: A match arm that an earlier arm covers is an error
summary: Usefulness check over nested, or- and literal patterns; guarded arms and refined patterns never cover

**Context.** static-semantics says match arms MUST NOT be unreachable, but
`bynk.types.unreachable_arm` fired only after a leading wildcard. `Some(_)`
then `Some(Red)` was accepted, and its emitted narrowing failed `tsc`. A
defensive trailing `_` after every variant compiled silently.

**Decision.**
- An arm is unreachable, and an error, when every value its pattern matches is
  already matched by an earlier **unguarded** arm.
- The check recurses through nested payloads, or-patterns and literals, and is
  sound but bounded like exhaustiveness. A multi-field payload is covered only
  when one earlier arm covers it field by field. An arm is reported only when
  it is certainly unreachable.
- A guarded arm and a refined pattern (`n where P`) never cover a later arm,
  since the guard or predicate may fail.
- Duplicate arms keep their own codes (`duplicate_variant_arm`,
  `duplicate_literal_arm`).
- Severity is an error, matching the spec's MUST (issue Decision A).

**Consequences.** Some programs are newly rejected, including a defensive
trailing `_`. The diagnostic says why, and suggests removing the arm or moving
it above the arm that covers it.

Proved by negatives covering each shape in the issue, and the behavioural
fixture `1656_reachable_arms`, which shows that guarded duplicates and
narrow-before-wide arms stay legal.
