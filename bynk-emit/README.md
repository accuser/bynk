# bynk-emit

[![crates.io](https://img.shields.io/crates/v/bynk-emit.svg)](https://crates.io/crates/bynk-emit)
[![docs.rs](https://img.shields.io/docsrs/bynk-emit)](https://docs.rs/bynk-emit)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**Build orchestration and TypeScript emission for the
[Bynk](https://github.com/accuser/bynk) compiler.**

It is the layer above [`bynk-check`](https://crates.io/crates/bynk-check) that
turns a checked program into output:

- `project` — the build driver: project discovery, the dependency graph,
  consistency and validation, symbols, paths, and the `compile_project` /
  `analyse_project` entry points. (Read it as "build orchestration" — it conducts
  the whole build.)
- `emitter` — lowers a type-checked program to TypeScript targeting Cloudflare
  Workers (or a single bundle), complete with the router, dependency wiring, the
  shared runtime, and a `wrangler.toml`.

The `workers` target emits one Worker per context; the `compile_project` result
is an in-memory tree of TypeScript files — writing it to disk is the driver's
job (`bynk-driver::write_output`), not this crate's.

## Where it sits

```text
bynk-syntax  ◀── bynk-render · bynk-fmt · bynk-check ◀── bynk-emit ◀── bynk-ide
```

The `bynkc`, `bynk`, and `bynk-lsp` binaries are front-ends over this set. Most
users compile Bynk through the [`bynkc`](https://crates.io/crates/bynkc) /
[`bynk`](https://crates.io/crates/bynk) CLIs rather than depending on this crate
directly.

## Use

```toml
[dependencies]
bynk-emit = "0.263"
```

```rust
use bynk_emit::project::{compile_project, CompileOptions, BuildTarget};

let options = CompileOptions::single(root).target(BuildTarget::Workers);
let output = compile_project(&options)?; // in-memory TypeScript tree — bynk-emit never touches disk
```

See the [API docs](https://docs.rs/bynk-emit) for the full surface.

## License

Licensed under either of [Apache-2.0](https://github.com/accuser/bynk/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/accuser/bynk/blob/main/LICENSE-MIT) at your option.
