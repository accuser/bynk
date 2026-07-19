---
level: minor
changelog: A workers context generates its own cross-context boundary codecs and imports no sibling context's module as a value
---

## ADR: self-contained-workers-codecs
title: Self-contained Workers — a context generates its own cross-context codecs
summary: A caller generates its own view of a callee-owned type's codec, so a workers build imports no sibling context's module as a value

**Provenance.** Issue #661, discharging ADR 0199 (One codec path at the workers
boundary) Decision G, which deferred this and — wrongly — called it a
prerequisite for the cross-context contract hash. ADR 0200 (The cross-context
contract hash) Decision H recorded that correction: the hash is computed by the
compiler from `CrossContextService`, not from the emitted codec, and each Worker
bundles separately, so the two constants sit in different artifacts frozen at
different deploy times. The hash shipped in v0.177 without this. **Nothing was
blocked on this increment; it stands on its own merits. Closes #661.**

**Scope.** The emitter (`bynk-emit`) only: the codec-collection walk, the
per-context codec emission, and the cross-Worker namespace import under
`--target workers`. Untouched: grammar, the checker's type rules, the `bundle`
target, the wire format, and the contract hash (unchanged — the proof that ADR
0200 Decision H's correction was right).

## Context

Under `--target workers` two contexts are separately deployed Workers, yet a
caller reached its callee's codecs through a **runtime import of the callee's
module**: `import * as commerce_payment from "../commerce-payment/handlers.js"`,
used as `commerce_payment.serialise_Money(...)` /
`commerce_payment.deserialise_Result_AuthId_PaymentError`.

That is a bundling leak. `serialise_*`/`deserialise_*` for a callee-owned type
live in the *same module* as the callee's provider implementation
(`export class StubPayments`), so tree-shaking, not the compiler, was the only
thing keeping `commerce-orders`'s bundle from carrying `commerce.payment`'s
provider class. It is also a modelling-honesty issue: a caller importing the
callee's module asserts they were compiled together — precisely the fiction
`deploy --context NAME` breaks. For a language whose pitch is independently
deployable contexts, the compiler should guarantee self-containment rather than
delegate it to a bundler optimisation.

Two obstacles had made the caller unable to generate its own codecs: the caller
has no local *declaration* of the callee's type (`AuthId` appears in
`commerce.orders` only via the imported namespace), and `emit_refined`
re-validated through the type's own `.of` constructor, which lives in the
owner's module.

## Decision

**[A] The caller generates its own codecs; the namespace import survives as
`import type`.** Codecs resolve to local helpers (no `<ns>.` prefix). The import
is not deleted — it becomes type-only, so it is erased outright rather than
relying on a bundler to elide an import whose every use is a type. Across every
workers golden the only runtime uses of a consumed context's namespace in
`handlers.ts` were the codecs; every remaining use (`deps: { Clock:
platform_time.Clock }`) is already a type position. Value uses (`new
commerce_payment.StubPayments()`) live only in `compose.ts`, a legitimate
composition root emitted by a different path. This applies to consumed
*contexts* under `workers` only — **not** to a consumed adapter (whose binding
namespace is a real value import in `compose.ts`) and **not** on `bundle`.

**[B] A consumed type's codec names the type through the type-only namespace.**
`deserialise_AuthId` in orders returns `Result<commerce_payment.AuthId,
BoundaryError>`. The codec emitter accepts a per-type-name qualifier; the codec
*function* names stay bare and local. Emitting orders' own *declaration* of the
callee's types was the rejected alternative — a much larger change touching the
`__ctxBrand` model that buys nothing, since the type reference is compile-time
and the runtime coupling is exactly what this increment removes.

**[C] A consumed *opaque* type's caller-side codec is structural; refinement
validation stays with the owner.** The generated codec validates the base type
and casts, rather than reaching for `.of`. This is forced (`.of` lives in the
owner's module, so calling it would resurrect the value import) and right: an
opaque type's representation is the owner's secret, and inlining its predicate
into the consumer would publish exactly what opacity withholds. The consequence
is real and bounded: an inbound value's refinement is not re-checked by the
consumer. It is sound because the value was produced by the owner's own typed
code, and a *skewed* owner is caught by the v0.177 contract hash — which is what
makes this decision affordable now and would not have been before.

**[D] A consumed *transparent* refined type is still fully validated.** Its shape
is visible to the consumer by declaration, so its codec checks the predicate —
inline, not through `.of`. The asymmetry with [C] is the point of the `exports
opaque`/`exports transparent` distinction, not an inconsistency.

Only the callee's *own exports* reachable from the services this context
actually **calls** are generated — not the callee's whole provided surface, and
not the commons types the caller already holds. This mirrors the `called`
narrowing the contract manifest applies to `expects`.

## Consequences

A Worker's emitted `handlers.ts` no longer imports a sibling context's module as
a value, and its bundle no longer contains that context's provider
implementations — a property the compiler now guarantees. Wire bytes, contract
hashes, and `bundle`-target output are unchanged.

The most plausible bug this could introduce is silent codec divergence between a
caller's generated codec and the callee's: `tsc --strict` type-checks both sides
independently, so it is necessary but not sufficient. The guard is a live
round-trip — the existing `cross_context_caller` driver, extended to drive a
user-typed payload through the *caller's own* codec and assert the decoded value,
not just a status code.
