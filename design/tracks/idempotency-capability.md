# The `Idempotency` capability — mechanical dedup for at-least-once delivery

- **Status:** Draft (settling). Spine issue
  [#921](https://github.com/accuser/bynk/issues/921) open; this doc landed via
  [#922](https://github.com/accuser/bynk/pull/922) ("Part of #921"), but that PR was marked
  ready for review and merged 55 seconds later with no review — the assertion that §3's
  questions are closed was never tested. They remain genuinely open; treat this doc as
  still settling, not adopted. Scopes issue
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
  is equally unimplemented. This track has no sibling implementation to mirror; both of the
  open questions below are genuinely novel.

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

- [x] **Multi-increment.** A working-but-narrow slice (single in-memory provider, one
  control-flow shape) is separable from the durable provider (§4.3) and from any
  Sagas-compensation or event-subscriber ergonomics layered on top later.
- [x] **Surface not yet settled.** Two structurally load-bearing questions have *no*
  existing pattern to lean on in the shipped language (§3.1, §3.2) — not "which of two
  known shapes", but "does either known-elsewhere shape even apply here".
- [x] **Security/safety boundary.** The capability's entire purpose is a correctness
  guarantee (§12's "safe by construction" claim); a wrong shape is either silently unsafe
  (doesn't dedup) or silently wrong (dedups across the wrong scope — see the threat model,
  §6).

## 3. Open design questions

### 3.1 — How does `dedup` short-circuit the *rest of the handler body*? (no existing primitive)

The design notes' worked example:

```
on reserve(qty: Int, orderId: OrderId) -> ReserveOutcome
    given Clock, Idempotency {
  <- Idempotency.dedup(on: orderId, expiresAfter: 24h)?
  ...
}
```

reads as ordinary `<- expr?` — an awaited capability call, propagated with `?`. But `?`
has one implemented shape, verified directly against `check_question`
(`bynk-check/src/checker/expressions.rs:2060`), not just the spec prose: the receiver must
be `Result[T, E]` (`bynk.types.question_on_non_result` otherwise; `Option[T]` is accepted
only inside an `HttpResult`-returning handler, `bynk.types.question_option_outside_http`
otherwise), the enclosing function's return type must itself be `Result[_, F]` or
`Effect[Result[_, F]]` (`bynk.types.question_outside_result` otherwise), and the call
always returns the **success** type `t` on the `Ok` path — the `Err(e)` path is what
propagates, forwarded or `embeds`-converted, never `t`. There is no path through this
function that injects an arbitrary externally-supplied `T` (a full cached `ReserveOutcome`
built by a *different* invocation) as this invocation's return value; that is categorically
different from anything `?` does. Bynk has **no `return` keyword** and no other early-exit
construct; the tail expression is the only way a block produces its value.

`reserve`'s return type, `ReserveOutcome`, is not shown as a `Result` in the excerpt, and
even if it were, `?`'s job is to forward `dedup`'s own `Err` variant *unmodified through the
same `Result[_, F]` shape* — it cannot inject an arbitrary previously-computed `T` (a full
cached `ReserveOutcome`, built by a *different* invocation of this handler) as this
invocation's return value. That is a different operation from anything `?` does today:
"abort the rest of this block and make the block's value be this externally-supplied thing"
is a control-flow primitive Bynk does not have. Firming this up is the track's central
question. Candidates to weigh during settling (not yet evaluated against each other):

- A **new statement form** — e.g. a `dedup`-flavoured bind that is checker-recognised
  (not just an ordinary capability call) and permitted only as a handler's first
  statement(s), lowered to an early-return in emitted TS. **No precedent exists for
  this in the shipped compiler.** The design notes' own closest analogue,
  `attempt`/`recover` (§13, fault-to-outcome conversion), is itself unimplemented —
  verified by grep: zero hits for `attempt`/`recover` in `bynk-syntax/src/keywords.rs`,
  `bynk-syntax/src/ast.rs`, or `tree-sitter-bynk/grammar.js`. So this candidate isn't
  "a second narrow control-flow form following an established pattern" — it would be
  the checker's **first** non-`?` control-flow special-case, full stop. That doesn't
  disqualify it, but it removes the main thing that made it feel like the safe default;
  weigh it as a genuinely novel mechanism, not an extension of one.
- **Restrict `Idempotency` to `Result`-returning handlers only**, and require the *whole*
  handler return type to literally be `Result[T, E]` (not a richer custom outcome sum —
  contradicting §12's own `PaymentConfirmed` example, which has no natural `Result` shape
  to unify with, since event subscribers return `Effect[()]`ish rather than a domain
  outcome). Needs checking against every worked example in §12, not just the first.
- **Something categorically different from `?` entirely** — e.g. `dedup` doesn't
  short-circuit *expression* evaluation at all; instead the *provider* wraps the handler
  invocation itself (interception rather than a call inside the body), and the visible
  `Idempotency.dedup(...)` call is closer to a declaration than a control-flow statement.
  This would be a real departure from the design notes' worked syntax and needs to be
  named as such if chosen, not quietly reinterpreted.

Whatever is chosen must also explain what happens to `given Clock, Idempotency` handlers
whose return type carries **no** domain outcome at all (design notes §13: "a handler
returning bare `Money`... any failure is a fault") — can such a handler use `Idempotency`,
and if the dedup path fires, what does it return?

### 3.2 — Provider-variant selection collides with ADR 0016

§12 states the capability "has multiple providers, the same way Sagas does: in-memory
(handler-local dedup, lost on restart)... and durable (records survive crashes)", with "the
handler shape... the same under both; the provider determines the durability semantics" —
i.e. a **developer picks which provider variant a given composition uses**, for one
capability, without changing the handler.

[ADR 0016](../decisions/0016-no-portable-infrastructure.md) ("No portable infrastructure
tier") already ruled on almost exactly this shape and rejected it: *"No selectable-provider
mechanism... A project's platform commitment is one greppable `consumes` line."* Decision
0005 (constructor injection) and the shipped `provides Cap = Impl { ... }` grammar back this
up structurally — one capability interface, one `provides` binding, wired by the compose
root in topological order (`bynk-fmt/tests/fixtures/09-capabilities-providers`). Nothing in
the shipped language today lets one project choose between two named implementations of the
same capability interface.

0016's context was specifically **cross-platform portability** (a lowest-common-denominator
`bynk.Kv` over Cloudflare KV vs. DynamoDB) — a different motivation from Idempotency's
in-memory-vs-durable axis, which is a **durability tradeoff available on a single
platform**, not a portability abstraction. Whether that distinction is enough to carve out
a narrow exception, or whether 0016's "no selectable-provider mechanism" consequence was
meant unconditionally, is itself the question to settle — likely via an ADR that either
narrows 0016's scope explicitly or extends the existing `provides` grammar with a
selection axis 0016 didn't anticipate. This cannot be answered by precedent; it has to be
decided.

### 3.3 — Does a durable provider need to join the *agent's own* atomic commit?

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

Secondary, but needs an answer before a durable provider can be built: what type(s) can
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

Provisional — the settling phase's job is to firm up 3.1–3.3 before slice boundaries can be
trusted.

- **Slice 0 — the control-flow primitive + an in-memory-only provider.** Settle 3.1.
  Ship `given Idempotency` with exactly one provider variant (in-memory, handler-local),
  proving the short-circuit mechanism end-to-end against a `tsc_verify` case reproducing
  §12's `reserve` example (or a corrected version of it, if 3.1 changes the surface). No
  provider selection yet — sidesteps 3.2 entirely by shipping only one provider.
- **Slice 1 — provider-variant selection.** Settle 3.2 (the ADR 0016 reconciliation).
  Extend `provides`/the compose root with whatever selection mechanism 3.2 settles on,
  proven against exactly two variants (in-memory, durable) rather than an open-ended set.
- **Slice 2 — the durable provider.** Settle 3.3 and 3.4. Build the durable backing store,
  the transactional-participation contract, key-scoping and eviction. The one most likely
  to reveal that 3.2's selection mechanism needs revisiting once a real second variant
  exists.
- **Slice 3 (possible, not yet scoped) — event-subscriber sugar.** §12's `e.eventId`
  canonical-key pattern for event subscribers; whether this deserves special syntax or is
  just documented convention once slices 0–2 land.

## 5. Front-loaded ADR candidates

- **The `Idempotency` short-circuit primitive** (3.1) — whatever control-flow shape is
  chosen, recorded as a decision distinct from `?`'s existing documented semantics, with
  the constraint on which handler return-type shapes can use `given Idempotency` stated
  explicitly.
- **Provider-variant selection vs. ADR 0016** (3.2) — must either narrow 0016's stated
  scope in an explicit follow-up decision or justify why Idempotency's variants are exempt
  from "no selectable-provider mechanism". Do not let this land as a silent contradiction
  of an Accepted ADR.
- **Durable-provider transactional participation** (3.3) — the contract a capability
  provider must satisfy to commit-or-abort alongside an agent handler it doesn't own, and
  its answer for service handlers (which have no `store` to join).

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
durable provider (slice 2) makes the blast radius real.

## 7. Slice status

- [ ] Slice 0 — control-flow primitive + in-memory provider
- [ ] Slice 1 — provider-variant selection
- [ ] Slice 2 — durable provider
- [ ] Slice 3 — event-subscriber sugar (unscoped)

## 8. Done when

- `given Idempotency` is checked and at least one provider variant is usable; §12's worked
  examples (or their settled corrections) compile and pass a `tsc_verify` case.
- The short-circuit mechanism (3.1) is a named, documented control-flow shape distinct from
  `?`, with its handler-return-type constraints explicit.
- Provider-variant selection (3.2) either amends ADR 0016's stated scope explicitly or is
  justified as consistent with it — never a silent contradiction.
- A durable provider exists with its transactional-participation contract (3.3) written
  down, and the key-scoping threat (§6) has a stated answer, not an implicit one.
- Issue [#554](https://github.com/accuser/bynk/issues/554) can be closed (or narrowed to
  its remaining two items — ADR 0020, the composition root — once Idempotency ships).
- ADRs written; spec gains the `Idempotency` capability's normative section. **On retire:**
  remove this doc.
