# 0182 — A refined type has no `.unsafe`; the unchecked escape hatch is opaque-only, in source and in emitted TypeScript

- **Status:** Accepted (v0.156)
- **Provenance:** design-review finding #545 (Bynk Language Design Review
  2026-07-05, §8 Language P1 #6) — "`.unsafe` on refined types is globally
  unrestricted … the largest credibility hole in the refinement guarantee",
  recommending it be confined to the defining commons, mirroring opaque
  `.unsafe` (`bynk.types.opaque_unsafe_outside`). **Closes #545** — by removing
  the hatch rather than confining it (see Decision).
- **Realises:** a refined or alias type constructs values through `.of` (a runtime
  predicate check) and compile-time literal admission only. It exposes **no**
  `.unsafe` — not in Bynk source (already so), and now not in the emitted
  TypeScript either: literal admission lowers to an inline brand cast, so no
  callable unchecked constructor is exported for host or adapter code to reach.
  The unchecked `.unsafe` hatch remains for **opaque** types alone.
- **Relates:** ADR 0014 (refined boundary IDs are constructed through `.of`; raw
  casts and `.unsafe` are disallowed for bindings — previously by convention,
  now enforced by the emitted shape), ADR 0001 (the closed compile-time
  literal-admission set), ADR 0181 (the sibling design-review language increment).

## Context

The review flagged refined `.unsafe` as an unconfined, global escape hatch — the
largest credibility hole in the refinement guarantee — and recommended confining
it to the defining commons, exactly as opaque `.unsafe` already is
(`bynk.types.opaque_unsafe_outside`).

Investigating the premise turned up a three-layer disagreement:

- **Bynk source.** The resolver and checker recognise `.unsafe` only for `opaque`
  bodies, so `Age.unsafe(-5)` — written anywhere, including inside the type's own
  defining commons, even on a literal — is already rejected
  (`bynk.resolve.unknown_static_member`), and has been since v0.62 (#245). No
  fixture constructs a refined value through it.
- **Emitted TypeScript.** Every refined, alias, and opaque type emitted a public
  `unsafe(value)` constructor whose body is `return value as T`. Literal admission
  lowered an admitted literal to `T.unsafe(literal)`, and the property-test
  generator minted inhabitants the same way.
- **Documentation.** §6.4, the reference API page, the glossary, and two guides
  described a refined `.unsafe(v) -> T` as "the deliberate escape hatch."

So the finding's "any code anywhere can `Age.unsafe(-5)`" is false for Bynk source
(rejected) but **true for hand-written host / adapter TypeScript**: a binding could
import the generated module and call `Url.unsafe(rawString)` to mint an
unvalidated `Url`, guarded only by the ADR 0014 convention, not the compiler. The
real, reachable hole was the exported `.unsafe` on refined types in emitted code.

## Decision

Remove the refined `.unsafe` hatch rather than confine it — and close it where it
was actually reachable, the emitted TypeScript.

**A refined or alias type has no `.unsafe`.** Its only construction paths are `.of`
(run-time check, `Result`) and compile-time literal admission (§5.3). The emitter
no longer exports a `unsafe` member on a refined/alias type's constructor object;
literal admission lowers to an inline **brand cast** — `(literal as T)`, byte-for-byte
the old `.unsafe` body at the call site — which is not a callable API surface a
consumer can reach. In generated test scaffolding, where a branded type is in
scope only as an `any`-typed value, a refined draw brands to `any` (the type the
old `T.unsafe(v)` already produced there), still erasing to the raw value at
runtime. The unchecked `.unsafe` constructor stays for **opaque** types, which
structurally need an in-commons constructor consumers cannot otherwise reach.

This makes the refinement guarantee real rather than conventional: no code —
Bynk **or** host TypeScript — can mint a refined value without its predicate
having been checked, at run time or compile time.

The asymmetry with opaque is motivated. An opaque type hides its representation,
so its defining commons needs a representation-level constructor that outside code
cannot reach; a refined type's representation *is* its base, always constructible
through the validating `.of`, so an unchecked hatch would only subtract a check.

### Rejected alternatives

- **Confine refined `.unsafe` (the finding's literal recommendation).** This would
  first *add* a user-facing unchecked constructor that does not exist today, then
  gate it — net-widening the trusted surface to close a hole better closed by
  removal. It buys only parity with opaque, whose hatch exists for a reason
  refined types do not share (representation hiding, above).
- **Docs-only correction.** Fixing the spec/reference to say refined has no
  `.unsafe`, leaving the emitted public `.unsafe` in place, would leave the real
  reachable hole (host code) open under convention. Rejected in favour of closing
  it in emission.
- **Build-summary of `.unsafe` sites (the finding's fallback).** Worth having for
  auditing *opaque* `.unsafe`, but orthogonal — there are no refined `.unsafe`
  sites to summarise.

## Consequences

- **Breaking at the host boundary (pre-1.0).** Hand-written TypeScript that called
  `RefinedType.unsafe(x)` on emitted output no longer compiles — exactly the
  bypass ADR 0014 forbade by convention. Bynk source is unaffected (it never had
  the surface). Emitted output for programs that don't hand-call the constructor
  is otherwise unchanged except for the dropped member and the admission-cast form.
- Spec §6.4, §6.1.2, §5.3, §7.3, the reference API page, glossary, operators
  table, guides, and the `coming-from-typescript` mapping are corrected: the
  unchecked hatch is opaque-only and confined; refined construction is `.of` +
  literal admission.
- A regression fixture pins the source rejection; `tsc_verify` (strict) and the
  property runner pin that the emitted output has no refined `.unsafe` yet still
  type-checks and runs.
