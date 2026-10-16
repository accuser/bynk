//! #1969: renders `reference/wire-format.md`, the published reference for wire
//! format version 1.
//!
//! Every fact the page states that the compiler also uses is read from the
//! compiler: the version, headers and check order from [`super`], the base-type
//! encodings from [`crate::wire`], and the contract hash from
//! [`crate::contract`], computed on [`EXAMPLE_SOURCE`]. The page's examples
//! ([`EXAMPLES`]) are executed: `bynkc/tests/wire_reference.rs` compiles
//! [`EXAMPLE_SOURCE`] to Workers, runs every example through the generated
//! codecs on Node, and checks the emitted hashes and dispatch against this page.
//! Regenerate the page with
//! `BYNK_BLESS=1 cargo test -p bynkc --test wire_reference`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use bynk_syntax::ast::{BaseType, CommonsItem, SourceUnit, TypeDecl};

use super::{
    ACCEPTED_WIRE_VERSIONS, CALL_CHECKS, CALL_METHOD, CALL_PATH_PREFIX, CallCheck, HEADERS,
    WIRE_HEADER, WIRE_VERSION,
};
use crate::contract::{FNV1A64_OFFSET, FNV1A64_PRIME, contract_hash, service_normal_form};
use crate::resolver::cross_context_service_for;
use crate::wire::{BaseGuard, JsonKind, WIRE_DISCRIMINANT, field_base_guards, json_kind_of};

/// The context the page's examples and worked hash are computed on.
pub const EXAMPLE_SOURCE: &str = r#"context wire.sample

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
"#;

/// What decoding an example's JSON does.
pub enum Outcome {
    /// Decodes, and the decoded value encodes back as this JSON.
    Accepted { encodes_as: &'static str },
    /// Refused with this error `kind` at this `path`.
    Rejected {
        kind: &'static str,
        path: &'static str,
    },
}

/// One executed example: `json` decoded as `ty`.
pub struct Example {
    /// The Bynk type, as the page shows it.
    pub ty: &'static str,
    /// The generated codec suffix the test drives (`__deserialise_<codec>`).
    pub codec: &'static str,
    pub json: &'static str,
    pub outcome: Outcome,
    pub note: &'static str,
}

const LINE: &str = r#"{"sku":"A-1","qty":2,"wait":1500,"at":1767225600000,"blob":"aGk=","note":{"kind":"Some","value":"gift"},"tags":["new"],"stock":[["lon",3]],"rush":false}"#;

/// The page's examples, each run through the generated codecs by
/// `bynkc/tests/wire_reference.rs`.
pub const EXAMPLES: &[Example] = &[
    Example {
        ty: "Line",
        codec: "Line",
        json: LINE,
        outcome: Outcome::Accepted { encodes_as: LINE },
        note: "Every field present. A record is an object keyed by field name.",
    },
    Example {
        ty: "Line",
        codec: "Line",
        json: r#"{"sku":"A-1","qty":2,"wait":1500,"at":1767225600000,"blob":"","tags":[],"stock":[],"rush":true,"extra":1}"#,
        outcome: Outcome::Accepted {
            encodes_as: r#"{"sku":"A-1","qty":2,"wait":1500,"at":1767225600000,"blob":"","note":{"kind":"None"},"tags":[],"stock":[],"rush":true}"#,
        },
        note: "An absent `Option` field is `None`. An unknown key is ignored.",
    },
    Example {
        ty: "Line",
        codec: "Line",
        json: r#"{"sku":"A-1","qty":2,"wait":1500,"at":1767225600000,"blob":"","note":"gift","tags":[],"stock":[],"rush":true}"#,
        outcome: Outcome::Accepted {
            encodes_as: r#"{"sku":"A-1","qty":2,"wait":1500,"at":1767225600000,"blob":"","note":{"kind":"Some","value":"gift"},"tags":[],"stock":[],"rush":true}"#,
        },
        note: "A bare value where an `Option` is expected is `Some`.",
    },
    Example {
        ty: "Line",
        codec: "Line",
        json: r#"{"sku":"A-1","qty":0,"wait":1500,"at":1767225600000,"blob":"","tags":[],"stock":[],"rush":true}"#,
        outcome: Outcome::Rejected {
            kind: "RefinementViolation",
            path: "$.qty",
        },
        note: "An inline field refinement (`qty: Int where Positive`) is checked.",
    },
    Example {
        ty: "Line",
        codec: "Line",
        json: r#"{"sku":"A-1","qty":1.5,"wait":1500,"at":1767225600000,"blob":"","tags":[],"stock":[],"rush":true}"#,
        outcome: Outcome::Rejected {
            kind: "StructuralMismatch",
            path: "$.qty",
        },
        note: "An `Int` is a whole number.",
    },
    Example {
        ty: "Line",
        codec: "Line",
        json: r#"{"sku":"A-1","qty":2,"wait":1.5,"at":1767225600000,"blob":"","tags":[],"stock":[],"rush":true}"#,
        outcome: Outcome::Rejected {
            kind: "StructuralMismatch",
            path: "$.wait",
        },
        note: "A `Duration` is whole milliseconds.",
    },
    Example {
        ty: "Line",
        codec: "Line",
        json: r#"{"sku":"A-1","qty":2,"wait":1500,"at":1767225600000,"blob":"%%","tags":[],"stock":[],"rush":true}"#,
        outcome: Outcome::Rejected {
            kind: "StructuralMismatch",
            path: "$.blob",
        },
        note: "`Bytes` is padded base64.",
    },
    Example {
        ty: "Line",
        codec: "Line",
        json: r#"{"sku":"A-1","qty":2,"wait":1500,"at":1767225600000,"blob":"","tags":[],"stock":{"lon":3},"rush":true}"#,
        outcome: Outcome::Rejected {
            kind: "StructuralMismatch",
            path: "$.stock",
        },
        note: "A `Map` is an array of pairs, never an object.",
    },
    Example {
        ty: "Qty",
        codec: "Qty",
        json: "-1",
        outcome: Outcome::Rejected {
            kind: "RefinementViolation",
            path: "$",
        },
        note: "A named refined type is its base value, checked against its predicates.",
    },
    Example {
        ty: "Shape",
        codec: "Shape",
        json: r#"{"kind":"Circle","radius":0.5}"#,
        outcome: Outcome::Accepted {
            encodes_as: r#"{"kind":"Circle","radius":0.5}"#,
        },
        note: "A variant's payload fields sit beside `kind`.",
    },
    Example {
        ty: "Shape",
        codec: "Shape",
        json: r#"{"kind":"Circle","radius":1e999}"#,
        outcome: Outcome::Rejected {
            kind: "StructuralMismatch",
            path: "$.radius",
        },
        note: "A `Float` is finite; `1e999` parses as infinity.",
    },
    Example {
        ty: "Shape",
        codec: "Shape",
        json: r#"{"kind":"Square"}"#,
        outcome: Outcome::Rejected {
            kind: "StructuralMismatch",
            path: "$",
        },
        note: "An unknown variant is refused.",
    },
    Example {
        ty: "Result[Shape, String]",
        codec: "Result_Shape_String",
        json: r#"{"kind":"Ok","value":{"kind":"Dot"}}"#,
        outcome: Outcome::Accepted {
            encodes_as: r#"{"kind":"Ok","value":{"kind":"Dot"}}"#,
        },
        note: "",
    },
    Example {
        ty: "Result[Shape, String]",
        codec: "Result_Shape_String",
        json: r#"{"kind":"Err","error":"sold out"}"#,
        outcome: Outcome::Accepted {
            encodes_as: r#"{"kind":"Err","error":"sold out"}"#,
        },
        note: "A domain error is a `200` carrying `Err`.",
    },
    Example {
        ty: "Result[(), String]",
        codec: "Result_Unit_String",
        json: r#"{"kind":"Ok","value":null}"#,
        outcome: Outcome::Accepted {
            encodes_as: r#"{"kind":"Ok","value":null}"#,
        },
        note: "`()` is `null`.",
    },
];

/// The page's worked contract-hash example: each `on call` service of
/// [`EXAMPLE_SOURCE`] with its normal form and hash, in source order.
pub fn example_contracts() -> Vec<(String, String, String)> {
    let tokens = bynk_syntax::lexer::tokenize(EXAMPLE_SOURCE).expect("the example source lexes");
    let unit = match bynk_syntax::parser::parse_unit(&tokens, EXAMPLE_SOURCE) {
        Ok(SourceUnit::Context(c)) => c,
        other => panic!("the example source is a context: {other:?}"),
    };
    let types: HashMap<String, Arc<TypeDecl>> = unit
        .items
        .iter()
        .filter_map(|i| match i {
            CommonsItem::Type(t) => Some((t.name.name.clone(), Arc::new(t.clone()))),
            _ => None,
        })
        .collect();
    unit.items
        .iter()
        .filter_map(|i| match i {
            CommonsItem::Service(s) => cross_context_service_for(&s.name.name, s),
            _ => None,
        })
        .map(|svc| {
            let form = service_normal_form(&svc, &types);
            let hash = contract_hash(&form);
            (svc.name, form, hash)
        })
        .collect()
}

fn json_kind_name(k: JsonKind) -> &'static str {
    match k {
        JsonKind::Number => "number",
        JsonKind::String => "string",
        JsonKind::Boolean => "boolean",
        JsonKind::Object => "object",
        JsonKind::Array => "array",
        JsonKind::Null => "null",
    }
}

/// The base-type rows of the encoding table, read off [`crate::wire`]'s model:
/// the JSON kind each base type wires as and the guards its decode applies.
fn base_type_rows(out: &mut String) {
    const BASES: [(BaseType, &str); 7] = [
        (BaseType::Int, "an integer"),
        (BaseType::Float, "a number"),
        (BaseType::Bool, "`true` or `false`"),
        (BaseType::String, "a string"),
        (
            BaseType::Bytes,
            "standard base64 with padding (RFC 4648 §4)",
        ),
        (BaseType::Duration, "whole milliseconds"),
        (BaseType::Instant, "milliseconds since the Unix epoch"),
    ];
    for (base, value) in BASES {
        let kind = json_kind_of(base);
        let rule = if base == BaseType::Bytes {
            "valid base64; anything else is `StructuralMismatch` (`expected: \"base64 string\"`)"
                .to_string()
        } else {
            let mut rule = format!("JSON {}", json_kind_name(kind));
            for guard in field_base_guards(base) {
                rule.push_str(match guard {
                    BaseGuard::Integral => {
                        ", and `Number.isSafeInteger` (whole, within ±(2^53 − 1)); otherwise `StructuralMismatch` (`expected: \"safe integer\"`)"
                    }
                    BaseGuard::Finite => {
                        ", and finite; otherwise `StructuralMismatch` (`expected: \"finite number\"`)"
                    }
                });
            }
            rule
        };
        let encode = match base {
            BaseType::Int | BaseType::Duration | BaseType::Instant => {
                "an unsafe value throws in the sender (a `500`), never a lossy number"
            }
            BaseType::Float => "a non-finite value throws in the sender",
            _ => "as is",
        };
        let _ = writeln!(
            out,
            "| `{}` | {} ({value}) | {rule} | {encode} |",
            base.name(),
            json_kind_name(kind),
        );
    }
}

fn status_rows(out: &mut String) {
    for (i, check) in CALL_CHECKS.iter().enumerate() {
        let response = match check {
            CallCheck::WireVersion => format!(
                "`{{\"kind\":\"{}\",\"accepted\":[{}]}}`",
                check.kind().unwrap_or_default(),
                ACCEPTED_WIRE_VERSIONS
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            _ => check.response().to_string(),
        };
        let _ = writeln!(
            out,
            "| {} | {} | `{}` | {response} |",
            i + 1,
            check.condition(),
            check.status(),
        );
    }
}

/// The whole page, in mdBook form (`# H1` title, relative `.md` links).
pub fn render_reference() -> String {
    let mut out = String::new();
    let accepted = ACCEPTED_WIRE_VERSIONS
        .iter()
        .map(|v| format!("`{v}`"))
        .collect::<Vec<_>>()
        .join(", ");

    let _ = write!(
        out,
        r#"# Wire format (version {WIRE_VERSION})

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

This is wire format version **{WIRE_VERSION}**. It is versioned on its own, not
with the compiler: a compiler release changes it only by adding a new wire
version.

- **Within a version, changes are additive only.** A new status code, a new
  error `kind` or a new optional header may appear. A caller must treat an
  error `kind` it does not know as an error.
- **Anything else is a new version:** changing an encoding, a check, a status,
  an existing error body, or the contract hash's normal form or function.
- **The caller states its version** in `{WIRE_HEADER}`; an absent header means
  `1`. **The callee accepts a range** (this compiler's callees accept
  {accepted}) and refuses any other version with the versions it accepts, so a
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
{CALL_METHOD} {CALL_PATH_PREFIX}<service>
```

`<service>` is the service's name as declared (`service quote` is
`{CALL_PATH_PREFIX}quote`). The host is ignored: a Bynk caller sends
`http://internal{CALL_PATH_PREFIX}<service>` over the Service Binding.

| Header | Required | Meaning |
|---|---|---|
"#
    );
    for h in HEADERS {
        let _ = writeln!(out, "| `{}` | {} | {} |", h.name, h.required, h.meaning);
    }
    let _ = write!(
        out,
        r#"
**Body.** JSON, encoding the handler's parameters:

- **no parameters:** any JSON value; Bynk callers send `{{}}`;
- **one parameter:** the parameter's value itself, not wrapped;
- **two or more:** an object keyed by parameter name. Unknown keys are ignored.

So `on call(line: Line, size: Qty)` takes `{{"line": {{…}}, "size": 3}}`, and
`on call(size: Qty)` takes `3`. Renaming or reordering parameters is a contract
change (see [Contract hash](#contract-hash)).

## Checks and responses

The callee makes these checks in this order. The first that fails answers the
request.

| # | Check | Status on failure | Body on failure |
|---|---|---|---|
"#
    );
    status_rows(&mut out);
    let _ = write!(
        out,
        r#"
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
| `RefinementViolation` | `400` | `path`, `violation`: `{{"field", "message", "value"}}`, a value of the right shape broke a predicate |

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
"#
    );
    base_type_rows(&mut out);
    let _ = write!(
        out,
        r#"| `()` | `null` | anything; the value is not inspected | `null` |

`Int`, `Duration` and `Instant` are limited to JavaScript's safe-integer range
on the wire, so every JSON parser reads them exactly.

### Composite types

| Type | JSON |
|---|---|
| record | an object keyed by field name. A missing field decodes as its default when it has one, as `None` when it is an `Option`, and otherwise fails. Unknown keys are ignored. |
| sum (`enum` included) | an object with `"{WIRE_DISCRIMINANT}"`: the variant's name, beside the variant's payload fields: `{{"{WIRE_DISCRIMINANT}":"Circle","radius":0.5}}`, `{{"{WIRE_DISCRIMINANT}":"Dot"}}` |
| `Result[T, E]` | `{{"{WIRE_DISCRIMINANT}":"Ok","value":<T>}}` or `{{"{WIRE_DISCRIMINANT}":"Err","error":<E>}}` |
| `Option[T]` | `{{"{WIRE_DISCRIMINANT}":"Some","value":<T>}}` or `{{"{WIRE_DISCRIMINANT}":"None"}}`. A sender may also send `null` or leave the field out for `None`, or send the bare value for `Some`; Bynk always encodes the tagged form |
| `List[T]` | an array |
| `Map[K, V]` | an array of `[key, value]` pairs, in insertion order |
| refined type (`type Qty = Int where NonNegative`) | its base type's encoding. The decoder checks the predicates, refusing with `RefinementViolation` |
| opaque type | its base type's encoding. A caller cannot see the predicate, so only the owning context checks it |
| inline field refinement (`qty: Int where Positive`) | the field's encoding. The decoder checks the predicates, like a refined type's |
| generic instantiation (`Page[Line]`) | the declaration's encoding with the arguments substituted |

The in-memory discriminant of a Bynk sum is `tag`; on the wire it is
`"{WIRE_DISCRIMINANT}"`. The encoding never depends on the in-memory layout.

### Examples

Each row is decoded by the codec the compiler generates for `wire.sample`
below, then encoded again. Rejections show the error's `kind` and `path`.

```bynk
"#
    );
    out.push_str(EXAMPLE_SOURCE.trim_end());
    out.push_str("\n```\n\n| Type | JSON sent | Outcome | Note |\n|---|---|---|---|\n");
    for ex in EXAMPLES {
        let outcome = match &ex.outcome {
            Outcome::Accepted { encodes_as } if *encodes_as == ex.json => {
                "accepted; encodes back the same".to_string()
            }
            Outcome::Accepted { encodes_as } => {
                format!("accepted; encodes back as `{encodes_as}`")
            }
            Outcome::Rejected { kind, path } => format!("`{kind}` at `{path}`"),
        };
        let _ = writeln!(
            out,
            "| `{}` | `{}` | {outcome} | {} |",
            ex.ty, ex.json, ex.note
        );
    }

    let contracts = example_contracts();
    let _ = write!(
        out,
        r#"
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
  - a record is `Name{{f: T, …}}`, each field written `f: T` (or
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
offset basis `0x{FNV1A64_OFFSET:016x}`; for each byte, XOR it in, then
multiply by the prime `0x{FNV1A64_PRIME:016x}` modulo 2^64. The result is
written as 16 lowercase hex digits.

For `wire.sample` above, each service's normal form, byte for byte, and its
hash:
"#
    );
    for (name, form, hash) in &contracts {
        let _ = write!(out, "\n`{name}`, hash `{hash}`:\n\n```text\n{form}\n```\n");
    }
    let _ = write!(
        out,
        r#"
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
"#
    );
    out
}
