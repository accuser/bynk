# The `Idempotency` capability — mechanical dedup for at-least-once delivery

- **Status:** Draft (settled — not yet sliced). Spine issue
  [#921](https://github.com/accuser/bynk/issues/921) open; this doc landed via
  [#922](https://github.com/accuser/bynk/pull/922) ("Part of #921"), but that PR was marked
  ready for review and merged 55 seconds later with no review — the assertion that §3's
  questions were closed was never tested. §3.1, §3.2, and §3.4 have since been genuinely
  argued and settled (see below; §3.1 depends on
  [#926](https://github.com/accuser/bynk/issues/926), its cut-out sub-issue); §3.2's
  settlement narrowed this track's scope and deferred §3.3 to a future, unfiled track. All
  four of §3's original questions are now resolved (settled or explicitly deferred) — this
  doc is ready for slice 0 to be cut as an increment-proposal sub-issue of #921, not yet
  adopted by a merge that actually tested it. Scopes issue
  [#554](https://github.com/accuser/bynk/issues/554) ("Ship the `Idempotency` capability
  ahead of the full Events track") down to its `Idempotency`-capability item.
- **Realises:** `design/bynk-design-notes.md` §4 ("Idempotency as a system convention",
  lines 103–109) and §12 ("Handler-level idempotency via the `Idempotency` capability",
  lines 640–674) — the architectural commitment that at-least-once delivery (commands,
  event replay, saga compensations) is made safe by construction rather than left to
  per-handler discipline.
- **Posture:** Feature track per [ADR 0076](../decisions/0076-feature-track-posture.md).
  It qualifies on all three axes (§ below), not just the usual two — in particular it is a
  **correctness/safety boundary**: the design notes name it the mechanism the whole
  consistency model (§12) leans on, and a wrong shape either fails to dedup (silent
  double-effects on retry) or dedups incorrectly (a handler wrongly short-circuited,
  returning a stale cached outcome).
- **What's already true:** nothing. `Idempotency` does not exist in the compiler today —
  `bynk-check/src/firstparty.rs:64` lists the only first-party capabilities as `Clock`,
  `Random`, `Logger`, `Fetch`, `Secrets`, `Locale`. The design notes' own aside — "the same
  way Sagas does" (multiple provider variants) — is not a working reference either; `Sagas`
  is equally unimplemented. This track has no sibling implementation to mirror; every
  question below was genuinely novel going in (§3.1, §3.2, §3.4 settled; §3.3 deferred).

## 1. The theme

A handler that needs deduplication declares `given Idempotency` and calls
`Idempotency.dedup[T](key, expiresAfter)` near its top. On first invocation with a given
key, the call passes through and the handler's own outcome is recorded against the key
when the handler commits. On a later invocation with the same key inside the retention
window — a retried command, a replayed event — the call instead short-circuits: the
handler's remaining body does not re-execute, and the caller sees the previously-recorded
outcome. The end state when this track retires: `given Idempotency` is a real, checked
capability with its one settled provider, and the §3.1 worked example below (a settled
correction of §12's `reserve` example) compiles and runs.

## 2. Why a track (the ADR 0076 trigger)

- [x] **Multi-increment.** Slice 0 (the in-memory provider + match-based short-circuit,
  depending on [#926](https://github.com/accuser/bynk/issues/926)) is separable from the
  possible event-subscriber-sugar slice 1. The durable provider, originally envisioned as
  later slices of this same track, was settled (§3.2) into a *separate*, future,
  currently-unfiled track instead — this track no longer carries that multi-increment
  weight itself, but still clears the bar on its own two remaining slices.
- [x] **Surface not yet settled (at settling time).** §3.1 (the `dedup` short-circuit
  shape), §3.2 (provider-variant selection), and §3.4 (key typing, scope, eviction) all
  had no existing pattern to lean on fully — see below for how each settled.
- [x] **Security/safety boundary.** The capability's entire purpose is a correctness
  guarantee (§12's "safe by construction" claim); a wrong shape is either silently unsafe
  (doesn't dedup) or silently wrong (dedups across the wrong scope — see the threat model,
  §6).

## 3. Open design questions

### 3.1 — How does `dedup` short-circuit the *rest of the handler body*? — SETTLED

**Decision.** `dedup` is not an early-exit primitive at all. It is an ordinary generic
capability method, matched on like any other `Option`:

```
capability Idempotency {
  fn dedup[T](key: String, expiresAfter: Duration) -> Effect[Option[T]]
}

on reserve(qty: Int, orderId: OrderId) -> ReserveOutcome given Clock, Idempotency {
  let cached <- Idempotency.dedup[ReserveOutcome](Json.encode(orderId), 24.hours)
  match cached {
    Some(outcome) => outcome,
    None => {
      ... the rest of the handler body, tail-typed to ReserveOutcome ...
    }
  }
}
```

(This is the real, settled call syntax — see §3.4 for why `on:`/`expiresAfter:`-as-labels
and `24h` from §12's own prose never parsed, and for the key-type and key-scoping
decisions.)

No new control-flow construct, and no change to `check_question`/`?` at all. `match` as a
tail expression with joined arm types already ships (the `join_ty` LUB mechanism,
[ADR 0230](../decisions/0230-join-match-if-branch-types.md)); what's missing is purely that
`dedup` needs to be **generic over its own type
parameter**, independent of any type the capability itself carries — `capability_op`
(`tree-sitter-bynk/grammar.js:572`) has no type-parameter slot today. That gap is filed as
its own increment proposal, **[#926](https://github.com/accuser/bynk/issues/926)**
("Generic capability methods — `capability X { fn op[T](...) }`"), a sub-issue of this
track's spine (#921) — its surface is dictated by existing precedent
(`Json.decode[T]`'s explicit-type-argument resolution, v0.22b), not itself an open
question this track needs to relitigate. This track's slice 0 depends on #926 landing
first (or alongside it).

**Why not the alternatives originally weighed here:**

- **A new checker-recognised statement form** (a `dedup`-flavoured bind, lowered to an
  early return, restricted to a handler's first statement) was the leading candidate, but
  it would have been the checker's first non-`?` control-flow special-case — verified by
  grep that even the design notes' own closest analogue, `attempt`/`recover` (§13), is
  itself unimplemented (zero hits in `bynk-syntax/src/keywords.rs`, `ast.rs`, or
  `tree-sitter-bynk/grammar.js`) — so there was no existing pattern to extend, only a new
  one to invent. The match-based shape needs no checker special-casing at all, so it wins
  on soundness surface alone: a smaller, better-precedented compiler change beats a novel
  one when both solve the problem.
- **Restricting `Idempotency` to literal `Result[T, E]`-returning handlers, reusing `?`**
  turned out not to work at all, not just to be narrow. Even with that restriction, `?`
  only ever injects a propagated value into the **`Err`** arm of the same `Result[_, F]`
  shape (`check_question`, `bynk-check/src/checker/expressions.rs:2060`) — so encoding a
  cached *success* value as `dedup`'s `Err` payload would make a caller observe a genuine
  `Err` for what was actually a cached `Ok`. That's a semantic inversion, not a narrowing;
  this candidate is dropped, not just deprioritised.
- **Interception** (the provider wraps the handler invocation itself; `dedup(...)` reads
  as a declaration, not a statement) is strictly more machinery than the match-based
  version for no additional power, and it hides the short-circuit behind provider-level
  magic the reader can't see operating from the handler body alone — working against the
  design notes' own "architectural cost is visible at the call site" principle (§4). Not
  pursued.

**The no-domain-outcome case, resolved for free.** A handler with no domain outcome at all
(design notes §13: `on currentBalance() -> Money`, where "any failure is a fault") is not a
special case under this decision — `Option[Money]` matches exactly the same way
`Option[ReserveOutcome]` does. The match-based shape needed no separate answer for this
because it never special-cased the return type's shape to begin with.

**Cost accepted, not hidden.** Placement of the `dedup` call relative to any effects the
handler performs is convention, not compiler-enforced — nothing stops a developer from
running a side effect before the `match`, which would re-run on every duplicate delivery
regardless of the cache hit. This mirrors the design notes' own accepted trade-off for
`Sagas.compensate`: no compiler enforcement, an explicit call that "gives the reviewer
somewhere to look" (§13). A future slice enforcing placement (e.g. a linter rule, not a
type-system one) is possible but not required to ship slice 0.

### 3.2 — Provider-variant selection collides with ADR 0016 — SETTLED (scope narrowed)

**Decision.** This track ships `Idempotency` with exactly **one** provider: the ambient,
portable, in-memory default — the same shape every other first-party capability (`Clock`,
`Random`, `Logger`, `Fetch`, `Secrets`, `Locale`) already has. No selectable-provider
mechanism is introduced, so [ADR 0016](../decisions/0016-no-portable-infrastructure.md)
("No portable infrastructure tier") is simply not implicated — there is nothing to
reconcile against a rule about choosing among providers when this track ships exactly one.
This deliberately narrows §12's own framing: the design notes describe `Idempotency` as
having "multiple providers, the same way Sagas does" (in-memory *and* durable,
developer-selectable). This track delivers only the in-memory half. The durable half is
not dropped, only deferred, for the reason below.

**The wider direction this sits inside (deferred, not solved here).** The real shape a
durable `Idempotency` provider wants is narrower than "a selectable-provider mechanism" in
the sense ADR 0016 rejected — the rejected shape was specifically **cross-platform
portability** (one interface abstracting over Cloudflare KV vs. DynamoDB, "lying about
what's underneath"). What's wanted instead: Bynk ships a capability's interface **and** a
default, portable provider (works on any platform, no platform dependency — trivially true
for in-memory dedup, which needs nothing but a plain in-process map); a **specific
platform adapter** (`bynk.cloudflare`) may separately supply its **own** provider for that
*same* capability, which supersedes the default when that platform is targeted. This is a
generalisation of a pattern the compiler already half-has — every `bynk`-surface
capability's concrete implementation already varies per platform binding
(`bynk-cloudflare.ts` vs. `bynk-node.ts` vs. `bynk-browser.ts`) — into a genuine two-tier
model: capabilities with **no override** (today's ambient primitives, unchanged) vs.
capabilities with a **default, optionally overridden by a specific platform** (new).
`Idempotency` is the natural first candidate: a future, **currently unfiled** track would
introduce a Cloudflare-native durable provider (plausibly backed by Durable Object
storage) under this model, without touching this track's `given Idempotency`/`dedup[T]`
call-site surface at all — the override would happen entirely at the composition root,
delivering §12's "handler shape unchanged" property, just on a longer timeline than this
track covers. Not filed now: there is no concrete durable-storage design to hang it on yet,
and filing a track needs more than "it would be nice" (ADR 0076's own bar).

**Why this doesn't need to be decided now.** Nothing in slice 0's shape (§3.1, the
match-based short-circuit over `Effect[Option[T]]`) commits to or forecloses either the
selectable-provider shape this section originally weighed *or* the default-plus-override
shape above — both are compose-root-level questions, entirely below the capability
interface and the handler-visible call site. Settling §3.2 now to "ship one provider,
defer the rest" keeps this track's committed surface honest (§12's full "in-memory and
durable" claim is *not* delivered here) without blocking slice 0 on an architectural
question wider than this track's driving need.

### 3.3 — Does a durable provider need to join the *agent's own* atomic commit? — DEFERRED

Moot for **this** track once §3.2 narrowed scope to the single in-memory provider: there is
no durable provider here to need an answer. Kept below, unedited, as groundwork for
whichever future track picks up the Cloudflare-native durable provider §3.2 named — the
analysis doesn't change just because the track that will need it hasn't been filed yet.

§12: "The dedup record is written atomically with the handler's other commits... If the
handler completes... the result is cached. If the handler aborts via fault, no record is
written." This describes the dedup write joining the *same* atomic transaction as the
enclosing agent handler's `store` writes (decision 0109's "handler is the atomic unit" —
confirmed implemented, not aspirational: `bynk-emit/src/emitter.rs` stages `store` writes
and flushes them once at handler end, unlike the false `attempt`/`recover` precedent 3.1
found and corrected). Every existing capability (`Clock`, `Random`, `Fetch`, `Secrets`, `Locale`) is an
independent side effect with no participation in the calling agent's storage transaction;
`Cache`/`Log` (the closest TTL/retention precedent) are `store` fields *owned by the agent
itself*, not capability-provider state at all. A durable `Idempotency` provider is asking
for a third shape: capability-provider state that must commit-or-abort in lockstep with an
*agent it doesn't own*. Needs settling whether this is: literally a `store`-field-shaped
mechanism wearing a capability interface (and if so, what that means for services, which
have no `store` to join); a new narrow transactional-participation contract capability
providers can opt into; or evidence that §12's "atomically with the handler's other
commits" is aspirational and the real guarantee is looser (e.g. dedup-write-then-handler,
accepting a narrow window where a duplicate could slip through on crash).

### 3.4 — Key typing, key scope, and eviction — SETTLED

**Decision — the real call shape.** §12's `dedup(on: x, expiresAfter: y)?` never parsed
against the shipped grammar (three independent mismatches: labelled arguments don't
exist, `24h` isn't the duration-literal form, and 3.1 already dropped the `?`). The
settled interface and call:

```
capability Idempotency {
  fn dedup[T](key: String, expiresAfter: Duration) -> Effect[Option[T]]
}

let cached <- Idempotency.dedup[ReserveOutcome](Json.encode(orderId), 24.hours)
match cached {
  Some(outcome) => outcome,
  None => { ... }
}
```

Declared parameter names (`key`, `expiresAfter`) stay for documentation and
hover/signature-help — only the *call site* is positional, matching `call`
(`tree-sitter-bynk/grammar.js:1406`, `sep1($._expression, ",")`) exactly, with no grammar
change needed. `24.hours` is the shipped `<int>.<unit>` `Duration` literal form (ADR 0112),
confirmed against the closed unit set in `bynk-syntax/src/ast.rs:1633`
(`Hours`/`Days` are both in it).

**Decision — the key type is `String`, not "any expression."** §12's "any expression in
scope" reads most naturally as generic-over-the-key-type (`fn dedup[K, T](key: K, ...) ->
Effect[Option[T]]`, `K` constrained to the JSON codec's boundary-legal domain). Rejected:
Bynk has no type-parameter bounds mechanism today (generic records and generic instance
methods both ship unconstrained), so a badly-chosen `K` would fail only late, at whatever
point the provider tries to serialise it — not at the `capability_op` declaration site. A
plain `String` sidesteps this entirely, matches how every real-world idempotency-key
system works (Stripe's `Idempotency-Key` header, AWS Lambda Powertools' idempotency
utility — both opaque strings), and keeps `dedup` single-generic (only `T`, the outcome
type, as settled in 3.1). Where a caller has a richer domain value, they derive the string
explicitly (`Json.encode(orderId)` above) — a visible step, not implicit codec magic,
which directly serves §6's "make the effective key visible for review" goal: an implicit
generic-and-serialised key would hide the effective key's shape from a reviewer who
doesn't already know the codec's output format.

**Decision — key scope is automatic at the call site, manual across callers.** The
*effective* key the provider stores against is the developer-supplied string, prefixed
with a compiler-synthesised, stable per-call-site identifier (e.g. derived from the
`dedup` call's own source span) — free (compile-time only, no runtime cost, no new
syntax), and it closes one real class of the §6 threat model for free: two unrelated
`dedup` call sites can never collide even if a developer reuses an identical literal
string at both, because the sites themselves are automatically distinguished. It does
**not** close the other class §6 names — two *different callers of the same call site*
(e.g. two tenants both invoking the same `reserve` handler) still collide if the
developer-supplied string doesn't itself differentiate them. That part stays the caller's
documented responsibility, same trade-off already accepted for `Sagas.compensate`
targeting a non-idempotent operation (§13): no compiler enforcement, an explicit,
reviewable call. Automatic call-site scoping narrows the burden; it doesn't remove it.

**Decision — eviction is lazy, matching `Cache`.** Reuses `Cache`'s shipped
check-on-read model (decision 0113: expired entries reap at next access, no background
sweep) rather than inventing proactive eviction. This carries the same accepted
trade-off `Cache` already lives with in this codebase — a key written once and never read
again isn't reclaimed until *something* reads it — so it is not a new risk this track
introduces, only one it inherits. A future proactive-sweep slice is a valid optimisation,
not a blocker for slice 0.

## 4. Candidate slice decomposition

With 3.1, 3.2, and 3.4 settled, and 3.3 deferred out of scope, this track is fully
settled: slice 0 is the only slice needed to deliver this track's actual end state (§1),
and its shape is now concrete end to end.

- **Slice 0 — the match-based short-circuit + the (sole) in-memory provider.** Depends on
  [#926](https://github.com/accuser/bynk/issues/926) (generic capability methods) landing
  first or alongside. Ship `given Idempotency` with its one provider (in-memory,
  handler-local; `String` keys, call-site-scoped, lazy eviction per §3.4), proving the
  settled §3.1 shape end-to-end against a `tsc_verify` case reproducing the (corrected)
  `reserve` example above.
- **Slice 1 (possible, not yet scoped) — event-subscriber sugar.** §12's `e.eventId`
  canonical-key pattern for event subscribers; whether this deserves special syntax or is
  just documented convention once slice 0 lands.

**Not slices of this track.** Provider-variant selection and the durable provider (the
original slices 1–2) are not deferred *within* this track — they move to the future,
currently unfiled track named in §3.2, along with §3.3's transactional-participation
question. Recorded there, not here, so this track's own scope stays honest about what it
actually delivers.

## 5. Front-loaded ADR candidates

- **The `dedup` match-based short-circuit** (3.1, settled) — records that `dedup` is an
  ordinary generic capability method returning `Effect[Option[T]]`, matched by the caller;
  no new control-flow construct, no `?`/`check_question` changes. Depends on
  [#926](https://github.com/accuser/bynk/issues/926)'s own ADR (generic capability
  methods) landing as its foundation.
- **`Idempotency` ships ambient-default-only; scope narrowed, not silently reduced** (3.2,
  settled) — records that this track ships exactly one provider, so ADR 0016 is not
  implicated, and names the wider default-plus-platform-override direction as future work,
  not decided here. Keeps the scope-narrowing decision itself durable and citable, not just
  implicit in the doc's diff history.
- **Durable-provider transactional participation** (3.3) — deferred; not this track's ADR.
  Belongs to whichever future track picks up the durable provider named in 3.2.
- **`Idempotency`'s key shape** (3.4, settled) — `String`-typed keys, positional call
  syntax (no labelled arguments), automatic compiler-synthesised call-site scoping
  layered under the developer-supplied key, and lazy eviction matching `Cache`. Records
  why "any expression" and implicit generic serialisation were rejected in favour of an
  explicit `Json.encode(...)` at the call site.

## 6. Threat model

**Asset.** The cached outcome recorded against a dedup key — potentially containing
domain data (§12's `ReserveOutcome`, a `Receipt`, etc.) — and the dedup mechanism's
correctness guarantee itself (a wrongly-short-circuited handler silently skips real work).

**Adversary.** A caller who controls or can guess part of a dedup key. §12 explicitly
sanctions caller-supplied keys for non-idempotent receivers ("the caller supplies a
deterministic identifier... the receiver dedupes against it"), and those keys can
originate from external actors — an HTTP client-supplied idempotency header is the classic
case. Two distinct collision classes exist: **cross-call-site** (two unrelated `dedup`
calls in different handlers happen to use the same literal string) and **cross-caller,
same-call-site** (two different tenants both invoke the *same* handler, and the
handler-derived key doesn't itself differentiate them). A second caller who lands on either
collision receives the *first caller's* cached outcome instead of executing their own
request — a cross-tenant data leak if the outcome carries tenant-specific data, and a
correctness bug even when it doesn't (wrong operation silently skipped).

**Where verification happens — settled in 3.4.** The cross-call-site class is closed
automatically and for free: §3.4's decision prefixes every key with a
compiler-synthesised, per-call-site identifier, so two different `dedup` call sites can
never collide regardless of what string a developer picks. The cross-caller,
same-call-site class is **not** closed by the mechanism — it stays the caller's documented
responsibility (the design notes' own framing: "the caller supplies a deterministic
identifier derived from its own context"), the same trade-off already accepted for
`Sagas.compensate` targeting a non-idempotent operation: no compiler enforcement, an
explicit, reviewable call that "gives the reviewer somewhere to look" (§13). This applies
to the in-memory provider now and will apply just as much to a future durable provider
(§3.2) — the stakes rise with durability (a longer retention window, crash survival), but
the mechanism and its accepted gap are the same.

## 7. Slice status

- [ ] Slice 0 — match-based short-circuit + the in-memory provider (incl. 3.4)
- [ ] Slice 1 — event-subscriber sugar (unscoped)

## 8. Done when

- `given Idempotency` is checked and its one provider is usable; the settled §3.1/§3.4
  `reserve` example compiles and passes a `tsc_verify` case.
- The match-based short-circuit (3.1) ships on top of
  [#926](https://github.com/accuser/bynk/issues/926) (generic capability methods), with no
  bespoke control-flow construct introduced for `Idempotency` itself.
- `String` keys, positional call syntax, compiler-synthesised call-site scoping, and
  `Cache`-style lazy eviction (3.4) all ship as specified; the cross-call-site collision
  class is closed by construction, and the cross-caller-same-site class is documented as
  the caller's responsibility, not left implicit.
- The doc is explicit that provider-variant selection and the durable provider (3.2's
  deferred half, 3.3) are **not** delivered by this track — named as future work, not
  silently dropped.
- Issue [#554](https://github.com/accuser/bynk/issues/554) can be narrowed to its
  remaining two items (ADR 0020, the composition root) once this track's slice 0 ships —
  it does not fully close here, since the durable provider it also named is now explicitly
  out of this track's scope.
- ADRs written (including 3.2's scope-narrowing decision); spec gains the `Idempotency`
  capability's normative section, scoped to the single provider this track ships.
  **On retire:** remove this doc.
