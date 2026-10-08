# 0443 — Durable Object classes are declared in `exports`, not migrations

- **Status:** Accepted (v0.312.1)

**Context.** Every agent compiles to a Durable Object class, and a context that
emits gets one more for the events fan-out (ADR 0284). The generated
`wrangler.toml` registered them all with a single migration, `tag = "v1"`, whose
class list grew with the source while the tag never changed. Wrangler picks
migrations by tag (`getMigrationsToUpload` in `cloudflare/workers-sdk`). It finds
the Worker's applied tag in the config and uploads only the migrations after it.
After the first deploy the applied tag is always the config's last, so no
migration is ever uploaded again. A class added later (a new `agent`, or the
fan-out class arriving with a context's first `emit`) got a binding and no
namespace, and the deploy was refused (#1796). ADR 0438 relied on the same skip
from the benign side.

Incremental tags would need a record of what each account has applied. ADR 0194
D1 rules out keeping that record in `bynk.deploy.lock`, and reading it from the
account at deploy time would make the pushed config depend on account state,
so `bynk build` and `bynk deploy` would disagree for the same source.

Cloudflare's declarative `exports` map (Wrangler 4.107.0) replaces the
migrations array. Each class is an `[exports.<Class>]` entry with
`type = "durable-object"` and a `storage` backend, and there is no tag: on every
deploy Cloudflare compares the declared set with the Worker's namespaces and
creates what's missing. Checked against Wrangler 4.107.0, the `WRANGLER_MIN`
floor. Its config schema has the entry shape. It rejects `exports` beside
`migrations` ("mutually exclusive") and an unknown `storage`. It reads `exports`
per environment as an *inheritable* key, as it did `migrations`. A
`wrangler deploy --dry-run` of a compiled agent-plus-fan-out Worker, top-level
and under `--env`, is accepted. Local dev takes each class's SQLite setting from
`exports` (`getDurableObjectClassNameToUseSQLiteMap`). The workers runtime
smoke's agent-state round trip passes under `wrangler dev` at 4.107.0 (run by
hand), and the workerd smokes pass on `wrangler@4` (4.148.0 when checked).

**Decision.**

1. *The emitter declares `exports`, never `[[migrations]]`.* One
   `[exports.<Class>]` per agent class, plus `__EventsFanout` when the context
   emits, each `type = "durable-object"`, `storage = "sqlite"`, beside the
   unchanged `[[durable_objects.bindings]]`. The config stays a pure function of
   the source. `WRANGLER_MIN` is unchanged.
2. *Storage is always SQLite, and key-value-backed Workers are a pre-1.0 break.*
   Bynk 0.309.10 and earlier created agents with `new_classes`, which makes them
   key-value-backed, and Cloudflare can't change a backend in place. The
   emitter can't know an account's backend. Probing the account and rewriting
   `storage` to `legacy-kv` would bring back the account-dependent config this
   decision exists to avoid, and an opt-in `legacy-kv` setting would be a
   permanent option for a few pre-1.0 deployments. So such a Worker must be
   deleted and redeployed, and the deploy guide says so, including that its
   agent state is lost unless the user moves it out and back through their own
   handlers. Workers first deployed by 0.309.11 or later already have
   SQLite-backed classes and redeploy unchanged.
3. *No tombstones.* The emitter never writes `state = "deleted"` or
   `"renamed"`. Either one destroys or moves an agent's data, and that is the
   durable-state migration's decision (#539). Per Cloudflare's docs, removing
   an agent then fails the deploy (an orphaned namespace), which keeps it a loud
   deploy failure.
4. *The plan names the declared set and its owner.* Each class gets one
   advisory line, `durable object <Class> (<storage>; advisory — Cloudflare
   reconciles it)`, read from the emitted config. In JSON that is
   `durable_objects: [{class, storage, reconciled_by: "Cloudflare"}]`, which
   replaces `migration: {tag, applied_by}`. A tombstone or a `type = "worker"`
   entry is not read as a declared class.
5. *Environments copy `exports` verbatim.* Wrangler would inherit it anyway.
   It's copied so the `[env.<name>]` block states every Durable Object fact
   that its non-inheritable bindings rely on.

6. *Below `WRANGLER_MIN`, an agent-bearing deploy is refused.* Before this,
   the floor only mattered to `bynk dev`, and `doctor` says so with a
   warning. A wrangler older than 4.107.0 doesn't read `exports`, so its push
   would carry Durable Object bindings with no namespace and fail with
   Cloudflare's own error. `bynk deploy` knows the project, so it refuses to
   push a context that declares a class on such a wrangler, before
   authenticating, naming the version and the upgrade. `doctor` doesn't know
   the project and keeps the row a warning, since an agent-free project
   still deploys. An npx wrangler can't be versioned without running it, so
   it isn't refused.

This supersedes ADR 0194 D1's migration-tag mechanics (the advisory
`migration v1` line and its JSON form). D1's principle, that the ledger records
nothing where another tool owns the state, is unchanged and now has nothing
left to decline: there is no tag.

**Consequences.** Adding an agent, or the first `emit`, to a deployed context
now deploys. The change is one-way: once a Worker has deployed with `exports`,
Cloudflare won't accept a `[[migrations]]` config for it, so an older Bynk can't
redeploy it. This can't be reverted in a patch. Live confirmation against a
Cloudflare account can't run in CI and is recorded on #1796. That covers a
redeploy after adding a second agent, the old emitter's failure, and the
removed-agent refusal Decision 3 relies on. When #539 needs to rename or
delete a class, `exports` tombstones give it a declarative mechanism.
