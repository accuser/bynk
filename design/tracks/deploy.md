# Deploy — `bynk deploy`: provisioning + remote deploy, the capstone of the driver arc

- **Status:** All six slices shipped (state model + KV MVP, DO/queue
  provisioning, multi-context ordering, secrets — plus the #632 follow-up on
  computed secret names — environments (#835), and reconciliation maturity
  (#839)). Ready for retirement per `design/tracks/README.md` §5 — the
  retirement PR is a separate, deliberate step, not folded into slice 5's.
  Live state on the track's **spine issue**,
  [#558](https://github.com/accuser/bynk/issues/558)
  ([ADR 0167](../decisions/0167-feature-tracks-run-github-native.md)).
- **Realises:** `design/bynk-tooling-roadmap.md` §5.1 (the driver arc `doctor → new → dev → deploy`, closing with "the on-ramp arc is complete; **`deploy` (provisioning + remote) follows**"), and the deferral [ADR 0096](../decisions/0096-bynk-dev.md) named by name — `bynk dev` (D4) "provisions nothing and never edits `wrangler.toml`… Real, provisioned remote support is **`deploy`'s defining problem, the next slice**." It executes the deploy half of the [ADR 0084](../decisions/0084-doctor-output-exit-contract.md) `Deploy` capability (`dev`/`deploy` share the Node + `wrangler` gate) and turns the deploy-time placeholders [ADR 0017](../decisions/0017-platform-lock-per-deployment-unit.md) locks a context to (`id = "<KV_NAMESPACE_ID>"`, DO migrations, queue consumers, Service Bindings) into live resources.
- **Posture:** Feature track per [ADR 0076](../decisions/0076-feature-track-posture.md). Qualifies on all three axes: **multi-increment** (an MVP single-context deploy, the provisioning-state model, multi-context topology + ordering, secrets/config, environments, reconciliation/drift); **surface not yet settled** (where provisioned resource identity lives given `wrangler.toml` is regenerated every build, the reconciliation model, the environment surface); and a **safety boundary** — this is the **first driver command with irreversible, outward-facing side effects**: it authenticates to a cloud account, *creates* and *mutates* live resources, pushes running code, and handles credentials. `doctor` reports, `new` writes local files, `dev` runs Miniflare locally; none of them touch anything a user cannot delete by hand. `deploy` is categorically different, and that difference is the reason it is a track and not a fourth additive verb alongside #487's `check`/`test`/`fmt`.
- **Front-loaded ADRs (named, not numbered):** the **provisioning-state model** (where real resource IDs live, why not in generated config, who authors and commits it, and the `dev --remote` merge seam it shares); the **deploy orchestration + idempotency/reconciliation** contract (plan → provision → wire → push, re-runnable); **multi-context deploy ordering** (Service-Binding dependency order across a project's Workers); **secrets at deploy time** (the `bynk.Secrets` → `wrangler secret` seam, ADR 0018). Each is created and numbered by the slice that lands it (§8) — this doc deliberately does not pre-allocate numbers, since concurrent tracks would collide.

## 1. Motivation

The driver arc was designed as `doctor → new → dev → deploy` and three of the
four have shipped: `doctor` (v0.46, the environment check), `new` (v0.58, the
runnable scaffold), `dev` (v0.57, build + local `wrangler dev`). The arc's whole
point is to collapse a multi-step ritual into one command runnable from anywhere
inside a project — and `dev` did exactly that for *local* running, deliberately
stopping at the water's edge:

> `dev` provisions nothing and never edits `wrangler.toml`. `wrangler dev`
> defaults to local mode (Miniflare)… the generated `id = "<KV_NAMESPACE_ID>"`
> placeholder is read only under `--remote`. Real, provisioned remote support is
> `deploy`'s defining problem, the next slice. — ADR 0096 (D4)

So today, taking a Bynk project *live* is back to the manual ritual `dev`
retired for local: compile, find each generated worker directory, create a KV
namespace by hand, paste its id into the generated `wrangler.toml` (which the
next compile overwrites), apply Durable Object migrations, create queues, set
secrets, and `wrangler deploy` each context in the right order. This is the last
structural gap in the on-ramp: a language that scaffolds, checks, tests, and
serves locally in one command each, but cannot *ship*.

Three forces converge on `deploy`:

1. **The developer ask — one command to go live.** The symmetry `dev` set:
   `bynk dev` serves locally; `bynk deploy` ships remotely. Everything between
   compile and a running Worker should be the driver's job, not a wiki page.
2. **The generated config is a *template with holes*, and only deploy can fill
   them.** `emit_wrangler_toml` (bynk-emit) already emits every stanza a context
   needs — `[[kv_namespaces]]` with an `id = "<KV_NAMESPACE_ID>"` placeholder,
   `[[durable_objects.bindings]]` + `[[migrations]]`, `[[services]]` Service
   Bindings, `[triggers]` crons, `[[queues.consumers]]` — but the deploy-time
   values (namespace ids, applied migrations, created queues) are placeholders
   by construction. Filling them *is* provisioning.
3. **`doctor` already models the capability and left the other half unbuilt.**
   ADR 0084 groups probes by capability; **dev / deploy** share the Node +
   `wrangler` gate, and `--only deploy` already promotes it to required. The
   capability exists; the verb behind half of it does not.

## 2. Scope and non-goals

**In scope.**

- A **`bynk deploy` driver verb** (thin orchestrator per ADR 0083) that takes a
  compiled project from source to running Workers on Cloudflare, provisioning
  the platform resources each context's closure requires.
- **Provisioning** the concrete resource surface `emit_wrangler_toml` declares:
  **KV namespaces** (create + inject id), **Durable Object migrations** (apply),
  **queues** (create the queues `[[queues.consumers]]` bind, and any produced),
  **crons** (ride the config), and **Service Bindings** across contexts.
- The **provisioning-state model** (§3) — the load-bearing decision: where real
  resource identities live so a regenerated `wrangler.toml` never loses them and
  a re-run never double-provisions.
- **Multi-context topology + deploy ordering** (§4.3): a project is several
  contexts → several Workers (one per ADR 0017); Service Bindings impose a
  dependency order `deploy` must respect.
- **Secrets at deploy time** (§4.4): setting an `actor`'s declared auth secrets
  and whatever `bynk.Secrets` names the user supplies via `wrangler secret put`,
  without ever writing secret values into generated config or the state file.
- **Idempotency / reconciliation** (§4.5): a second `deploy` reconciles existing
  resources rather than recreating them; a plan/dry-run surface.
- **Environments** (§4.6): a named target (default / `--env staging`) so the
  same project deploys to more than one account/namespace set.
- The **`--` passthrough** posture (ADR 0096 D5): the driver curates only its own
  concepts and forwards the rest to `wrangler deploy` verbatim.

**Non-goals (and why).**

- **Operating or abstracting over non-Cloudflare targets.** A context under
  `--target workers` is Cloudflare-locked by ADR 0017; `deploy` ships that target
  and does not invent a portable deploy abstraction. Other targets (`bundle`,
  the browser platform ADR 0138) deploy differently and are their own problem.
- **A provisioning engine that replaces wrangler.** The thin-orchestrator
  posture (ADR 0083) holds: **every v1 resource has a `wrangler` verb**
  (`wrangler kv namespace create`, `wrangler queues create`,
  `wrangler secret put`, DO migrations riding `wrangler deploy`), so v1 shells
  `wrangler` end-to-end and **never touches the raw Cloudflare API** — it is not
  a Terraform. It curates the plan and owns the state mapping; wrangler owns the
  wire. (A future resource with *no* wrangler verb would reopen this — and the §6
  credential invariant with it, since a direct API call must handle the token
  itself; noted, explicitly not v1.)
- **Rollback / traffic-splitting / gradual rollout / observability.** Real
  release-engineering surface (canaries, versioned rollback, tail logs) is a
  large follow-on, noted (Q5), not v1. v1 deploys the current source.
- **CI/CD integration as a product.** `deploy` must be *scriptable* (exit codes,
  `--format json` plan, non-interactive credential source) so a CI job can call
  it, but the pipeline/GitHub-Action wrapper is not this track.
- **Provisioning resources the closure does not declare.** `deploy` provisions
  exactly what a context's given-closure locks it to (ADR 0017) — no speculative
  or user-freeform resource creation. If it is not in the emitted config, it is
  not `deploy`'s to create.
- **Secret *storage* / a secrets manager.** `deploy` moves secret *values* from a
  user-controlled source into `wrangler secret`; it never stores them. Where the
  values come from (env, prompt, an external manager) is a thin input, not a
  vault this track builds.

## 3. The core problem: the provisioning-state model

This is the decision the whole track turns on, and the reason `dev` could punt
and `deploy` cannot.

**The bind.** The generated `wrangler.toml` is, verbatim, *"Generated by bynkc —
do not edit by hand."* `bynk dev` (ADR 0096 D1) **clears `<build>/workers/`
before every compile** precisely so a renamed or deleted context leaves no
phantom worker directory. So the one file wrangler reads for a resource id is
**regenerated and may be wiped on every build**. Writing a provisioned KV
namespace id back into that file — the manual recipe's step — is therefore
self-defeating: the next `bynk dev` or `bynk deploy` erases it. Provisioned
identity **cannot live in generated config**. It must live somewhere the build
owns but never regenerates from source.

**The shape of the answer (the front-loaded ADR).** A **driver-owned,
persistent deploy-state file** — the deploy-time analogue of what `bynk.lock` is
to resolution — recording, per environment, the mapping from a context's
*logical* resource (the KV binding, the queue name, the DO class, the Worker
name) to its *concrete* Cloudflare identity (namespace id, queue id, applied
migration tag, deployed version). Candidate name `bynk.deploy.lock` (or a
`[deploy]` section keyed by env). Load-bearing properties:

- **Regeneration-proof, injected at use.** It lives *outside* `<build>/workers/`
  (which `dev` wipes) — at the project root, next to `bynk.toml`/`bynk.lock` — so
  a rebuild never touches it. The ledger's ids are merged into the
  freshly-generated `wrangler.toml` **just before wrangler runs** (a step
  wrangler never sees the placeholder through), and newly-provisioned ids are
  written back.
- **The injection seam is shared — and it closes the `dev --remote` hole.** ADR
  0096 (D4) reads the `<KV_NAMESPACE_ID>` placeholder **only under `--remote`**,
  and remote dev is a real, shipping path via `bynk dev -- --remote` — which
  *today* reads the literal placeholder and fails against remote KV. The ledger
  is the only place the real id lives, so the id-injection merge belongs to
  **neither command exclusively**: it is a shared "materialise deploy-state into
  the build config" step both `deploy` **and** `dev --remote` call. The ledger is
  therefore **read by both, written only by `deploy`** — `dev` never provisions.
  The fallback (if the shared seam proves awkward) is the conscious opposite:
  `dev --remote` stays unfilled and only `deploy` bridges to remote. Either way
  the state ADR must *decide* this — it fixes where the merge lives and who reads
  the file, and cannot stay an unstated omission. **The seam has a precondition
  worth stating outright:** `dev --remote` on a never-deployed project reads an
  *empty/absent* ledger, so the placeholder stays literal and it fails against
  remote KV — which is arguably correct (you cannot remote-dev against a KV
  namespace nobody has provisioned) and strictly better than today, since the
  cause is now *nameable* rather than a mystifying placeholder error. But it does
  mean `dev --remote`'s usability is **gated on a prior `deploy` having populated
  the ledger**; the ADR should say so, so the precondition is documented, not
  discovered.
- **Per-environment.** Keyed by target (§4.6) so `staging` and `production`
  namespace ids do not collide.
- **Committed and secret-free — authored by the first (human) deploy.** Resource
  *ids* are not secrets and belong in version control (like `bynk.lock`) so a
  team shares one provisioned topology; secret *values* never enter it (§4.4,
  §6). But "committed" and "written incrementally mid-deploy" (§6) compose only
  under a stated ownership rule, or they contradict: **the first deploy of an
  environment is a human who commits the resulting ledger; CI deploys read an
  already-populated ledger and push code, and do not provision.** Otherwise a CI
  job provisions → mutates a committed file it cannot push back → the next run
  re-provisions — the exact litter §6 warns of. So a CI deploy that meets an
  **unrecorded** resource **fails with "provision locally first,"** rather than
  silently creating orphans. (Whether CI may ever provision — behind an explicit
  flag that emits a machine-readable delta for a human to commit — is an open
  sub-question of Q1's ownership rule; the *default* is human-authored,
  CI-reads.)
- **The reconciliation ledger.** Its presence/absence per resource is exactly
  what makes a second `deploy` idempotent (§4.5): a recorded entry means *reuse*,
  an absent one means *provision* — on each resource's own key (§4.2), not a
  single uniform id.

Whether this is a new file or a `[deploy]`/`[deploy.<env>]` section of an
existing artefact, and its exact schema, is the first ADR and slice-0 work. The
*principle* — provisioned identity lives in persistent driver state, injected
into generated config at deploy time, never sourced from it — is the load-bearing
call this track exists to make.

## 4. Internal architecture

### 4.1 Orchestration pipeline

`deploy` extends the `dev` pipeline (`pre-flight → compile → select → serve`)
into **`pre-flight → compile → plan → provision → wire → push → record`**,
reusing every seam `dev` already built:

- **pre-flight** — the ADR 0084 `Deploy` capability (Node + `wrangler`), plus a
  **credential/account check** (`wrangler whoami` / an API token from the
  environment) that `dev` did not need. A missing tool or absent auth fails here,
  before any remote call, with doctor's remedy text.
- **compile** — `bynkc compile --target workers` into the managed `.bynk/`
  build, as `dev` does. Unlike `dev`, deploy operates over **all** the project's
  contexts by default (§4.3), not select-or-default-one.
- **plan** — derive the resource set each context's closure declares (§4.2) and
  diff it against the deploy-state (§3): what must be created, what is reused,
  what has drifted. This is the `--dry-run` / `--format json` surface.
- **provision** — create the missing resources (KV namespaces, queues) via their
  `wrangler` verbs; apply DO migrations via `wrangler deploy` (whose migration
  tracking `deploy` delegates to, §4.2); **idempotent** against the ledger.
- **wire** — inject the resolved ids into the generated `wrangler.toml` (the §3
  merge) and set secrets (§4.4).
- **push** — `wrangler deploy` per context, in **dependency order** (§4.3).
- **record** — write provisioned ids and the deployed version back to the
  deploy-state.

### 4.2 Resource derivation (already emitted, now provisioned)

**Shipped — slice 1, v0.171, ADR 0194** (except secrets, slice 3). Every v1
resource a single context's closure can declare is now provisioned: KV
namespaces (slice 0), queues, and DO migrations.

The two forks below were taken as written. **DO migrations** delegate to wrangler
entirely — and the ledger records **nothing**, not even the advisory last-applied
tag this section left room for: nothing would read it, and a field load-bearing
for nothing is a claim waiting to be believed. **Queues** reconcile by name,
create-if-absent, with the sharper consequence spelled out: the ledger's queue
set is read by the *plan* and by nothing else, so the provision step attempts the
create on every deploy and reads "already exists" as success — which is what
makes an out-of-band deletion self-heal, and what lets the set be honestly called
advisory. ADR 0194 also settles a question this section did not ask: the CI
provisioning refusal (§3) covers **KV alone**, because it protects minted ids and
a queue's name comes from the source. See ADR 0194.

The provisioning surface is not open-ended — it is exactly what
`bynk-emit/src/emitter/wrangler.rs` already writes per context, and `deploy`
mirrors that derivation to know what to create:

| Emitted stanza | Locked by (ADR 0017) | Deploy action |
|---|---|---|
| `[[kv_namespaces]] id = "<…>"` | closure reaches `bynk.cloudflare` KV | create namespace, **store + inject the CF-generated id** |
| `[[durable_objects.bindings]]` + `[[migrations]]` | an `agent` in the context | apply via `wrangler deploy`; **migration state tracked by wrangler/CF, not the ledger** (§4.5) |
| `[[queues.consumers]]` (`on queue "n"`) | a queue-protocol service | create-if-absent **by name** (no stored id) |
| `[triggers] crons` | an `on cron` handler | rides the config; no provisioning |
| `[[services]]` Service Bindings | cross-context `consumes` | deploy order (§4.3); no resource |
| *(no stanza — secrets are a runtime store, not config)* | an `actor`'s declared `auth` secret; plus any `bynk.Secrets` name the user supplies (§4.4) | `wrangler secret put`; **presence-only, no id, no value** (§4.4) |

`deploy` reads this set from the **emitted artifacts** — the config wrangler is
about to upload — rather than from an in-memory project model (ADR 0193 D3: the
graph is read from the emitted `[[services]]`, so the plan orders against the
thing that can actually fail). That is also the only shape that survives the
driver's *other* compile path: under a `bynkc` override the compiler runs as a
child process and hands back an exit status, so there is no model to consult.
The consequence for secrets is the one §4.4 draws: a name the compiler knows
must reach the driver **in the build output**, or not at all — secrets being the
one row above with no stanza to read it from.

**The four resources reconcile on four different keys** — the ledger is *not* a
uniform logical→id map, and the state ADR (Q1) must model each separately: **KV**
stores a CF-generated id and reuses it; **queues** reconcile **by their
user-given name** (`on queue "n"`), create-if-absent, needing no stored id;
**secrets** are **presence-only** (no id, no value — §4.4); and **DO migrations**
are the deliberate **counterexample: the one resource whose truth `deploy` does
*not* own.** Wrangler/CF already track applied migration tags server-side; a
second ledger of applied tags would be a rival source of truth that can silently
disagree with the account (the ledger says `v2`; an out-of-band reset left the
account at `v1`). So `deploy` **delegates migration state to wrangler entirely**
and records, at most, the last tag it *asked* wrangler to apply — advisory, never
authoritative. DO is thus the clarifying case for Q1: **the ledger owns the ids
it alone mints, and defers, for state another tool already owns.**

### 4.3 Multi-context topology and deploy ordering

**Shipped — slice 2, v0.170, ADR 0193.** Both halves now handle the multi-context
case: `bynk dev` serves every context with live Service Bindings (ADR 0192,
v0.167, superseding ADR 0096 D3), and `bynk deploy` ships every context in
Service-Binding dependency order.

The Q3 gating question below was **answered, and the working assumption was
wrong**: Cloudflare resolves a Service Binding at **upload**, not by name at
request time, so the order is a hard correctness barrier rather than a soft
nicety — an upload whose bound target does not yet exist is rejected outright.
The two-pass deploy that finding would imply for cycles was **not needed**:
`bynkc` already rejects a `consumes` cycle before emit, so the language's own
acyclicity invariant supplies the precondition Cloudflare demands. See ADR 0193
for the record. The original framing follows, for provenance:

- **Service Bindings impose a *soft* partial order.** `[[services]]` in context
  A's config binds context B's Worker **by name** (`consumed_binding_name` /
  `worker_dir_name`). Cloudflare resolves Service Bindings by name **at request
  time**, so A can be *uploaded* before B exists — the binding does not fail the
  upload, it fails live requests until B is up. The topo-sort by `consumes` edges
  therefore avoids a **transient half-wired window** (A live and serving before B
  answers), *not* a hard upload dependency — worth doing, but a weaker claim than
  a build-order barrier. Consequently a `consumes` cycle is **not** a hard
  blocker (both upload, then both serve once live); whether `deploy` still warns
  on one, and the confirmation of these CF semantics, is Q3.
- **All-or-selected.** Default deploys the whole project (topo-ordered);
  `--context NAME` deploys one (accepting the dotted name or its dasherised
  worker-dir form, as `dev` does) — with its dependencies assumed already live,
  or a diagnostic if they are not.
- **Partial-failure semantics.** If context C fails to push after A and B
  succeeded, the deploy-state records what *did* deploy; a re-run resumes rather
  than restarts. This is the reconciliation ledger (§4.5) doing its job under
  failure.

### 4.4 Secrets at deploy time

`dev` handled local secrets via the `-- --var KEY:VALUE` passthrough into
Miniflare (ADR 0096 D5). Remote secrets are different: they are
`wrangler secret put`, stored in Cloudflare, and are **values, not ids** — so
they must never enter the deploy-state file or generated config (§6). The seam:

- **What needs a secret** splits in two, and only one half is derivable. An
  `actor`'s auth secret — `auth = Bearer(secret = "AUTH_JWT_SECRET")`,
  `Signature(secret = …)` — is a **string literal fixed at parse time**
  (`SchemeArgValue::Str` admits nothing else), required at compile time
  (`bynk.actor.bearer_missing_secret`), and already resolved into the seams the
  emitter lowers (`bynk-check/src/actors.rs`). Those names `deploy` can know —
  and should, because an unset auth secret does not fail the deploy: it **401s
  every request** in production, fail-closed and silent until traffic arrives.
  But `bynk.Secrets` (config-as-capability, ADR 0018: config arrives through
  the deps object, read only inside first-party platform bindings) reads its
  name from a **runtime `String` expression** — `Secrets.get(someVar)`
  type-checks — so a context that `consumes bynk { Secrets }` declares only
  *that* it reads secrets, never **which**. Nothing in the compiler collects
  those names and they reach no emitted artifact; they are the user's to
  supply. So the derived set is a **floor, not a census**, and the surface must
  not present it as one — on a fail-closed path a list that is *usually* right
  is worse than no list, because it gets trusted. (Making it a census would
  take a new static rule forcing `Secrets.get`'s argument to a literal — the
  `cors`/`@cache`/`@limit` precedent exists — but that is a language surface
  change with a real expressiveness cost, and its own proposal.)
- **Where values come from** is a thin, user-controlled input — and **names and
  values are separate questions**. Names: the declared (auth) set, plus
  whatever the user lists — a `--secrets-file`'s keys, or a `--secret NAME`.
  Values, per already-known name: the file, else the environment, else an
  interactive prompt — never committed. The environment is a *value* source
  only; it cannot be a name source, since sweeping `env` into Cloudflare would
  exfiltrate the user's whole shell. `deploy` moves the value to
  `wrangler secret put <NAME>` (on stdin, never argv) and forgets it.
- **Reconciliation** for secrets is presence, not value (Cloudflare does not
  return secret values): the plan can say "will set N secrets" but cannot diff
  them. The leaning is **set-if-absent, `--force` to overwrite** — *not*
  always-set, whose cost is concrete: every deploy would re-push N secrets as N
  separate `wrangler secret put` calls, each cutting a fresh Cloudflare version
  (Q4).

### 4.5 Idempotency and reconciliation

**Shipped — slice 5 (#839), the reconciliation-maturity ADR (number assigned at
merge).** The property that makes `deploy` safe to re-run — and the reason the
state file (§3) is a *ledger*, not a cache:

- **Recorded ⇒ reuse; absent ⇒ provision — on each resource's own key.** The
  plan (§4.1) is a diff of derived-resources (§4.2) against the ledger, and the
  key differs by kind (§4.2): KV by stored id, queues by name, secrets by
  presence, DO migrations delegated to wrangler's own tracking (not the ledger).
  A second `deploy` with no source change provisions nothing and pushes the
  current code.
- **Drift detection — KV only, at provision time, once per run.** Resolved
  narrower than this section originally framed. Queues already self-heal
  (ADR 0194 D2's create-every-time shape); KV was the one asymmetry — a
  recorded id was trusted unconditionally, so a namespace deleted out-of-band
  got injected dead. `deploy` now asks Cloudflare's live namespace-id set
  once per run, before the per-context loop (not once per context, and not
  at plan time — `--dry-run` still never authenticates), and a recorded id
  absent from that set is re-provisioned exactly as an absent record would
  be.
- **Orphan reporting — a pure, offline diff, per resource kind.** Resource
  removal (a deleted context's namespace, a queue nothing declares anymore)
  is *reported* in the plan before any mutation, computed against the current
  build's full declared resources regardless of `--context`. `kv` and
  `workers` are independent checks — a removed context with KV is two
  reported orphans, not one (they're two resources, tracked separately in
  the ledger).
- **`--prune` (Q6, resolved) — KV namespaces and queues, never a Worker.**
  Opt-in, its own confirmation on top of the creation gate (§6), and
  idempotent: deleting an already-gone resource is treated as success and
  still clears the ledger entry, so a half-completed prune never wedges on
  re-run. `wrangler delete` (a whole Worker) stays out of scope — its blast
  radius (routes, custom domains, cron triggers) is categorically larger
  than a namespace or a queue.
- **`--dry-run` / `--plan`.** The plan is printed (and `--format json` pinned,
  in the ADR 0084 style) before any mutation, so a user sees exactly what will
  be created/changed/pushed — orphans included. This is the
  confirm-before-first-billable-mutation surface (§6); `--prune` layers its
  own, stronger gate on top, as this section originally anticipated.

### 4.6 Environments

**Shipped — slice 4 (#835), the environment-selection ADR (number assigned at
merge).** A project deploys to more than one place. A named environment
(default / `--env staging` / `--env production`) keys:

- the **deploy-state** section (§3) — distinct namespace/queue ids per env,
  exactly as anticipated: the schema was env-keyed from slice 0, so this
  slice was additive, not a migration;
- the **wrangler environment** — resolved, and not the way this section
  originally framed the choice. Q7 asked whether `deploy` curates
  `[env.<name>]` in generated config or rides wrangler's own model via
  passthrough; **neither, cleanly.** Cloudflare confirmed bindings are
  non-inheritable into a named environment, so passthrough alone would deploy
  with zero bindings — but `emit_wrangler_toml` runs at compile time, before
  any `--env` is known, so the emitter cannot curate them either. The driver
  synthesises the `[env.<name>]` block itself, at deploy time, generalising
  the seam that already materialises the KV placeholder (§3). Queue names and
  Service Binding targets are environment-qualified (`<name>-<env>`) in that
  synthesised block — a gap this section didn't originally name, since
  Cloudflare auto-suffixes a deployed Worker's own name the same way and an
  unqualified binding would resolve to the wrong, or a nonexistent, Worker;
- the **account/credential** the pre-flight resolves — unchanged (Q2's
  original leaning held): still "is *some* account authenticated," not "is
  the *right* account for this environment." Left open.

## 5. Tooling delta (the standing rule)

Per the [tooling roadmap](../bynk-tooling-roadmap.md) §5, each slice enumerates
its LSP/fmt/tree-sitter/driver delta. Headlines: this is **driver-only** — no
language surface, no grammar, no fmt change (deploy adds no syntax; the
deploy-state file, if TOML, is data the driver reads, not a source language).
`doctor` already models the `Deploy` capability (ADR 0084); `deploy` may extend
its pre-flight with the **credential/account check** (Q2 — whether that becomes a
new `doctor` line or stays `deploy`-local). The new verb rides the ADR 0083
thin-orchestrator posture and the ADR 0096 `--` passthrough. The deploy-state
file joins `bynk.lock` as a driver-owned, committed artefact — the `.gitignore`
story (§3: it is committed, unlike `.bynk/`) is part of the format ADR.

## 6. Security & threat model

This is the axis that makes `deploy` a track and not a verb: it is the first
command whose actions are **outward-facing and hard to reverse**.

- **Credentials are the crown jewels.** `deploy` authenticates to a Cloudflare
  account. It **must not** invent its own credential store — it defers to
  `wrangler`'s auth (`wrangler login` / `CLOUDFLARE_API_TOKEN`), so the token
  never passes through Bynk-owned code or state. The pre-flight *checks* auth
  presence; it never reads or persists the token.
- **Secret values never touch persistent Bynk state.** §4.4: secret values move
  from a user source straight to `wrangler secret` and are dropped. The
  deploy-state file (§3) holds only non-secret resource *ids*, so committing it
  is safe by construction — a property the format ADR must *guarantee*, not merely
  observe (no field may hold a value class).
- **Outward-facing, billable actions are gated — even though v1 deletes
  nothing.** v1 never prunes (§4.5) and creating a KV namespace is *additive*, so
  the gate does **not** guard destruction; framing it as "confirm before
  destructive mutation" would be dishonest — it would have no referent in v1.
  What it guards is real all the same: **provisioning bills the user's account
  and `wrangler deploy` pushes live code**. `deploy` prints the plan (§4.5
  `--dry-run`) and **confirms before the first mutation that creates a billable
  resource or goes live**, unless explicitly authorised (`--yes` for CI).
  Approval for one environment does not carry to another. (When `--prune` lands,
  deletion is the strictly stronger gate on top of this one.)
- **No silent destruction.** A resource the source no longer declares is
  **reported, not deleted** in v1 (§4.5 / Q6). Auto-deleting a KV namespace
  because a context was renamed would be catastrophic and unrecoverable; the safe
  default is to surface the orphan and let a human (or an explicit `--prune`)
  act. This mirrors the "look before you overwrite/delete" posture.
- **Deploy order guards a half-wired *window*, not an upload barrier.** Because
  Cloudflare resolves Service Bindings by name at request time (§4.3), pushing A
  before B does not fail the upload — it leaves A **live and serving with a
  binding that errors on every call** until B is up. The topo-sort shrinks that
  window to nothing: a real safety property, but the failure it prevents is a
  transient runtime one, not an upload-time dependency (this bullet and §4.3/Q3
  state the same, hedged, confidence — pending the Q3 CF-semantics confirmation).
- **Idempotency is a safety property.** A non-idempotent `deploy` that
  double-provisions on re-run (a retried CI job, a flaky network) would litter an
  account with orphaned namespaces/queues. The ledger (§4.5) is the mitigation,
  and it must be written *incrementally* (record each resource as it is created)
  so a crash mid-deploy does not lose the record of what was already made.
- **Scriptable but not silently powerful.** The `--format json` plan and defined
  exit codes make CI integration real; `--yes` makes it non-interactive. But the
  destructive default stays confirm-first for humans — the machine surface opts
  *into* automation explicitly.

## 7. Open questions (settle before slicing)

- **Q1 — the deploy-state model. [the front-loaded decision]** New file
  (`bynk.deploy.lock`) vs a `[deploy.<env>]` section of an existing artefact. The
  schema is **not** a uniform logical→id map (§4.2): it must model **four
  distinct reconciliation semantics** — KV (stored CF id), queues (by-name, no
  id), secrets (presence-only, no value), and **DO migrations (server-tracked by
  wrangler — the resource whose truth `deploy` does *not* own)**. It must also
  settle: the committed-and-secret-free guarantee and the **human-authored /
  CI-reads ownership rule** (§3), and **where the id-injection merge lives given
  `dev --remote` shares it** (§3). *Settle in slice 0; this is the load-bearing
  ADR.*
- **Q2 — credential/account pre-flight.** A new `doctor` capability line vs a
  `deploy`-local check; how account selection interacts with `--env` (§4.6, §5).
  *Leaning:* reuse `wrangler`'s auth, check presence in pre-flight, do not model
  the token in `doctor`.
- **Q3 — `consumes` cycles & order confidence.** Working assumption (§4.3, §6):
  Cloudflare resolves Service Bindings **by name at request time**, so upload
  order is *soft* — any order uploads, the topo-sort only avoids a transient
  half-wired serving window, and a cycle is not a hard blocker. **Confirm against
  live CF semantics before slice 2** — it is the difference between a correctness
  barrier and a nicety; if upload *does* require the target to exist first,
  §4.3/§6 harden back to a hard order and cycles need a two-pass deploy.
- **Q4 — secret reconciliation. [leaning: set-if-absent + `--force`]**
  Cloudflare does not return secret values, so the plan cannot diff them.
  Always-set has a concrete cost — N `wrangler secret put` calls per deploy, each
  a new CF version — so **set-if-absent with `--force`-to-overwrite is the
  leaning**, not an open toss-up (§4.4).
- **Q5 — release semantics.** Rollback / versioned deploys / traffic splitting
  are a large follow-on (§2 non-goal). Confirm v1 is deploy-current-source only,
  and that the state schema does not foreclose adding version history later.
- **Q6 — orphan handling. Resolved, slice 5 (§4.5).** Both, not an either/or:
  report-only is the default, `--prune` is the explicit opt-in, and neither
  ever auto-deletes. `--prune` itself is scoped narrower than "any orphan" —
  KV and queues only, never a Worker — a line the original framing left open.
- **Q7 — environment model. Resolved, slice 4 (§4.6).** Neither of the framed
  options: the driver synthesises `[env.<name>]` at deploy time (confirmed
  necessary — Cloudflare does not inherit bindings into a named environment),
  since the emitter cannot curate it (environment names are unknowable at
  compile time). The smallest-curated-surface instinct behind the framing
  still won on where the *decision logic* lives (the driver, not a new
  emitter concept) — just not on whether curation happens at all.
- **Q8 — packaging-track naming coupling.** The [packaging track](packaging.md)
  re-addresses contexts as `org.package.context` and says "wrangler/worker naming
  derives from that." `deploy`'s Worker names and deploy-state keys must assume
  that identity model (or at least not fight the flat-name cutover, packaging
  slice 0), so a rename does not orphan provisioned state. *Sequence deploy's
  naming against packaging's identity ADR.*

## 8. Slice decomposition (ordered)

Each slice is an ordinary [increment proposal](../proposals/README.md) — an
issue opened as a sub-issue of the track's spine
([#558](https://github.com/accuser/bynk/issues/558)) citing this doc and its
ADRs; accepting the proposal authorises the build. Slice 0 is standalone; later
slices build on the state model.

- **Slice 0 — the deploy-state model + KV-only single-context MVP.** The
  provisioning-state ADR (Q1) and the orchestration/idempotency ADR; `bynk
  deploy` for a **single context provisioning only KV** — the canonical
  `<KV_NAMESPACE_ID>` placeholder, the cleanest reconciliation (a stored CF id).
  Pre-flight (Deploy capability + credential check), compile, plan, `wrangler kv
  namespace create`, inject id, `wrangler deploy`, record; `--dry-run` and the
  confirm-before-first-billable-mutation gate. **Deliberately KV-only** so slice
  0's spotlight is the **state ADR**, not wrangler-migration edge cases. Lands
  the provisioning-state and orchestration/idempotency ADRs.
- **Slice 1 — DO migrations + queue provisioning.** Extend provisioning to the
  other two v1 resources: apply DO migrations via `wrangler deploy`,
  **delegating migration state to wrangler** (§4.2/§4.5, the counterexample), and
  create queues by-name-if-absent. DO migration application is the single hardest
  piece (§4.2) and earns its own slice rather than crowding the state ADR.
- **Slice 2 — multi-context topology + ordering.** Deploy all contexts,
  topo-sorted by `consumes` Service-Binding edges (§4.3, the *soft* order);
  partial-failure resume off the ledger; `--context` for one. **Confirm the CF
  ordering semantics (Q3) here.** Lands the deploy-ordering ADR.
- **Slice 3 — secrets at deploy.** The declared-auth-secret and `bynk.Secrets` →
  `wrangler secret` seam (§4.4), the secret-value-never-persisted guarantee (§6),
  set-if-absent reconciliation (Q4), and the **floor-not-census** contract (§4.4)
  — with the emitted manifest that carries the declared names to the driver.
  Lands the secrets-at-deploy ADR.
- **Slice 4 — environments. Shipped (#835).** `--env` over the already-env-keyed
  state schema (§4.6) — additive, as slice 0's schema anticipated. Resolved
  Q7: the driver, not the emitter, synthesises `[env.<name>]` at deploy time.
- **Slice 5 — reconciliation maturity + orphan reporting. Shipped (#839).**
  KV drift detection (§4.5, once-per-run), orphan report and `--prune`
  scoped to KV/queues (Q6, the track's first deletion gate), the richer
  `--format json` plan. Hardens the re-run story; the release-semantics
  follow-ons (Q5) are explicitly *out* and noted for a future track. **This
  was the track's last slice.**

## 9. Risks

- **The state model is wrong and everything downstream inherits it.**
  Provisioned identity in the wrong place (regenerated config, a cache, an
  uncommitted file) breaks re-runs and team sharing. *Mitigation:* front-load the
  state ADR (slice 0, Q1); it is the one hard-to-reverse decision.
- **A non-idempotent deploy litters or corrupts an account.** Double-provisioned
  namespaces, half-wired multi-context deploys. *Mitigation:* the ledger written
  incrementally (§6); topo-ordered push (§4.3); plan-before-mutate (§4.5).
- **Destructive default.** Auto-deleting an orphaned resource on a rename is
  unrecoverable. *Mitigation:* report-only in v1, explicit `--prune` only (§6,
  Q6); the confirm-first posture.
- **Credential mishandling.** A token flowing through Bynk-owned code or into the
  state file is a leak. *Mitigation:* defer entirely to wrangler auth; the
  state-file format ADR *guarantees* no value fields (§6).
- **Naming coupling to the packaging cutover.** Worker names / state keys tied to
  today's context identity churn when packaging lands `org.package.context`.
  *Mitigation:* sequence deploy naming against the packaging identity ADR (Q8).
- **Scope creep into release engineering.** Rollback/canaries/observability are a
  gravity well. *Mitigation:* v1 is deploy-current-source; the state schema
  leaves room but the track does not build the release surface (§2, Q5).

## 10. Relationship to the north star

This track finishes the arc the tooling roadmap drew and `dev` deliberately left
open: `doctor → new → dev → deploy`. It changes nothing in the language — no
surface, no ABI, no spec — and everything in the *reach* of the toolchain, from
"runs on my machine" to "running in production", in one command. It honours the
thin-orchestrator posture (ADR 0083) — `deploy` shells wrangler and curates only
its own concepts — and the platform-lock model (ADR 0017): it provisions exactly
what a context's closure commits it to, no more. Its one genuinely new idea is
the **provisioning-state model** — the deploy-time counterpart to `bynk.lock`,
the persistent home for the real resource ids a regenerated config cannot hold —
and its one genuinely new *responsibility* is stewarding irreversible,
outward-facing actions safely. `dev` proved the arc collapses a ritual to a
command; `deploy` collapses the last, riskiest ritual, and is the step that turns
Bynk from a language you can *run* into a platform you can *ship on*.
