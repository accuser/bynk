# CLI (`karnc`)

<!-- GENERATED FILE — do not edit by hand.
     Source: karnc/src/cli.rs (`render_markdown`).
     Regenerate with: KARN_BLESS=1 cargo test -p karnc --test cli_reference -->

The Karn compiler

Run `karnc <command> --help` for the authoritative help text.

## `karnc check`

Type-check a `.karn` file or project without writing output

```text
karnc check <INPUT> [--format <FORMAT>]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `INPUT` | yes | — | Input `.karn` file or project root |
| `--format` | no | `rich` | Diagnostic output format. `rich` (default) is the ariadne source-context rendering; `short` emits one terse `path:line:col: severity[category]: message` line per diagnostic, for tooling (the VS Code problem-matcher, CI, scripts) (one of: rich, short) |

## `karnc compile`

Compile a `.karn` file (single-file commons) to a TypeScript file, or a directory project to a tree of TypeScript files mirroring the source layout

```text
karnc compile <INPUT> --output <OUTPUT> [--target <TARGET>] [--platform <PLATFORM>]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `INPUT` | yes | — | Input `.karn` file, or directory project root |
| `--output` (`-o`) | yes | — | Output `.ts` file (for single-file input) or output root directory (for project input) |
| `--target` | no | `bundle` | Build target. `bundle` (default) produces a single deployment unit; `workers` produces one Cloudflare Worker per context with Service Binding plumbing (v0.8) (one of: bundle, workers) |
| `--platform` | no | `cloudflare` | Deploy platform selecting the `karn` surface binding (v0.17). A new axis, distinct from `--target`. The MVP supports `cloudflare` only (one of: cloudflare, node) |

## `karnc fmt`

Format `.karn` source files in place. Passing `-` reads from stdin and writes to stdout

```text
karnc fmt [INPUTS] [--check]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `INPUTS` | no | — | Files to format. Use `-` for stdin → stdout |
| `--check` | no | — | Check formatting without writing changes. Exits non-zero if any file is not already canonical |

## `karnc test`

Discover and run test declarations in a project. Compiles the project (including all generated `tests/*.test.ts` modules), then invokes Node.js on the aggregated runner script. Requires `tsc` and `node` to be on PATH

```text
karnc test [INPUT] [--output <OUTPUT>] [--no-run]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `INPUT` | no | `.` | Input project root directory. Defaults to the current directory |
| `--output` (`-o`) | no | — | Where to write compiled TypeScript test runner modules. Defaults to `<input>/out` |
| `--no-run` | no | — | Skip the runner invocation; just emit the generated test files. Useful for CI flows that drive the runner separately |
