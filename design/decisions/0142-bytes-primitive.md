# 0142 — `Bytes` is a distinct base type for binary data, erased to `Uint8Array`; content equality (not host `===`); base64 JSON string on the wire; not `Map`-keyable

- **Status:** Accepted (v0.110; 2026-07-01)
- **Provenance:** the v0.110 `Bytes` increment — the shared prerequisite under every binary platform surface (R2 objects, binary HTTP bodies, `Stream[Bytes]` downloads), built alone. Those consumers are named follow-ons, out of this increment.
- **Realises:** `bynk-type-system.md` §1.1, which already listed `Bytes` in the primitive set but shipped it unbuilt — the spec/impl primitive divergence flagged in `bynk-status-and-roadmap.md`. Building `Bytes` closes that gap for binary data (`Decimal` remains the spec-name divergence for the built `Float`).
- **Relates:** ADR 0040 (`Float` is a distinct base type erased to `number` — `Bytes` follows the distinct-base-type playbook, but is the first base type **not** erased to `number`); ADR 0112 (`Duration`) and ADR 0114 (`Instant`) — same "no source literal, explicit construction" shape; ADR 0048 (partial parses return `Option[T]` — the precedent for `fromBase64`/`decodeUtf8`); ADR 0038 (`Map` keys are value-keyable — `Bytes` is the first equatable base type it excludes); ADR 0136 (strip-only emission invariant — the `Uint8Array` erase target is a plain runtime value, so `Bytes` is strip-only-clean).

## Context

Bynk had no representation for arbitrary binary data. The only "bag of bytes"
available was `String`, which is UTF-8 text — feeding it raw octets (an image, a
gzip blob, an HMAC digest, an R2 object) corrupts them at the first encode
boundary. So the entire class of binary platform surfaces was unreachable, and
the type-system spec already conceded the gap by listing a `Bytes` primitive it
never built.

`Float`/`Duration`/`Instant` set the playbook: a distinct base type the checker
refuses to confuse with its neighbours, erased to the host's natural
representation. `Bytes` follows it — with one genuine departure. Those three all
erase to `number`, whose `==` is host `===` (value equality on a JS primitive).
`Bytes` erases to a `Uint8Array`, where `===` is **reference** equality, so
content equality is real emitter work, not a kernel-table row. That departure
ripples into keyability and boundary-crossing.

## Decisions

**D1 — `Bytes` is a seventh base type, erased to `Uint8Array`.** It joins
`Int`/`String`/`Bool`/`Float`/`Duration`/`Instant` as a `BaseType`, but unlike
all of them it does **not** lower to `number` — a `Bytes` lowers to a TS
`Uint8Array`, an immutable finite octet sequence at the Bynk level. Usable
anywhere a type is written. The `Uint8Array` erase target is a plain runtime
value, so `Bytes` is strip-only-clean (ADR 0136 untouched).

**D2 — No source literal; construction is explicit, mirroring `Instant`.**
`Bytes.fromUtf8(s: String) -> Bytes` (total, the UTF-8 encoding),
`Bytes.fromBase64(s: String) -> Option[Bytes]` (partial, `None` on invalid
base64), and `Bytes.empty() -> Bytes` (the zero value). No parser/lexer/tree-
sitter change beyond the keyword — exactly `Instant`'s shape.

**D3 — String interop is the load-bearing surface: UTF-8 and base64, both
directions.** `b.decodeUtf8() -> Option[String]` (partial, `None` on invalid
UTF-8) and `b.toBase64() -> String` (total). With `fromUtf8`/`fromBase64` this is
the full round-trip: encoding (text → bytes) is total; decoding (bytes → text) is
partial, and the partiality is surfaced as `Option`, not hidden.

**D4 — Length and content equality; no arithmetic, not orderable (v1). Equality
is the one piece of genuinely new emitter work.** `b.length() -> Int` is the octet
count. `==`/`!=` compare **by content**, byte for byte: at any equality site whose
operands are statically `Bytes`, the emitter lowers to a `__bynkBytesEqual`
content-compare instead of the bare `===`, exactly as division is already
operand-typed. A record or sum carrying a `Bytes` field gets correct equality when
its (hand-written, per the established record-equality idiom) field comparator
compares that field with `==` — the inner `==` then does a content compare. We do
**not** synthesise general structural equality for records/sums (whole-record `==`
remains reference equality today; changing that is a separate decision). `Bytes`
is **not** orderable (no `<`/`sortBy` key) and has **no** arithmetic or
concatenation in v1; lexicographic ordering, `concat`, `slice`, `fromHex`/`toHex`,
and content-`distinct` are named follow-ons (none needed by the motivating
consumers).

**D5 — Codec: a `Bytes` is a base64 JSON string on the wire; zero is `empty`.**
JSON has no binary type, so a `Bytes` serialises as a base64-encoded JSON string
and deserialises requiring a valid base64 string (an invalid or non-string wire
value is rejected, as `Instant` rejects a non-integer). A `Bytes` in a record or
`store` field round-trips. Its implicit zero is `Bytes.empty` (`""` in base64). A
`Bytes` and a `String` field both appear as JSON strings but are distinct Bynk
types decoded by different rules — the same harmless surface coincidence as
`Instant`/`Int` both being JSON numbers.

**D6 — `Bytes` is a fully ordinary, serialisable value — the opposite of
`Stream`/`Connection`.** A `Bytes` is a finite immutable blob: it **is**
serialisable (D5), **is** storable in any `store` kind, and **may** cross a
context boundary — it sits with `Int`/`String` in the *serialisable* set. The
motivation is binary *streaming*, but only the streaming is special: a live
`Stream[Bytes]` is non-serialisable because it is a `Stream`; its *element*
`Bytes` is an everyday value carrying no linearity, lifetime, or held-resource
discipline.

**D7 — `Bytes` is equatable but not `Map`-keyable; the keyable set stays
`String`/`Int`.** ADR 0038 confines `Map` keys to value-keyable types on the
premise that "branded primitives keep JS value equality." `Bytes` is the first
base type to break that premise: equatable (D4) yet neither JS-value-equal nor
orderable. So `Map[Bytes, V]` is rejected with `bynk.types.unkeyable_map_key`, and
`Bytes` is not added to the keyable set. A keyed-by-bytes need is served by keying
on `b.toBase64()` (a `String`).

**D8 — Boundary-crossing is guaranteed for the *typed* paths; the erased
`workers` edge is diagnosed, not assumed.** The `bundle` cross-context path and
`store`/record codecs are fully typed, so they round-trip a `Bytes` correctly. But
`workers`-mode cross-context emission leans on `any` + runtime serialisation
helpers, and there a `Uint8Array` — which must be base64-encoded — silently
mis-round-trips (a raw `number` survives raw JSON, so `Instant` is fine; a
`Uint8Array` is not). Therefore v1 **guarantees** `Bytes` round-trip in `bundle`
calls and `store`/record fields, and a **bare** `Bytes` crossing a `workers`
cross-context signature (capability/service/agent handler) is diagnosed
`bynk.types.bytes_at_workers_boundary` rather than mis-encoded. A `Bytes` *inside
a record* crosses the `workers` boundary fine, because the record's typed codec
base64-encodes it. The restriction lifts when the roadmap's typed cross-context
boundary fix lands.

## Consequences

- **Checker:** most rules fall out by omission — `is_orderable` excludes `Bytes`,
  `check_map_key_keyable` rejects it, `json_codable` admits it (base types are
  codable), and equality is allowed by default (only `Stream`/held are rejected).
  New work is the kernel method/static tables (`length`/`toBase64`/`decodeUtf8`;
  `fromUtf8`/`fromBase64`/`empty`).
- **Emitter:** `Bytes` lowers to `Uint8Array`; `==`/`!=` on `Bytes` lower to a
  runtime `__bynkBytesEqual` content-compare; the four `__bynkBytes*` runtime
  helpers (equality, base64 encode/decode, fatal UTF-8 decode) are imported only
  when the module references them. The base64 codec threads through record/store
  codecs; the `workers`-edge diagnostic is a target-conditional project-validation
  pass.
- **Docs:** the type-system spec promotes `Bytes` to built; the roadmap's
  spec/impl divergence and workers-edge notes are updated.

## Alternatives considered

- **`Result[Bytes, EncodingError]` for the partial decoders (D3).** Rejected in
  favour of `Option` on the `Int.parse`/`Float.parse -> Option[T]` precedent (ADR
  0048): these are "a value was supplied and is malformed," a single failure mode,
  so an error enum is heavier than one case justifies. Revisit if a consumer must
  branch on *why* a decode failed.
- **A `b"…"` byte-string literal.** Rejected for v1 (deferred): arbitrary octets
  in source are an unportable foot-gun (`Instant`'s reasoning), and the
  constructors cover the constant case with no parser change.
- **General auto-derived structural equality for records/sums (D4).** Rejected as
  out of scope: whole-record `==` is reference equality today; making it deep is a
  semantics-changing feature with its own decision, touching `Set`/`distinct`/
  `Cache`. The hand-written field-comparator idiom already gives correct
  record-over-`Bytes` equality.
- **Supporting a bare `Bytes` across the `workers` edge now (D8).** Rejected: the
  erased `any` wire path would mis-encode it. Diagnosing is honest; the typed
  in-record path already works.
