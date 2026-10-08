---
level: patch
changelog: "**Agents deploy as SQLite-backed Durable Objects** (emitter/tooling; closes [#1779](https://github.com/accuser/bynk/issues/1779)). The generated `wrangler.toml` declared every agent class, and the events fan-out class, with a `new_classes` migration, which creates key-value-backed Durable Objects. The Workers Free plan allows only SQLite-backed ones, so `bynk deploy` of any project with an `agent` failed there (`code: 10097`). The migration now uses `new_sqlite_classes`, which Cloudflare also recommends for new classes on every plan. Agent code and generated runtime are unchanged: a SQLite-backed Durable Object keeps the key-value storage API, and the emitted agent uses only `storage.get`/`storage.put` on one key. A Worker already deployed with a key-value `v1` keeps it: Wrangler sends only migrations after the deployed tag, so redeploying sends none."
---

## ADR: agents-sqlite-durable-objects
title: Agents are SQLite-backed Durable Objects
summary: The generated migration declares agent classes with new_sqlite_classes; deployed key-value Workers are untouched

**Context.** Every agent compiles to a Durable Object class, and the events
fan-out (ADR 0284) adds one more. The generated `wrangler.toml` registered them
with one migration, `tag = "v1"`, `new_classes = [...]`, which creates
**key-value-backed** Durable Objects. Cloudflare's Workers Free plan allows only
**SQLite-backed** Durable Objects, created with `new_sqlite_classes`, so
`bynk deploy` of any agent-bearing project failed on a free account with
`code: 10097` (#1779, found deploying bynk-lang/compat-board). Cloudflare also
recommends SQLite for every new class on any plan.

Two facts decide whether this can change without a user-facing choice:

1. *The runtime's storage needs.* The emitted agent persists its whole state
   under one `"state"` key with `storage.get` and `storage.put`. It uses no
   `list`, `transaction`, alarms or `storage.sql`, and the fan-out class uses no
   storage at all. A SQLite-backed Durable Object supports the same key-value
   storage API, so the generated code runs unchanged on either backend.
2. *Workers already deployed with `new_classes`.* Cloudflare can't convert a
   deployed class between backends, and a later migration that names an
   existing class in `new_sqlite_classes` fails. But Wrangler picks migrations
   by tag (`getMigrationsToUpload` in `cloudflare/workers-sdk`, checked
   2026-10-08). It reads the Worker's applied tag, finds it in the config, and
   uploads only the migrations after it. When the applied tag is the config's
   last, it uploads no migration at all. Bynk only ever emits the single tag
   `v1`, so a Worker that applied a key-value `v1` gets no migration on
   redeploy, and its classes keep their backend and data.

**Decision.** The generated migration declares agent and fan-out classes with
`new_sqlite_classes` instead of `new_classes`. The tag stays `v1`. The backend
isn't configurable: there's no runtime difference to choose between, and
Cloudflare's own `exports` declaration only allows SQLite for new classes. The
deploy guide says which backend agents use.

**Consequences.** Agent-bearing projects deploy to Workers Free accounts. A
fresh deployment gets SQLite-backed agents, and an existing one keeps
key-value-backed agents, with the same code running on both. One related gap
stays as it was. The single fixed `v1` tag means an agent *added* to an
already-deployed Worker isn't registered by any migration. Cloudflare's
declarative `[exports]` table, which supersedes the `migrations` array and
can't be undone once used, would close that gap, but it's a separate decision.
