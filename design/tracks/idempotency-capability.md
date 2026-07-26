# The `Idempotency` capability — mechanical dedup for at-least-once delivery

- **Status:** Draft (settling — partial). Spine issue
  [#921](https://github.com/accuser/bynk/issues/921) open; this doc landed via
  [#922](https://github.com/accuser/bynk/pull/922) ("Part of #921"), but that PR was marked
  ready for review and merged 55 seconds later with no review — the assertion that §3's
  questions were closed was never tested. §3.1 and §3.2 have since been genuinely argued
  and settled (see below; §3.1 depends on
  [#926](https://github.com/accuser/bynk/issues/926), its cut-out sub-issue); §3.2's
  settlement narrowed this track's scope and deferred §3.3 to a future, unfiled track.
  §3.4 remains open. Treat this doc as still settling, not adopted. Scopes issue
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
  is equally unimplemented. This track has no sibling implementation to mirror; the open
  question remaining below (§3.4; §3.1 and §3.2 settled, §3.3 deferred) is genuinely novel.

## 1. The theme

A handler that needs deduplication declares `given Idempotency` and calls
`Idempotency.dedup(on: <key>, expiresAfter: <duration>)` near its top. On first
invocation with a given key, the call passes through and the handler's own outcome is
recorded against the key when the handler commits. On a later invocation with the same
key inside the retention window — a retried command, a replayed event — the call instead
short-circuits: the handler's remaining body does not re-execute, and the caller sees the
previously-recorded outcome. The end state when this track retires: `given Idempotency` is
a real, checked capability with at least one usable provider, and the design notes' §12
worked examples (`reserve`, the `PaymentConfirmed` subscriber) compile and run.

## 2. Why a track (the ADR 0076 trigger)

- [x] **Multi-increment.** Slice 0 (the in-memory provider + match-based short-circuit,
  depending on [#926](https://github.com/accuser/bynk/issues/926)) is separable from the
  possible event-subscriber-sugar slice 1. The durable provider, originally envisioned as
  later slices of this same track, was settled (§3.2) into a *separate*, future,
  currently-unfiled track instead — this track no longer carries that multi-increment
  weight itself, but still clears the bar on its own two remaining slices.
- [x] **Surface not yet settled.** §3.1 (the `dedup` short-circuit shape) and §3.2
  (provider-variant selection) both settled during this track's own settling pass — see
  below. §3.4 (key typing, scope, eviction) remains open, with no existing pattern to lean
  on fully.
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
  fn dedup[T](on: Key, expiresAfter: Duration) -> Effect[Option[T]]
}

on reserve(qty: Int, orderId: OrderId) -> ReserveOutcome given Clock, Idempotency {
  let cached <- Idempotency.dedup[ReserveOutcome](on: orderId, expiresAfter: 24h)
  match cached {
    Some(outcome) => outcome,
    None => {
      ... the rest of the handler body, tail-typed to ReserveOutcome ...
    }
  }
}
```

(Written here with §12's illustrative `on:`/`expiresAfter:`/`24h` syntax; §3.4 settles the
real call syntax — labelled arguments and bare-suffix durations don't exist as shown, see
below.)

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

### 3.4 — Key typing, key scope, and eviction

Still needed for slice 0 (the in-memory provider needs *some* retention/eviction answer
too — `expiresAfter` isn't durable-only), and doubly so whenever the future durable
provider (§3.2) gets filed: what type(s) can
`on:` accept (design notes say "any expression in scope" — does that mean anything
`Json.encode`-able, i.e. the same boundary-legal domain as the existing typed JSON codec,
`static-semantics.md`'s "The typed JSON codec" section?); whether the dedup record's key
namespace is automatically scoped (per-agent-instance? per-capability-composition? global
per-provider?) — this doubles as part of the threat model (§6); and whether eviction
follows `Cache`'s lazy/check-on-read model or needs to be proactive given dedup records may
be read from a *different* invocation than the one that wrote them.

Two more surface details in §12's own syntax don't match what's shipped, worth folding
into whichever slice settles the real call shape: **labelled arguments** — `dedup(on: x,
expiresAfter: y)` — have no grammar today; `call` (`tree-sitter-bynk/grammar.js:1406`) is
strictly positional (`sep1($._expression, ",")`), and `name: value` labelling exists only
inside `record_construction`'s `field_init`. And the **duration literal** `24h`/`7d` isn't
the shipped form either — `Duration` literals are `<int>.<unit>` (`5.minutes`, ADR 0112;
`bynk-syntax/src/ast.rs:1629`), not a bare numeric suffix. Neither is a deep problem on its
own (both are plausibly-addable surface, not architectural), but together with 3.1 they
mean §12's `dedup` example has **three** independent points where it doesn't parse against
the shipped grammar today — good evidence it was written as illustrative pseudocode for
the architectural commitment, not as a literal preview of the surface. This track should
not treat any part of the example's concrete syntax as settled.

## 4. Candidate slice decomposition

With 3.1 and 3.2 settled and 3.3 deferred out of scope, this track's remaining work is
small: slice 0 is the only slice needed to deliver this track's actual end state (§1).

- **Slice 0 — the match-based short-circuit + the (sole) in-memory provider.** Depends on
  [#926](https://github.com/accuser/bynk/issues/926) (generic capability methods) landing
  first or alongside. Ship `given Idempotency` with its one provider (in-memory,
  handler-local), proving the §3.1 shape end-to-end against a `tsc_verify` case
  reproducing (a syntactically corrected version of) §12's `reserve` example, and settle
  3.4 (key typing/scope/eviction) as part of the same slice, since the provider can't ship
  without it.
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

## 6. Threat model

**Asset.** The cached outcome recorded against a dedup key — potentially containing
domain data (§12's `ReserveOutcome`, a `Receipt`, etc.) — and the dedup mechanism's
correctness guarantee itself (a wrongly-short-circuited handler silently skips real work).

**Adversary.** A caller who controls or can guess part of a dedup key. §12 explicitly
sanctions caller-supplied keys for non-idempotent receivers ("the caller supplies a
deterministic identifier... the receiver dedupes against it"), and those keys can
originate from external actors — an HTTP client-supplied idempotency header is the classic
case. If the key namespace (3.4) is not automatically scoped per-caller/per-tenant/per-
composition, a second caller who reuses or guesses a first caller's key receives the
*first caller's* cached outcome rather than executing their own request — a cross-tenant
data leak if the outcome carries tenant-specific data, and a correctness bug even when it
doesn't (wrong operation silently skipped). This risk is structural, not an implementation
slip: it exists because the capability's entire contract is "key equality means treat as
the same call", and nothing in §12 states who is responsible for making sure two
unrelated calls can never coincide on a key.

**Where verification happens.** The design notes place the uniqueness burden on the
*caller* ("the caller supplies a deterministic identifier derived from its own context")
— this track should make that a checked or at least visibly-documented discipline, not a
silent assumption. Candidates worth weighing during settling: auto-composing the key with
an implicit scope (composition/context/agent identity) the developer cannot omit, so a
caller-supplied component is always only *part* of the effective key; or leaving it fully
manual but requiring the emitted/generated code to make the effective key visible for
review (mirroring how `Sagas.compensate` targeting a non-idempotent operation is "a latent
bug the explicit call gives the reviewer somewhere to look" — §13). No enforcement
mechanism is settled yet; this section exists so the question is not lost by the time a
future durable provider (§3.2) makes the blast radius real — it applies to the in-memory
provider too, just with lower stakes (a shorter typical retention window, no
crash-survival exposure).

## 7. Slice status

- [ ] Slice 0 — match-based short-circuit + the in-memory provider (incl. 3.4)
- [ ] Slice 1 — event-subscriber sugar (unscoped)

## 8. Done when

- `given Idempotency` is checked and its one provider is usable; §12's worked examples (or
  their settled corrections) compile and pass a `tsc_verify` case.
- The match-based short-circuit (3.1) ships on top of
  [#926](https://github.com/accuser/bynk/issues/926) (generic capability methods), with no
  bespoke control-flow construct introduced for `Idempotency` itself.
- 3.4 (key typing, scope, eviction) has a stated answer, and the key-scoping threat (§6)
  is addressed, not implicit.
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
