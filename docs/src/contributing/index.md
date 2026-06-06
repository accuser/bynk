# Contributing to the compiler

This section is for people working on Karn itself — the `karnc` compiler and its
sibling tools — rather than people writing Karn programs. If you are here to
*use* Karn, start with the [tutorials](../tutorials/01-first-program.md).

## The repository

Karn is a Cargo workspace plus two JavaScript/TypeScript projects:

| Crate / project | What it is |
|---|---|
| `karnc` | The compiler and CLI (`karnc`): lex → parse → resolve → check → emit. |
| `karn-fmt` | A thin crate re-exporting the formatter from `karnc::fmt`. |
| `karn-lsp` | The language server (`karnc-lsp`), built on `tower-lsp`. |
| `mdbook-karn-highlight` | The docs' syntax-highlighting preprocessor (compiles the tree-sitter grammar). |
| `tree-sitter-karn` | The grammar (`grammar.js` → generated parser) and highlight queries. |
| `vscode-karn` | The VS Code extension. |

The workspace targets the **Rust 2024 edition**.

## Build and test

```sh
cargo build                 # build all workspace crates
cargo test                  # run the whole Rust test suite
cargo test -p karnc         # just the compiler's tests
```

`cargo test -p karnc` runs, among others:

- the **fixture suite** (`tests/e2e.rs`) — the heart of the compiler's tests;
- the **`tsc` verification gate** (`tests/tsc_verify.rs`);
- the **generated-reference** checks and the **doc-example gate** for the book.

See [Testing & fixtures](testing.md) for how these work and how to update them.

## A few conventions

- **`KARN_BLESS=1`** is the project-wide "regenerate expected output" switch. It
  re-blesses fixture expectations *and* the generated reference pages. Run it
  deliberately and review the diff.
- **Specs are the design record, not always current.** The root `*.md` design
  notes and `karn-mvp-grammar-*.md` files capture the spec-first history; some of
  the older ones are historical or aspirational (a few even predate the Rust
  rewrite). Trust the fixtures and the code for current behaviour.
- **Docs ship with the feature.** Each increment updates the book in the same
  change — see [Working on the docs](documentation.md).

## Where to look

- [Compiler architecture](architecture.md) — the pipeline and the crates.
- [Testing & fixtures](testing.md) — the fixture formats, the bless workflow, the
  `tsc` gate.
- [Working on the docs](documentation.md) — the book, its generators, and its
  guardrails.
