# 0080 — Authentication schemes are a closed, compiler-known nominal set; actors are nominal contracts

- **Status:** Accepted (v0.45)
- **Spec:** `lexical-grammar.md` (reserved scheme names), `syntactic-grammar.md` (`actor_decl`), `static-semantics.md` (`bynk.actor.unknown_scheme`, `scheme_unsupported`)
- **Realises:** the actors track (`design/tracks/actors.md`), question Q1.

## Context

An `actor` declaration tells the language what a boundary expects of an external
party (design notes §6). The first question is the shape of the scheme axis: an
open registry (anyone adds an auth method) or a closed, compiler-known set? And
is an actor's conformance to a scheme structural or nominal?

## Decision

Authentication schemes are a **closed, compiler-known set** —
`None | Internal | Bearer | Signature` — and an **actor is a nominal contract
type** layered on one scheme (`actor Name { auth = Scheme }`, optionally
`, identity = T`). Foundations admits the two **zero-crypto** schemes (`None`,
`Internal`); `Bearer`/`Signature` are reserved-and-rejected
(`bynk.actor.scheme_unsupported`), an unknown name is `bynk.actor.unknown_scheme`.

Closed because a scheme is an *inbound* boundary verifier — it owns secret
sourcing, failure shaping, and the trust assertion — far too sharp to hand to an
open registry, and the surveyed cover (none/internal/bearer/signature) is
near-complete. Nominal because actor invariants are *semantic* (a carried claim,
a replay expectation), not shape — structural conformance (cf. PEP 544) can only
witness member presence, never predicate-level constraints — and a sealed
vocabulary needs a nameable conformance to seal.

"Sealed now, opened later" = **widen the enum**: a new scheme is new surface
against the one scheme descriptor (verification codegen + identity shape +
failure mapping), not a re-architecture — mirroring the v0.44 protocol-descriptor
seam (0079). The refinement form `actor A = B where p` is reserved-and-rejected
(`bynk.actor.refinement_unsupported`) so the invariants slice adds admission, not
grammar.

## Consequences

The closed set keeps boundary verification total and trustworthy; the nominal
contract gives each actor a sealable name. Foundations builds the whole machine
against `None`/`Internal`; every later scheme is an additive entry in the scheme
descriptor. Actors are context-only (`bynk.actor.outside_context`) — boundary
contracts belong to the context whose services consume them.
