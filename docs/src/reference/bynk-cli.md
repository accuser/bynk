# CLI (`bynk` driver)

The **`bynk`** driver is the developer front-end — it **links the compiler
pipeline in-process** and orchestrates the Node toolchain (`bynk` is to `bynkc`
as `cargo` is to `rustc`). A fresh `cargo install bynk` is self-contained: it
compiles without a separately-installed `bynkc`. This page is the argument and
exit-code reference for its subcommands. The pure-pipeline binary
[`bynkc`](cli.md) (`compile`, `check`, `fmt`, `test`) survives for CI and direct
use.

```text
bynk <command> [options]
```

| Command | What it does |
|---|---|
| [`bynk doctor`](#bynk-doctor) | Check whether your machine is ready to compile, test, and deploy. |
| [`bynk new`](#bynk-new) | Scaffold a new, runnable project. |
| [`bynk dev`](#bynk-dev) | Build the project and serve it locally with `wrangler dev`. |

---

## `bynk new`

Scaffold a new project: a complete, runnable single-context HTTP service you can
serve immediately with [`bynk dev`](#bynk-dev). See the guide [Start a new
project](../guides/projects-build-and-deployment/start-a-project.md) for a worked
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
├── bynk.toml            # [project] name/version + [paths] src/tests
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
locally](../guides/projects-build-and-deployment/run-locally.md) for a worked
walkthrough.

```text
bynk dev [PATH] [--context NAME] [-- <wrangler args>]
```

| Argument | Default | Meaning |
|---|---|---|
| `PATH` | `.` | A directory inside the project. The root is found by walking up for `bynk.toml`. |
| `--context NAME` | — | Which context's worker to serve, for multi-context projects. Accepts the dotted name (`a.b`) or its worker-directory form (`a-b`). |
| `-- <wrangler args>` | — | Everything after `--` is forwarded to `wrangler dev` verbatim (e.g. `-- --port 8788`). |

**Behaviour**

1. Locate the project root and read `[paths] src`.
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

- `bynk dev` provisions nothing and never edits `wrangler.toml`. Real namespaces
  and deployment are a separate, manual step — see [Target Cloudflare
  Workers](../guides/projects-build-and-deployment/cloudflare-workers.md). There
  is no `--remote` flag; reach remote dev, if you must, via `bynk dev --
  --remote`.
- `wrangler` is resolved with the same provenance ordering as `doctor`
  (project-local `node_modules/.bin` → `PATH` → `npx`). An `npx` resolution is
  surfaced as a notice — it downloads on first use.

---

## `bynk doctor`

Survey the toolchain — grouped by capability — and print the exact remedy for
anything missing. Documented in full in the guide [Check your environment with
`bynk doctor`](../guides/editor-and-tooling/doctor.md).

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
