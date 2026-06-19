# bynkc

[![crates.io](https://img.shields.io/crates/v/bynkc.svg)](https://crates.io/crates/bynkc)
[![docs.rs](https://img.shields.io/docsrs/bynkc)](https://docs.rs/bynkc)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **Bynk compiler** — both a library and the `bynkc` command-line tool. It
takes [Bynk](https://github.com/accuser/bynk) source, type-checks it, and emits
typed TypeScript targeting Cloudflare Workers.

Bynk is a statically typed, architecture-first language: contexts, services,
agents, refined types, and capabilities are part of the language. See the
[Bynk Book](https://github.com/accuser/bynk/tree/main/docs) for the full guide
and reference.

## Pipeline

```text
lex  →  parse  →  resolve  →  check  →  emit
```

Each phase lives in its own module under `src/`:

- `lexer.rs` — `logos`-driven token stream.
- `parser.rs` — hand-written recursive descent, one function per precedence level.
- `resolver.rs` — builds the symbol table; flags duplicates, name overlap,
  unresolved references, and arity mismatches.
- `checker.rs` — type-checks every declaration and expression; validates
  refinement predicates and detects contradictory combinations.
- `emitter.rs` / `emitter/` — walks the typed AST and writes TypeScript.
- `project.rs` — multi-file projects: a directory of `.bynk` units compiled into
  a tree of TypeScript mirroring the source layout.

Diagnostics flow through `error.rs` and [`ariadne`](https://crates.io/crates/ariadne).
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

Requires a stable Rust toolchain, 2024 edition (MSRV 1.85).

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
[CLI reference](https://github.com/accuser/bynk/blob/main/docs/src/reference/cli.md).

## Library

```rust
use bynkc::compile_project;

// Compile a project root (a directory containing `bynk.toml`) into an
// in-memory tree of TypeScript files.
let output = compile_project(std::path::Path::new("path/to/project"))?;
```

The crate exposes the full compiler surface (`ast`, `lexer`, `parser`,
`resolver`, `checker`, `emitter`, `project`, `diagnostics`, …). The
single-string [`compile`] entrypoint handles a self-contained commons; the
[`compile_project`] family handles multi-file projects, build targets, and
platforms. See the [API docs](https://docs.rs/bynkc).

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
[`docs/src/spec/`](https://github.com/accuser/bynk/tree/main/docs/src/spec)
(rendered in the Bynk Book), kept current per increment. The decisions behind
the increments are recorded in
[`design/decisions/`](https://github.com/accuser/bynk/tree/main/design/decisions).

## License

Licensed under either of [Apache-2.0](https://github.com/accuser/bynk/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/accuser/bynk/blob/main/LICENSE-MIT) at your option.
