# Bynk

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust 2024 (MSRV 1.95)](https://img.shields.io/badge/rust-2024%20edition%20%C2%B7%20MSRV%201.95-orange.svg)](rust-toolchain.toml)

**Bynk** is a statically typed, *architecture-first* programming language for
building services. The shape of a program — its contexts, services, agents, and
the types that flow between them — is part of the language, not a convention
layered on top. Bynk compiles to **typed TypeScript** and targets **Cloudflare
Workers**.

> ⚠️ Bynk is **pre-1.0 and under active development.** The language evolves in
> small, spec-first increments. The [Bynk Book](docs/) documents *what compiles
> today*; planned features are marked as planned.

## The idea in one example

```bynk
context greet

service api from http {
  on GET("/ping") () -> Effect[HttpResult[String]] by v: Visitor {
    Ok("pong")
  }
}
```

Compiling this with `bynkc` produces TypeScript you can read, run, and deploy —
the router, boundary validation, and the Worker entry point are generated for
you.

## What makes Bynk distinct

- **Make illegal states unrepresentable.** *Refined types* (types carrying a
  predicate), *opaque types*, and errors-as-values (`Result`, `Ok`/`Some`/
  `None`) let whole classes of bug go unexpressed.
- **Architecture in the language.** Contexts, services, and stateful *agents*
  are first-class, and a `context` is the unit of deployment — it becomes one
  Cloudflare Worker.
- **Honest effects and capabilities.** Dependencies are declared in signatures
  (`given Logger`), supplied by the platform, and mockable in tests.
- **Compiles to TypeScript.** You get JavaScript-ecosystem interop and a natural
  fit for Cloudflare Workers, with a static type system in front of it.
- **Testing is built in.** `suite`/`case` blocks, `expect`, and `Val[T]` value
  fabrication ship with the language.

## Install

Bynk is not yet on a package registry for end users; install it by building from
source with a recent Rust toolchain (stable, 2024 edition — see
[rustup](https://rustup.rs/)).

```sh
git clone https://github.com/accuser/bynk.git
cd bynk
cargo install --path bynkc      # the `bynkc` compiler
cargo install --path bynk       # the `bynk` driver (doctor / dev / deploy / new / check / fmt / test)
cargo install --path bynk-lsp   # optional: the `bynkc-lsp` language server
```

`bynkc --help` lists the four compiler commands (`compile`, `check`, `fmt`,
`test`); `bynk --help` lists the driver's (`doctor`, `dev`, `deploy`, `new`,
`check`, `fmt`, `test`).

## Quick start

Scaffold a complete, runnable project and serve it — three commands from nothing
to a running service on `http://localhost:8787`:

```sh
bynk new hello       # scaffold bynk.toml + src/hello.bynk
cd hello
bynk dev             # compile and serve it locally
```

`bynk new` only writes files (no toolchain needed), and `bynk dev` serves what it
wrote unmodified. Prefer a worked example? The bundled
[`examples/hello-world`](examples/hello-world/) is a complete project you can also
`bynkc check src`, `bynkc test .`, and deploy.

See [Start a new project](https://bynk-lang.org/book/guides/projects-build-and-deployment/start-a-project/)
or [Compile your first program](https://bynk-lang.org/book/tutorials/01-first-program/).

## Repository layout

This is a Cargo workspace. The published crates are `bynkc`, `bynk`, `bynk-fmt`,
`bynk-grammar`, and `bynk-lsp`.

| Path | What it is | Published as |
| ---- | ---------- | ------------ |
| [`bynkc/`](bynkc/) | The compiler library and `bynkc` CLI (lex → parse → resolve → check → emit). | [crates.io](https://crates.io/crates/bynkc) |
| [`bynk/`](bynk/) | The `bynk` driver — a thin orchestrator over `bynkc` and the Node toolchain (`doctor` / `new` / `dev`). | [crates.io](https://crates.io/crates/bynk) |
| [`bynk-fmt/`](bynk-fmt/) | The Bynk formatter, behind a small public surface. | [crates.io](https://crates.io/crates/bynk-fmt) |
| [`bynk-grammar/`](bynk-grammar/) | Renders the tree-sitter grammar to EBNF for the book's grammar reference. | [crates.io](https://crates.io/crates/bynk-grammar) |
| [`bynk-lsp/`](bynk-lsp/) | The `bynkc-lsp` Language Server (diagnostics, hover, go-to-definition, …). | [crates.io](https://crates.io/crates/bynk-lsp) |
| [`tree-sitter-bynk/`](tree-sitter-bynk/) | The tree-sitter grammar — the source of truth for syntax highlighting. | npm |
| [`vscode-bynk/`](vscode-bynk/) | The VS Code extension (bundles the language server). | — |
| [`site/`](site/) | The Bynk Book (Astro + Starlight): tutorials, how-to guides, reference, and the normative spec. Published at [bynk-lang.org](https://bynk-lang.org/book/). | — |
| [`design/`](design/) | Internal design notes and decision records (ADRs). | — |
| [`examples/`](examples/) | Example projects. | — |

## Documentation

The **[Bynk Book](https://bynk-lang.org/book/)** is the canonical guide and
reference. It follows [Diátaxis](https://diataxis.fr/), grouped concern-first so
each topic keeps its explanation, recipes, and reference together:

- **[Tutorials](https://bynk-lang.org/book/tutorials/01-first-program/)** — learn Bynk by building.
- **[Guides](https://bynk-lang.org/book/guides/)** — task-focused recipes, each section
  opening with the *why* before the *how*.
- **[Reference](https://bynk-lang.org/book/reference/)** — exact behaviour, including the
  [normative spec](https://bynk-lang.org/book/spec/) and [CLI reference](https://bynk-lang.org/docs/cli/).

The Book's source lives in [`site/`](site/). Build it locally with
[Astro](https://astro.build/): `cd site && npm install && npm run dev`.

## Status

Bynk is pre-1.0. Some designed features (events, sagas, storage kinds) are
**deferred, not missing**, and land in later increments. See the
[status and roadmap](design/bynk-status-and-roadmap.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any additional
terms or conditions.
