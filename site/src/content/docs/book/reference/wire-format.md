---
title: Wire format (version 1)
---
<!-- GENERATED FILE — do not edit by hand.
     Source: bynk-check/src/wire_format.rs and wire_format/reference.rs, via bynkc/tests/wire_reference.rs.
     Regenerate with: BYNK_BLESS=1 cargo test -p bynkc --test wire_reference -->

How one Bynk context calls another context's `on call` service over a
Cloudflare Service Binding: the request, the checks the callee makes, the
responses, the JSON encoding of every type that can cross, and the contract
hash. Code that is not Bynk can call a Bynk service by speaking this format.

This page is generated from the compiler. The header names, the check order
and the status codes are the constants the emitter builds the call dispatch
from; the encoding table is read from the compiler's wire model; and every
example below is run through the codecs the compiler generates for the
example context, by `bynkc/tests/wire_reference.rs`.

## Version and stability

This is wire format version **1**. It is versioned on its own, not
with the compiler: a compiler release changes it only by adding a new wire
version.

- **Within a version, changes are additive only.** A new status code, a new
  error `kind` or a new optional header may appear. A caller must treat an
  error `kind` it does not know as an error.
- **Anything else is a new version:** changing an encoding, a check, a status,
  an existing error body, or the contract hash's normal form or function.
- **The caller states its version** in `X-Bynk-Wire`; an absent header means
  `1`. **The callee accepts a range** (this compiler's callees accept
  `1`) and refuses any other version with the versions it accepts, so a
  version is retired after an overlap window, not on a flag day.
- **Authentication is the exception until per-edge signatures land.** Version 1
  fixes where authentication sits in the check order and what its refusal
  looks like. The credential itself, today the deployment's shared call
  secret, is replaced by a per-caller signature (the interop track's slice I2)
  without a new wire version. See [Authentication](#authentication).

What the format does **not** cover: the TypeScript the compiler emits, the
in-memory representation of values, and the Worker's other entry points
(`from http` routes, events, queues). Those follow the compiler's own version.

## Request

```text
POST /_bynk/call/<service>
```

`<service>` is the service's name as declared (`service quote` is
`/_bynk/call/quote`). The host is ignored: a Bynk caller sends
`http://internal/_bynk/call/<service>` over the Service Binding.

| Header | Required | Meaning |
|---|---|---|
| `X-Bynk-Call-Secret` | yes | The deployment's call secret (`BYNK_CALL_SECRET`). Compared in constant time. Its value, and this header, are replaced by a per-edge signature before non-Bynk callers are supported; see *Authentication* below. |
| `X-Bynk-Contract` | yes | The caller's hash of the service's contract, 16 lowercase hex digits. Must equal the callee's; a missing header does not. |
| `X-Bynk-Caller` | when the handler binds `by c: Caller` | The calling context's qualified name (`shop.orders`), which the handler reads as `c.identity`. Not authenticated beyond the call secret. |
| `X-Bynk-Wire` | no | The wire version the caller speaks, as a decimal integer. Absent means `1`. |
| `content-type` | no | Bynk callers send `application/json`. The callee does not check it. |

**Body.** JSON, encoding the handler's parameters:

- **no parameters:** any JSON value; Bynk callers send `{}`;
- **one parameter:** the parameter's value itself, not wrapped;
- **two or more:** an object keyed by parameter name. Unknown keys are ignored.

So `on call(line: Line, size: Qty)` takes `{"line": {…}, "size": 3}`, and
`on call(size: Qty)` takes `3`. Renaming or reordering parameters is a contract
change (see [Contract hash](#contract-hash)).

## Checks and responses

The callee makes these checks in this order. The first that fails answers the
request.

| # | Check | Status on failure | Body on failure |
|---|---|---|---|
| 1 | The method is `POST`. | `405` | empty body, `Allow: POST` |
| 2 | The call secret matches the callee's own. | `401` | text `Unauthorized` |
| 3 | `X-Bynk-Wire` is absent or names a version the callee accepts. | `400` | `{"kind":"UnsupportedWireVersion","accepted":[1]}` |
| 4 | The path names a service of the callee's context with an `on call` handler. | `404` | text `Not found` |
| 5 | `X-Bynk-Contract` equals the callee's hash of that service. | `409` | `{"kind":"ContractMismatch","service":"…","expected":"…","actual":"…"}` |
| 6 | The body parses as JSON. | `400` | `{"kind":"MalformedJson","details":"Invalid request body"}` |
| 7 | `X-Bynk-Caller` is present and non-empty. Checked only when the handler binds `by c: Caller`. | `401` | `{"kind":"Unauthorized","details":"missing caller identity"}` |
| 8 | The body decodes as the handler's parameters (see *Encoding*). | `400` | a `StructuralMismatch` or `RefinementViolation` body |

If every check passes, the handler runs:

- **`200`**, `content-type: application/json`, with the handler's return value
  encoded as below. A domain error is a `200` too: `Result[T, E]`'s `Err` is a
  value, not a status.
- **`500`**, text `Internal Server Error`, if the handler faults (including
  encoding a value the format cannot carry, such as an unsafe `Int`). The
  callee logs the fault; the body says nothing more.

A status not listed here (a platform error between the Workers, for example)
is a transport failure. A Bynk caller reports a `409` `ContractMismatch` by
name and every other non-`2xx` as `Transport`, with the status.

### Error bodies

Every JSON error body is an object with a `kind`:

| `kind` | Status | Fields |
|---|---|---|
| `UnsupportedWireVersion` | `400` | `accepted`: the wire versions the callee accepts |
| `ContractMismatch` | `409` | `service`, `expected` (the callee's hash), `actual` (the header as received) |
| `MalformedJson` | `400` | `details` |
| `Unauthorized` | `401` | `details` |
| `StructuralMismatch` | `400` | `path`, `expected`, `actual`: the value had the wrong shape |
| `RefinementViolation` | `400` | `path`, `violation`: `{"field", "message", "value"}`, a value of the right shape broke a predicate |

`path` is a JSONPath-like location in the body: `$` is the whole body (one
parameter), `$.<param>` a parameter (several), then `.<field>` or `[<index>]`
inward. An unknown variant fails at the sum's own `path`, with
`expected: "sum variant kind"` and the `kind` that was sent as `actual`.

## Encoding

Each type has one JSON encoding, the same in both directions. A value is
decoded by checking its shape, then any predicates.

### Base types

| Type | JSON | Decode accepts | Encode |
|---|---|---|---|
| `Int` | number (an integer) | JSON number, and `Number.isSafeInteger` (whole, within ±(2^53 − 1)); otherwise `StructuralMismatch` (`expected: "safe integer"`) | an unsafe value throws in the sender (a `500`), never a lossy number |
| `Float` | number (a number) | JSON number, and finite; otherwise `StructuralMismatch` (`expected: "finite number"`) | a non-finite value throws in the sender |
| `Bool` | boolean (`true` or `false`) | JSON boolean | as is |
| `String` | string (a string) | JSON string | as is |
| `Bytes` | string (standard base64 with padding (RFC 4648 §4)) | valid base64; anything else is `StructuralMismatch` (`expected: "base64 string"`) | as is |
| `Duration` | number (whole milliseconds) | JSON number, and `Number.isSafeInteger` (whole, within ±(2^53 − 1)); otherwise `StructuralMismatch` (`expected: "safe integer"`) | an unsafe value throws in the sender (a `500`), never a lossy number |
| `Instant` | number (milliseconds since the Unix epoch) | JSON number, and `Number.isSafeInteger` (whole, within ±(2^53 − 1)); otherwise `StructuralMismatch` (`expected: "safe integer"`) | an unsafe value throws in the sender (a `500`), never a lossy number |
| `()` | `null` | anything; the value is not inspected | `null` |

`Int`, `Duration` and `Instant` are limited to JavaScript's safe-integer range
on the wire, so every JSON parser reads them exactly.

### Composite types

| Type | JSON |
|---|---|
| record | an object keyed by field name. A missing field decodes as its default when it has one, as `None` when it is an `Option`, and otherwise fails. Unknown keys are ignored. |
| sum (`enum` included) | an object with `"kind"`: the variant's name, beside the variant's payload fields: `{"kind":"Circle","radius":0.5}`, `{"kind":"Dot"}` |
| `Result[T, E]` | `{"kind":"Ok","value":<T>}` or `{"kind":"Err","error":<E>}` |
| `Option[T]` | `{"kind":"Some","value":<T>}` or `{"kind":"None"}`. A sender may also send `null` or leave the field out for `None`, or send the bare value for `Some`; Bynk always encodes the tagged form |
| `List[T]` | an array |
| `Map[K, V]` | an array of `[key, value]` pairs, in insertion order |
| refined type (`type Qty = Int where NonNegative`) | its base type's encoding. The decoder checks the predicates, refusing with `RefinementViolation` |
| opaque type | its base type's encoding. A caller cannot see the predicate, so only the owning context checks it |
| inline field refinement (`qty: Int where Positive`) | the field's encoding. The decoder checks the predicates, like a refined type's |
| generic instantiation (`Page[Line]`) | the declaration's encoding with the arguments substituted |

The in-memory discriminant of a Bynk sum is `tag`; on the wire it is
`"kind"`. The encoding never depends on the in-memory layout.

### Examples

Each row is decoded by the codec the compiler generates for `wire.sample`
below, then encoded again. Rejections show the error's `kind` and `path`.

```bynk
context wire.sample

type Qty = Int where NonNegative

type Shape =
  | Circle(radius: Float)
  | Dot

type Line = {
  sku: String,
  qty: Int where Positive,
  wait: Duration,
  at: Instant,
  blob: Bytes,
  note: Option[String],
  tags: List[String],
  stock: Map[String, Int],
  rush: Bool,
}

service quote {
  on call(line: Line, size: Qty) -> Effect[Result[Shape, String]] {
    Ok(Dot)
  }
}

service ping {
  on call() -> Effect[Result[(), String]] {
    Ok(())
  }
}
```

| Type | JSON sent | Outcome | Note |
|---|---|---|---|
| `Line` | `{"sku":"A-1","qty":2,"wait":1500,"at":1767225600000,"blob":"aGk=","note":{"kind":"Some","value":"gift"},"tags":["new"],"stock":[["lon",3]],"rush":false}` | accepted; encodes back the same | Every field present. A record is an object keyed by field name. |
| `Line` | `{"sku":"A-1","qty":2,"wait":1500,"at":1767225600000,"blob":"","tags":[],"stock":[],"rush":true,"extra":1}` | accepted; encodes back as `{"sku":"A-1","qty":2,"wait":1500,"at":1767225600000,"blob":"","note":{"kind":"None"},"tags":[],"stock":[],"rush":true}` | An absent `Option` field is `None`. An unknown key is ignored. |
| `Line` | `{"sku":"A-1","qty":2,"wait":1500,"at":1767225600000,"blob":"","note":"gift","tags":[],"stock":[],"rush":true}` | accepted; encodes back as `{"sku":"A-1","qty":2,"wait":1500,"at":1767225600000,"blob":"","note":{"kind":"Some","value":"gift"},"tags":[],"stock":[],"rush":true}` | A bare value where an `Option` is expected is `Some`. |
| `Line` | `{"sku":"A-1","qty":0,"wait":1500,"at":1767225600000,"blob":"","tags":[],"stock":[],"rush":true}` | `RefinementViolation` at `$.qty` | An inline field refinement (`qty: Int where Positive`) is checked. |
| `Line` | `{"sku":"A-1","qty":1.5,"wait":1500,"at":1767225600000,"blob":"","tags":[],"stock":[],"rush":true}` | `StructuralMismatch` at `$.qty` | An `Int` is a whole number. |
| `Line` | `{"sku":"A-1","qty":2,"wait":1.5,"at":1767225600000,"blob":"","tags":[],"stock":[],"rush":true}` | `StructuralMismatch` at `$.wait` | A `Duration` is whole milliseconds. |
| `Line` | `{"sku":"A-1","qty":2,"wait":1500,"at":1767225600000,"blob":"%%","tags":[],"stock":[],"rush":true}` | `StructuralMismatch` at `$.blob` | `Bytes` is padded base64. |
| `Line` | `{"sku":"A-1","qty":2,"wait":1500,"at":1767225600000,"blob":"","tags":[],"stock":{"lon":3},"rush":true}` | `StructuralMismatch` at `$.stock` | A `Map` is an array of pairs, never an object. |
| `Qty` | `-1` | `RefinementViolation` at `$` | A named refined type is its base value, checked against its predicates. |
| `Shape` | `{"kind":"Circle","radius":0.5}` | accepted; encodes back the same | A variant's payload fields sit beside `kind`. |
| `Shape` | `{"kind":"Circle","radius":1e999}` | `StructuralMismatch` at `$.radius` | A `Float` is finite; `1e999` parses as infinity. |
| `Shape` | `{"kind":"Square"}` | `StructuralMismatch` at `$` | An unknown variant is refused. |
| `Result[Shape, String]` | `{"kind":"Ok","value":{"kind":"Dot"}}` | accepted; encodes back the same |  |
| `Result[Shape, String]` | `{"kind":"Err","error":"sold out"}` | accepted; encodes back the same | A domain error is a `200` carrying `Err`. |
| `Result[(), String]` | `{"kind":"Ok","value":null}` | accepted; encodes back the same | `()` is `null`. |

## Contract hash

`X-Bynk-Contract` carries a hash of the service's signature, so a callee can
refuse a caller that was compiled against a different version of it (`409`).
It detects skew; it is not a credential and not a security control.

**Normal form.** One line per service:

```text
<service>(<param>: <type>, …) -> <type>
```

- Parameters keep their names and order. `Effect[T]` is written as `T`.
- Base types are their names; `()` is `()`; `List[T]`, `Option[T]`,
  `Map[K, V]`, `Result[T, E]` keep their arguments in order.
- A named type is its name followed by its structure, so both a rename and a
  change of shape change the hash:
  - a record is `Name{f: T, …}`, each field written `f: T` (or
    `f: T where P, …` with an inline refinement), the fields sorted by that
    text's UTF-8 bytes and joined with `, `;
  - a sum is `Name|A|B(T, …)`: each variant written as its name, followed by
    its payload types in order in parentheses if it has any, the variants
    sorted by that text's UTF-8 bytes, each preceded by `|`;
  - a refined type is its name and base run together, then its predicates:
    `type Qty = Int where NonNegative` is `QtyInt where NonNegative` (with no
    predicates, `QtyInt ` with a trailing space);
  - an opaque type is `NameBase opaque`, without its predicates;
  - a generic instantiation is the declaration's form with the arguments
    substituted, followed by `[A, …]`;
  - a type met again inside its own expansion is `@Name`.
- Predicates are a set: written as below, sorted by their UTF-8 bytes, with
  duplicates removed, and joined with `, `. They are `Positive`,
  `NonNegative`, `InRange(a, b)`, `InRangeF(a, b)`, `MinLength(n)`,
  `MaxLength(n)`, `Length(n)` and `Matches("…")`; `NonEmpty` is written
  `MinLength(1)`. Bounds are written as decimal numbers in their shortest
  round-trip form (`01` is `1`). A `Matches` pattern is quoted, with `\`, `"`
  and control characters backslash-escaped.
- No spaces other than those shown; no doc comments, defaults, `given` or
  `by` clauses.

**Hash.** FNV-1a, 64-bit, over the normal form's UTF-8 bytes: start from the
offset basis `0xcbf29ce484222325`; for each byte, XOR it in, then
multiply by the prime `0x00000100000001b3` modulo 2^64. The result is
written as 16 lowercase hex digits.

For `wire.sample` above, each service's normal form, byte for byte, and its
hash:

`quote`, hash `e6b21871790fbcdb`:

```text
quote(line: Line{at: Instant, blob: Bytes, note: Option[String], qty: Int where Positive, rush: Bool, sku: String, stock: Map[String, Int], tags: List[String], wait: Duration}, size: QtyInt where NonNegative) -> Result[Shape|Circle(Float)|Dot, String]
```

`ping`, hash `619a25c88aaf9af8`:

```text
ping() -> Result[(), String]
```

| Changes the hash | Does not |
|---|---|
| the service's name; a parameter's name, order or type; the return type | field order; variant order; predicate order or duplicates |
| a field added, removed, renamed or retyped; a variant added, removed, renamed or re-shaped | an opaque type's predicates |
| a refined type's or an inline field refinement's predicates | doc comments; `given`; `by c: Caller` |
| renaming a named type | field defaults |

The normal form and the hash function are part of the wire version: a client
built once embeds hashes, so they change only with a new wire version.

## Authentication

A callee refuses a call without the deployment's call secret
(`X-Bynk-Call-Secret`, compared with the Worker's `BYNK_CALL_SECRET`) with a
bare `401`, before it looks at anything else. `bynk deploy` gives every Worker
in a deployment the same secret. So, today:

- **A Worker holding the secret can claim any caller.** `X-Bynk-Caller` is not
  authenticated beyond the secret, and the secret is shared by the whole
  deployment, so it does not tell one context from another.
- **Do not give the secret to code outside the deployment.** Giving a non-Bynk
  Worker the secret, or a Service Binding to a context together with it, gives
  that Worker every context's identity.

Per-edge signatures (the interop track's slice I2) replace the shared secret
with one key per caller and callee, signing the request; foreign callers are
supported only after that. Until then this format is published so that it can
be reviewed and built against, not so that non-Bynk code can be granted a call
secret.

## Not yet in version 1

- **A body-size limit.** The callee reads the whole body. A limit, refused with
  `413`, is planned; adding it is an additive change.
