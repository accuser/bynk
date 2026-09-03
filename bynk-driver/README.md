# bynk-driver

[![crates.io](https://img.shields.io/crates/v/bynk-driver.svg)](https://crates.io/crates/bynk-driver)
[![docs.rs](https://img.shields.io/docsrs/bynk-driver)](https://docs.rs/bynk-driver)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **shared front-end of the `bynkc` and `bynk` CLIs**, for the
[Bynk](https://github.com/accuser/bynk) compiler.

Both binaries expose `fmt` and `check` with identical semantics; this crate
holds the one implementation of each command body — project rooting,
project-failure flattening, and output writing — parameterised by the
program name that prefixes messages, instead of each binary carrying its own
by-hand copy pinned together only by a parity test.

It holds:

- `discovery` / project rooting — picks project vs. single-tree mode the same
  way for every command that needs it (`check`, `compile`, `test`, `dev`).
- `output` — `write_output`/`write_document`, the real filesystem write
  boundary; the one place emitted `Document::Ts` artefacts are printed
  through `bynk-ts`'s printer (the only code that writes a character).
- `test_json` / `test_runner` / `coverage` — the shared `test` subcommand's
  JSON report shape, runner, and V8-coverage parsing.
- `probe` — toolchain detection (`tsc`/`node` on `PATH`).
- `schema_lock` — the shared `bynk.schema.lock` read/write path both CLIs use.

## Where it sits

This crate depends on [`bynk-emit`](https://crates.io/crates/bynk-emit),
[`bynk-fmt`](https://crates.io/crates/bynk-fmt),
[`bynk-render`](https://crates.io/crates/bynk-render), and
[`bynk-ts`](https://crates.io/crates/bynk-ts) — the `bynkc` and `bynk`
binaries are the two front-ends built on top of it.

## Use

`bynk-driver` is consumed by the `bynkc` and `bynk` binaries directly; it has
no standalone CLI of its own.

```toml
[dependencies]
bynk-driver = "0.289"
```

See the [API docs](https://docs.rs/bynk-driver) for the full surface.

## License

Licensed under either of [Apache-2.0](https://github.com/accuser/bynk/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/accuser/bynk/blob/main/LICENSE-MIT) at your option.
