---
title: "Check your environment with `bynk doctor`"
---
**Goal:** find out whether your machine is ready to compile, test, and deploy
Bynk — before you hit a broken command.

Bynk has a Rust side (the `bynkc` compiler) and a Node side (`tsc`/`tsx` to run
the emitted TypeScript, `wrangler` to deploy to Cloudflare). `bynk doctor`
checks both in one go and prints the exact remedy for anything missing.

> **Understand — the toolchain has two halves.** `bynkc` compiles Bynk to
> TypeScript; that's pure and needs nothing but the compiler. *Running* the
> output (`bynk test`) needs Node and a TypeScript runner; *deploying* needs
> `wrangler`. `bynk doctor` groups its checks along exactly those lines, so you
> are never told you're "unhealthy" for lacking a tool you don't need.

## Run it

```sh
bynk doctor
```

You'll get a grouped report — one block per capability — showing each tool's
**presence, version, and provenance**, with a fix line under anything that
isn't ready:

```text
bynk doctor — environment report
driver: bynk 0.66.0
compiler: in-process

✓ compile [ok]
    compiler — in-process
✓ test [ok]
    node — v20.11.0 (path)
    tsc | tsx — tsc v5.4.2 (path)
! deploy [warn]
    node — v20.11.0 (path)
    wrangler — provisionable via npx (not installed)
      ↳ fix: npm install -g wrangler
· editor [note] (optional)
    bynkc-lsp — missing
      ↳ fix: install bynkc-lsp (or download from releases)
```

## The capability groups

| Capability | Needs | Missing means |
|---|---|---|
| **compile / check / fmt** | nothing — the compiler is built into `bynk` | always available |
| **`bynk test`** | Node **and** `tsc` or `tsx` | you can't run `test` blocks |
| **dev / deploy** | Node **and** `wrangler` | you can't deploy to Cloudflare |
| **editor** *(optional)* | `bynkc-lsp` | a note — editor features only |
| **build-from-source** *(optional)* | a Rust toolchain | shown only inside the Bynk repo |

### Provenance, and why `npx` isn't "ok"

Each tool reports **where** it was found: on your global `PATH`, in a
project-local `node_modules/.bin`, or only **provisionable via `npx`**. That last
one is a warning, not a pass: `npx --yes` *downloads* the package the first time
you use it, so an environment that "works via npx" still pauses to fetch on first
real use. `doctor` tells you the difference.

### The in-process compiler (and the `BYNK_BYNKC` override)

`bynk` **links the compiler in-process**, so compiling needs no separate `bynkc`
binary — `compile` is always available, and there is no version to drift. Power
users can still point `bynk` at an *external* compiler with the **`BYNK_BYNKC`**
environment variable (e.g. to pin a specific version); only then does `doctor`
resolve that binary and report **driver↔compiler skew** — a minor drift warns, a
major drift is an error. With no override there is no second compiler to check.

## Exit codes — for scripts and CI

The exit code depends on **what you asked about**:

- **Bare `bynk doctor`** is informational. It surveys everything but only fails
  if the compile floor is broken — and with the in-process compiler that only
  happens under a broken `BYNK_BYNKC` override — so a compile-only user exits `0`
  even without Node or `wrangler`.
- **`bynk doctor --only <capability>`** gates on one capability. `bynk doctor
  --only deploy` exits non-zero on a machine that genuinely can't deploy.
- **`bynk doctor --strict`** turns *every* warning — optional gaps, `npx`
  provisionability, minor skew — into a failure. Use it for an all-green CI gate.

## Machine-readable output

Two formats are a stable, scriptable contract:

```sh
bynk doctor --format short    # one `capability: level (remedy)` line per row
bynk doctor --format json     # structured, for CI
```

```text
compile: ok
test: ok
deploy: warn (npm install -g wrangler)
editor: note (install bynkc-lsp (or download from releases))
```

`doctor` only **reports** — it never installs anything. Copy the fix line it
prints and run it yourself.

## Related

[`bynk dev`](/book/guides/projects-build-and-deployment/run-locally/) pre-flights the
same **deploy** capability (Node + `wrangler`) before it serves, so a green
`bynk doctor --only deploy` is exactly what `dev` needs to run.
