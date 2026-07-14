# 0096 — `bynk dev`: build + serve locally, no provisioning

- **Status:** Accepted (v0.57). **D3 superseded by [[0191]]** (v0.166) — `dev`
  now serves *every* context at once with the Service Bindings between them
  wired, so the select-or-default rule and its ambiguity error are withdrawn;
  `--context` survives as a repeatable narrowing flag. D1, D2, D4 and D5 stand.
  0191 also withdraws the D5 note below that an explicit `-- --inspector-port N`
  "still wins": wrangler rejects a repeated flag rather than taking the last.
- **Realises:** the v0.57 `bynk dev` proposal (consumed and removed on landing,
  per the proposals lifecycle; history in `git log -- design/proposals/`).
- **Relates:** [[0083]] (the `bynk` driver, thin orchestrator), [[0084]] (the
  `doctor` capability/exit contract `dev` pre-flights), [[0017]] (platform lock
  per deployment unit).

## Context

Running a project locally was a multi-step ritual — `bynkc compile … --target
workers`, `cd` into a generated worker directory whose name you had to look up,
an apparent KV-provisioning step, then a bare `wrangler dev`. The driver already
owns the surrounding concerns ([[0083]]): `bynkc` resolution, tool probing,
project rooting. `dev` is the third step of the driver arc `doctor → new → dev`;
it collapses the ritual to one command runnable from anywhere inside a project.

The decisions below are the defining calls — where the build output lives, how
a project's worker is chosen, and what `dev` is *not* responsible for. No
language surface changes; this is driver tooling.

## Decision

**Orchestration.** `dev` is `pre-flight → compile → select → serve`, each step
reuse: `compiler::resolve` for `bynkc`, the [[0084]] `Deploy` capability for the
Node + `wrangler` gate (a missing required tool fails here, before any build,
with doctor's own remedy text), `bynkc compile --target workers`, then
`wrangler dev` spawned with **cwd set to the selected worker directory** — the
emitted `index.ts` imports `../../runtime.js`, so the tree must stay intact and
wrangler must run from the worker dir (exactly the manual recipe's `cd`).

**(D1) Build to a managed `.bynk/dev/`.** `dev` compiles into a driver-owned
`.bynk/dev/` under the project root — the cargo-shaped answer (`bynk` is to
`bynkc` as `cargo` is to `rustc`). The driver writes `.bynk/.gitignore`
containing `*` on first build, so a `dev` run never dirties `git status`. The
user's own `out/` (explicit `bynkc compile`) is left untouched. Because `bynkc
compile` is **additive** (it `create_dir_all`s and writes per file but never
prunes), `dev` **clears `<build>/workers/` before each compile** — otherwise a
renamed or deleted context would leave a phantom worker dir that the selection
rule below would read as a live context. This clean step is the concrete
coupling between D1 and D3.

**(D3) Context selection is select-or-default.** A project may hold several
contexts → several workers; `wrangler dev` serves one. One context → served
automatically; `--context NAME` chooses (accepting the dotted name or its
dasherised worker-dir form); ambiguous → fail and **list the contexts**.
Multi-worker local dev with live cross-context Service Bindings is a real
feature with its own design and is **out of scope** — named as a limitation, not
silently half-done.

**(D4) No provisioning at `dev` time; local mode needs none.** `dev` provisions
nothing and never edits `wrangler.toml`. `wrangler dev` defaults to local mode
(Miniflare), which simulates KV / Durable Objects / queues keyed by **binding
name**; the generated `id = "<KV_NAMESPACE_ID>"` placeholder is read only under
`--remote`. So `dev` serves a KV- or agent-backed project against the generated
config untouched. There is no `--remote` flag — a flag whose only behaviour is
to error is worse than no flag; remote dev is reachable via the passthrough
(`bynk dev -- --remote`). Real, provisioned remote support is `deploy`'s
defining problem, the next slice.

**(D5, posture) Forward-compatibility via `--` passthrough.** The driver curates
**only `--context`** — its own concept, not a wrangler flag — and forwards
everything after `--` to `wrangler dev` verbatim (`-- --port`, `-- --var
KEY:VALUE` for local secrets). No curated `--port`: it rides the passthrough,
since two paths to one wrangler flag is the drift this avoids.

**(D2, posture) Compile-once MVP; watch is a follow-up.** This slice ships
compile-once + hand-off (wrangler reloads the TypeScript it serves). The watch
loop over `.bynk` sources — a file-watcher dependency, debounce, and a
recompile-failure UX — is a named v0.x follow-up, not folded in here.

**Exit / signals.** On a clean hand-off `dev` exits with wrangler's own exit
code. The driver and wrangler share the terminal's foreground process group, so
a Ctrl-C `SIGINT` reaches both — there is nothing to "forward"; the driver waits
and propagates rather than bailing early. Pre-flight and build failures exit
non-zero before serving. The deterministic surface (the pre-flight report and
the selection messages) is golden-pinned in the style of [[0084]]; the
non-deterministic `wrangler dev` stream is not.

## Consequences

The felt friction is gone: one command, from anywhere in the project, serves
every example in the gallery — including the KV- and Durable-Object-backed ones
— with nothing to provision. `dev` is tractable independently of `deploy`
precisely because local mode carries the simulated resources.

The local-mode premise (D4) was the one open risk at sign-off — it rested on
wrangler's documented behaviour, not a live run. It is now **verified**: against
`wrangler dev` 4.103.0, the KV-backed example served with the binding reported
`local` and the `<KV_NAMESPACE_ID>` placeholder never validated, a full
KV write → read-back → miss round-trip succeeded, and a process-group `SIGINT`
tore down `bynk`/`wrangler`/`workerd` cleanly. So the hedged D4 fallback
("rewrite the placeholder to a throwaway local id before serving") is **not
needed** and was not built.

The serve step is fully encapsulated behind selection/pre-flight, so the v1
first-party `workerd` dev server anticipated in the roadmap can replace the
wrangler hand-off without touching the rest. Deferred and logged as next intent:
the watch loop (D2), multi-worker local dev (D3), and `deploy` / provisioning
(D4).
