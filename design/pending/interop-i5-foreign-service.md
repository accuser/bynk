---
level: patch
changelog: Experimental `bynk.cloudflare` `Service` calls a Worker outside the project over a Service Binding, `capability Billing = Service @worker("legacy-billing")` (bound as `SERVICE_BILLING`); `bynk deploy` binds it without provisioning and warns when the Worker is missing from the account, and `bynk dev` binds a local stand-in unless `--local-worker legacy-billing` names it. `bynk deploy --format json` gains a per-context `foreign_services` list (additive)
probes: emit_diagnostics, ts_writes, diagnostic_coverage
---

## ADR: foreign-worker-service-capability
title: A non-Bynk Worker is reached through a `Service` platform capability
summary: `bynk.cloudflare`'s experimental `Service`, used only through instances that name their Worker with `@worker`, sends `Fetch`'s `Request` over a Service Binding; the binding is compiler-derived, deploy warns and never provisions, dev and tests stand in for the Worker

**Context.** A Bynk context could reach a Worker that is not part of its project only through
`Fetch`, over the public internet: every generated `wrangler.toml` sets
`global_fetch_strictly_public`, and `consumes` reaches only Bynk contexts. The interop track
(`design/tracks/interop.md`, spine #1935) settled D4 (#1972) as option (a): a first-party platform
capability used through the standard-library track's derived capabilities (its D3, ADR 0469), and
listed this as its front-loaded ADR 4, landing with slice I5. §11 Q8 settled the two platform
behaviours: `bynk deploy` warns, never refuses, when the named Worker is missing, and `bynk dev`
binds a per-instance test double by default and a locally running Worker where Miniflare supports
it. Both were to be confirmed before shipping. I5 is platform work only (ADR 0023): no grammar
changes.

**Decision.**

- **D1 — The capability.** `bynk.cloudflare` exports `Service` with one operation,
  `send(req: Request) -> Effect[Result[Response, FetchError]]`. The types are `bynk`'s, so
  `bynk.cloudflare` now `consumes bynk { Fetch }`. The call is untyped, as `Fetch` is. A typed
  client for the foreign Worker is ordinary Bynk over it (a provider over the instance).
- **D2 — Instances only, naming their Worker.** `capability Billing = Service @worker("legacy-billing")`.
  `bynk-std`'s `Instantiable` gains `required` options and a `bare` flag. `Service` requires
  `@worker` (`bynk.capability.missing_option`) and is never usable bare
  (`bynk.capability.instance_only`, on the `given`). An option takes one positional string literal,
  once (`bynk.capability.invalid_option`). `@worker`'s value must be a Worker name: 1–63 lowercase
  letters, digits, `-` or `_`, not starting or ending with `-`, and not the Worker of one of the
  project's own contexts: deploy and dev read any build Worker as a project edge, so a Bynk context
  is reached with `consumes`. The resolved value is carried on the instance
  (`DerivedInstance::options`).
- **D3 — `@worker`, not D4's `@service`.** `service` is a keyword, so `@service(…)` does not parse.
  Admitting keywords as annotation names is a grammar change, which ADR 0023 keeps out of a platform
  increment. `@worker` also names what the value is: the callee's Worker name.
- **D4 — One binding list.** A new `BindingSource::ForeignService { capability, worker }` joins the
  per-Worker binding list (ADR 0462/0464/0466). The binding is the kind's prefix and the capability
  (`Billing` → `SERVICE_BILLING`). From that one entry come the `[[services]]` stanza
  (`service = "legacy-billing"`, the name as declared, not a project `worker_dir_name`), the `Env`
  member (`__ServiceBinding`), and the duplicate-name check. It is listed with the Service Bindings
  to project contexts. Handlers never see it on `deps.env`: they reach it through the instance's
  provider. The binding list stays compiler-derived, so user code can bind only the Worker its
  source names.
- **D5 — The provider.** `WorkersService(env, binding)` reads `env[binding]` per call and throws a
  clear error naming it when absent. It delegates to the Cloudflare `FetchProvider`, which now takes
  an optional fetch function, so `send` shares `Fetch.send`'s header rules (`InvalidHeader` before
  anything is sent), response reading and `Network`/`Timeout` mapping. A 5xx from the Worker is an
  `Ok` response.
- **D6 — Deploy reads foreignness from the build.** Deploy reads only generated output (ADR 0193
  D3), so a `[[services]]` entry is foreign exactly when its target is none of the build's Workers;
  every context a project consumes is built beside it. A foreign binding is kept out of the deploy
  order, the `--context` absent-dependency gate and the contract-skew gate, and under `--env` its
  target is copied as declared rather than environment-qualified: Bynk did not name it, so
  `legacy-billing-staging` would be a Worker nobody made.
- **D7 — Deploy warns, never refuses (§11 Q8).** The plan shows each binding with no action
  (`service SERVICE_BILLING legacy-billing (foreign Worker; not provisioned)`; in JSON,
  `foreign_services: [{binding, worker}]`). After authenticating and before confirming, deploy asks
  the account once per Worker with `wrangler deployments list --name <worker> --json`. Cloudflare's
  "Worker not found" codes (10007, or 10090 for a legacy environment) produce a warning that the
  upload will be rejected. Any other failure produces a warning that the check could not be made.
  `--dry-run` stays offline and does not ask. The driver refuses nothing: a Worker created between
  the check and the push is fine.
- **D8 — Dev stands in by default (§11 Q8).** For each context it serves, `bynk dev` writes one
  stand-in Worker per foreign Worker the context binds, under `.bynk/dev/stand-ins/`, and passes it
  to the context's own `wrangler dev` as an extra `-c` config. A stand-in answers every request with
  503 and a body naming the bindings and the way out.
  - It runs under a name of its own, `bynk-stand-in-<context>-<worker>` (cut and hashed past 63
    characters). A `-c` Worker also registers in wrangler's shared dev registry, so under the real
    name it would answer other local sessions' bindings to that Worker, and once stopped it could
    leave a stale entry.
  - The context is served from `wrangler.dev.toml`, a copy of its generated `wrangler.toml` with
    the binding pointed at the stand-in, refreshed by an atomic rename after every build. Editing
    `wrangler.toml` in place would let a watching `wrangler dev` reload once, between the rebuild
    and the edit, with the real Worker bound.
  - `--local-worker <name>` (repeatable) drops a Worker's stand-in and leaves the binding as
    declared, so the dev registry connects it to the user's own `wrangler dev` of that Worker.
  - `-- --remote` runs no stand-ins.
  - A `-c`/`--config` passthrough while stand-ins run is refused, as `--port` is.
  - The stand-in is per context, which is per instance in effect: two instances naming one Worker
    in one context share its stand-in.
- **D9 — Tests.** At the unit and integration tiers each instance has its own double, which faults
  until `send` is stubbed (`stub Billing.send(_) returns …`), as `Fetch`'s does. A `system` suite
  runs participants' Workers with their own bindings, and binds a foreign Worker to the same
  503 stand-in `bynk dev` uses.
- **D10 — Admission.** Admitted through the standard-library track's process:
  - **Placement:** L3, in `bynk.cloudflare`.
  - **Stability:** `experimental`, the first experimental first-party item. Using it needs
    `[unstable] allow = ["bynk.cloudflare.Service"]`. Its surface is untyped and its option's
    spelling is new, so it waits outside the 1.0 promise.
  - **Binding scaffold:** behavioural tests of `WorkersService.send` in
    `bynk-std/test/cloudflare.binding.test.mjs`, required by the coverage gate.
  - **Standalone scaffold:** the scaffold now allows every experimental item, so it consumes the
    whole library.

**Platform facts confirmed (2026-10-16, wrangler 4.149.0, Linux).**

- *Local, run here.*
  - A `wrangler dev` whose `[[services]]` binding names a Worker no local session runs answers the
    binding's `fetch` with 503, body `Worker "legacy-billing" not found. Make sure it is running
    locally.`. It shows the binding as `local [not connected]`.
  - With a separate `wrangler dev` of `legacy-billing` running, the binding shows
    `local [connected]` and reaches it through the dev registry. This is the "locally running
    Worker" path, and `--local-worker` selects it.
  - `wrangler dev -c wrangler.toml -c <stand-in>/wrangler.toml` answers the primary's binding from
    the in-process `-c` Worker.
  - That `-c` Worker also registers in the dev registry (`~/.config/.wrangler/registry`), under
    its own name. A stand-in named as the real Worker once left a stale entry that a later
    `[connected]` binding reached, failing every call. Hence D8's own name. With it, `bynk dev`
    reached the stand-in while a real `legacy-billing` session ran, and with `--local-worker
    legacy-billing` it reached the real one (`200`).
  - Cloudflare documents multi-config `wrangler dev` as experimental and subject to change, so D8
    depends on it.
- *Account, read from wrangler 4.149.0's source.* No live account was available.
  - `wrangler deployments list --name X` reads `GET /accounts/<id>/workers/scripts/X/deployments`.
  - A missing Worker fails with the API's code 10007 (`WORKER_NOT_FOUND_ERR_CODE`; 10090 for a
    legacy environment), rendered as `… [code: 10007]`.
  - Cloudflare's Service Bindings documentation says a deploy fails when a bound Worker "does not
    yet exist". So the warning is advance notice: Cloudflare, not Bynk, rejects the upload.
  - The live listing is to be checked against a real account at the first deploy that uses it.

**Consequences.**
- `bynk.cloudflare` consuming `bynk` means every project that consumes `bynk.cloudflare` also gets
  `bynk`'s emitted modules (`bynk.ts`, `bynk-<platform>.ts`, `bynk/locale/types.ts`), and its
  `bynk/cloudflare.ts` imports `bynk`'s `Request`, `Response` and `FetchError`. KV-only fixtures'
  expected output grows accordingly. Runtime behaviour is unchanged.
- A context that calls `send` must also `consumes bynk` to name `Request`. Without it, the call's
  type does not resolve in the consumer, and the error is the misleading
  `bynk.effect.bind_on_non_effect`. This is a pre-existing gap for any adapter operation typed by
  another unit's types, left for its own fix. The capability's doc comment states the requirement.
- Because `service` is a keyword, no context name can start with `service.`, so a consumed context's
  binding never takes `SERVICE_…`. An agent `ServiceBilling` and `capability Billing = Service` can
  still clash, and the duplicate check reports it.
- `global_fetch_strictly_public` stays: `Fetch` remains public, and `Service` is the only private
  path.
- Deferred:
  - an RPC (`WorkerEntrypoint`) surface (track §11 Q9);
  - typed clients generated from a foreign Worker's types;
  - user-chosen stand-in responses under `bynk dev`;
  - per-environment Worker names for a foreign Worker.
