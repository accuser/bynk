---
level: minor
changelog: "Every generated `wrangler.toml` now carries `compatibility_flags = [\"global_fetch_strictly_public\"]`, so a `Fetch` to a URL goes over the public internet even when the URL is another Worker on the same Cloudflare account; before, Cloudflare refused it (error 1042, or a fast `404` from a `workers.dev` sibling). `bynk.toml` gains a `[workers]` table whose `compatibility_flags` list is appended after the default: names pass through unchecked, a duplicate is dropped with a `bynk.project.duplicate_compatibility_flag` warning, and anything but a list of strings is an error. Every deployed Worker's configuration changes on its next deploy (#1890)"
---

## ADR: workers-compatibility-flags
title: Every Worker fetches over the public internet, and a project can add compatibility flags
summary: `global_fetch_strictly_public` is on by default; `[workers] compatibility_flags` appends unchecked extras

**Context.** A generated `wrangler.toml` set only `compatibility_date`, pinned by
the compiler, and `bynk.toml` had no way to set Cloudflare compatibility flags.
That blocked a Worker fetching another Worker on the same account: Cloudflare
refuses such a `fetch()` unless `global_fetch_strictly_public` is on (error
1042). bynk-lang/status's cron checks of two sibling Bynk services' `workers.dev`
URLs came back `404` in about 20 ms while the same URLs answered `200` from
outside, and the only workaround was passing `--compatibility-flags` to
`wrangler deploy` from outside the project (#1890).

**Decision.**

1. **The flag is on by default.** Every emitted `wrangler.toml` carries
   `compatibility_flags = ["global_fetch_strictly_public"]`
   (`DEFAULT_COMPATIBILITY_FLAGS`, beside `COMPATIBILITY_DATE` in
   `bynk-emit/src/emitter/wrangler.rs`). In Bynk, `Fetch` to a URL means over
   the public internet, and that is exactly what the flag does: `fetch()` leaves
   through the internet even when the URL resolves to the same zone or account,
   instead of being routed, or refused, inside it. Contexts in one project
   reach each other through Service Bindings (`consumes`), which the flag
   doesn't touch, so the default changes nothing a project's own wiring relies
   on. It isn't opt-in because the failure it fixes is silent and
   confusing, and a project that wants same-zone routing has no Bynk construct
   that would express it.
2. **`bynk.toml` gains `[workers] compatibility_flags`**, a list of strings
   appended after the default set, in manifest order, in every Worker of the
   project. The CLIs read it with the rest of the manifest
   (`bynk-driver`'s option builders), and it reaches `bynk-emit` on
   `CompileOptions::compatibility_flags`. `bynkc compile`, `bynk dev` and
   `bynk deploy` all go through that path, so `bynk deploy` needs no `--`
   passthrough for a flag.
3. **Names pass through unchecked.** Cloudflare adds flags faster than a list in
   the compiler could follow, and a known-names check would block a flag newer
   than the compiler. A misspelt name is left for Wrangler and the runtime to
   reject. A flag listed twice, or one already in the default set, is dropped
   with a `bynk.project.duplicate_compatibility_flag` warning against
   `bynk.toml`, once per build rather than once per Worker, and `check`
   reports it as `compile` does. It fires whatever the build target, since the
   duplicate is a fact about the manifest. A non-string entry,
   or a value that isn't a list (including a bare string), is a manifest error,
   reported with the other `bynk.toml` errors.
4. **No per-environment copy.** `bynk deploy --env` appends an `[env.<name>]`
   block to the generated config; `compatibility_flags` is inheritable in
   Wrangler's configuration, so the block doesn't repeat it.
5. **The default set is reviewed with the date.** The compatibility-date review
   (`design/bynk-release-discipline.md`, Part 3) also checks each default flag:
   one the new date turns on is redundant, and one a new default contradicts
   needs a decision.

Not decided here: overriding `compatibility_date` from the manifest, and service
bindings between separately deployed Bynk projects, which would be the
idiomatic Worker-to-Worker route. Both were raised in #1890 and left for later.

**Consequences.** Every deployed Bynk Worker's configuration changes on its next
deploy, and a Worker that fetched a same-zone URL now reaches it over the
public internet, with that URL's public routing and any access rules it has. A
project can set any compatibility flag without leaving the project. The
`[deploy]` table stays reserved for #551.
