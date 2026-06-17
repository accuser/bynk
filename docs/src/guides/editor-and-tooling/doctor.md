# Check your environment with `karn doctor`

**Goal:** find out whether your machine is ready to compile, test, and deploy
Karn — before you hit a broken command.

Karn has a Rust side (the `karnc` compiler) and a Node side (`tsc`/`tsx` to run
the emitted TypeScript, `wrangler` to deploy to Cloudflare). `karn doctor`
checks both in one go and prints the exact remedy for anything missing.

> **Understand — the toolchain has two halves.** `karnc` compiles Karn to
> TypeScript; that's pure and needs nothing but the compiler. *Running* the
> output (`karn test`) needs Node and a TypeScript runner; *deploying* needs
> `wrangler`. `karn doctor` groups its checks along exactly those lines, so you
> are never told you're "unhealthy" for lacking a tool you don't need.

## Run it

```sh
karn doctor
```

You'll get a grouped report — one block per capability — showing each tool's
**presence, version, and provenance**, with a fix line under anything that
isn't ready:

```text
karn doctor — environment report
driver: karn 0.46.0
compiler: karnc at /usr/local/bin/karnc

✓ compile [ok]
    karnc — 0.46.0 (path)
✓ test [ok]
    node — v20.11.0 (path)
    tsc | tsx — tsc v5.4.2 (path)
! deploy [warn]
    node — v20.11.0 (path)
    wrangler — provisionable via npx (not installed)
      ↳ fix: npm install -g wrangler
· editor [note] (optional)
    karnc-lsp — missing
      ↳ fix: install karnc-lsp (or download from releases)
```

## The capability groups

| Capability | Needs | Missing means |
|---|---|---|
| **compile / check / fmt** | `karnc` itself | the compile floor is broken |
| **`karn test`** | Node **and** `tsc` or `tsx` | you can't run `test` blocks |
| **dev / deploy** | Node **and** `wrangler` | you can't deploy to Cloudflare |
| **editor** *(optional)* | `karnc-lsp` | a note — editor features only |
| **build-from-source** *(optional)* | a Rust toolchain | shown only inside the Karn repo |

### Provenance, and why `npx` isn't "ok"

Each tool reports **where** it was found: on your global `PATH`, in a
project-local `node_modules/.bin`, or only **provisionable via `npx`**. That last
one is a warning, not a pass: `npx --yes` *downloads* the package the first time
you use it, so an environment that "works via npx" still pauses to fetch on first
real use. `doctor` tells you the difference.

### Driver↔compiler skew

Because `karn` and `karnc` are separate binaries, a globally-installed `karn`
can end up shelling an older `karnc`. `doctor` flags that: a minor version drift
warns, a major drift is an error.

## Exit codes — for scripts and CI

The exit code depends on **what you asked about**:

- **Bare `karn doctor`** is informational. It surveys everything but only fails
  if `karnc` itself is unusable — so a compile-only user exits `0` even without
  Node or `wrangler`.
- **`karn doctor --only <capability>`** gates on one capability. `karn doctor
  --only deploy` exits non-zero on a machine that genuinely can't deploy.
- **`karn doctor --strict`** turns *every* warning — optional gaps, `npx`
  provisionability, minor skew — into a failure. Use it for an all-green CI gate.

## Machine-readable output

Two formats are a stable, scriptable contract:

```sh
karn doctor --format short    # one `capability: level (remedy)` line per row
karn doctor --format json     # structured, for CI
```

```text
compile: ok
test: ok
deploy: warn (npm install -g wrangler)
editor: note (install karnc-lsp (or download from releases))
```

`doctor` only **reports** — it never installs anything. Copy the fix line it
prints and run it yourself.
