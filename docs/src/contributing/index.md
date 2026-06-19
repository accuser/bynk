# Contributing to the compiler

This section is for people working on Bynk itself — the `bynkc` compiler and its
sibling tools — rather than people writing Bynk programs. If you are here to
*use* Bynk, start with the [tutorials](../tutorials/01-first-program.md).

## The repository

Bynk is a Cargo workspace plus two JavaScript/TypeScript projects:

| Crate / project | What it is |
|---|---|
| `bynkc` | The compiler and CLI (`bynkc`): lex → parse → resolve → check → emit. |
| `bynk-fmt` | A thin crate re-exporting the formatter from `bynkc::fmt`. |
| `bynk-lsp` | The language server (`bynkc-lsp`), built on `tower-lsp`. |
| `mdbook-bynk-highlight` | The docs' syntax-highlighting preprocessor (compiles the tree-sitter grammar). |
| `tree-sitter-bynk` | The grammar (`grammar.js` → generated parser) and highlight queries. |
| `vscode-bynk` | The VS Code extension. |

The workspace targets the **Rust 2024 edition**.

## Build and test

```sh
cargo build                 # build all workspace crates
cargo test                  # run the whole Rust test suite
cargo test -p bynkc         # just the compiler's tests
```

`cargo test -p bynkc` runs, among others:

- the **fixture suite** (`tests/e2e.rs`) — the heart of the compiler's tests;
- the **`tsc` verification gate** (`tests/tsc_verify.rs`);
- the **generated-reference** checks and the **doc-example gate** for the book.

See [Testing & fixtures](testing.md) for how these work and how to update them.

## A few conventions

- **`BYNK_BLESS=1`** is the project-wide "regenerate expected output" switch. It
  re-blesses fixture expectations *and* the generated reference pages. Run it
  deliberately and review the diff.
- **The spec is the source of truth; the design notes are rationale.** The
  normative spec under `docs/src/spec/` defines the current language and is
  updated per increment; the decisions behind increments live in
  `design/decisions/`. The remaining `design/*.md` notes are rationale and
  history — some are aspirational or predate the Rust rewrite. Trust the
  fixtures and the code for current behaviour.
- **Docs ship with the feature.** Each increment updates the book in the same
  change — see [Working on the docs](documentation.md).

## Where to look

- [Compiler architecture](architecture.md) — the pipeline and the crates.
- [Testing & fixtures](testing.md) — the fixture formats, the bless workflow, the
  `tsc` gate.
- [Working on the docs](documentation.md) — the book, its generators, and its
  guardrails.
