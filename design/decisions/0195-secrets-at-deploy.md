# 0195 — Secrets at deploy: values never at rest, and a derived set that admits what it cannot know

- **Status:** Accepted (v0.172)
- **Provenance:** #602, deploy track slice 3 (spine #558). Builds on slice 0
  (#583, v0.154), slice 2 (#601, v0.170) and slice 1 (#600, v0.171).
- **Relates:** [[0018]] (config-as-capability — how a secret reaches a binding at
  all), [[0179]] (provisioning state — the ledger this deliberately does not
  extend), [[0180]] (deploy plans before provisioning and is idempotent),
  [[0193]] (multi-context deploy ordering — the loop this step drops into, and
  the precedent for reading the emitted artifacts), [[0194]] (the ledger owns the
  ids it mints and defers for state another tool owns — the posture this
  follows), [[0083]] (the driver is a thin orchestrator over wrangler),
  [[0096]] (`dev`'s local `--var` passthrough, the only secret path before this).

## Context

Three of `deploy`'s four v1 resource kinds were provisioned by slices 0 and 1.
Secrets were the last, and they are categorically different from the other
three: they are **values, not ids**. A KV namespace id is minted by Cloudflare
and safe to commit; a queue name comes from the source. A secret value comes from
the user and must reach Cloudflare without being written down anywhere on the
way.

The slice was proposed on a premise that did not survive grounding, and the
correction is most of this ADR's content. The proposal — and the track doc §4.4
before it — claimed `deploy` could derive the secret **names** a context requires
from the closure walk it already runs, and therefore that the checker and emitter
deltas were both *None*. Neither holds:

- **`bynk.Secrets` names are not derivable.** The capability is
  `capability Secrets { fn get(name: String) -> Effect[Option[String]] }`
  (`bynk-check/src/firstparty/bynk.bynk`). `name` is an ordinary expression in an
  ordinary argument position, checked for arity and type-compatibility and
  nothing else (`bynk-check/src/checker/calls.rs`) — `Secrets.get(someVar)`
  type-checks. `consumes bynk { Secrets }` says a context *may* read secrets, not
  **which**. Nothing in the compiler collects the names, and they reach no
  emitted artifact: a `Secrets`-consuming fixture's `compose.ts` emits an empty
  `Env` interface and the name survives only as a live call argument.
- **The proposal omitted the class that *is* derivable, and it is the one that
  matters most.** An `actor`'s auth secret —
  `auth = Bearer(secret = "AUTH_JWT_SECRET")`, `Signature(secret = …)` — is a
  string literal fixed at parse time (`SchemeArgValue` admits only `Str`/`Int`),
  required at compile time (`bynk.actor.bearer_missing_secret`), and already
  resolved into the seams the emitter lowers (`bynk-check/src/actors.rs`). Its
  absence is **fail-closed and silent**: the emitted entry answers 401 on every
  request rather than failing the deploy.

So the honest scope is narrower than proposed but sharper. The class `deploy` can
speak for, it can speak for **totally**; the class it cannot, it must not pretend
to. Getting that boundary wrong in the *generous* direction is the worst outcome
available here: a list that is usually right is worse than no list on a
fail-closed path, because it gets trusted.

## Decision

**(A) No secret value — and no secret name — is ever written to Bynk state.**
Values move from the user's source straight to `wrangler secret put <NAME>` on
**stdin** and are dropped. Not argv, and not merely by preference: `wrangler
secret put` has no value option at all (the value is stdin or an interactive
prompt), so stdin is the only interface — and it is the one that keeps the value
out of the process list. The ledger records **nothing** about secrets: not the
value ([[0179]]'s format guarantees no field holds a value class), and not even
presence. Presence is a live question (D4), so a recorded answer could only ever
be a stale one — a field load-bearing for nothing, which [[0194]] already
established is a claim waiting to be believed. `WorkerRecord` stays a one-field
struct, declining the invitation its own comment extended to this slice. The plan
carries names and never values, in both formats, asserted with a sentinel rather
than described.

**(B) The derived set is a floor, not a census — and the surface says so.** The
compiler contributes exactly the actor-auth secrets: Bearer, Signature, and the
members of a multi-actor sum. `Oidc` names no secret (its trust root is the
provider's published JWKS) and a sum's `None` member (a catch-all such as
`Visitor`) verifies nothing; both are **skipped rather than defaulted**, since
inventing a name for them would ask the user to set a secret nothing reads.
`bynk.Secrets` names are the user's to supply. **`deploy` does not scan for
`Secrets.get(<StrLit>)`** — every call site in the repo today happens to use a
literal, which is exactly what would make such a scan look reliable while being
incomplete by construction. Each plan line is marked `declared` or `supplied` so
a reader can tell the compiler's word from the user's, because the compiler's
silence about a `bynk.Secrets` name is not evidence that no such name exists.
*Consequence, stated rather than hidden:* a `Secrets.get("API_KEY")` the user
forgets to supply is still a production `None` — unchanged from before this
slice, and addressed by the follow-up below rather than papered over.

**(C) Names and values are separate inputs.** Names: the declared set, plus a
`--secrets-file`'s keys, plus each `--secret NAME`. Values, per already-known
name: the file, else the environment, else an interactive prompt. The environment
is a **value** source only, looked up per name already known — it is never
scanned for names, because sweeping `env` into Cloudflare would exfiltrate the
user's whole shell. (The proposal's original precedence, "`--secrets-file` >
environment > prompt", was incoherent for exactly this reason: it read as though
the environment could tell `deploy` *which* secrets exist.) `--secret NAME` is
what makes env-only CI work for a `bynk.Secrets` name without writing a file. A
required name with no value from any source is a **hard error naming it** when
there is no terminal to ask — never a blank, which would report success and then
401 in production with nothing to read that says why. A malformed `--secrets-file`
line is likewise an error naming its line number, never a skip. *Consequence:*
the source stays thin and user-controlled; `deploy` builds no vault. A supplied
name is set on **every** context in the run, since nothing says which contexts
read it — the plan lists it per context so the spread is visible rather than
implied.

**(D) Reconciliation is set-if-absent, `--force` to overwrite, and presence is
asked live.** Cloudflare does not return secret values, so the only observable is
presence (`wrangler secret list`, whose names this reads structurally). Always-set
would re-push N secrets every deploy as N separate `wrangler secret put` calls,
each cutting a fresh Cloudflare version — concrete cost, no benefit. A failed or
impossible presence query is read as "assume nothing is set and try": a first
deploy genuinely has none, and a real auth failure then surfaces as the put's own
complaint rather than as a diagnosis the driver invented (the `queue_exists`
posture, [[0194]]). **The plan cannot forecast the skip.** Presence needs auth,
and the plan is derived before `deploy` authenticates — which is what keeps
`--dry-run` working offline, a property worth more than a forecast. So the plan's
action is `set` (or `overwrite` under `--force`), and the *run* reports the skip
where it happens. This is the one place this slice's surface is narrower than
#602 described, and deliberately: the alternative was making `--dry-run` require
a Cloudflare account.

**(E) The declared names reach the driver in a compiler-emitted manifest, not an
API.** The emitter writes `bynk-secrets.json` beside each Worker's
`wrangler.toml` (`{"version":1,"declared":[…]}`), omitted entirely when a context
declares none. The driver reads it back, as it already reads the config
(`read_resources`). The alternative — a new `pub fn` over the project model, plus
a `bynk-check` dependency the driver does not have — is rejected because
`compile_once` has **two** paths: in-process, and a shelled `bynkc` under an
override that hands back only an exit status (`bynk/src/dev.rs`). An API-shaped
answer would work on one and silently derive an **empty set** on the other, which
under a floor-not-census contract is indistinguishable from "this context
declares nothing". Reading the build output is the only shape correct on both,
and it extends [[0193]] D3's principle (the graph is read from the emitted
`[[services]]`) to the one resource with no stanza to read. The manifest is
derived from the same seams, over the same handler enumeration, that the entry
emitter lowers the `env` reads from — so it cannot describe a Worker other than
the one emitted beside it. *Consequence:* "Emitter: None" was false; this is a
new emitted artifact, with golden churn across the nine fixtures that already
carry an auth secret under a workers target.

**(F) Secrets straddle the push, and which side depends on the ledger.** A Worker
the ledger has pushed before has its secrets set **before** the push, as the
phase order intends: running code never sees a request without them. A **first**
deploy pushes first and sets after. This is not symmetry for its own sake —
`wrangler secret put` against a Worker not yet on the account does not fail. It
calls `createDraftWorker`, which uploads a **stub Worker** (`export default {
fetch() {} }`) and puts the secret on that. Non-interactively its confirm falls
back to *yes*, so CI would silently create a stub the plan never mentioned;
interactively it prompts mid-deploy, and a decline returns before the success log
— so wrangler exits **0 having set nothing**, which `deploy` would read as
success and push behind, producing exactly the silent-blank failure (C) exists to
prevent. Pushing first avoids both. The window it opens is fail-closed by
construction (a handler whose auth secret is unset answers 401; it does not serve
unauthenticated) and it is a Worker that did not exist a moment earlier, so there
is no traffic to lose. *Consequence:* if the post-push secret step fails on a
first deploy, the ledger does not record the Worker as deployed even though it is
live. The ledger understates rather than lies — its rule is that it claims only
what it watched succeed — and the next run re-pushes and re-sets, which is
idempotent.

## Consequences

`deploy` now sets every secret the compiler can prove a Worker reads, or fails
naming the one it could not — closing the last gap in the v1 resource surface
(KV, DO migrations, queues, multi-context ordering, secrets) and completing the
deploy MVP. The safety line is the sharpest in the track and is enforced rather
than intended: no code path may put a secret value in persistent state, and the
ledger has no field that could hold one.

The cost is a boundary that must be *read* to be used correctly. `deploy` speaks
totally for actor-auth secrets and not at all for `bynk.Secrets` names, and the
`declared`/`supplied` marks are the whole mechanism by which a user can tell.
This is a documentation-load-bearing design: a reader who takes an absent
`declared` line for "no secret needed" is misreading a surface that told them the
truth. The Book's deploy guide carries the distinction, not just the flag list.

`--dry-run` stays offline, at the price of a plan that cannot forecast a skip.

**Deferred, and named so it is not re-derived:** a static rule requiring
`Secrets.get`'s argument to be a string literal would turn the floor into a
census. The precedent exists (`cors` `origins`, `@cache`, `@limit` all require
literal arguments) and all six call sites in the repo already comply, so the
break is empty in practice. It is not taken here because it is a **language
surface change** with a real expressiveness cost — no computed secret names,
ever — and it deserves its own proposal and ADR rather than riding a driver
slice.
