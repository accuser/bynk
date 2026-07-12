---
title: "CLI (`bynk` driver)"
---
The **`bynk`** driver is the developer front-end — it **links the compiler
pipeline in-process** and orchestrates the Node toolchain (`bynk` is to `bynkc`
as `cargo` is to `rustc`). A fresh `cargo install bynk` is self-contained: it
compiles, checks, and formats without a separately-installed `bynkc`. This page
is the argument and exit-code reference for its subcommands. The pure-pipeline
binary [`bynkc`](/docs/cli/) (`compile`, `check`, `fmt`, `test`) survives for CI
and direct use.

The everyday commands — [`bynk check`](#bynk-check), [`bynk fmt`](#bynk-fmt), and
[`bynk test`](#bynk-test) — mirror their `bynkc` counterparts exactly (same flags,
same output, same exit codes), so the two are drop-in equivalent. `check` and
`fmt` run the pipeline in-process; `test` delegates to the `bynkc` the driver
resolves (see [Which `bynkc`?](#which-bynkc)), so an editor or developer inherits
the driver's resolution instead of locating the compiler themselves.

```text
bynk <command> [options]
```

| Command | What it does |
|---|---|
| [`bynk doctor`](#bynk-doctor) | Check whether your machine is ready to compile, test, and deploy. |
| [`bynk new`](#bynk-new) | Scaffold a new, runnable project. |
| [`bynk dev`](#bynk-dev) | Build the project and serve it locally with `wrangler dev`. |
| [`bynk deploy`](#bynk-deploy) | Provision the required KV namespace and deploy one Worker to Cloudflare. |
| [`bynk check`](#bynk-check) | Type-check a file or project without writing output. |
| [`bynk fmt`](#bynk-fmt) | Format `.bynk` source files in place. |
| [`bynk test`](#bynk-test) | Discover and run a project's tests. |

---

## `bynk new`

Scaffold a new project: a complete, runnable single-context HTTP service you can
serve immediately with [`bynk dev`](#bynk-dev). See the guide [Start a new
project](/book/guides/projects-build-and-deployment/start-a-project/) for a worked
walkthrough.

```text
bynk new <PATH> [--name NAME]
```

| Argument | Default | Meaning |
|---|---|---|
| `PATH` | *(required)* | Directory to create for the new project (e.g. `hello` or `./hello`). Parent directories are created. |
| `--name NAME` | `PATH`'s final component | Project name / context identifier. Must be a legal Bynk identifier (a letter followed by letters, digits, or underscores — no dashes or dots). |

**What it writes**

```text
<PATH>/
├── bynk.toml            # [project] name/version + optional [paths] include/exclude
├── .gitignore           # /.bynk
└── src/
    └── <name>.bynk      # context <name> — a GET "/" HTTP service
```

**Behaviour** — `bynk new` is pure, offline file-writing: it shells nothing,
compiles nothing, and reads no network, so it works before `bynkc`, Node, or
`wrangler` are installed.

1. Derive the project name from `PATH`'s final component (or `--name`) and
   validate it as a legal Bynk identifier — both `[project] name` and the
   starter's context use it.
2. Refuse to clobber: if the target exists and is non-empty, fail before writing
   anything. An empty directory is fine; VCS/OS cruft (`.git`, `.gitignore`,
   `.DS_Store`, …) doesn't count as non-empty.
3. Write the scaffold and print next steps (`cd <path> && bynk dev`).

**Exit code** — `0` on a written scaffold. A non-empty target or a name that
isn't a legal identifier exits non-zero, **touching nothing**.

**Notes**

- `bynk new` never overwrites a file it didn't create, and never runs `git init`
  or writes outside the project — the scaffold drops cleanly into an existing
  repository.
- The `.gitignore` covers only `/.bynk`, the build directory
  [`bynk dev`](#bynk-dev) writes (compiled workers and local wrangler state).

---

## `bynk dev`

Build the project and serve it locally — one step in place of the manual
`bynkc compile` + `cd` + `wrangler dev` recipe. See the guide [Run your project
locally](/book/guides/projects-build-and-deployment/run-locally/) for a worked
walkthrough.

```text
bynk dev [PATH] [--context NAME] [--inspect] [--inspect-port N] [-- <wrangler args>]
```

| Argument | Default | Meaning |
|---|---|---|
| `PATH` | `.` | A directory inside the project. The root is found by walking up for `bynk.toml`. |
| `--context NAME` | — | Which context's worker to serve, for multi-context projects. Accepts the dotted name (`a.b`) or its worker-directory form (`a-b`). |
| `--inspect` | off | Serve with the V8 inspector enabled (`wrangler dev --inspector-port`) so a JavaScript debugger can attach. Breakpoints set in `.bynk` sources resolve through the emitted source maps, composed into the worker bundle. Prints the inspector URL on start. |
| `--inspect-port N` | `9229` | Inspector port for `--inspect`. |
| `-- <wrangler args>` | — | Everything after `--` is forwarded to `wrangler dev` verbatim (e.g. `-- --port 8788`). |

**Behaviour**

1. Locate the project root and read `[paths] include`.
2. Pre-flight the `deploy` capability (`bynkc`, Node, `wrangler`) exactly as
   [`doctor`](#bynk-doctor) does; a missing required tool fails here, before any
   build, with doctor's remedy text.
3. Compile to the managed **`.bynk/dev/`** build directory (gitignored
   automatically; the `workers/` tree is cleared before each build).
4. Select the worker: one context is served automatically; `--context` chooses
   among several; an ambiguous project fails and lists the available contexts.
5. Run `wrangler dev` from inside the selected worker directory, in local mode
   (Miniflare) — **no namespace provisioning is needed** and `wrangler.toml` is
   served untouched.

**Exit code** — On a successful hand-off, `bynk dev` exits with `wrangler`'s own
exit code (a clean Ctrl-C stop is a `0`). A pre-flight or build failure exits
non-zero before serving.

**Notes**

- `bynk dev` provisions nothing. For `bynk dev -- --remote`, a prior
  [`bynk deploy`](#bynk-deploy) must have recorded the KV namespace id in
  `bynk.deploy.lock`.
- `wrangler` is resolved with the same provenance ordering as `doctor`
  (project-local `node_modules/.bin` → `PATH` → `npx`). An `npx` resolution is
  surfaced as a notice — it downloads on first use.

---

## `bynk deploy`

Provision a KV namespace when needed and deploy a single-context Worker to
Cloudflare. See [Deploy to Cloudflare](/book/guides/projects-build-and-deployment/deploy-to-cloudflare/)
for the workflow.

```text
bynk deploy [PATH] [--dry-run] [--format short|json] [--yes] [-- <wrangler args>]
```

| Argument | Default | Meaning |
|---|---|---|
| `PATH` | `.` | A directory inside the project. The root is found by walking up for `bynk.toml`. |
| `--dry-run`, `--plan` | off | Print the KV and deploy plan and exit without changing Cloudflare or `bynk.deploy.lock`. |
| `--format FORMAT` | `short` | Plan output: line-oriented `short` or machine-readable `json`. |
| `--yes` | off | Skip the confirmation required before a namespace is created or a Worker is published. Required for non-interactive calls. |
| `-- <wrangler args>` | — | Everything after `--` is forwarded to `wrangler deploy` verbatim. |

**Behaviour** — the command pre-flights Node and Wrangler, compiles into
`.bynk/deploy/`, reads the generated KV declaration, prints a plan, checks
Wrangler authentication, and then provisions, materialises, and deploys. The
Cloudflare id is recorded in the committed, secret-free `bynk.deploy.lock` at
the project root. A recorded id is reused on later deploys. CI refuses to
create an unrecorded namespace; provision it locally and commit the lock file
first.

**Exit code** — `0` on a successful plan or deploy; non-zero for missing tools
or authentication, declined confirmation, compilation failures, an unrecorded
CI resource, or a Wrangler failure.

---

## `bynk check`

Type-check a `.bynk` file or project without writing output — the same behaviour
as [`bynkc check`](/docs/cli/#bynkc-check), through the driver. Runs the compiler
pipeline **in-process**: no separately-installed `bynkc` is required.

```text
bynk check [INPUT] [--format rich|short]
```

| Argument | Default | Meaning |
|---|---|---|
| `INPUT` | `.` | A `.bynk` file, or a project root directory (a `bynk.toml` or `src/` subdir selects project mode; otherwise the directory is itself the source tree). |
| `--format` | `rich` | `rich` is the source-context rendering; `short` emits one terse `path:line:col: severity[category]: message` line per diagnostic, for the VS Code problem-matcher, CI, and scripts. |

**Exit code** — `0` when the input type-checks (warnings are surfaced but do not
fail the build, per the [diagnostics rule](/docs/cli/#exit-codes-and-diagnostics));
non-zero on any error-severity diagnostic.

---

## `bynk fmt`

Format `.bynk` source files in place — the same behaviour as
[`bynkc fmt`](/docs/cli/#bynkc-fmt), through the driver, run **in-process**.

```text
bynk fmt <INPUTS>... [--check]
```

| Argument | Default | Meaning |
|---|---|---|
| `INPUTS` | *(required)* | Files to format. Pass `-` to read from stdin and write the formatted result to stdout. |
| `--check` | off | Report files that are not already canonically formatted **without writing changes**. Exits non-zero if any file would change. For CI. |

**Behaviour** — each file is formatted and rewritten only when it changes; a file
already canonical is left untouched. A file that does not parse is reported and
skipped; the other inputs are still processed.

**Exit code** — `0` when every input was formatted (or, under `--check`, already
canonical). Non-zero if a file could not be read/written, failed to parse, or
(under `--check`) was not canonical.

---

## `bynk test`

Discover and run a project's test declarations — the same behaviour as
[`bynkc test`](/docs/cli/#bynkc-test), through the driver. Unlike `check` and
`fmt`, `test` **delegates** to the resolved `bynkc` (it orchestrates external
`tsc`/`node` anyway), forwarding every flag verbatim. Requires `tsc` (with
Node.js) or `tsx` on `PATH`, exactly as `bynkc test`.

```text
bynk test [INPUT] [-o OUTPUT] [--no-run] [--format rich|json] [--inspect] [--seed HEX] [--case NAME]
```

| Argument | Default | Meaning |
|---|---|---|
| `INPUT` | `.` | Project root directory. |
| `-o, --output OUTPUT` | `<input>/out` | Where to write the compiled TypeScript test-runner modules. |
| `--no-run` | off | Skip the runner. With `--format rich`, emit the generated test files; with `--format json`, emit a discovery document listing every suite and case without running them. |
| `--format` | `rich` | `rich` is the grouped ✓ / ✗ human output; `json` is a single pinned JSON document of results, for tooling. |
| `--inspect` | off | Launch the runner under Node's inspector (`node --inspect-brk`) and print the inspector URL. Requires Node ≥ 22.18 (or ≥ 23.6 unflagged). Does not run `tsc`. |
| `--seed HEX` | random | Root seed for generative `property` tests (e.g. `0x5f3a`). A failing property prints the seed it used; re-running with `--seed <hex>` reproduces the run byte-for-byte. |
| `--case NAME` | — | Run only test cases whose name matches `NAME` — the filter behind the editor's per-case *▷ Run Test* lens. No effect with `--no-run`. |

**Exit code** — follows the runner's own process status: `0` when every case
passed, non-zero on a failing case, a compile error, or a missing runner.

---

## Which `bynkc`?

`bynk` locates the compiler it needs — for `test`, and for the `check`/`fmt`
escape hatch below — in this order:

1. the **`BYNK_BYNKC`** environment variable, if set (an explicit pin);
2. **`bynkc` on `PATH`**;
3. a **`bynkc` sibling** of the running `bynk` binary (how a paired install ships).

`bynk check` and `bynk fmt` run in-process and need none of this — **unless**
`BYNK_BYNKC` is set, in which case they shell that pinned compiler so an
externally-managed `bynkc` still governs the result. `bynk doctor` reports this
resolution and any driver↔compiler version skew.

---

## `bynk doctor`

Survey the toolchain — grouped by capability — and print the exact remedy for
anything missing. Documented in full in the guide [Check your environment with
`bynk doctor`](/docs/editor-and-tooling/doctor/).

```text
bynk doctor [PATH] [--only CAPABILITY] [--strict] [--format human|short|json]
```

| Argument | Default | Meaning |
|---|---|---|
| `PATH` | `.` | Project directory, for project-local `node_modules/.bin` resolution. |
| `--only CAPABILITY` | — | Scope the check — and the exit code — to one of `compile`, `test`, `deploy`, `editor`, `build`. |
| `--strict` | — | Treat every warning (optional gaps, `npx` provisionability, minor skew) as a failure. For CI. |
| `--format` | `human` | `human` is a grouped table; `short` and `json` are the stable scriptable surface. |

**Exit code** — Bare `bynk doctor` is informational: it exits `0` unless `bynkc`
itself is unusable. `--only <capability>` gates on that capability; `--strict`
fails on any warning.
