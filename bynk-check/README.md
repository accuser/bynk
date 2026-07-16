# bynk-check

[![crates.io](https://img.shields.io/crates/v/bynk-check.svg)](https://crates.io/crates/bynk-check)
[![docs.rs](https://img.shields.io/docsrs/bynk-check)](https://docs.rs/bynk-check)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **semantic-analysis layer of the [Bynk](https://github.com/accuser/bynk)
compiler** — name resolution and type checking over the
[`bynk-syntax`](https://crates.io/crates/bynk-syntax) AST.

It holds:

- `resolver` — builds the symbol table; flags duplicates, name overlap,
  unresolved references, and arity mismatches.
- `checker` — type-checks every declaration and expression, validates refinement
  predicates, and resolves capabilities, services, agents, and actors.
- `kernel_methods` / `builtin_names` — the registries the checker dispatches and
  the editor reads for `.`-member completion.
- `firstparty` — the embedded first-party `bynk` surface, stdlib, and adapters
  (re-exporting `Platform`).
- `actors` — actor-contract analysis (auth schemes, identities).
- `requirements` — the capability/requirement analysis the checker draws on.
- `index` / `hints` / `expr_types` / `locals` — the **captured analysis tables**
  written during checking (the binding index, inlay hints, expression types,
  scoped locals) that the IDE layer queries.

## Where it sits

```text
bynk-syntax ◀── bynk-check ◀── bynk-emit ◀── bynk-ide
```

The captured tables live here, with their producers; the IDE *queries* over them
live up in [`bynk-ide`](https://crates.io/crates/bynk-ide). Most users compile
Bynk through the [`bynkc`](https://crates.io/crates/bynkc) /
[`bynk`](https://crates.io/crates/bynk) CLIs rather than depending on this crate
directly.

## Use

```toml
[dependencies]
bynk-check = "0.183"
```

```rust
use bynk_check::{checker, resolver};

let resolved = resolver::resolve(commons)?;
let typed = checker::check(resolved)?;
```

See the [API docs](https://docs.rs/bynk-check) for the full surface.

## License

Licensed under either of [Apache-2.0](https://github.com/accuser/bynk/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/accuser/bynk/blob/main/LICENSE-MIT) at your option.
