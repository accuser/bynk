# bynk

[![crates.io](https://img.shields.io/crates/v/bynk.svg)](https://crates.io/crates/bynk)
[![docs.rs](https://img.shields.io/docsrs/bynk)](https://docs.rs/bynk)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **Bynk developer front-end** — the `bynk` driver. It **links the compiler
pipeline in-process** and orchestrates the Node toolchain; `bynk` is to
[`bynkc`](https://crates.io/crates/bynkc) what `cargo` is to `rustc`. A fresh
`cargo install bynk` is self-contained: it compiles, scaffolds, and serves
projects with no separately-installed `bynkc`. Environment orchestration — *is
my machine ready?*, *scaffold me a project*, *build and serve it locally* — is
the driver's job.

See the [Bynk Book](https://github.com/accuser/bynk/tree/main/docs) for the full
guide and reference, and the
[`bynk` CLI reference](https://github.com/accuser/bynk/blob/main/docs/src/reference/bynk-cli.md)
for every argument and exit code.

## The project-lifecycle arc

The driver's commands form a three-step on-ramp, **`doctor → new → dev`**:

```sh
bynk doctor       # is my machine ready to compile, test, and deploy?
bynk new hello    # scaffold a complete, runnable project
cd hello
bynk dev          # build it and serve it locally on http://localhost:8787
```

| Command | What it does |
|---|---|
| `bynk doctor` | Survey the toolchain grouped by capability (compile · test · dev/deploy · editor · build-from-source), reporting presence + version + provenance, and print the exact remedy for anything missing. |
| `bynk new <path>` | Scaffold a complete, runnable single-context HTTP service (`bynk.toml`, `.gitignore`, `src/<name>.bynk`) that `bynk dev` serves unmodified. Pure offline file-writing — no toolchain required. |
| `bynk dev` | Build the project and serve it locally with `wrangler dev` in local mode (Miniflare) — one step in place of the manual `bynkc compile` + `cd` + `wrangler dev` recipe. No provisioning needed. |

Each command has a pinned argument/exit contract; run `bynk <command> --help`
for the flags.

## Install

```sh
cargo install bynk
```

Or build from the workspace:

```sh
cargo build --release -p bynk   # → target/release/bynk
```

Requires a stable Rust toolchain, 2024 edition (MSRV 1.85). The compiler is
linked in — `bynk dev` compiles a project in-process — so no separate `bynkc` is
needed; `dev` additionally shells Node + `wrangler` to serve, and `bynk new`
needs neither (it only writes files). Power users can point `bynk` at an external
compiler with `$BYNK_BYNKC` (e.g. to pin a version); `bynk doctor` reports that
override and any driver↔compiler skew.

## Design

- **Links the pipeline** — the driver links the compiler library crates
  (`bynk-emit` / `bynk-syntax` / `bynk-render`) and compiles in-process, rather
  than shelling the `bynkc` binary, so it is self-contained
  ([ADR 0101](https://github.com/accuser/bynk/blob/main/design/decisions/0101-front-end-links-pipeline-binary-topology.md),
  building on [ADR 0083](https://github.com/accuser/bynk/blob/main/design/decisions/0083-bynk-driver-thin-orchestrator.md)).
  A `$BYNK_BYNKC` override is kept as a power-user escape hatch.
- **Single-concern modules** — `probe` (portable tool detection: presence +
  version + provenance, via the `which` crate), `compiler` (locate `bynkc` and
  report driver↔compiler version skew), `doctor`, `new`, `dev`, and `report`.
- **Deterministic surface** — the human-facing output of each command is pinned
  by goldens in `tests/` (blessed with `BYNK_BLESS=1 cargo test -p bynk`), and
  `new`'s embedded starter template is compile-tested so the scaffold can't rot.

The decisions behind each command are recorded in
[`design/decisions/`](https://github.com/accuser/bynk/tree/main/design/decisions)
(ADRs 0083–0084 for `doctor`, 0096 for `dev`, 0097 for `new`).

## Tests

```sh
cargo test -p bynk
```

## License

Licensed under either of [Apache-2.0](https://github.com/accuser/bynk/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/accuser/bynk/blob/main/LICENSE-MIT) at your option.
