---
level: minor
changelog: The cross-context call format is published as [wire format version 1](/book/reference/wire-format/), generated from the compiler and versioned on its own, and callers now send `X-Bynk-Wire: 1`. A `/_bynk/call/` request that is not a `POST` gets `405`, malformed JSON gets `400 MalformedJson` instead of a `500`, and an unsupported `X-Bynk-Wire` gets `400 UnsupportedWireVersion`. A record field's inline refinement (`qty: Int where Positive`) is now checked when the record is decoded, at a call boundary or in an HTTP body, and is part of the contract hash; a `Duration` field must be a whole number of milliseconds. **Breaking for deployed projects:** contracts with an inline field refinement change hash, so redeploy the whole project once
---

## ADR: wire-format-is-a-published-surface
title: The cross-context wire format is a published surface with its own version, amending ADR 0310
summary: Wire format v1 (path, method, headers, check order, statuses, error bodies, JSON encoding, contract hash) is generated from the compiler, versioned apart from it, and changes additively within a version

**Context.** A Bynk context's `on call` services are reached as
`POST /_bynk/call/<service>`, with a JSON body and a few headers. That format
was specified only for the compiler (`spec/emission.md` §7.3.4), and
`bynk-1.0-definition.md` Gate 1 puts it outside the frozen contract as
"emitted TypeScript". Code that is not Bynk had nothing to build against
(#1969), and the format had rough edges an outsider would trip on: a
malformed body was a `500`, the method was never checked, an inline field
refinement was neither hashed nor checked on decode, and a `Duration` field
was not guarded. ADR 0310 publishes one small surface beside a codegen that
stays free: the emit ABI, "exactly four items". The interop track
(`design/tracks/interop.md` §8 D1, §11 Q2) settled the same treatment for the
wire format, versioned on its own.

**Decision.** The cross-context wire format is a second published surface
beside ADR 0310's emit ABI, with its own version, starting at **1**. It
consists of: the path, the method and the headers of a call; the callee's
checks in order, with their statuses and error bodies; the JSON encoding of
every boundary type; and the contract hash's normal form and function.
Nothing else of the emitted TypeScript is published by it.

- **One source.** The constants live in `bynk_check::wire_format`, and the
  emitter builds the `/_bynk/call/` dispatch from them. The reference page
  (`reference/wire-format.md`) is rendered from those constants, from
  `bynk_check::wire`'s encoding model and from `bynk_check::contract`'s hash,
  computed on an example context. `bynkc/tests/wire_reference.rs` fails if the
  page, the emitted dispatch, the runtime's `callService` or the generated
  codecs disagree, running every example on the page through the codecs the
  compiler generates for it.
- **Check order.** Method (`POST`, else `405` with `Allow: POST`);
  authentication (`401`, bare); wire version (`400 UnsupportedWireVersion`,
  naming the accepted versions); service (`404`); contract
  (`409 ContractMismatch`); JSON (`400 MalformedJson`); caller, when the
  handler binds `Caller` (`401 Unauthorized`); arguments (`400`, the codec's
  `StructuralMismatch` or `RefinementViolation`). Success is `200` with the
  encoded return value, a domain `Err` included.
- **Versioning.** A caller states the version it speaks in `X-Bynk-Wire`;
  absent means `1`. A callee accepts a set of versions and refuses any other
  with the set, which is how a caller learns it. The version check comes after
  authentication, so an unauthenticated sender cannot probe it, and before
  the service lookup, because the version decides how everything after it
  reads.
- **Stability.** Within a version, changes are additive only: a new status, a
  new error `kind` (callers treat an unknown kind as an error) or a new
  optional header. Changing an existing encoding, check, status or error body,
  or the hash's normal form or function, is a new version, accepted beside the
  old one for an overlap window. The hash is frozen with the version because a
  client built once embeds hashes.
- **Authentication is outside the freeze until the track's slice I2.** Version
  1 fixes where authentication sits and what its refusal looks like. The
  credential, today the deployment's shared call secret (`X-Bynk-Call-Secret`),
  is replaced by per-edge signatures without a new wire version. Nothing
  outside a deployment can hold the shared secret, so the swap breaks no
  published client; foreign callers are supported only after I2.
- **The rough edges are fixed before the freeze.** Inline field refinements
  join the hash (`f: T where P, …`, a sorted set like a refined type's) and
  are checked on decode, failing as `RefinementViolation` at the field's
  path. The codec is shared, so an HTTP request body checks them too. A
  `Duration` or `Instant` field must be a safe integer on decode, and encoding
  one that is not faults in the sender, as an `Int` does. The body is read as
  text and parsed, so malformed JSON is the caller's `400`.

`bynk-1.0-definition.md` Gate 1 gains the sentence that the emitted TypeScript
is not frozen *except* the wire format, which follows its own version.
ADR 0310's "exactly four items" stands for the emit ABI; this is a second
surface beside it, not a fifth item in it.

**Consequences.** Non-Bynk code has a contract to speak, and the interop
track's typed TypeScript client (D3) has a floor. The `/_bynk/call/` behaviour
changes for Bynk-to-Bynk calls too: a wrong method or malformed body is now a
`4xx` rather than a `500`, and a value breaking an inline field refinement is
refused rather than admitted. Every contract with an inline field refinement
changes hash, so a deployed project redeploys once (ADR 0200's flag day, for
this change only). **Not done here:** a body-size limit (`413`), which D1
places between the method and authentication. Adding it later is additive
within version 1. The `/_bynk/event/` path is not part of the call wire format
and is unchanged.
