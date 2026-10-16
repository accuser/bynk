# Interop with existing TypeScript Workers — the wire contract, foreign callers, and calling out

- **Status:** Settling. Opened 2026-10-16. D1–D4 (§8) and every §11 question
  were closed with the maintainer on 2026-10-16: the recommended option
  wherever the draft gave one, and the option its analysis favours elsewhere. §3.3's live defect is being fixed outside the track (§11 Q1).
- **Realises:** `design/bynk-design-notes.md`'s bounded-context model at the
  edge of a deployment: a context is a Worker, and a team adopting Bynk keeps
  its other Workers. Also ADR 0092's named extension ("signed caller headers
  … extend this additively if a multi-trust-domain model ever arrives") and
  ADR 0310's deferred collaboration story (a published surface other code can
  build against).
- **Posture:** Feature track per [ADR 0076](../decisions/0076-feature-track-posture.md).
  Qualifies on all three axes (§1.2).
- **What's already true** (surveyed at v0.330.x, 2026-10-16, against
  `bynk-lang/shop` compiled with `--target workers`): a context's `on call`
  services are reachable as `POST /_bynk/call/<service>` over a Service
  Binding, with an `X-Bynk-Contract` hash and an `X-Bynk-Caller` identity
  header (ADR 0092, 0200). The codec is generated per type (ADR 0199). It is
  specified only for the compiler (`spec/emission.md` §7.3.4a–c), and none of
  it is promised to code outside Bynk. A
  context can call only other Bynk contexts privately; anything else goes over
  the public internet (`global_fetch_strictly_public`, every generated
  `wrangler.toml`). Spine [#1973](https://github.com/bynk-lang/bynk/issues/1973).
- **Governing records:** ADR 0023 (platform work and stdlib work never share
  an increment), 0045 (typed JSON codec, no untyped `Json`), 0092 (caller
  identity, one trust domain), 0199 (one codec path at the Workers boundary),
  0200 (the contract hash: one canonical form, fail-closed at runtime, refused
  at deploy), 0305–0308 (the wire-contract IR, `WireModel`, and why the hash
  form stays separate from it), 0310 (only the four-item emit ABI is
  published), 0462/0464/0466 (one binding list per Worker), and
  `bynk-1.0-definition.md` Gate 1 ("the emitted TypeScript" is not frozen).

## 1. The theme

### 1.1 What this track settles

A team rarely starts a Cloudflare account with Bynk. It has Workers already,
written in TypeScript, and it wants to move one bounded context at a time.
Today it can do that only by putting the Bynk Worker in front, or by calling
its public `from http` routes. Each private path either works by accident or
does not exist:

| Direction | Today | Issue |
|---|---|---|
| TS Worker → Bynk context, over a Service Binding | Works, against a private, unpublished codec | #1969, #1970 |
| TS Worker → Bynk context, *as whom* | It can claim any context's identity | #1971 |
| Bynk context → TS Worker, over a Service Binding | Impossible; only public `Fetch` | #1972 |

This track decides four things (§8):

1. **D1:** whether and how the cross-context wire format becomes a published,
   versioned contract (#1969).
2. **D2:** what identity a non-Bynk caller gets, given ADR 0092's single trust
   domain (#1971).
3. **D3:** what a TypeScript caller builds against — a generated client over
   the published format, or Cloudflare's native RPC — and how its build sees
   skew (#1970).
4. **D4:** how a Bynk context declares and calls a non-Bynk Worker over a
   Service Binding (#1972).

**End state when this track retires:** a TypeScript Worker can call a Bynk
context through a generated, typed client whose build breaks on skew, under an
identity the callee can tell apart from every Bynk context; a Bynk context can
call a named TypeScript Worker privately; and the format underneath is
documented, versioned and guarded.

### 1.2 Why a track (the ADR 0076 trigger)

- **Multi-increment.** A documentation and guard slice, an identity change in
  the emitted boundary, a new emitted artefact (the client), and a new kind of
  binding. ADR 0023 keeps the platform-capability half (D4) out of any
  increment that touches the language.
- **Surface not yet settled** when the track opened: the stability promise
  for the format, the foreign-caller identity, the client's shape and the
  outbound binding's syntax (now settled, §8 and §11).
- **Security/safety boundary.** D2 is an authentication boundary. The survey
  for this doc found the exposure is wider than #1971 states (§3.3): the
  `/_bynk/call/` prefix is reachable from the public internet whenever the
  Worker has a public route, not only from Workers holding a binding.

## 2. Vocabulary

- **Bynk caller:** a Worker emitted by `bynkc` that reaches another context
  through `consumes`. It stamps `X-Bynk-Caller` with its qualified context
  name.
- **Foreign caller:** any other code that sends a request to
  `/_bynk/call/<service>`: a hand-written Worker holding a Service Binding, or
  anything that can reach the Worker's public routes.
- **Foreign callee:** a non-Bynk Worker a Bynk context wants to call (D4).
- **Wire contract:** the request and response format of
  `POST /_bynk/call/<service>`, including the JSON encoding of every boundary
  type. Distinct from the **contract hash** (ADR 0200), which is one header
  value inside it.

## 3. What exists today

### 3.1 The call boundary

Fixture `bynkc/tests/fixtures/positive/216_cross_context_caller/` is the
golden example. The emitter is `bynk-emit/src/emitter/workers_entry.rs:688-807`
(dispatch) and `:2251-2393` (`emit_call_handler_dispatch`). The caller is
`callService` (`bynk-emit/runtime/src/boundary.ts:114-190`, bundled into
`bynk-emit/src/emitter/runtime.ts`). The only normative text is
`spec/emission.md` §7.3.4a–c, written for the compiler, not for outside
callers. `spec/runtime-library.md:190` still documents a `callService`
without the contract hash, so even that text is partly stale.

**Request.** The caller sends
`POST http://internal/_bynk/call/<service>` with `content-type:
application/json`, `X-Bynk-Caller: <qualified context name>` (for example
`shop.orders`) and `X-Bynk-Contract: <16 hex digits>`. The body is `{}` for
no parameters, the bare encoded value for one, and an object keyed by
parameter name for two or more.

**Callee checks, in emitted order:**

| # | Check | Failure | Status | Body |
|---|---|---|---|---|
| 1 | Path names a service with an `on call` handler | unknown service | 404 | text `Not found` |
| 2 | `X-Bynk-Contract` equals the callee's constant (missing fails too) | skew | 409 | `{"kind":"ContractMismatch","service":…,"expected":…,"actual":…}` |
| 3 | `await request.json()` | malformed JSON, or a bodyless `GET` (inferred from the emitted code, not run) | **500** | text `Internal Server Error` |
| 4 | `X-Bynk-Caller` non-empty, **only** if the handler has `by c: Caller` | missing caller | 401 | `{"kind":"Unauthorized","details":"missing caller identity"}` |
| 5 | Argument decode (`$` for one parameter, `$.<name>` for several; extra keys ignored) | shape or refinement | 400 | the `BoundaryError`: `{"kind":"StructuralMismatch","path","expected","actual"}` or `{"kind":"RefinementViolation","path","violation":{…}}` |
| 6 | Handler runs | any throw, including an out-of-range `Int` on encode | 500 | text `Internal Server Error` |
| — | Success, including a domain `Err` | — | 200 | the encoded return value |

There is no method check, no content-type check and no body-size limit on
this path. ADR 0092 says an absent caller is "rejected before the body"; the
emitted code reads the body first (step 3 before step 4).

**Caller handling.** A 409 `ContractMismatch` is logged and thrown as a
`BoundaryError`; every other non-2xx becomes `{kind:"Transport", status,
details}`. A 2xx body is decoded with the return type's codec. Errors are
thrown, not returned, so a skewed call surfaces as a 500 in the calling
Worker.

**Encoding** (`bynk-emit/src/emitter/serialisation.rs`; the IR is
`bynk-check/src/wire.rs`):

| Bynk type | JSON | Decode rule |
|---|---|---|
| `Int` | number | `Number.isSafeInteger`, else `StructuralMismatch` `expected: "safe integer"`; encoding an unsafe value throws (ADR 0199 B, ADR 0425) |
| `Float` | number | finite only; encoding a non-finite value throws |
| `Bool`, `String` | boolean, string | `typeof` |
| `()` | `null` | value not inspected |
| `Bytes` | base64 string, padded | alphabet and length checked |
| `Instant`, `Duration` | number (epoch ms, whole ms) | safe integer at top level; in a field, `Duration` is not guarded (`wire.rs:1066-1072`) |
| record | object, keys = field names | object guard, field by field; extra keys ignored; defaulted fields optional |
| sum | `{"kind":"<Variant>", <payload fields flattened>}` | unknown `kind` is `StructuralMismatch` |
| `Result[T, E]` | `{"kind":"Ok","value":…}` / `{"kind":"Err","error":…}` | — |
| `Option[T]` | `{"kind":"Some","value":…}` / `{"kind":"None"}` | lenient (ADR 0450, #1887): `null`/absent is `None`, and a bare value is `Some` |
| `List[T]` | array | element-wise |
| `Map[K, V]` | array of `[k, v]` pairs, insertion order | — |
| named refined type | its base value | the owner's `T.of`, failing as `RefinementViolation`; a caller decoding an *opaque* callee type checks the base only |
| inline field refinement (`f: Int where NonNegative`) | base value | **not validated** on decode |

In memory the discriminant is `tag`; on the wire it is `kind` (`wire.rs:1192`).
ADR 0310's emit ABI names "`Result`/`Option` tag layout" without saying
which of the two forms it means; D1 has to make that explicit.

### 3.2 The contract hash and skew detection

**Normal form** (`bynk-check/src/contract.rs:48-70`):
`<service>(<p>: <T>, …) -> <T>`, with `Effect` stripped. Named types render
nominally: a record as `Name{f: T, …}` with fields sorted, a sum as
`Name|A|B(T)` with variants sorted, a transparent refined type as
`NameBase where P1, P2` with predicates sorted and de-duplicated, an opaque
type as `NameBase opaque`. **Hash:** FNV-1a 64-bit over the UTF-8 bytes,
printed as 16 lowercase hex digits (`contract.rs:327-344`). It is not
cryptographic and was never meant to be (ADR 0200 F: "a skew detector, not a
security control").

| Changes the hash | Does not |
|---|---|
| service name; parameter names and order; return type | field or variant order; predicate order or duplicates |
| a field's presence, name or type; a variant added, removed, renamed or re-shaped | an opaque type's predicate |
| a transparent refined type's predicate set | doc comments; `by c: Caller`; `given` |
| renaming a named type (nominal) | field defaults; **inline field refinements** |

The last cell is a gap: `spec/emission.md:419` says a field's type must
change the hash, but `f: Int where NonNegative` hashes as `f: Int`, and (§3.1)
is not validated on decode either.

**Where skew is recorded.** Each Worker's `bynk-contracts.json`
(`bynk-emit/src/emitter/contracts.rs`) has `{"version": 1, "provides":
{svc: hash}, "expects": {"<context>": {svc: hash}}}`, with `expects` listing
only services actually called. `bynk deploy` compares `expects` with the
deploy ledger's `live_contracts` and refuses on `bynk.deploy.contract_skew`
(`bynk/src/deploy/graph.rs:139-175`). A dependency the ledger has no record
of produces no finding. `bynk.schema.lock` is the events schema registry
(ADR 0321), not part of this. Nothing records a consumer the compiler did not
build.

### 3.3 Who can reach `/_bynk/call/`

ADR 0092 says the prefix "is platform-dispatched and not externally routable",
and builds its trust model on that: forging `X-Bynk-Caller` is out of scope
because only the compiler writes it, and every context in a deployment is one
trust domain.

The emitter disagrees. Since the #1826 review, `workers_entry.rs` logs a
mismatched contract header bounded to 16 characters *because* "`/_bynk/call/`
is reachable from outside, and nothing about the request is trusted yet"
(`bynk-emit/src/emitter/workers_entry.rs:747`). The Worker's single `fetch`
entry point dispatches on path alone: a request on any route the Worker
serves, whose path starts with `/_bynk/call/`, reaches the call dispatcher.
The generated `wrangler.toml` sets neither `workers_dev` nor `routes`, so a
deployed context keeps wrangler's default `workers.dev` route (*verify*
against a real deploy, which the out-of-track fix does).

So the forgery #1971 describes is reachable two ways:

1. **From a Worker holding a Service Binding** (the issue's case). Granting a
   binding grants every context's identity.
2. **From the public internet**, for any context whose Worker is publicly
   routable, by a plain HTTP request to the call prefix. The contract hash is not a secret: it is in `bynk-contracts.json`, and a
   wrong one is echoed back in the 409 body.

Case 2 needs no interop at all, so it is a defect in today's boundary rather
than a feature of this track. It is recorded here because D2's design has to
close it anyway, and because a supported client (D3) must not ship before it
is closed. §11 Q1 settled it as an urgent fix outside this track.

### 3.4 Calling out

A Bynk context reaches other code three ways, and none is a private channel to
a non-Bynk Worker:

- `consumes` reaches only Bynk contexts. Each consumed context with services
  becomes a `[[services]]` binding, uppercased from its dotted name
  (ADR 0462/0464).
- `Fetch` goes over the public internet. Every generated `wrangler.toml` sets
  `compatibility_flags = ["global_fetch_strictly_public"]`
  (`bynk-emit/src/emitter/wrangler.rs:33`), so even a same-zone URL leaves the
  Worker.
- An adapter is wired in-process, "never over a Service Binding"
  (`reference/adapters.md`). Its binding class gets a deps object keyed by its
  `given` capabilities, never `env`.

The standard-library track's inventory already records this gap: "Service
bindings | S (language: `consumes context`) … Binding to a *non-Bynk* Worker
is A" (`standard-library.md` §6.3), without giving it a home.

## 4. Requirements

Every decision below must meet these.

- **I-1 One source of truth.** The documented wire format, the emitter and
  any generated client derive from one model. A prose spec that can drift
  from the emitter is not a contract.
- **I-2 Skew is caught before it ships.** ADR 0200 and 0310 set the bar:
  build-time guard, deploy-time refusal, fail-closed at runtime. A foreign
  caller gets at least the first and last.
- **I-3 No identity without a channel that proves it.** A handler that reads
  `c.identity` gets a value no request sender can choose freely. Where the
  channel cannot prove an identity, the handler is told so in the type, not
  in a comment.
- **I-4 Additive for Bynk-only projects.** A project with no foreign callers
  or callees keeps its emitted output, apart from changes that close §3.3.
- **I-5 Platform honesty.** Calling a foreign Worker is a Cloudflare-only
  capability and says so (ADR 0016, 0017, 0024). It is not portable.
- **I-6 The emitted TypeScript stays free.** `bynk-1.0-definition.md` keeps
  the codegen out of the frozen layer. Whatever this track publishes is a
  small, enumerated surface beside it, in the style of ADR 0310, not a freeze
  of the emitter.

## 5. Non-goals

- Calling a Bynk *agent* (Durable Object) directly from a foreign Worker.
  Agents stay behind their context's services.
- Non-Cloudflare foreign callers (another cloud, a browser). A public
  `from http` route already serves them.
- A general FFI to npm packages; adapters already cover in-process TypeScript.
- Cross-account Service Bindings, which Cloudflare does not offer.
- Inter-context *authorisation* (who may call what). ADR 0092 mints identity,
  not permission, and so does this track.

## 6. Prior art

- **Cloudflare JS RPC** (`WorkerEntrypoint`, compatibility date 2024-04-03 or
  later): a Worker exports a class whose public methods a bound Worker calls
  as `env.SVC.method(args)`. Arguments travel by structured clone, not JSON.
  There are no request headers on an RPC call; the callee sees the call, not
  a `Request`. *Verify* the current serialisation rules and `ctx.props`
  behaviour before D3 settles.
- **Cloudflare Service Bindings** (`fetch` form): the binding's `fetch`
  delivers a `Request` straight to the target Worker, never through the
  public internet. The target cannot tell, from the request alone, whether a
  request arrived by binding or by a public route.
- **gRPC / Protocol Buffers, Smithy, OpenAPI:** a published schema plus
  generated clients, with the wire format versioned separately from either
  side's implementation. The pattern D1 and D3 follow.
- **tRPC:** typed client from the server's own types, no published format.
  The pattern this track rejects for a cross-language boundary, because the
  client is only as stable as the server's source.

## 7. Prerequisites before any slicing

- **PR-1 Contain §3.3 case 2. SETTLED (§11 Q1): an urgent fix outside this
  track,** in its own PR: D2 (d), a shared per-deployment secret that
  `bynk deploy` mints and every `/_bynk/call/` request must carry, verified
  by the callee. I2 later replaces it with per-edge signatures. No slice and
  no reference note that advertises the call boundary to outsiders ships
  before it.
- **PR-2 One binding list.** Already met: ADR 0462/0464/0466 give each Worker
  one list driving `wrangler.toml`, `Env` and the duplicate check. D4 adds a
  kind to it.

## 8. Settled decisions

All four were settled with the maintainer on 2026-10-16 on the recommended
option, together with the §11 answers that refine them. Each
keeps its options and consequences for the record. Load-bearing ones land as front-loaded
ADRs with the slice that first depends on them (§10).

### D1 — The cross-context wire format as a published contract (#1969)

**The problem.** §3.1–3.2 are the format a foreign caller has to speak. It is
specified only for the compiler, has rough edges an outsider would trip on
(a malformed body is a 500, the method is never checked, inline refinements
are neither hashed nor validated), and `bynk-1.0-definition.md` puts it
outside the frozen layer as "emitted TypeScript". ADR 0200 adds that the hash
mints "detection, not evolution": there is no way to run two contract
versions side by side.

**Options.**

- **(a) Document as-is, marked unstable.** A reference page describes
  today's behaviour and promises nothing. *Cost:* cheap. *Consequence:* a
  client generated in one repository and a callee built in another can drift
  apart on any compiler release, with no rule to say which side is wrong.
  Fails I-2 for foreign callers.
- **(b) Keep the format private; the generated client (D3) is the only
  contract.** *Consequence:* the client is built at one time and the callee
  at another, from different compiler versions, so the format underneath
  still has to stay compatible. Hiding it does not remove the promise, it
  only leaves it unstated.
- **(c) Publish wire format version 1 as a separate, versioned surface.**
  - *What is published:* the path, method, headers and status codes; the
    JSON encoding of every boundary type; the error bodies; the contract-hash
    normal form and hash function. Nothing else of the emitted TypeScript.
  - *Source of truth (I-1):* the reference page is generated from
    `WireModel` (ADR 0307) and the hash normal form in `contract.rs`, with a
    drift guard in the style of `bynkc/tests/grammar_reference.rs`, so the
    prose cannot drift from the emitter.
  - *Clean-up before freezing:* fix or deliberately keep each rough edge in
    §3.1–3.2: `POST` only (405 otherwise), malformed JSON as 400
    `MalformedJson` rather than 500, the caller header checked before the
    body is read, D2's signature verified after the raw bytes are read (under
    a body-size cap, 413 above it, since §3.1 has no limit today) but before
    `JSON.parse` and the codec decode, inline field refinements hashed and validated,
    and the `Duration` field guard. `Option`'s lenient decode stays: ADR 0450
    adopted it precisely so non-Bynk JSON decodes, so it is part of the
    format, and a foreign caller may send either form.
  - *Stability rule for 1.x:* additive only within wire version 1. A new
    status code, a new error `kind` (callers must treat unknown kinds as
    errors), or a new optional header may be added. Changing an existing
    encoding or check is a new wire version. The callee announces the
    versions it accepts and the caller the one it speaks
    (`X-Bynk-Wire: 1`, absent meaning 1), so a version can be retired with an
    overlap window rather than a flag day (§11 Q2).
  - *The contract hash is part of the format.* Its normal form and FNV-1a
    function are frozen within a wire version, because a foreign client
    embeds hashes computed by an older compiler. Changing the normal form is
    a wire-version change.

**SETTLED 2026-10-16: (c).** Versioned independently of the compiler, with
an `X-Bynk-Wire` version header the callee accepts a range of (§11 Q2). It
is the floor for D3 and for any non-Bynk caller,
and it costs the codegen nothing beyond the clean-up list: like ADR 0310's
emit ABI, it is a small enumerated surface beside a codegen that stays free
(I-6).

**Consequences.**

- `bynk-1.0-definition.md` Gate 1 gains one sentence: the emitted TypeScript
  is not frozen, *except* the published wire format, which follows its own
  version. ADR 0310 D1's "exactly four items" gains a second published
  surface beside it, versioned on its own.
- The clean-up fixes change Bynk-to-Bynk behaviour (a 500 becomes a 400 or a
  405, a refinement starts being checked, inline refinements start changing
  hashes). They land before version 1 is declared, as ordinary pre-1.0
  increments. Changing the hash normal form forces one redeploy of every
  context (ADR 0200's flag day, once).
- `spec/runtime-library.md:190` is corrected, and ADR 0092's "rejected
  before the body" becomes true (settled with D1's clean-up).

### D2 — A non-Bynk caller's identity (#1971)

**The problem.** ADR 0092's identity is `CallerId = String`, the calling
context's qualified name, read from a header any sender can set. §3.3 shows
the senders are not only Bynk Workers. A `by c: Caller` handler therefore
trusts a value it cannot verify, and nothing in its type says so.

**Constraint.** Actors, `Caller` and `by` binders are Foundations-layer
(`bynk-1.0-definition.md` Gate 1). Whatever D2 adds must leave a 1.0
Foundations program compiling with its documented semantics, so the
change has to be additive to `Caller`, or a tightening of runtime behaviour
that only refuses requests no Bynk caller sends.

**Options.**

- **(a) Document only.** State in the reference that granting a Service
  Binding to a non-Bynk Worker grants it every context's identity, and that a
  public route exposes the same. *Cost:* nothing to build. *Consequence:*
  fails I-3, leaves §3.3 case 2 open, and makes D3's client an impersonation
  kit. Acceptable only as an interim note while (b) or (c) is built.
- **(b) A declared foreign-caller actor.** A context declares each foreign
  Worker it accepts as an actor with a new `call`-protocol scheme, beside
  `Bearer`, `Signature` and `Internal` (§11 Q3):
  `actor Legacy { auth = Foreign(secret = "LEGACY_CALL_KEY") }` (spelling
  fixed in I3's proposal). A handler admits it through the existing `by`
  sum, `by c: Caller | Legacy`, and the body `match`es the resolved actor,
  so "a Bynk context" and "the foreign caller `Legacy`" are distinct in the
  type of `c` without restructuring `CallerId`. *Weakness:* by itself it is still a header claim; it
  needs (c) or a channel proof to stop a foreign Worker from claiming
  `shop.orders`.
- **(c) A signed caller header.** Every caller signs
  `(caller, method, full path, contract, wire version, timestamp, body digest)`
  with HMAC-SHA256 under a per-binding secret. Every header the callee acts
  on is in the tuple, so an unsigned `X-Bynk-Wire` cannot make the callee
  decode the same bytes under another version's rules. The callee reads the
  raw body under a size cap, verifies in constant time before decoding
  the body and refuses unsigned, stale or mis-signed calls with 401. This is
  ADR 0092's named "multi-trust-domain integrity" extension. *Cost:* one secret
  per (caller, callee) edge, provisioned by `bynk deploy` through the secrets
  manifest; a crypto dependency on Web Crypto (the standard-library track's
  K4 `bynk.crypto` work, or a runtime-only verifier in `auth.ts` as Bearer
  already has); a small per-call cost (*verify*: not measured). It closes §3.3 for Bynk
  callers as well, not only foreign ones.
- **(d) A shared deployment secret.** One secret per deployment, sent as a
  bearer header. Simpler than (c), but every holder can claim every identity,
  so it closes §3.3 case 2 and nothing else.

**SETTLED 2026-10-16: (c) as the mechanism, (b) as the surface, (a) once
§3.3 is contained, (d) as the interim containment** (PR-1, §11 Q1).

- Ship (a) as a reference note as soon as PR-1's mitigation has landed, not
  before: a public note should not advertise an open boundary.
- Every Bynk-to-Bynk call becomes signed under a per-edge secret the deploy
  mints. The callee refuses a call whose signature does not verify for the
  claimed caller. This closes §3.3 for both cases without new syntax, and
  keeps ADR 0092's `CallerId` value unchanged for Bynk callers.
- A context that wants foreign callers declares each one (b). Its deploy
  mints that edge's secret and prints, in the deploy plan's output, the
  ready-to-run `wrangler secret put` command for the foreign Worker's
  operator; `bynk deploy` never writes to a Worker it does not own (§11 Q4).
  A call signed under a foreign edge's secret resolves to that actor, never
  to `Caller`, so it is structurally distinct from every context name.
- An undeclared foreign caller has no secret, so it cannot call any `on call`
  handler: every `/_bynk/call/` request must carry a valid signature,
  including one for a binder-less handler that reads no identity (§11 Q5).

**Consequences.**

- ADR 0092's "one trust domain" becomes "one trust domain per signing key".
  That needs a new ADR amending ADR 0092's trust paragraph (the repo's
  convention: a new record that amends or supersedes, never an in-place
  edit), landing with D2's first slice.
- The system-tier wire tests (ADR 0210, 0249, 0251) and the `bynk test`
  harness (ADR 0467) drive the dispatch directly, so they have to sign their
  calls too, with the same fixed key `bynk dev` uses.
- The callee's prologue gains a verify step before the body decode. Bynk-only
  projects see that change (I-4's stated exception) and a new secret per
  edge; `bynk deploy` provisions them, and `bynk dev` uses a fixed local key.
- `CallerId` stays `String`, and `Caller` keeps its meaning (a Bynk
  context), so the Foundations layer is untouched: foreign callers are a new
  actor scheme, which is additive (§11 Q3). The actor-sum rule "peers are
  distinguished by scheme (no duplicates)" has to admit several `Foreign`
  actors in one sum, told apart by their secrets; I3 settles how.
- **#1970 must not ship before D2's mechanism does.** A supported client
  (D3) would make the forgery a one-line import.
- Key rotation needs a two-key window (accept old and new). The deploy ledger
  records the edge, never the key.

### D3 — What a TypeScript caller builds against (#1970)

**The problem.** A TypeScript Worker calling a Bynk context today copies the
contract hash out of `bynk-contracts.json`, hand-encodes arguments in the
private codec, and learns about a signature change from a production 409. The
callee's `bynk-contracts.json` and the deploy ledger record only Bynk
consumers (`expects`, §3.2), so the callee's deploy cannot see the outside
consumer either.

**Options.**

- **(a) A generated typed client over the D1 format.** For each context with
  `on call` services, emit `out/workers/<context>/client.ts` (or a `.d.ts`
  and a small runtime): one function per service, boundary types as TS types,
  the contract hash embedded as a literal type, the codec shared with the
  Bynk caller, and D2's signing. The client imports its codec and signing
  helpers from a small runtime package versioned with the wire format (§11
  Q7), so it carries no copy of the codegen.
  - *Skew in the caller's build:* the client exports
    `CONTRACT = "4517a2cd32354abb" as const` per service, and the caller's
    deploy step can compare it with the callee's published
    `bynk-contracts.json`. A changed signature changes the client's TS
    types, so `tsc` breaks on the shape change in the caller's build. A
    change that alters the hash but not the TS shape (a transparent refined
    type's predicate, §3.2) does not break `tsc`, only the deploy comparison or the 409.
- **(b) Native JS RPC.** Emit a `WorkerEntrypoint` subclass beside the fetch
  dispatcher, with one method per service, plus a `.d.ts`. A caller writes
  `env.INVENTORY.reserve(sku, qty)`, as the platform intends.
  - *No header to stamp.* RPC calls carry no `Request`, so the contract hash
    and the caller signature have to move into the method surface (an extra
    leading argument, or a `connect(contract, credential)` method returning a
    stub) or into `ctx.props` if the platform's semantics fit (*verify*).
  - *A second codec.* Arguments travel by structured clone, not JSON. Refined
    types still have to be re-validated, and `Result`/sum values have to keep
    their tag layout, so the emitter grows a second boundary path beside
    ADR 0199's one codec path. That is the cost ADR 0199 exists to avoid.
  - *A second dispatch.* Bynk-to-Bynk calls stay on fetch (or move too, a
    larger change). Two dispatchers for one service is two places for the
    caller check (D2) to be wrong.
- **(c) Both:** (a) first, (b) later as an alternative entry point emitted
  from the same `WireModel`.

**SETTLED 2026-10-16: (a) now, with (b) recorded as a candidate for a future
track, not a commitment (§11 Q9).** One codec path (ADR 0199) and one
dispatcher keep D2's
verification in one place, and the client is a thin wrapper over a format D1
already documents. (b)'s ergonomic win is real but it is a second boundary
with its own security review; it becomes worth doing if Bynk-to-Bynk calls
move to RPC too, which is outside this track.

**Making the outside consumer visible to skew checks.** Three layers, after
ADR 0200:

1. **Build (caller side).** The generated client is checked into, or built
   by, the caller's repository. Its `CONTRACT` literals and TS types are the
   caller's lock.
2. **Deploy (callee side).** A context that declares a foreign caller (D2 b)
   also pins the contract hashes that caller's client was generated at, in a
   checked-in file beside `bynk.toml` (the foreign caller has no
   `bynk-contracts.json` of its own to supply them). `bynk deploy` compares
   the pins with the callee's new `provides` and reports a mismatch as it
   reports `bynk.deploy.contract_skew` for a Bynk consumer, and **refuses**
   (§11 Q6): updating the pin is the user's explicit acknowledgement that the
   foreign caller has been regenerated.
3. **Runtime.** The 409 `ContractMismatch` stands, and the callee logs it
   (#1826).

**Consequences.**

- A new emitted artefact per context, with its own drift guard: a fixture
  corpus test that the client's embedded hashes equal the callee's constants
  (the `bynkc/tests/contract_hash.rs` pattern).
- The client's runtime half ships as its own package, versioned with the
  wire format rather than with ADR 0310's emit ABI (§11 Q7).
- The callee's contract record learns about non-Bynk consumers. That needs D2's
  declaration to name them.

### D4 — Calling a non-Bynk Worker over a Service Binding (#1972)

**The problem.** §3.4. A Bynk context can call a foreign Worker only over the
public internet, which loses the private channel, adds latency, and needs the
foreign Worker to expose a public route.

**Options.**

- **(a) A first-party platform capability.** `bynk.cloudflare` gains a
  `Service` kind, used through the standard-library track's derived
  capabilities (its D3):
  `capability Billing = Service @service("legacy-billing")`. The provider
  sends a `Request` over the named binding and returns a `Response`: `Fetch`'s
  shape, privately. The `wrangler.toml` binding comes from the declaration,
  kind-prefixed (`SERVICE_BILLING`), through the one binding list
  (ADR 0462/0464), with the duplicate-name check for free.
  - *Fit with ADR 0023:* it is L3 platform work, so it is its own increment,
    separate from any language change. It needs no grammar beyond the derived
    capabilities the stdlib track already shipped (S4a, #1966).
  - *Typing:* the call is untyped (`Request` → `Response`), as `Fetch` is.
    A typed client for the foreign Worker is user code on top: a Bynk
    `provides` block over `Billing`.
- **(b) Adapters declare platform bindings.** An adapter states the bindings
  its binding class receives (`binding BILLING: Service`), and the context
  that consumes the adapter gets them in `wrangler.toml`. A hand-written
  typed client for the foreign Worker then lives in the adapter's TypeScript.
  - *Cost:* breaks the adapter model's "never `env`" property
    (`reference/adapters.md`): an adapter becomes platform-locked through a
    side door, which ADR 0024 keeps as first-party metadata. Every adapter
    author can now declare any binding kind, which widens the binding list's
    sources from compiler-derived to user-declared.
  - *Benefit:* the foreign Worker's typed surface lives in TypeScript, where
    its types already are.
- **(c) A language-level `consumes worker "legacy-billing"`.** Extend
  `consumes` to name a non-Bynk Worker. *Cost:* `consumes` today promises a
  typed, hash-checked contract; a foreign Worker has none, so the clause
  would mean two different things.

**SETTLED 2026-10-16: (a).** It reuses the instance model the stdlib track built
for exactly this kind of named, context-owned platform resource, keeps
adapters platform-neutral, and keeps the binding list compiler-derived. A
typed wrapper is ordinary Bynk over an untyped capability, the same layering
`Fetch` already has. (b) can be revisited if (a)'s untyped surface proves too
thin in practice; (a) does not foreclose it.

**Consequences.**

- The stdlib track's §6.3 row moves from A to this track's D4 slice; that
  track's §6.4 is unchanged (service bindings *to Bynk contexts* stay
  language work).
- `bynk deploy` does not provision the foreign Worker: it is not Bynk's. The
  deploy plan shows the binding and *warns*, never refuses, when the named
  Worker does not exist in the account (§11 Q8; I5 confirms the listing API).
- `bynk dev` binds a per-instance test double by default (the S4a pattern),
  and a locally running Worker of that name where Miniflare supports it
  (§11 Q8; I5 confirms).
- `global_fetch_strictly_public` stays: `Fetch` remains public, and `Service`
  is the only private path.
- A foreign callee called through (a) sees an ordinary request. If it is
  itself a Bynk context later, the user switches to `consumes` and gains the
  contract hash.

## 9. Candidate slice decomposition

Provisional until this doc settles. Each slice becomes an increment proposal,
filed as a sub-issue of the spine. Platform-capability work (I5, D4) never
shares an increment with language or emitter work (ADR 0023).

| Slice | What | Needs |
|---|---|---|
| I0 | *Outside this track* (§11 Q1): the urgent §3.3 fix, the D2 (d) shared per-deployment secret on every `/_bynk/call/` request, minted by `bynk deploy`. Listed for ordering only; I2 later replaces it | — |
| I1 | **D1, documentation and guard.** The wire-contract reference page generated from `WireModel` plus a drift guard; the wire-format version header; the format's ADR and the Gate 1 note; D2 (a)'s reference note | D1, PR-1 (I0 or the urgent fix) |
| I2 | **D2 mechanism.** Signed caller headers for every Bynk-to-Bynk edge; per-edge secrets in `bynk deploy` and a fixed key in `bynk dev`; ADR 0092 amendment | D2, I1 |
| I3 | **D2 surface.** The `Foreign` actor scheme on `call`, foreign actors in `by` sums, foreign edges' secrets minted by deploy with a printed `wrangler secret put` line | I2 |
| I4 | **D3.** The generated TS client, its drift guard, and foreign callers recorded in `bynk-contracts.json` and the deploy ledger | I1, I3 |
| I5 | **D4.** `bynk.cloudflare { Service }` as a derived capability, binding-list kind, dev stand-in, deploy plan line | Stdlib S4 (done) |

Order: I0 (the out-of-track fix) → I1 → I2 → I3 → I4, with I5 free to run
in parallel once settled, because it shares nothing with the others. This is
the D1 docs → D2 → D3 → D4 order with D4 unblocked early.

## 10. Front-loaded ADRs

The load-bearing, hard-to-reverse calls. Each lands with, or just before, the
slice that first depends on it. Numbers are assigned at merge.

1. **The cross-context wire format is a published surface with its own
   version.** D1. Amends ADR 0310's "exactly four items" by adding a second,
   separately versioned published surface, and adds a sentence to
   `bynk-1.0-definition.md` Gate 1. Lands with I1.
2. **Caller identity is proved by a per-edge signature.** D2 mechanism,
   amends ADR 0092's trust paragraph. Lands with I2.
3. **Foreign callers are declared actors with their own `call` scheme.** D2
   surface; `CallerId` and `Caller` are unchanged.
   Lands with I3.
4. **A non-Bynk Worker is reached through a `Service` platform capability.**
   D4. Lands with I5.

## 11. Settling-review questions (all resolved)

All nine were closed with the maintainer on 2026-10-16. Where the draft gave a
recommendation, it was accepted. Q3, Q4, Q6, Q7 and Q8 had none, and each was
resolved on the option this doc's own analysis favours, as stated.

1. **§3.3 case 2. Resolved: an urgent fix outside this track.** A separate PR
   contains the public reachability of `/_bynk/call/` with D2 (d), a shared
   per-deployment secret verified on every call request
   (`workers_dev = false` alone would not cover a Worker with `from http`
   routes). It also confirms the `workers.dev` default. PR-1 and I0 point at
   it; I2 replaces the shared secret with per-edge signatures.
2. **D1's stability rule. Resolved: versioned independently of the
   compiler**, like ADR 0310's ABI, with an `X-Bynk-Wire` header (absent
   means 1). The callee accepts a range of versions, so a version is retired
   after an overlap window rather than on a flag day. The contract hash keeps
   detecting signature skew within a version.
3. **D2 surface. Resolved: a new actor scheme for `call`.** The draft listed
   three spellings without picking one. The existing actor model decides it:
   actors already carry schemes, secrets and `by` sums with a `match` on the
   resolved actor, so a `Foreign` scheme gives a foreign caller a distinct
   type with no new declaration form. A `bynk.toml` table would put an
   identity the type checker needs outside the source. `CallerId` stays
   `String`; neither a sum type nor a reserved prefix is needed, which keeps
   the Foundations layer untouched (D2's constraint).
4. **D2 secret hand-off. Resolved: printed by `bynk deploy`.** The deploy
   mints the edge secret and prints a ready-to-run `wrangler secret put`
   command for the foreign Worker's operator. Writing to the foreign Worker
   was rejected because Bynk does not own it (D4 makes the same call for
   provisioning). Leaving it to the user entirely was rejected because a
   secret the user invents is weaker than one the deploy mints.
5. **Binder-less `on call`. Resolved: every request is signed**, as
   recommended. The boundary is not open because a handler ignores the
   identity.
6. **D3 pin semantics. Resolved: refuse.** ADR 0200's model is "refused at
   deploy", and a warning on a production deploy is easily missed. The
   callee's user updates the pin to acknowledge that the foreign caller was
   regenerated; that is the override.
7. **D3 packaging. Resolved: a separate package, versioned with the wire
   format.** D1 makes the wire format a surface with its own version, apart
   from ADR 0310's emit ABI (the in-memory layout). The client runtime
   renders the wire format, so it follows the wire version; joining the ABI
   package would tie two independently versioned surfaces together.
8. **D4 verification. Resolved: warn, and a test double by default.**
   `bynk deploy` warns, never refuses, when the named Worker is missing from
   the account. `bynk dev` binds a per-instance test double, and a locally
   running Worker where Miniflare supports it. I5 confirms both platform
   facts before it ships, as the stdlib track's P5 does for R2.
9. **RPC later? Resolved: recorded as a candidate for a future track**, as
   recommended, not rejected. It becomes worth doing if Bynk-to-Bynk calls
   move to RPC too.

## 12. Threat model

- **Assets.** A callee's data and side effects, gated by `by c: Caller`
  checks on `c.identity`; a foreign Worker's private endpoints (D4).
- **Adversaries.** (1) Anyone on the internet who can reach a Bynk Worker's
  public routes (§3.3 case 2). (2) A non-Bynk Worker in the same account given
  a Service Binding, compromised or merely buggy (case 1). (3) A replayed
  captured call.
- **Today.** (1) and (2) can claim any context's identity. The contract hash
  is public and echoed in 409 bodies, so it is not a credential and must
  never be treated as one.
- **After D2.** Each edge has its own secret, so a holder can claim only its
  own identity. A body digest and a timestamp bound the signature to one
  request for a short window, which D2's ADR must name (a requirement, not a
  tuning knob). Full replay protection needs a nonce store, which this track
  does not build (ADR 0092 left replay and ordering to the events track, its
  Q8). `on call` handlers are not idempotent in general, so a captured call
  can be replayed inside the window. A handler with side effects should use
  the Idempotency capability (#921) for that, and the reference note says so.
  Verification is constant-time and runs before the body is decoded, so an
  unauthenticated sender cannot reach the codec. The body is read under a
  size cap before it is hashed, so such a sender cannot make the callee
  buffer and hash an unbounded body either.
- **D4.** A `Service` capability reaches only the Worker named in the
  declaration; the binding list is compiler-derived, so user code cannot bind
  an arbitrary Worker at runtime.
- **Logging.** The callee keeps #1826's bounded logging of untrusted header
  values.

## 13. Ordering criteria

1. **Close the open boundary first** (§3.3, PR-1). Nothing that invites
   outside callers ships while the identity is forgeable.
2. **Document before you generate.** The client (D3) is a rendering of the
   format (D1), so the format and its guard come first.
3. **Identity before convenience.** #1970 does not ship before D2's
   mechanism.
4. **Independent work runs in parallel.** D4 shares no code with D1–D3 beyond
   the binding list, and is platform work (ADR 0023), so it may proceed as
   soon as it settles.
