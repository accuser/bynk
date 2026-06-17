# Feature track — Actors: boundary contracts (`actor` + the `by` handler clause)

- **Phase:** **Building — Foundations (v0.45) + BearerToken (v0.47) + the optional
  binder (v0.50) + Signature (v0.51) + multi-actor sum dispatch (v0.52) landed.** The
  foundational ADRs are accepted: Q1 → [0080](../decisions/0080-actor-schemes-closed-nominal.md),
  Q2 → [0081](../decisions/0081-verified-identity-context-sealed.md),
  Q5 → [0082](../decisions/0082-by-clause-verify-then-body-defaults.md). The
  remaining per-question decisions below stay *proposed* until their slices land.
  See [ADR 0076](../decisions/0076-feature-track-posture.md) for the track posture.
- **Realises:** design notes §6 *Actor Declarations as Contracts*, §7 *Services
  and Protocol Composition* (the actor side).
- **Depends on / sequences after:** the v0.44 `from <protocol>` restructure (the
  `by` clause attaches to the header-relative handler forms). **Feeds:** the
  future Events / WebSocket / Alarm tracks (same boundary machinery).
- **Keywords already reserved:** `actor`, `by` (design notes, *reserved
  keywords*). Nothing new to reserve for the Foundations slice.

## Why a track and not a proposal

Multi-increment **and** surface-not-yet-settled **and** a security boundary —
all three of ADR 0076's triggers. A single delete-on-merge increment proposal
would discard the connective design, has nowhere to *work out* the unsettled
surface, and offers no first-class place for the threat model.

## The throughline

> **Karn already owns both primitives this feature needs** — refinement `where`
> types and closed, exhaustively-matched sums. An actor *invariant* is a
> refinement of a base actor; an actor *union* is a closed sum of base actors;
> an actor *identity* is a context-sealed value. The design adds almost nothing
> new to the type system — it composes what is already there.

The design was settled against a prior-art pass over (a) auth-as-types in typed
web frameworks, (b) object-capability security, and (c) nominal/structural
conformance + refinement subtyping. Sources are listed at the end; each decision
cites the precedent it rests on.

## Conceptual model (settled — from the design notes)

An `actor` declaration is a **contract type, not a runnable entity**. It tells
the language what the system expects of an external party, and the compiler
generates the boundary verification a service would otherwise hand-write. Its
information falls into four categories (design notes §6):

1. **Authentication scheme** — how the party proves identity (`None` anonymous,
   `Internal` in-system/platform, bearer token, request signature).
2. **Identity** — the typed value attached to verified messages, available in
   the handler and passed downstream to agents.
3. **Authorisation invariants** — properties that must hold for the party to *be*
   this actor (an `Admin` is a `User` carrying an `admin` claim).
4. **Replay / ordering** — what the runtime should expect (webhooks retry; queue
   producers may deliver out of order).

A handler **consumes** an actor on a `by` clause; the body runs **only if the
contract is satisfied**, payload already parsed, identity available as a typed
value. Anonymous endpoints are **not exempt** — they declare a `Visitor` actor
(scheme `None`). Multi-actor routes take a **sum** of actors and dispatch
*structurally* on which verified. Actors are **relative to a context**: a sibling
context calling in is an actor too (`auth = Internal`), verified statically.

## The surface (proposed)

Illustrative spelling — the *semantics* below are settled; exact tokens may move.

```karn,ignore
// Schemes are a closed, compiler-known set. An actor is a nominal contract on a
// scheme; `identity =` names the typed value a verified party yields.
actor Visitor   { auth = None }                        // anonymous; identity = ()
actor Scheduler { auth = Internal }                    // platform (cron/events/call)
actor User      { auth = Bearer, identity = UserId }   // external, authenticated
actor Admin     = User where hasClaim("admin")         // refinement of User (Q3)

service api from http {
  // HTTP requires an explicit actor; identity threads in as a named binding.
  on GET("/health") by v: Visitor () -> Effect[HttpResult[Status]] { … }
  on GET("/me")     by u: User    () -> Effect[HttpResult[Profile]] {
    // u.identity : UserId — verified at the boundary, a sealed value (Q2)
  }

  // Admin is a refinement of User (Q3): verify = User scheme + the claim.
  // Missing claim → 403; missing/invalid token → 401.
  on DELETE("/notes/:id") by a: Admin (id: NoteId) -> Effect[HttpResult[()]] { … }

  // Multi-actor = an ordered sum of *peer* actors, first-wins (Q4). Visitor
  // (auth = None) accepts everyone, so it must come last; `User | Admin` would
  // be rejected (every Admin is a User — a dead arm).
  on GET("/notes/:id") by who: User | Visitor (id: NoteId) -> Effect[HttpResult[Note]] {
    match who {
      User(u) => …   // u.identity : UserId — authenticated view
      Visitor => …   // public view
    }
  }
}

service sweeper from cron {
  // No `by`: inherits the protocol's default actor (Scheduler, Internal) (Q5).
  on schedule("*/5 * * * *") () -> Effect[Result[(), E]] { … }
}
```

## Settled design (proposed), by question

### Q5 — the `by` clause & handler shape

- **`by name: Actor` is a typed contract on the handler; identity threads in as a
  named binding — never ambient.** (Servant's "principal as the handler's first
  argument," but named. The explicit anti-pattern is Yesod's `requireAuthId`
  reaching into ambient monad state — the requirement must be visible in the
  signature.)
- **Two-phase semantics:** verification is a distinct phase that must succeed
  *before* the body runs, with the payload already parsed. This is tapir's
  `serverSecurityLogic → serverLogic` split and matches the design notes' stated
  service semantics verbatim.
- **Per-protocol / per-context default actor** (tapir's reusable secured-base
  endpoint): cron → `Scheduler`, queue → producer, `on call` → calling context,
  events → bus — all `Internal`. A handler inherits the protocol default unless
  it overrides with `by`. **HTTP has no safe default, so `by` is required there**
  (this closes most of the old Q9 — there is no implicit anonymous surface; a
  public endpoint writes `by v: Visitor`).

### Q1 — scheme set & representation

- **A closed, compiler-known scheme enum** (`None | Internal | Bearer |
  Signature`); the **actor is a nominal contract type** layered on a scheme.
- **Nominal, not structural, conformance.** Actor invariants are *semantic* (a
  carried claim, a replay expectation), not shape — and PEP 544 /
  `@runtime_checkable` is explicit that structural conformance can only witness
  *member presence*, never predicate-level constraints. A sealed vocabulary also
  needs a nameable conformance to seal.
- **"Sealed now, opened later" = widen the enum.** Backed by two prior-art
  mechanisms: Rust's private-supertrait sealed-trait idiom (seal *who may be* an
  actor; relaxing it later is non-breaking) and Scala `sealed` / Rust
  `#[non_exhaustive]` (exhaustive dispatch *inside* the defining module, a forced
  catch-all at the boundary so the set can grow without breaking external
  matchers).

### Q2 — identity model

- **The verified identity is a context-sealed value — not a plain record, and
  not (yet) a linear capability.** Lean on Karn's existing rule that
  context-owned types are minted only inside the owning context: per the W7
  result, *lexical/module ownership is itself a sufficient seal* — no crypto
  token needed. This upgrades unforgeability from by-convention to
  by-construction and removes any temptation for the agent to re-validate.
- **Identity flows service → agent and is never re-checked downstream.** This is
  the object-capability *introduction* rule, and re-checking would reintroduce
  ambient authority — the confused-deputy footgun "Capability Myths Demolished"
  warns against. The design notes' "agent never re-checks auth" is directly
  endorsed by 40 years of ocap practice (E, W7, Cap'n Proto).
- **Borrow Austral's signature-visibility of authority, not its linearity.** The
  agent's typed parameters make "operates under identity X" explicit and
  checkable; full linear consumption is the wrong ergonomic grain for an app DSL
  (a verified identity is read-many, fanned-out, logged, tested).

### Q3 — authorisation invariants *(the hard one)*

- **Desugar as refinement:** `actor Admin = User where hasClaim("admin")`
  lowers to `{ u: User | hasClaim(u, "admin") }`, reusing Karn's **existing**
  `where` machinery verbatim (new predicates like `hasClaim` plug into the same
  checker path as `Matches` / `InRange`). Consequences, all favourable:
  - the closed actor set stays closed — a refinement *carves a subset* of an
    existing actor, it does not add a member;
  - exhaustiveness stays tractable — refinements are *boundary predicates*, not
    match arms;
  - "an `Admin` is-a `User`" comes free from the refinement elimination rule
    (Liquid Haskell / F*), so an `Admin` is usable anywhere a `User` is.
- **Reserve nominal actor extension** as the escape hatch *only* if a sub-actor
  must add new structure/operations a base actor lacks (refinement can restrict,
  not add fields). **Reject composition/intersection** — it fights both the
  closed set and exhaustiveness.

### Q4 — multi-actor dispatch

- **A multi-actor handler is an *ordered sum of peer base actors*** — distinct
  parties distinguished by **scheme** (`by who: User | Webhook`,
  `by who: User | Visitor`), each *yielding* its own identity type. Identity is
  what a variant produces, not what discriminates it: resolution is scheme-level
  (below), so **two peers may not share a scheme** (a second same-scheme actor is
  unreachable), and any finer distinction — a token audience, a claim — is a
  refinement guard inside the arm, not a point on the sum axis. This is the one
  genuinely novel construct — no surveyed framework offers a closed, discriminated
  actor sum (Servant collapses multiple schemes to one shared principal and loses
  *which* verified).
- **Resolution is first-wins; the body matches on the resolved variant.** The
  boundary tries the variants in declared order and binds the first whose scheme
  verifies; the body then `match`es on the resolved actor (the variants are
  nominally distinct, so the match is well-defined). "First-wins" is the
  resolution rule; "structural" describes the post-resolution match — an earlier
  draft conflated the two.
- **Overlap is allowed and ordered; unreachable arms are rejected.** `Visitor`
  (scheme `None`) accepts everyone, so it is a catch-all and **must come last**.
  The compiler rejects unreachable arms by a *decidable, scheme-level* check (a
  `None` variant followed by anything; a scheme an earlier peer already
  subsumes); it does **not** reason about predicate-level disjointness.
- **Refinements are never sum variants.** `User | Admin` is rejected — every
  `Admin` is a `User` (Q3), so the arm is dead. Narrowing to a refinement is
  either the handler's sole contract (`by a: Admin`) or a guard within the
  resolved actor's arm. Keeping refinements off the sum axis is exactly what
  keeps the reachability check decidable and the sum-failure response
  unambiguous (Q6).

*Open at the multi-actor slice:* keying a peer by abstract **scheme** (above) is
the conservative default; it forecloses same-scheme multi-provider routes (e.g.
two `Signature` webhook senders with different secrets and distinct payload
identities). Admitting those would mean keying peers by concrete *verification*
rather than scheme — more expressive, with "peer = independent verifier,
refinement = base verifier + predicate" as the dividing line — at the cost of a
softer reachability guarantee. Deferred to the multi-actor slice, flagged so the
scheme-level default is not mistaken for a closed question.

### Q6 — verification seam, fail-closed, and the 401/403 split

- **Fail-closed is structural:** no verified identity ⇒ the body does not run
  (ocap's "no authority without designation"). The `by` clause is the boundary's
  authority-minting entry point.
- **The 401/403 split falls out of Q3:** authentication-scheme failure → **401**;
  a satisfied scheme but a failed *refinement* invariant (e.g. missing `admin`
  claim) → **403**. The two are structurally distinct error channels (Yesod's
  `AuthResult` trichotomy; tapir's separate security-error output), so the
  runtime can never confuse "who are you" with "you may not." For non-HTTP
  protocols the verdict maps to that protocol's outcome type (e.g. a rejected
  queue actor → drop/retry per `QueueResult`).
- **For a sum, the failure response is unambiguous: 401.** A sum's members are
  peer base actors with no invariants, so total failure means *no party
  authenticated* → **401**; there is no 403 path (authorisation invariants live
  only on refinement actors, which cannot be sum members, Q4). A handler whose
  sole contract is a refinement (`by a: Admin`) keeps the single-actor 401/403
  split.
- **Verification is side-effect-free and idempotent.** Beyond its declared
  secret/key lookup, scheme verification has no effects: first-wins resolution
  short-circuits, so the set and order of verifications attempted is observable,
  and audit/logging belongs *after* resolution, not inside the verification step.
  Re-running a verification yields the same verdict.

## ▶ Key decisions for reviewers

The design above is internally consistent and prior-art-backed; these three are
the load-bearing, hard-to-reverse commitments most worth scrutiny:

1. **Authorisation invariants ride the refinement system (Q3).** Elegant and
   reuses existing machinery, but permanently couples auth invariants to `where`.
   Is `hasClaim`-style predicate refinement the right long-term home, or do
   invariants eventually need their own vocabulary?
2. **Identity is a context-sealed value, not a plain record (Q2).** Strong
   guarantee at low cost, but it requires every identity type to be
   context-owned. Acceptable constraint?
3. **Multi-actor as an ordered sum of peer actors, first-wins (Q4).** The novel
   construct. Does ordered first-wins resolution over *peer* actors (refinements
   excluded, `Visitor` last), with per-arm identity types, hold up against real
   multi-actor routes? In particular: is scheme-level peer dispatch enough, or
   must same-scheme providers (two webhook senders) be expressible — see the Q4
   open note.

## Security / threat model

*Gates the verification-bearing slices (ADR 0076).*

- **Guarantees.** Verification is fail-closed and total: a handler body executes
  only with a verified, sealed identity of the declared actor type. Identity is
  unforgeable (context-sealed construction) and authoritative downstream without
  re-check.
- **Authentication vs authorisation.** Scheme verification (401) and invariant
  satisfaction (403) are separate phases and separate error channels; neither is
  expressible as the other.
- **Secret material** (bearer keys, signing secrets) is sourced via the secrets
  capability (ADR 0021), never logged, and lives only in the verification seam.
- **Explicitly out (for now):** rate limiting, replay windows beyond the
  `Idempotency` capability, key rotation, and any cryptographic primitive choice
  beyond what the first external-scheme slice commits to.
- **Blast radius.** A verification bug is a boundary vulnerability, so every slice
  that emits verification logic carries a `/security-review` + `/code-review`
  gate before landing.

## Internal architecture

A sealed **scheme descriptor** — the spine — mirroring v0.44's protocol
descriptor and the `karn`-surface capability treatment: each scheme contributes
its verification codegen, its admissible identity shape, and its
failure-response mapping, behind one uniform interface. Adding a scheme later is
new *surface* against that interface, not a re-architecture. The actor layer
(nominal contracts + refinements + sums) sits above the scheme descriptor and is
type-system reuse, not new machinery.

## Slice decomposition (proposed)

1. **Foundations.** ✅ **Landed (v0.45).** `actor` declaration + the `by` clause +
   identity binding + checker contract-satisfaction + the verification seam +
   per-protocol default actors — schemes limited to `Internal` + `None`
   (zero-crypto). Builds the whole machine; every later slice is "add a
   scheme/feature," not a re-build. *Foundational ADRs Q1/Q2/Q5 landed as
   0080/0081/0082.* (Q7's calling-context identity folded in as the typed
   prelude `Caller`; its live runtime value lands with the authenticated schemes.)
2. **BearerToken** ✅ **Landed (v0.47).** Compiler-generated JWT/HS256
   verification + secret sourcing (via `Secrets`/env) + fail-closed 401 shaping;
   the first external, authenticated, real (non-unit) identity, minted from the
   `sub` claim through the identity type. HTTP-only. *ADR 0085.*
3. **Signature / webhook** scheme (HMAC) + replay posture. ✅ **Landed (v0.51).**
   Compiler-generated HMAC-SHA256 over the raw body, configurable header
   (bare-hex or `sha256=` prefix), timestamp-tolerance replay window, raw-body
   read-once, fail-closed 401; no identity (authenticity, not a principal),
   body-required, HTTP-only. The seam lives in the entry dispatch (the body-read
   site). *ADR 0089.* (The **optional binder**, v0.50/ADR 0088, landed between
   slices 2 and 3 — `by Webhook (body: T)` is the canonical webhook form.)
4. **Multi-actor sum dispatch** (Q4) ✅ **Landed (v0.52).** A `by` clause may name
   an ordered sum of peer actors (`by who: User | Visitor`), resolved first-wins,
   the body matching on the resolved actor. Composes the three landed schemes
   (no new scheme); scheme-level peer keying, binder required, refinements
   excluded, `None` catch-all last, HTTP-only, total failure → 401. The boundary
   wrapper reads the body once and parses from the same bytes for a mixed
   header/body sum. *ADR 0090.* (Concrete-verification keying for same-scheme
   multi-provider routes is deferred — the Q4 open note.)
5. **Authorisation invariants** (Q3) — *its ADR lands here.*
6. **Cross-context `Internal` actors** (Q7) — may fold into Foundations.
7. **Replay / ordering hints** (Q8) — may ride with the Events track.

Each slice is an ordinary `vX.Y-<slug>.md` proposal citing this doc and the
foundational ADRs. Status tracked here as slices land.

## Still open (not closed by the prior-art pass)

- **Q7 — cross-context actors.** How a `Ref[OtherCtx.Agent]` call presents as an
  `Internal` actor to the callee, and its relationship to the existing
  cross-context service-binding dispatch (the v0.6 pipeline / `on call`). Likely
  static-only verification; to be settled with the Foundations or a dedicated
  slice.
- **Q8 — replay / ordering hints.** What an actor declares (webhook retry,
  at-least-once) and how it surfaces; ties to the `Idempotency` capability.
  Likely deferred to the Events track; the declaration shape must not foreclose
  it.

## Foundational ADRs to land

- **Q1** — auth schemes are a closed, compiler-known nominal set; actors are
  nominal contracts; sealed-now-openable-later (with the Foundations slice).
- **Q2** — verified identity is a context-sealed value, flows service→agent,
  never re-checked (with the Foundations slice).
- **Q5** — the `by` clause; two-phase verify-then-body; per-protocol default
  actors (with the Foundations slice).
- **Q3** — authorisation invariants desugar to refinement; the 401/403 split
  (with the invariants slice).
- **Q4** — multi-actor handlers are ordered sums of peer actors (first-wins,
  refinements excluded, `Visitor` last) with per-variant identity (with the
  multi-actor slice).

## Decision log (track-level)

- **v0.45 (Foundations):** Q1 → [0080](../decisions/0080-actor-schemes-closed-nominal.md),
  Q2 → [0081](../decisions/0081-verified-identity-context-sealed.md),
  Q5 → [0082](../decisions/0082-by-clause-verify-then-body-defaults.md). Q7's
  calling-context identity folded in (typed prelude `Caller`, value deferred).
  The remaining ADRs (Q3/Q4) are drafted as their slices land.
- **v0.47 (BearerToken):** [0085](../decisions/0085-bearer-token-jwt-hs256.md) —
  compiler-generated JWT/HS256, identity from the `sub` claim through the identity
  type, HTTP-only, fail-closed 401. The first authenticated identity; resolves the
  v0.45 `.identity`-lowering note.
- **v0.50 (optional binder):** [0088](../decisions/0088-optional-by-binder.md) —
  the `by` binder is optional (`by <Actor>` for anonymous / verify-and-discard);
  amends 0082. The canonical form for an identity-less scheme like Signature.
- **v0.51 (Signature):** [0089](../decisions/0089-signature-hmac-sha256-webhooks.md)
  — compiler-generated HMAC-SHA256 over the raw body, configurable header,
  timestamp-tolerance replay window, HTTP-only, body-required, identity `()`. The
  seam lives in the entry dispatch (the body-read site); generalises the scheme
  config to keyed args.
- **v0.52 (multi-actor sum):** [0090](../decisions/0090-multi-actor-sum-dispatch.md)
  — an ordered sum of peer actors (`by who: A | B`) resolved first-wins, the body
  matching the resolved nominal actor; scheme-level peer keying, binder required,
  refinements excluded, `None` catch-all last, HTTP-only, total failure → 401.
  Composes the three landed scheme seams; the boundary wrapper reads the body once.

## Prior-art sources

**Auth-as-types in typed web frameworks (Q5/Q1/Q2/Q4).**
Servant authentication — [tutorial](https://docs.servant.dev/en/latest/tutorial/Authentication.html),
[`Servant.Auth.Server`](https://hackage.haskell.org/package/servant-auth-server)
(the `Auth '[JWT, Cookie] User` combinator, `AuthResult`),
[generalized auth](https://github.com/haskell-servant/servant/blob/master/servant-server/src/Servant/Server/Experimental/Auth.hs)
(`AuthServerData` per-scheme principal). Scala tapir —
[server logic](https://tapir.softwaremill.com/en/latest/server/logic.html)
(`securityIn` / `serverSecurityLogic` two-phase). Yesod —
[authn & authz](https://www.yesodweb.com/book/authentication-and-authorization)
(`AuthResult` 401/403 trichotomy; `requireAuthId` as the ambient anti-pattern).
axum [extractors](https://docs.rs/axum/latest/axum/extract/index.html) (the
open-set contrast).

**Object-capability security (Q2/Q6).**
[Capability Myths Demolished](https://srl.cs.jhu.edu/pubs/SRL2003-02.pdf)
(ambient authority, the confused deputy). Austral —
[how capabilities work](https://borretti.me/article/how-capabilities-work-austral),
[capability-based security](https://austral-lang.org/tutorial/capability-based-security).
The [E language](https://en.wikipedia.org/wiki/E_(programming_language)) sealer/
unsealer and Miller's *Robust Composition*. Rees,
[A Security Kernel Based on the Lambda-Calculus](https://dspace.mit.edu/bitstream/handle/1721.1/36956/32890570-MIT.pdf;sequence=2)
(lexical ownership as a sufficient seal). *(Pony reference capabilities are about
data-race freedom — a different meaning of "capability"; kept out of scope.)*

**Conformance & refinement (Q1/Q3/Q4).**
Swift [SE-0156](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0156-subclass-existentials.md)
(composition vs inheritance). [PEP 544](https://peps.python.org/pep-0544/)
(structural Protocols can't carry predicate constraints).
[Rust sealed traits](https://rust-lang.github.io/api-guidelines/future-proofing.html)
and [`#[non_exhaustive]`](https://doc.rust-lang.org/reference/attributes/type_system.html).
Scala 3 sealed/enum exhaustiveness. Refinement subtyping —
[Liquid Haskell](https://nikivazou.github.io/lh-course/Lecture_01_RefinementTypes.html),
[F* intro/elim rules](https://fstar-lang.org/tutorial/book/part1/part1_getting_off_the_ground.html).
