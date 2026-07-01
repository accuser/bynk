# bynkc

[![crates.io](https://img.shields.io/crates/v/bynkc.svg)](https://crates.io/crates/bynkc)
[![docs.rs](https://img.shields.io/docsrs/bynkc)](https://docs.rs/bynkc)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **Bynk compiler CLI** — the `bynkc` command-line tool (and the library
behind it). It takes [Bynk](https://github.com/accuser/bynk) source,
type-checks it, and emits typed TypeScript targeting Cloudflare Workers.

Bynk is a statically typed, architecture-first language: contexts, services,
agents, refined types, and capabilities are part of the language. See the
[Bynk Book](https://github.com/accuser/bynk/tree/main/docs) for the full guide
and reference.

## Pipeline

```text
lex  →  parse  →  resolve  →  check  →  emit
```

`bynkc` is a thin front-end: it owns the CLI and the compile/diagnose glue, and
the pipeline itself lives in a layered set of library crates it depends on and
re-exports (so `bynkc::ast`, `bynkc::checker`, `bynkc::compile_project`, … resolve
unchanged):

- [`bynk-syntax`](https://crates.io/crates/bynk-syntax) — lexer, parser, AST,
  spans, the `CompileError` type, and the `bynk.*` diagnostic-code registry.
- [`bynk-check`](https://crates.io/crates/bynk-check) — name resolution, type
  checking, the kernel/builtin registries, first-party sources, and actors.
- [`bynk-emit`](https://crates.io/crates/bynk-emit) — build orchestration
  (`project`) and the TypeScript emitter.
- [`bynk-render`](https://crates.io/crates/bynk-render) — the shared diagnostic
  renderer ([`ariadne`](https://crates.io/crates/ariadne) human + `short`/`json`).
- [`bynk-fmt`](https://crates.io/crates/bynk-fmt) — the formatter, behind
  `bynkc fmt`.

Every error carries a dotted category (`bynk.parse.expected_token`,
`bynk.types.invalid_regex`, …), a source span, and a primary message; many carry
secondary labels and notes.

## Install

```sh
cargo install bynkc
```

Or build from the workspace:

```sh
cargo build --release -p bynkc   # → target/release/bynkc
```

Requires a stable Rust toolchain, 2024 edition (MSRV 1.95).

## CLI

```sh
bynkc check   <input>                         # type-check without emitting
bynkc compile <input> -o <output> [--target bundle|workers]
bynkc fmt     [inputs...] [--check]           # format in place (or `-` for stdin)
bynkc test    [project] [--no-run]            # compile and run `test` blocks
```

`<input>` is either a single-file commons (`foo.bynk`) or a project directory
containing a `bynk.toml`. The `workers` target emits one Cloudflare Worker per
context, complete with router, dependency wiring, the shared runtime, and a
`wrangler.toml`. `bynkc test` needs `node` and `tsc` on `PATH`.

Run `bynkc <command> --help` for every flag, and see the
[CLI reference](https://bynk-lang.org/docs/cli/).

## Library

```rust
use bynkc::compile_project;

// Compile a project root (a directory containing `bynk.toml`) into an
// in-memory tree of TypeScript files.
let output = compile_project(std::path::Path::new("path/to/project"))?;
```

The crate re-exports the full compiler surface from the layered library crates
(`ast`, `lexer`, `parser`, `resolver`, `checker`, `emitter`, `project`,
`diagnostics`, …), so existing `bynkc::…` paths keep working. The single-string
`compile` entrypoint handles a self-contained commons; the `compile_project`
family handles multi-file projects, build targets, and platforms. To depend on
just one layer, use the individual crate (e.g.
[`bynk-syntax`](https://crates.io/crates/bynk-syntax) to lex/parse without the
checker). See the [API docs](https://docs.rs/bynkc).

## Tests

```sh
cargo test -p bynkc
```

The end-to-end harness in `tests/` runs fixture-driven positive and negative
cases. Set `BYNK_REQUIRE_TSC=1` to additionally type-check the emitted
TypeScript with `tsc` (CI does this).

## The language

The normative definition of the language this compiler accepts is the
specification in
[the normative spec](https://bynk-lang.org/book/spec/)
(rendered in the Bynk Book), kept current per increment. The decisions behind
the increments are recorded in
[`design/decisions/`](https://github.com/accuser/bynk/tree/main/design/decisions).

## License

Licensed under either of [Apache-2.0](https://github.com/accuser/bynk/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/accuser/bynk/blob/main/LICENSE-MIT) at your option.
