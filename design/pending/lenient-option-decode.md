---
level: minor
changelog: "**An `Option` decodes from absent, `null` or a bare value** ([#1887](https://github.com/accuser/bynk/issues/1887)). Decoding an `Option[U]` now accepts an absent key and `null` as `None`, and a bare value as `Some`, checked and refinement-checked as `U`, as well as Bynk's tagged `{\"kind\": \"Some\", \"value\": …}` / `{\"kind\": \"None\"}`. It is the one `Option` codec, so the rule holds for `Json.decode`, HTTP request bodies, cross-context calls, agent-store rehydration and WebSocket frames. Encoding is unchanged, so contract hashes and stored payloads are unaffected. The tagged forms win where a value could be read both ways, so whatever the encoder writes decodes back to itself."
---

## ADR: lenient-option-decode
title: An Option decodes leniently from absent, null, a bare value or the tagged form
summary: Amends 0045; Option decoding accepts the forms non-Bynk JSON uses, tagged forms first

**Context.** [ADR 0045](0045-typed-json-codec.md) made `Json.decode[T]` a
compiler-backed static over the per-type boundary codecs. Those codecs decoded
an `Option[U]` only from the tagged form the encoder writes: `{"kind": "Some",
"value": …}` or `{"kind": "None"}`. JSON that Bynk did not write marks an
optional value differently. It leaves the key out, sets it to `null`, or gives
the bare value. Each of these failed the whole document (#1887), so a Bynk
program could not model an optional field of a JSON Schema, an OpenAPI
response, a webhook payload or a config file. The record codecs already ignore
keys a record does not declare, so open-world reading worked except for
optional fields.

**Decision.** `__deserialise_Option_<U>` accepts four forms, tried in order:

1. `undefined` or `null` is `None`. An absent key reaches the decoder as
   `undefined`, because a record codec reads `obj["f"]` with no presence check.
2. An object whose only key is `kind`, set to `"None"`, is `None`.
3. An object whose only keys are `kind` and `value`, with `kind` set to
   `"Some"`, is `Some(U(value))`. Its errors are reported at `<path>.value`.
4. Any other value is bare and becomes `Some(U(json))`, decoded and
   refinement-checked as `U`.

Encoding is unchanged. The encoder still writes only the tagged form, so
contract hashes do not move and every payload written before this change still
decodes. There is one `Option` codec, so the rule holds at every boundary that
decodes JSON: `Json.decode`, an HTTP request body, a cross-context call,
agent-store rehydration and a WebSocket frame. No boundary can disagree with
another.

*Ambiguity.* The tagged forms are tried before the bare one and win when a
value could be read both ways. Recognition is exact: the object must have only
`kind`, or only `kind` and `value`. This keeps `decode(encode(x)) == x` for
every `x`, because the encoder always wraps:

- `Some(None)` of an `Option[Option[T]]` encodes as `{"kind": "Some", "value":
  {"kind": "None"}}`. The outer decoder sees an exact tagged `Some`, and the
  inner one sees an exact tagged `None`.
- `Some(r)`, where `r` is a record with `kind` and `value` fields or a sum
  variant with a `value` payload, wraps `r` in an outer tag. The inner decoder
  therefore sees `r` intact.

The cost falls only on JSON that Bynk did not write, and it is pinned in tests:

- A bare `{"kind": "None"}` for an `Option[Option[T]]` is `None`, not
  `Some(None)`.
- A bare `1` is `Some(Some(1))`.
- A bare `{"kind": "Some", "value": 1}` for an `Option[{kind: String, value:
  Int}]` is read as the tagged form, so decoding `1` as the record fails.
- A bare `{"kind": "Wrap", "value": 3}` for an `Option` of a sum with a
  `Wrap(value: Int)` variant is not exact-tagged (its `kind` is neither `Some`
  nor `None`), so it decodes as `Some(Wrap(3))`.

*Events.* An event field default still rescues only an absent key (the `"f" in
obj` test of #972). An explicit `null` is now a present key, so it decodes to
`None` rather than the default, just as an explicit `{"kind": "None"}` always
has.

*Errors.* A bare value that fails as a `StructuralMismatch` at the `Option`'s
own path has its `expected` extended with every accepted form, for example
`expected string | null | {"kind": "None"} | {"kind": "Some", "value": ...},
got number`. A deeper error, or a `RefinementViolation`, is returned as `U`
reported it, because rewriting it would point at the wrong place.

*Emission.* `U`'s checks are emitted once, in a module-local
`__option_value_<U>(raw, at)` that rules 3 and 4 share. It is not exported, so a
barrel's `export *` cannot collide on it.

**Consequences.** A Bynk service can model optional fields of external JSON
directly, including through an HTTP request body from a non-Bynk client. An
opt-in encoding (absent or `null` for `None`, and the bare value for `Some`)
for APIs that other software consumes is a separate decision and is not made
here. Every emitted `Option` decoder grows by a small module-local helper, so
the positive-fixture goldens were re-blessed.
