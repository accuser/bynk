# CLI (`bynkc`)

<!-- GENERATED FILE — do not edit by hand.
     Source: bynkc/src/cli.rs (`render_markdown`).
     Regenerate with: BYNK_BLESS=1 cargo test -p bynkc --test cli_reference -->

The Bynk compiler

Run `bynkc <command> --help` for the authoritative help text.

## Exit codes and diagnostics

A diagnostic's **severity** decides whether it fails a build (v0.89). An **`Error`** rejects the program: `bynkc compile`/`check` exit non-zero and produce no output. A **`Warning`** is surfaced but does **not** fail the build: these commands still **succeed (exit 0)** and emit their output, with warnings reported alongside. The build-failure gate counts error-severity diagnostics only. See the normative rule in the [specification](../spec/diagnostics.md) and the [diagnostic index](diagnostics.md) (warning-severity codes are marked *(warning)*).

## `bynkc check`

Type-check a `.bynk` file or project without writing output

```text
bynkc check <INPUT> [--format <FORMAT>]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `INPUT` | yes | — | Input `.bynk` file or project root |
| `--format` | no | `rich` | Diagnostic output format. `rich` (default) is the ariadne source-context rendering; `short` emits one terse `path:line:col: severity[category]: message` line per diagnostic, for tooling (the VS Code problem-matcher, CI, scripts) (one of: rich, short) |

## `bynkc compile`

Compile a `.bynk` file (single-file commons) to a TypeScript file, or a directory project to a tree of TypeScript files mirroring the source layout

```text
bynkc compile <INPUT> --output <OUTPUT> [--target <TARGET>] [--platform <PLATFORM>]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `INPUT` | yes | — | Input `.bynk` file, or directory project root |
| `--output` (`-o`) | yes | — | Output `.ts` file (for single-file input) or output root directory (for project input) |
| `--target` | no | `bundle` | Build target. `bundle` (default) produces a single deployment unit; `workers` produces one Cloudflare Worker per context with Service Binding plumbing (v0.8) (one of: bundle, workers) |
| `--platform` | no | `cloudflare` | Deploy platform selecting the `bynk` surface binding (v0.17). A new axis, distinct from `--target`. The MVP supports `cloudflare` only (one of: cloudflare, node) |

## `bynkc fmt`

Format `.bynk` source files in place. Passing `-` reads from stdin and writes to stdout

```text
bynkc fmt [INPUTS] [--check]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `INPUTS` | no | — | Files to format. Use `-` for stdin → stdout |
| `--check` | no | — | Check formatting without writing changes. Exits non-zero if any file is not already canonical |

## `bynkc test`

Discover and run test declarations in a project. Compiles the project (including all generated `tests/*.test.ts` modules), then invokes Node.js on the aggregated runner script. Requires `tsc` and `node` to be on PATH

```text
bynkc test [INPUT] [--output <OUTPUT>] [--no-run] [--format <FORMAT>] [--inspect]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `INPUT` | no | `.` | Input project root directory. Defaults to the current directory |
| `--output` (`-o`) | no | — | Where to write compiled TypeScript test runner modules. Defaults to `<input>/out` |
| `--no-run` | no | — | Skip the runner invocation. With `--format rich` this emits the generated test files (for CI flows that drive the runner separately); with `--format json` it emits a discovery document listing every suite and case (each `outcome: "discovered"`) without running them — a pure compile, no `tsc`/Node |
| `--format` | no | `rich` | Output format. `rich` (default) is the grouped ✓ / ✗ human output; `json` is a single pinned JSON document of results, for tooling (one of: rich, json) |
| `--inspect` | no | — | Compile a debug build and launch the test runner under Node's inspector (`node --inspect-brk`), printing the inspector URL for a JavaScript debugger to attach (slice 2, ADR 0104). The emitted `.ts` runs directly under Node's line-preserving type-stripping, so source maps resolve breakpoints back to `.bynk`. Requires Node ≥ 22.18 (or ≥ 23.6 unflagged). Does not run `tsc` |
