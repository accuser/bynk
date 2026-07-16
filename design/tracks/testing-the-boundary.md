# Testing the boundary — teaching the tier dial the other door

- **Status:** **Settling — not adopted.** The spine is
  [#656](https://github.com/accuser/bynk/issues/656)
  ([ADR 0167](../decisions/0167-feature-tracks-run-github-native.md)); this doc
  lands via a **settling draft PR** ("Part of #656" — never `Closes`, which would
  kill the spine at adoption). Draft status *is* the settling phase: **Q2 is
  settled (§4.2.1)**; Q1 and Q3–Q6 remain open in §7 and must be closed before the
  PR is marked ready for review. Merging the PR settles *direction* and is **not**
  build authorisation — a slice is approved to build only when its own proposal is
  `accepted`. Live slice state is on the spine.
- **Realises:** the rung the retired testing track's subject ladder
  (`value → domain → call → snapshot → step → history`,
  [`../archive/retired-tracks.md`](../archive/retired-tracks.md)) never had — the
  **boundary**. It adds no new axis to the test model: it teaches the existing
  tier dial ([ADR 0153](../decisions/0153-tier-is-a-dial-on-the-case-header.md))
  the entry it was never taught.
- **Posture:** Feature track per
  [ADR 0076](../decisions/0076-feature-track-posture.md). Qualifies on **all
  three** axes — **multi-increment** (the addressing generalisation, the
  unit-tier surface, and the system-tier boundary are each their own MINOR with
  its own fixtures), **surface not yet settled** (there is no Bynk expression
  that names a route, and no agreed spelling for one), and **a security/safety
  boundary** (§6 — though far less of one than a first pass suggests).
- **Front-loaded decisions (named, not numbered):** **a tier already chooses the
  entry — an http service has a second door** (§3); **`system_needs_wire` relaxes
  to "≥ 2 contexts *or* a public boundary"** (§3.4); **a case names its principal
  with `by <Actor>(<identity>)` at the call site — an actor is the caller, not a
  seam** (§4.2.1, Q2 settled); **what that principal means is tier-dependent —
  injected at `unit`, a credential at `system`** (§4.2). Each is created and
  numbered by the slice that lands it — this doc deliberately does not
  pre-allocate numbers, since concurrent tracks would collide.

## 1. Motivation

Every one of the ten projects under `examples/` declares a trigger-driven
service. Not one has a test that drives it. Every example test file opens by
disclaiming exactly that:

- `examples/todo/tests/todos.bynk:3` — constructs the agent by key, "drives its
  handlers directly"; the `service api from http` is never entered.
- `examples/link-shortener/tests/codes.bynk:3` — "no Random, no KV, no HTTP".
- `examples/feature-flags/tests/keys.bynk:3` — "no KV, no auth, no HTTP".
- `examples/uptime-monitor/tests/status.bynk:3` — "no Fetch, no KV, no cron".

Across `bynkc/tests/fixtures/positive/`, 29 fixtures contain `from http` and 35
contain a `suite`. **The intersection is empty.**

Three things make this more than a coverage statistic.

**The untested part is the part the examples exist to demonstrate.** `todo`'s
headline is the sealed identity — "minted at the boundary, never forged
downstream" — and `src/todos.bynk:13` claims an empty or oversized title "is
rejected at the boundary before any handler runs". Both are boundary claims. The
tests cover the agent and are silent on every claim the example was written to
make.

**A test can assert a boundary claim while testing something that isn't the
boundary.** `examples/feature-flags/tests/keys.bynk:15`:

```bynk
case "an empty flag name is rejected at the boundary" {
  expect FlagKey.of("") is Err(_)
}
```

This tests the refinement *predicate*. If the emitted route table dropped its
`name: FlagKey` validation entirely, the case would still pass. The claim in the
case name is not the claim under test.

**`scheduled` and `queue` have never been executed — by anyone, including the
compiler.** They have golden-output coverage (emitted TypeScript diffed against
expected files), but no test in the repo calls `worker.scheduled(...)` or
`worker.queue(...)`; a grep across `bynkc/tests/*.rs` returns zero hits. The only
entry ever driven is `fetch`, by five files. Two of the four protocols have no
behavioural coverage at any level.

## 2. Scope and non-goals

**In scope:** driving a `call`, `http`, `cron`, or `queue` endpoint from a Bynk
`case`, at the tier the case declares.

**Deliberately separable — the unit-tier surface and the system-tier boundary are
two concerns.** Most of the value is in the first: naming a handler and calling
it with an identity, which needs no crypto, no `fetch`, and no ADR amendment.
The second — a full `fetch` request carrying a properly formed identity — is a
distinct problem with its own prerequisites (§4.2, §8). Conflating them is the
main way this track could stall: the cheap, high-value slice would end up waiting
on a signer and an ADR amendment it does not need.

**Out of scope — `websocket`.** It is structurally different, not merely harder,
and should get its own concept rather than be bent into this one:

- No addressable key. Routed by the `Upgrade` header alone; at most one WS
  service per context (`bynk-emit/src/project/validate.rs:734-744`).
- Its handlers emit **no service-surface methods on the Workers target at all**
  (`bynk-emit/src/emitter/emit.rs:965-975`) — the body runs inside a Durable
  Object driven by `webSocketMessage`/`webSocketClose`.
- `TestConnection` (`bynk-emit/src/emitter/runtime.ts:1216-1228`,
  [ADR 0131](../decisions/0131-from-websocket-protocol-bundle.md) D4) is real
  prior art for a test-time driver object — but it is **bundle-only**. That
  `bynkc/tests/ws_behaviour.rs` compiles against `BuildTarget::Bundle` while every
  HTTP suite compiles against `Workers` is forced, not incidental.
- Its interaction model is a *session* (open → n messages → close) asserting on
  accumulated `sent`/`closed` state — not request/response.

**Out of scope — runtime fidelity.**
[ADR 0009](../decisions/0009-integration-tests-simulated-wire.md) put
Cloudflare-runtime quirks explicitly out of scope for `bynkc test`;
`bynkc/tests/workers_runtime_smoke.rs` is the separate answer to fake-vs-real
drift. This track inherits that split and does not revisit it.

**Out of scope — a second router.** [ADR 0159](../decisions/0159-cors-policy.md)
D9 and [ADR 0162](../decisions/0162-http-method-correctness.md) D7 both state
there is no bundle-mode HTTP router and none is needed, because tests dispatch
through the emitted Workers `fetch` in-process. That is a feature for us: it is
precisely the seam §3 wants. Any design that emits a parallel test-only router
contradicts two settled ADRs and should be rejected on sight.

## 3. The core insight: the dial already knows one door

A `case` is a unit test unless it says otherwise. A unit test of an http service
should exercise **the handler**. That is not a workaround for a missing feature —
it is what the tier dial already means, and the model needs no new axis.

The tempting mis-reading (and the one this doc previously made) is that a tier
governs only the *outbound* realness of collaborators, leaving the inbound
boundary off the dial entirely. [ADR 0009](../decisions/0009-integration-tests-simulated-wire.md)
line 15 says otherwise:

> **Entry** and inter-participant calls travel the real serialise → JSON →
> deserialise path.

**The entry already crosses a real boundary at `system` tier.** A system case
enters through the emitted Worker's real `fetch`
(`bynk-emit/src/emitter/runtime.ts:246`):

```ts
const request = new Request(`http://internal/_bynk/call/${servicePath}`, { method: "POST", … });
```

So the dial is not boundary-blind. It enters at `/_bynk/call/` — the **internal**
door — because that is the only door it was ever taught. An http service has a
second door, the public route table, sitting in the same `fetch` a few blocks
later (`bynk-emit/src/emitter/workers_entry.rs:271-393`). The two are disjoint by
construction: the internal prefix is an exhaustive early gate (its `switch`
carries a `default: 404` *inside* the `if`), and `bynk.http.reserved_prefix`
(`bynk-emit/src/project/validate.rs:3178-3187`) forbids a user route under
`/_bynk/…`.

The hard stop is upstream of the dial, at `bynk-emit/src/project/symbols.rs:614-620`:

```rust
let Some(handler) = sdecl.handlers.iter()
    .find(|h| matches!(h.kind, HandlerKind::Call))
else { continue; };
```

Only `HandlerKind::Call` services enter `consumed_services`. Every http, cron,
queue, and websocket service is silently skipped — at every tier.

### 3.1 One body, three lowerings

The worked example, `examples/todo`:

```bynk
case "gets a user's todos" {
  stub User.identity returns "bob"

  let todos <- api.GET("/todos")
  expect todos is Ok(_)
}
```

| tier | `api.GET("/todos")` lowers to | the stubbed actor means |
|---|---|---|
| `unit` | `handlers.api.http_GET_todos(deps)` | inject `deps.identity` |
| `integration` | the same, with real collaborators | the same |
| `system` | `worker.fetch(Request)` — the real route table | a properly formed credential the seam verifies |

`unit` and `integration` are mechanically identical today in any case: ADR 0153
D8 ships "un-overridden seams keep their real provider", so the difference between
them is "the *default provision discipline* an author follows, not a
compiler-enforced auto-stub". There are really **two** lowerings — direct, and
through the door.

This is exactly ADR 0153 D1 — "**one body promoted, not a distinct kind of
test**" — and D7's "the body is byte-for-byte identical across tiers, so 'did I
stub this faithfully?' becomes a checkable question by changing one word".

### 3.2 What each tier proves

At `unit`, `"/todos"` is the handler's **name**. This is not a decorative string
standing in for a missing identifier: `HandlerKind::Http` carries no name
(`bynk-syntax/src/ast.rs:1110` — "For service handlers, this is None") and the
emitted key is a synthesised `http_GET_todos`. Method + path is the **only** name
that handler has.

Because it is a name, it is **resolved against the service's handlers at compile
time**. A mistyped path in a case is therefore an unknown-handler error, not a
green test — the same failure a mistyped method name would be. Promotion has
nothing to do with catching it, and a design in which the path is a free-form
string that only fails at `system` would be strictly worse.

At `system`, the same string becomes a **route**. What promotion surfaces is
everything *between the door and the handler* — the wrapper and the router, which
`unit` bypasses entirely:

- **The credential actually verifies.** At `unit` the case assumes an identity; at
  `system` it must produce one, and it travels `verifyBearerJwtHs256` →
  `UserId.of(sub)`. A misconfigured actor — a wrong secret env-var name, an
  identity type that rejects the `sub` it is handed — passes at `unit` and 401s at
  `system`.
- **The body actually deserialises, and refinements actually run.** At `unit` the
  case hands over a typed `AddRequest`, valid by construction. At `system` it is
  JSON through `deserialise_AddRequest`. This is §1's feature-flags case exactly:
  `FlagKey.of("")` proves the *predicate*; only a system case proves the **route
  applies it**.
- **Path parameters actually match.** `matchPath("/todos/:id/complete", path)` runs
  at `system` and not at `unit`, where the pattern is only a name.
- **Method matching and the 405/OPTIONS fall-through**, the `HttpResult` → status
  mapping, and the wrapper stack (§4.3).

That is D7's "promoting a `unit` case can surface a collaborator's invariant a
stub was hiding, with no new test code" — applied to the boundary, with no new
vocabulary.

**A mistyped route in the *source* is caught too — at compile time, by the same
name resolution.** If the handler declares `on GET("/todoss")` and the case names
the route the author meant (`api.GET("/todos")`), nothing resolves and the project
does not build. No promotion, no execution, no tier: a case that names a route
independently of the declaration is already a check that the two agree. (A case
that *copies* the route from the declaration checks nothing — but that is true of
every test that names the thing it tests, and is not a property of routes.)

### 3.3 The refinement problem dissolves

An earlier draft made heavy weather of this: a boundary test wants to assert that
an invalid `Title` is rejected with a 400 before the handler runs, but the type
system forbids constructing one, and [ADR 0182](../decisions/0182-refined-no-user-unsafe.md)
closed every hatch (`.unsafe` removed; `Val[T]` pins rejected via
`bynk.val.literal_violates`; generation valid-by-construction per
[ADR 0149](../decisions/0149-generation-is-valid-inhabitants.md) D1).

Under the tier reading there is no tension. At `unit` there **is** no
deserialisation, so there is no rejection to test — the question does not arise.
At `system` there is a real `Request`, so raw JSON is the natural argument and no
refined value is ever minted. **The 400-before-the-handler test is inherently a
system-tier test.** ADR 0182 is untouched.

The one thing a slice must not do is frame this as "let a test construct an
invalid `Title`". That ask loses against a v0.156 decision that already rejected
a *weaker* version of it.

### 3.4 The one amendment: `system_needs_wire`

`suite todos as system` is refused today:

```
[bynk.tier.system_needs_wire] `system`-tier suite for `todos` has nothing to wire
  — the target consumes no other context
  note: … test a single context with `unit` or `integration`
```

`todo` is a single context that consumes nothing. Under ADR 0153 D5/D6 the
`system` tier is defined as the cross-context wired tier and requires ≥ 2
contexts, because when it was written the only wire *was* cross-context.

Once the public route table is a door, a single context with an http service
**does** have something to wire: its own boundary. The rule becomes:

> a `system` suite needs **≥ 2 contexts _or_ a public boundary**

This is a real amendment to a settled ADR, and it is the track's one genuine
change to the existing test model. It is narrow and well-motivated — far smaller
than the new orthogonal axis an earlier draft proposed. Note the diagnostic's
current note ("test a single context with `unit` or `integration`") also becomes
misleading and must change: those tiers have no boundary.

## 4. The surface

### 4.1 An address

`service + handler-address` is a *total* scheme, because one service is exactly
one protocol adapter (`bynk.service.mixed_protocols`) and every protocol has a
stable, source-derivable key:

| protocol | entry | dispatch key | in source? |
|---|---|---|---|
| call | `fetch` `/_bynk/call/` | service name | yes |
| http | `fetch` route table | method + path | yes |
| cron | `scheduled` | `event.cron` | yes (the schedule string) |
| queue | `queue` | `batch.queue` | yes (the header arg) |

The blocker is that the checker's only service-invocation branch is gated on the
literal `method.name == "call"` (`bynk-check/src/checker/calls.rs:1741-1744`) —
it matches on the method being *spelled* `call` rather than resolving a handler.
That single hardcode is why no other protocol is addressable at any tier, and it
is also live defect #654.

Naming routes (`on POST("/todos") as addTodo`) is the obvious idea and should be
rejected: it reinvents `on call`, and a name-addressed invocation would skip the
route table at `system` — the thing under test.

### 4.2 An identity, per tier

`deps_identity_binder` (`bynk-emit/src/emitter/emitter.rs:2240`) is the single
generalised field Bearer, Oidc, and Caller all thread through; `u.identity`
lowers to plain `deps.identity` (`bynk-emit/src/emitter/lower.rs:3533-3538`). The
handler is already a pure function of `(args, deps)` — there is no in-handler
auth code. The whole seam is the compose wrapper
(`bynk-emit/src/emitter/workers.rs:676-732`):

```ts
async http_GET_todos(request: Request) {
  const __authz = request.headers.get("Authorization");
  if (__authz === null || !__authz.startsWith("Bearer ")) return HttpResult.Unauthorized;
  const __secret = env["AUTH_JWT_SECRET"] ?? globalThis.process?.env?.["AUTH_JWT_SECRET"];
  const __claims = await verifyBearerJwtHs256(__authz.slice(7), __secret);
  if (__claims.tag === "Err") return HttpResult.Unauthorized;
  const __id = handlers.UserId.of(__claims.value.sub);
  return handlers.api.http_GET_todos({ ...deps, identity: __id.value });
}
```

**At `unit`** the wrapper is not in the picture: the case calls the handler, and a
stubbed actor injects `deps.identity`. `makeTestDeps()`
(`bynk-emit/src/project/tests_emit.rs:3589-3648`) emits no `identity` field today,
which is live defect #655 for the one binder shape that is already addressable.
Fixing it is the foundation for this tier.

**At `system`** the wrapper *is* the point, and it only reads a header — so an
injected identity has nothing to attach to and would be inert. The case must
present a properly formed credential. Two facts make this cheaper than it looks:

- The emitted secret read **already falls back to `process.env`**
  (`bynk-emit/src/emitter/workers.rs:693`), so a Node harness can supply the test
  secret with **no compiler change**.
- `bynkc/tests/bearer_auth.rs:96-108` already contains a complete HS256 signer in
  Node (`crypto.subtle.importKey` + `crypto.subtle.sign`). It is ~15 lines,
  already written, and simply not in the emitted runtime.

So a stubbed actor at `system` arranges a credential that really verifies: the
identity is minted at the real seam, by the real code. The case never asserts
"the identity is bob"; it arranges "a caller who *is* bob".

The spelling is **`by <Actor>(<identity>)` at the call site** — settled, §4.2.1.

#### 4.2.1 The spelling: `by <Actor>(<identity>)` at the call site — SETTLED

```bynk
let todos <- api.GET("/todos") by User("bob")
```

**What is being spelled is a principal, not a substitution.** `stub` replaces a
*collaborator's return value*; an actor is not a collaborator, it is the **caller**.
That is a different semantic category, and it is exactly why
[ADR 0154](../decisions/0154-test-doubles-are-provides.md) D4 reads as it does —
it *positively* assigns participant realness to the tier dial rather than to a
stub. Widening `stub` to actors is rejected on three further grounds, each
independent of that ADR:

- **The grammar wants an operation.** `stub_clause` is
  `stub <cap>.<op>( args ) <rhs>` (`tree-sitter-bynk/grammar.js:940-945`);
  `.identity` is a field, so `stub User.identity()` invents a call that does not
  exist.
- **`returns` would mean two irreconcilable things.** At `unit` it would supply an
  identity; at `system` it would arrange a credential the seam mints *from*. One
  keyword cannot honestly carry both.
- **It forces the payload to be spelled by hand.** For
  `actor Admin = User where hasClaim("admin")` the case would hand-write the
  claims, duplicating the actor declaration and free to drift from it.

**Why `by`, and why the call site.** `by` gives parameter/argument duality on one
keyword — `by u: User` at the declaration ("*a* User may call; bind them as `u`"),
`by User("bob")` at the call site ("the caller *is* the User bob"). The parse
precedent is direct: ADR 0153 D1 faced the same objection for `as` (already taken
by `consumes … as Alias`) and resolved it by showing the header is a distinct
production, then D4 made the tier names contextual rather than reserved. `by`
currently appears in exactly six places, all handler declarations
(`grammar.js:764-849`), so a service-call position is new.

**The call site, not the header, is forced** — by the test the track exists to
make possible. Agent state is fresh per case (ADR 0153 D7), so an isolation claim
needs two principals in one case:

```bynk
case "each owner's list is private" {
  let _    <- api.POST("/todos", AddRequest { title: "bob's" }) by User("bob")
  let mine <- api.GET("/todos")                                 by User("carol")
  expect mine is Ok([])
}
```

A case-header or leading-body-item form (`case "…" by User("bob")`) cannot express
this, and this is not a contrived edge case: it is `todo`'s headline claim (§1).

**What falls out, with no further design.** Naming the *actor* rather than the
payload is what makes the hard cases collapse:

- **Refined actors.** `by Admin("bob")` — the compiler already knows `Admin`'s
  predicate, so at `system` it synthesises a token carrying `sub=bob` *and* the
  admin claim. The spelling never changes; most of Q6 dissolves.
- **Unit-identity actors.** `by Visitor`, no argument — `by_clause` already makes
  the binder optional (`grammar.js:923-931`).
- **`Signature` actors.** No identity at all
  (`site/src/content/docs/book/reference/actors.md:33`), so `by Webhook` with no
  argument.
- **Sums.** A handler declaring `by who: User | Visitor` is called by a case that
  picks one.
- **The identity is refinement-checked at compile time.** `User("")` against
  `UserId = String where NonEmpty` is an ordinary literal-admission error — no new
  machinery, and no route to smuggling in an invalid identity.
- **A missing principal is checkable**, mirroring `bynk.actor.missing_by_on_http`:
  a handler whose actor carries a Declared identity requires `by` at the call site.

**Left open deliberately.** Two details, neither blocking:

- **A case-level default.** `case "…" by User("bob")`, inherited and overridable at
  the call site, would mirror `as <tier>`'s suite→case precedence (ADR 0153 D2) and
  would spare `todo`'s single-principal cases three repetitions. Recommended as a
  **named follow-on**, not part of the first slice: sugar cannot be removed once
  shipped, and the repetition may prove to read fine.
- **Binding.** `by` binds to the **service-call expression**, not the `let`. Trivial
  in `let x <- api.GET("/todos") by User("bob")`; worth specifying rather than
  discovering in `let x <- f(api.GET("/todos")) by …`.

### 4.3 What a case receives

`HttpResult` **is** the status vocabulary: `Ok`→200, `Created`→201, `NotFound`→404,
across a flat 30-variant table (`bynk-emit/src/emitter/runtime.ts:385-417`). So
`expect todos is Ok(_)` asserts the status *in Bynk's own words* — better than a
raw `res.status == 200`, and it is the same assertion at both tiers.

Handing it back typed has precedent: `callService` already decodes a response
through a generated deserialiser (`deserialise_Result_Int_OrderError`). The same
move yields `HttpResult[List[TodoItem]]` from a real `Response`.

It does not cover everything. `Ok`/`Streaming`/`Raw` all map to 200, and headers
are not in `HttpResult` — and the wrapper stack is deep
(`bynk-emit/src/emitter/workers_entry.rs:1159-1197`):

```
headResponse(applySecurityHeaders(applyCors(notModifiedIfMatch(applyCache(
  httpResultToResponse(result, ser, {weakEtag:true}), maxAge, scope), request), policy, origin), secPolicy))
```

`applySecurityHeaders` is applied to **every** `from http` service, even one with
no `security {}` block — a system case cannot opt out of it. Cache/ETag/HEAD are
GET-only. Whether a case can assert on any of that is Q4.

The result channel is where the protocols genuinely diverge:

- **http** — a `Response`: status + headers + body.
- **cron** — `void`. The handler's `Err` is `console.error`'d and **dropped**
  (`workers_entry.rs:505-508`), so a cron case must observe the surface method's
  `Result` rather than the entry. At `unit` that is what it does anyway.
- **queue** — `void`, but the verdict arrives by the runtime calling
  `ack()`/`retry()` on a **caller-supplied** message object
  (`workers_entry.rs:523-573`). The test supplies the batch, so it can pass spies
  and read the verdict cleanly. `Ack | Retry(reason)` is two variants against
  `HttpResult`'s thirty.

## 5. Prior art — this is already done, from Rust

The compiler tests the boundary today with hand-written Node drivers invoked from
Rust integration tests. `bynkc/tests/http_method_behaviour.rs:75-157` is the
model:

```ts
import worker from "./workers/api/index.js";
const env = {} as never;
let r = await worker.fetch(new Request("https://x.test/notes", { method: "GET" }), env);
assert(r.status === 200, "GET /notes is 200");
r = await worker.fetch(new Request("https://x.test/notes", { method: "DELETE" }), env);
assert(r.status === 405, "DELETE /notes is 405");
assert(r.headers.get("allow") === "GET, HEAD, OPTIONS, POST", "405 Allow is the union");
```

Same shape in `http_security_behaviour.rs`, `http_caching_behaviour.rs`,
`http_limits_behaviour.rs`, and — over the internal door —
`cross_context_caller.rs:93-118`.

**`fetch(request, env)` is already the seam**, and the system harness already
stands it up in-process (`bynk-emit/src/project/tests_emit.rs:940-952`). This
track proposes no new harness: the reference implementation exists and is in daily
use; it has no Bynk spelling.

(`bearer_auth.rs` and `oidc_auth.rs` are *crypto unit tests* — they call
`verifyBearerJwtHs256` directly and never touch `worker.fetch`. They are prior art
for **minting a real token**, not for driving a route.)

## 6. Security & threat model

Less of a confrontation than it first appears, because the tier reading confines
the question rather than forcing it.

[ADR 0081](../decisions/0081-verified-identity-context-sealed.md) makes a verified
identity a context-sealed value, "**minted only at the verification seam** …
unforgeable **by construction, not convention**".
[ADR 0082](../decisions/0082-by-clause-verify-then-body-defaults.md) makes
verification structural and fail-closed;
[ADR 0088](../decisions/0088-optional-by-binder.md) insists the binder relaxation
was "a ceremony reduction, **not** a return to ambient authority", naming Yesod's
`requireAuthId` as the anti-pattern.

**At `system` the question does not arise**: the case presents a real credential
and the real seam mints the identity (§4.2). ADR 0081 is satisfied by
construction, and the seam stays "one cohesive, reviewable block" — the property
ADRs 0085/0087 protect to keep `/security-review` tight. Supplying a request adds
no branch inside the wrapper.

**At `unit` an injected identity does mint outside the seam** — and that is what
the tier is for. Three things make it sound:

- Doubling collaborators is the *definition* of the tier, not a loophole in it.
- The suite is stripped from the build
  ([ADR 0147](../decisions/0147-structural-test-ness-and-flat-paths.md)) — the same
  mechanism that already makes `stub` honest.
- Tests already mint context-owned refined values: `Val[FlagKey]("new-dashboard")`
  does exactly this via a test-only brand cast. "A test may construct a sealed
  value" is existing practice, not a new breach.

ADR 0081 itself points this way: it rejects full linearity partly because identity
is "the wrong ergonomic grain for a read-many, fanned-out, logged, **tested**
value" — the one place the corpus contemplates identity being tested, and it
argues for keeping it an ordinary value.

`Caller` is a different case again and must not be conflated: its identity is a
plain `string` on an explicitly trusted internal channel
([ADR 0092](../decisions/0092-cross-context-caller-value.md) scopes a forged
`X-Bynk-Caller` out of the threat model — all contexts are one trust domain).
Supplying a caller name from a test mints nothing. That is why #655 is a bug fix
while the Bearer question is a design decision.

## 7. Open questions (settle before slicing)

- **Q1 — Does `system_needs_wire` relax as §3.4 proposes?** This is the one
  amendment to a settled ADR and it gates the system-tier slice (not the unit-tier
  one). ADR 0153 D5/D6 attach the ≥2-context rule to the tier; the proposal
  re-attaches it to "has something to wire".
- **Q2 — What is the spelling for arranging an actor? — SETTLED (§4.2.1).**
  `by <Actor>(<identity>)` at the **call site**. `stub` is not widened: an actor is
  the caller, not a collaborator, so it is the wrong semantic category and ADR 0154
  D4 stands untouched. The call site (not the case header) is forced by the
  two-principal isolation case. A case-level default is a **named follow-on**, and
  `by` binds to the service-call expression.
- **Q3 — Does a `system` address accept both typed args and raw JSON?** The happy
  path wants `api.POST("/todos", AddRequest { … })`; the rejection path needs raw
  `{"title": ""}`. One address with two argument modes, or two spellings?
- **Q4 — Can a case assert on the wrapper stack?** Security headers are
  unconditional (§4.3). Is asserting `nosniff` in scope, or the compiler's own
  business?
- **Q5 — Where does the signer live?** Emitted runtime (present in production
  bundles) vs test-only emission (stripped per ADR 0147, which is the honesty
  mechanism and probably the answer).
- **Q6 — OIDC** (*narrowed by Q2*). Refined actors are no longer open: `by
  Admin("bob")` names the actor, and the compiler derives the claims from its
  declaration (§4.2.1) — nothing is spelled by hand. What remains is OIDC's
  *mechanics*: RSA/ES256 plus a mocked JWKS rather than a shared secret.
  `bynkc/tests/oidc_auth.rs` proves it is doable but it is not one line. System-tier
  only, and deferrable behind Bearer.

Adjacent live threads worth pulling rather than re-deriving: ADR 0153's
re-openables (**observing/asserting across the `system` wire**; per-case `system`
wiring in a mixed suite) and ADR 0154's (**provider substitution across the
`system` wire**).

## 8. Slice decomposition (ordered, candidate)

The ordering follows §2's separability: the unit-tier surface ships first and
alone, because it needs no crypto, no `fetch`, and no ADR amendment.

- **Slice 0 — the addressing generalisation.** Widen `symbols.rs:614-620` beyond
  `HandlerKind::Call`, and replace the `calls.rs:1741` `method.name == "call"`
  hardcode with a real handler lookup. Fixes #654 as a side effect. Buys nothing
  user-visible alone, but every later slice needs it and it is the one change to
  the checker's service branch.
- **Slice A — the unit-tier surface.** Address `http`/`cron`/`queue` handlers at
  `unit`, and give `makeTestDeps()` an `identity` (fixing #655). This is where
  most of the value is: it makes every example's service testable, and gives
  `scheduled` and `queue` their **first-ever execution coverage** (§1). No
  signer, no `fetch`, no `system_needs_wire` change. **Q2 is settled (§4.2.1), so
  this slice has no open question blocking it** — it lands `by <Actor>(<identity>)`
  at the call site together with the `unit` lowering, and is ready to propose.
- **Slice B — the system-tier boundary.** `as system` on an http suite: a full
  `fetch` request carrying a properly formed identity, asserting on the decoded
  `HttpResult`. Needs Q1 (the `system_needs_wire` relaxation), Q3, Q5.
- **Slice C — the rejection paths.** Raw pre-validation input → 400; missing or
  invalid credential → 401; the 405/OPTIONS fall-through. This is the slice that
  pays for the boundary work (§1), and it depends on Slice B's raw-payload
  position.
- **Deferred — websocket** (§2), **OIDC** (Q6), and the **case-level `by` default**
  (§4.2.1).

## 9. Risks

- **Conflating the two concerns.** The main failure mode (§2): letting Slice A
  wait on Slice B's signer and ADR amendment. They are independent; the cheap one
  should not be held hostage.
- **A parallel test router.** The cheap-looking path — emit a bundle-mode router
  for tests — contradicts ADR 0159 D9 and ADR 0162 D7 and would recreate exactly
  the fake-vs-real drift `workers_runtime_smoke.rs` exists to catch. §2.
- **A fourth tier word.** ADR 0153 bought a lot with "the tiers *are* the testing
  pyramid". Anything that reads like a fourth tier — or an orthogonal axis —
  gives that back. §3 exists to avoid spending it.
- **Litigating ADR 0182.** A slice that asks to construct an invalid refined value
  will lose, and deserves to. §3.3 is the framing that avoids the fight.
- **Unit-tier false confidence.** A green `unit` case says the *handler* is right.
  It says nothing about the credential verifying, the body deserialising, the
  refinements running, or the path parameters matching — all of which `unit`
  bypasses (§3.2). The risk is not a mistyped route (that is a compile error at
  either tier); it is an author reading "the handler works" as "the endpoint
  works". The docs must draw that line explicitly.

## 10. Relationship to the north star

`../bynk-design-notes.md` sells the boundary as the language's centrepiece: types
are enforced at the edge, identity is sealed at the edge, and the author writes
neither check. The compiler is why that is true — and the compiler's work is
precisely the work no Bynk test can currently observe. The subject ladder ends at
`history`; the edge was never a rung.

The retired testing track's own throughline is the argument for this one:
*behaviour is generated by driving, not fabricating*. Today a test can drive an
agent, a function, and an `on call` service. It cannot drive the one component
the language's pitch rests on — and the dial that would take it there already
exists, one door short.
