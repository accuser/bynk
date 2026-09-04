# bynk-lower

[![crates.io](https://img.shields.io/crates/v/bynk-lower.svg)](https://crates.io/crates/bynk-lower)
[![docs.rs](https://img.shields.io/docsrs/bynk-lower)](https://docs.rs/bynk-lower)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **AST-analysis helpers the [Bynk](https://github.com/accuser/bynk) emitter
reads resolved declaration-level facts through** — handler kinds and `given`
clauses, service protocols, store-field shapes, capability op and
attached-method signatures, route cache/limit annotations, event-subscriber
shapes, and the store-write walk behind an agent's implicit-commit decision.

Each helper takes a checked program (or its `TypedCommons`) and a
syntax-tree node, and returns a [`bynk-ir`](https://crates.io/crates/bynk-ir)
value — none of them lowers a whole expression body; that machinery existed
here once and was deleted once it proved unreachable from production (see
`bynk-ir`'s own README for the full story).

## Where it sits

This crate depends on [`bynk-syntax`](https://crates.io/crates/bynk-syntax),
[`bynk-check`](https://crates.io/crates/bynk-check), and `bynk-ir`.
[`bynk-emit`](https://crates.io/crates/bynk-emit) is the one production
consumer of every helper here — each has a real call site in the emitter, not
just a test. The `bynk-project` dev-dependency is test-fixture plumbing only
(building checked programs to lower in this crate's own tests), not a
production edge.

## Use

```toml
[dependencies]
bynk-lower = "0.290"
```

```rust
// `kind: &bynk_syntax::ast::HandlerKind`
let ir_kind = bynk_lower::lower_handler_kind_ir(kind);
```

See the [API docs](https://docs.rs/bynk-lower) for the full surface.

## License

Licensed under either of [Apache-2.0](https://github.com/accuser/bynk/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/accuser/bynk/blob/main/LICENSE-MIT) at your option.
