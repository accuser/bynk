# 0200 — A compiled contract hash at the cross-context boundary: one canonical normal form, fail-closed at runtime, refused at deploy

- **Status:** Accepted (v0.177)
- **Provenance:** issue #643, the second half of the design-review finding #550
  ("Type the workers boundary + cross-context contract-hash check", §5.1(2), §8
  Platform #3). The first half shipped as [0199](0199-one-codec-path-at-the-workers-boundary.md)
  (v0.176). **Closes #643 and #550.**
- **Spec:** `emission.md` (§7.3.4c, the contract seam), `static-semantics.md`
  (§4.3, refinement matching), `runtime-library.md` (`callService`, `CallError`).
- **Realises:** every cross-context call carries a compiled contract hash; a
  callee whose contract differs fails closed with a named `ContractMismatch`
  instead of misinterpreting the payload; and `deploy` refuses a push that would
  create the skew rather than letting production find it.
- **Corrects:** [0199](0199-one-codec-path-at-the-workers-boundary.md) Decision G
  (see Decision H).
- **Relates:** [0092](0092-cross-context-caller-value.md) (the seam and the
  pattern this follows exactly — a compile-time constant in a reserved header,
  metadata beside the payload, channel trust, no crypto), [0193](0193-multi-context-deploy-ordering.md)
  (the D4 gate this extends), [0179](0179-deploy-provisioning-state.md) (the
  ledger this records into), [0195](0195-secrets-at-deploy.md) (the
  manifest-to-driver seam this reuses), [0107](0107-logging-discipline.md)
  (never report the offending value).

## Context

Bynk's pitch is independently deployable contexts, so interface evolution
*between* them is the language's central runtime problem, not an ops concern.
Context A compiles against B's contract — the compiler reads B's AST out of the
same source tree — and then nothing checks that the **deployed** B is the one A
was compiled against. `deploy --context NAME` (added by ADR 0193) makes that
routine: it pushes one context against dependencies assumed already live, and its
D4 gate checks a dependency **exists**, never that it **matches**.

ADR 0199 made the boundary *typed*. It did not make it *verified*: a skewed
callee's response deserialises against the caller's stale codec and is silently
misinterpreted.

**Why the hash and the predicate fix are one increment.** They look unrelated in
the issue and are not. A hash must be computed over a *canonical* form, or two
contexts that agree semantically hash differently and 409 spuriously. Refinement
predicates are part of a contract's type, and were compared — and would be
hashed — in **source order** (`checker.rs`'s `refinements_match` zipped them).
`String where NonEmpty && MaxLength(10)` and the same predicates reordered are
the same type, agree under any sane matcher, and would have produced **different
hashes**. Order-insensitivity is not adjacent to the hash; it is a
**precondition** for it, and shipping the hash without it would have shipped a
spurious-failure generator.

## Decisions

**[DECISION A] One canonical normal form, owned by `bynk-check`, shared by the
matcher and the hash (Recommended: yes).** A deterministic string per service:
`<service>(<param>: <type>, …) -> <type>`. Named types expand **structurally**
(the wire carries the fields, so a name-only form would miss a renamed field
entirely) while keeping their name (Bynk's types are nominal, so swapping `AuthId`
for a structurally identical `SessionId` is a contract change even though the
bytes are not). Record fields and sum variants sort by name — a JSON object is
unordered and a sum carries a `kind` discriminant, so *order* is not
wire-observable while *presence* is exactly what the hash pins. Generic arguments
stay positional, because there they are semantic. A recursive record emits a
named back-reference (`Node{next: Option[@Node]}`), so the walk terminates while
two different recursive shapes still differ.

A generic record's body expands with its **type parameters substituted**, so a
parameter's *name* never reaches the form: `type Page[T] = { items: List[T] }`
and the same declaration spelled with `U` are the same type with the same wire
shape, and a rename across a deploy must not 409 every call. (The first revision
expanded the body unsubstituted and did; caught in review of #658 — the same
class as field order and predicate order, and the one the false-positive standard
exists to forbid.)

One form, two consumers. If the matcher and the hash disagreed about "the same
contract", a program could type-check at compile time and 409 at runtime — the
worst failure this increment could produce.

**[DECISION B] Both sides canonicalise the callee's contract in the **callee's**
namespace, from a table built by the same function (Recommended: yes, and this is
the correctness argument).** The caller reaches the callee's combined type table
through `consumed_types[callee]`; the callee builds its own. Both come from
`symbols.rs`'s `combined_types_for`, so on a single build they are byte-identical
and the two hashes agree **by construction** rather than by care.

This is the decision that makes the mechanism safe. Had the caller canonicalised
in *its own* namespace, its rebranding of shared commons types (`Money &
{ __ctxBrand: "commerce.orders" }`) would render the same type differently and
409 every call. A standing guard (`bynkc/tests/contract_hash.rs`) asserts over
the whole fixture corpus that every stamped hash equals its callee's constant —
the no-false-positive property, mechanically enforced rather than argued.

**[DECISION C] Refinement predicates are a set, not a list (Recommended: yes).**
`refinements_match` compares canonical forms instead of zipping. This retires the
status doc's "brittle cross-context structural matching" foot-gun and is the
precondition described above. Predicates are conjunctive and side-effect-free, so
order carries no meaning and comparing it was always accidental. The `Some`/`None`
asymmetry (a more restrictive sender into a more permissive receiver) is
unchanged. One consequence of sharing the form: it de-duplicates, so
`where NonEmpty && NonEmpty` now matches `where NonEmpty` — correct, since a
conjunction is idempotent, and it must hold on the hash side regardless.

**[DECISION D] An opaque type's predicate is excluded; its representation is not
(Recommended: exclude).** A consumer cannot see an opaque predicate by
construction — that is what `exports opaque` means — so no consumer behaviour can
depend on it: it can hold and pass an `AuthId`, never inspect or mint one.
Including the predicate would manufacture skew between two contexts that cannot
disagree: the owner tightening `Matches(…)` would 409 every caller for a change
none of them can observe. ADR 0199 took the same position on opacity from the
same premise (its Decision F), and the two increments agree.

**[DECISION E] Per-service hash, not per-context — and the *gate* must honour the
same granularity (Recommended: per-service throughout).** A context may evolve
service `X` while callers of `Y` are untouched. A per-context hash would break
every caller on any change to any service — turning the guard into a deployment
tax and training authors to route around it.

The same reasoning binds the deploy gate, one layer up, and the first revision
missed it. `expects` recorded every service the dependency *provides*, not the
subset this context *calls* — so if `payment` provided `authorise` and `refund`,
`orders` called only `authorise`, and `refund` changed, `deploy --context orders`
was refused over a service `orders` never touches and whose runtime check could
never fire. That is a per-context gate over per-service hashes: precisely the
deployment tax this decision rejects, reintroduced. `expects` now records only
the called subset, discovered the same way the lowering discovers a call site (an
ident chain on the receiver resolving to a consumed context), so the gate and the
runtime check agree about what "skew" means. Caught in review of #658; fixture
377 discriminates the two designs, which no earlier fixture did — every callee in
the corpus provided exactly one service.

**[DECISION F] FNV-1a 64-bit, hand-rolled, no new dependency (Recommended:
FNV-1a).** 16 lowercase hex chars. Trust here is static and channel-based and this
increment does not change that (ADR 0092): `/_bynk/call/` is platform-dispatched
and not externally routable, every context in a deployment is one trust domain,
and a malicious first-party context is out of the threat model. This is a **skew
detector, not a security control** — forging it buys an attacker nothing they
could not already do, so `sha2`'s ~6-crate tree would buy nothing either. A
collision degrades to *pre-v0.177* behaviour for that one pair, not to something
worse, and at ~1e-14 for a 1000-contract project is not the risk worth
engineering against. Hand-rolled because
`std::collections::hash_map::DefaultHasher` is explicitly **not stable across
Rust releases**, so it cannot back a value that crosses a wire or is compared
between two separately-compiled binaries; FNV-1a is specified, so two compilers
agree forever. Pinned against the published reference vectors.

**[DECISION G] `X-Bynk-Contract` beside `X-Bynk-Caller`; mismatch → 409
`ContractMismatch`, checked before the body is read; absent → fail closed
(Recommended: yes).** This follows ADR 0092 precisely: a compile-time constant in
a reserved header, metadata beside an unchanged args body, no runtime hashing.

- **Before the body is read**, not merely before deserialisation: once the
  contracts disagree the body's *interpretation* is what is in doubt, so
  validating it first would report a misleading `StructuralMismatch` for the real
  fault — and there is no reason to parse a payload already refused. It therefore
  also precedes the `X-Bynk-Caller` check: with contracts in doubt nothing about
  the request is trustworthy, including the identity.
- **409, not 400:** the payload is not malformed and the caller cannot fix it by
  sending different bytes. The two *deployments* conflict.
- **Absent → fail closed.** A Bynk caller always stamps one, so absence means a
  non-Bynk or pre-upgrade caller — skewed by definition. This departs from ADR
  0092's conditional posture (a missing `X-Bynk-Caller` fail-closes *only* on a
  `by c: Caller` handler) because there is no binder to condition on: identity
  matters only when read, but the contract always matters.

`ContractMismatch` is deliberately **not** a `BoundaryError` variant.
`BoundaryError` is the *codec's* error domain — what deserialising can conclude —
and a codec can never produce this: it is decided from a header before any codec
runs. Widening `BoundaryError` would oblige every consumer of a codec result
(`Json.decode`'s error mapping among them) to narrow a case it cannot observe;
`tsc --strict` said so immediately. The call surface is wider than the codec
surface, so it gets its own type (`CallError = BoundaryError | ContractMismatch`).
`callService` surfaces a 409 as the named error rather than a generic
`Transport`, because the point of failing closed is that the operator learns
*what* is wrong.

**The cost is a flag day, and it is worth naming.** The first deploy after v0.177
must rebuild every context: callees deploy before callers (ADR 0193's topo-order),
and a not-yet-rebuilt caller stamps nothing. Accepted because Bynk is pre-1.0,
because Decision H moves the discovery from production traffic to the deploy
command, and because the alternative — treating an absent header as "legacy,
allow" — silently exempts precisely the callers most likely to be skewed, which
is the bug. Failing closed on a *change* is the feature; failing closed on the
*rollout* is the one-time price.

**[DECISION H] Extend `deploy`'s D4 gate from "exists" to "matches"
(Recommended: yes, in this increment) — and this corrects ADR 0199 Decision G's
prerequisite claim.** Each Worker emits `bynk-contracts.json` (the
`bynk-secrets.json` seam, ADR 0195 D5, including an explicit schema version)
carrying two facts: what it **provides** per service, and what it **expects** of
each dependency. `deploy` records `provides` in the ledger when it pushes, and
`absent_dependencies`'s sibling compares a context's `expects` against the
ledger's record of its live dependencies, refusing with
`bynk.deploy.contract_skew`.

This is what actually answers the issue. A runtime 409 does not
un-institutionalise `deploy --context`'s skew; it makes the failure legible
*after* it reaches production. The runtime check stays regardless — it is the
backstop for a manual `wrangler` push, a drifted ledger, and anything else
routing around the driver — but the deploy gate is what makes the guarantee
usable. **Silence is not a match:** a dependency the ledger has no contract record
for (pushed by a pre-v0.177 driver) yields no finding. The gate reports what it
*knows* is skewed, never what it merely cannot rule out.

The ledger field is `Option<BTreeMap>`, not a bare map, because an empty map
cannot carry that distinction: a callee that removes **all** its `on call`
services emits no manifest, so a bare map would record `{}` — indistinguishable
from "old ledger" — and the gate would wave through the most total skew there is.
`None` is "no record"; `Some({})` is "known to provide nothing", which is a
finding. (Caught in review of #658.)

**The correction.** ADR 0199 Decision G stated that self-contained Workers (its
deferred Decision B) was a **prerequisite** for this increment, "which would
verify nothing while there is only one view". That is wrong, and the error was
conflating compile-time source sharing with runtime comparison:

- the hash is computed by the **compiler** from `CrossContextService`, not from
  the emitted codec, so a borrowed *codec* does not imply a borrowed *hash*;
- each Worker emits its own `wrangler.toml` with `main = "index.ts"`, so each
  bundles separately and **inlines its imports at build time** — a caller's
  artifact carries its callee's codec *frozen as of the caller's build*, not a
  live cross-Worker link;
- so the two constants live in **different deployed artifacts, frozen at
  different deploy times**, and are compared across a wire at runtime — never
  against themselves.

The scenario the issue names works today: deploy both at rev1 (match); change B;
`deploy --context B`; live B expects `H(B_rev2)` while live A stamps `H(B_rev1)`
→ 409. Self-contained Workers remains a worthwhile follow-on on its own merits —
a caller's bundle currently carries its callee's provider implementation, resting
on tree-shaking — but it is **orthogonal**, not a blocker.

## Consequences

- `bynk-check` gains `contract` (the normal form + FNV-1a); `refinements_match`
  routes through it, so the matcher and the hash cannot drift.
  `bynk-emit` gains `contracts` (the manifest) and stamps/checks the constant;
  the runtime gains `ContractMismatch` and `CallError`; `bynk` gains the ledger
  field and the gate.
- **#550 closes.** The boundary is typed (v0.176) *and* verified (v0.177).
- **What this mints is detection, not evolution.** There is still no way to run
  two contract versions concurrently — no compatible-window or versioned-contract
  model — so a contract change remains a coordinated deploy. That is a coherent
  future increment, not a gap this one leaves by accident. Nor is the hash signed:
  a multi-trust-domain deployment would need that, and ADR 0092's channel-trust
  model says v1 does not.
- The residual unchecked arm from ADR 0199 (the runtime-owned error types) is
  unchanged and still named in `emission.md` §7.3.4b.
