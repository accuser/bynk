---
level: patch
changelog: **Breaking (Rust API, not language surface):** `bynkc` no longer re-exports `bynk-syntax`'s `ast`/`diagnostics`/`error`/`keywords`/`lexer`/`parser`/`span` modules, `bynk-driver`'s `coverage`/`test_json` modules, or the whole `bynk-fmt` crate as `bynkc::fmt` (only `bynkc`'s item re-exports, `CompileError`, `CompileOptions`, `compile_project`, and others, remain public)
closes_rule: R10.4
---

## ADR: narrow-bynkc-syntax-driver-fmt-api
title: bynkc's published Rust API drops its remaining whole-module/whole-crate re-exports
summary: Three re-exports T-D1 didn't scope (bynk-syntax's 7 modules, bynk-driver's coverage/test_json, bynk-fmt as fmt) are deleted; bynkc keeps its item re-exports

**Context.** [ADR 0312](../decisions/0312-narrow-bynkc-public-api.md) (T-D1)
deleted fourteen whole-module re-exports from `bynkc/src/lib.rs` — the ones
reaching into `bynk-check`/`bynk-emit` — but its scope named only those two
crates. Three more re-exports of the same shape were left standing:

- `pub use bynk_syntax::{ast, diagnostics, error, keywords, lexer, parser,
  span};` — the whole `bynk-syntax` module tree.
- `pub use bynk_driver::{coverage, test_json};` — two whole modules.
- `pub use bynk_fmt as fmt;` — the entire `bynk-fmt` crate.

R10.4 ("No crate exports a facade over another crate's internals. The
published surface of each crate is enumerated and reviewed.") applies to
these identically — `bynkc` is published to crates.io, so `bynkc::ast`,
`bynkc::lexer`, `bynkc::parser`, `bynkc::fmt`, and the rest were public API of
a released crate, undoing at the top the same leaf-crate decomposition ADR
0312 argued for.

Auditing every consumer of these seven module paths found none in `bynkc`'s
own `src/` (`main.rs`'s `run_fmt` already calls `bynk_driver::run_fmt`
directly, not `bynkc::fmt`) — the only in-repo readers were `bynkc`'s own
integration tests (`bynkc::keywords::KEYWORDS`, `bynkc::fmt::format_source`,
`bynkc::ast::BaseType`, `bynkc::test_json::{Case, ...}`, …, across eight
`bynkc/tests/*.rs` files). `bynkc::coverage` had zero readers, in-repo or
otherwise findable. `CompileError` is not part of this: it names the error
type of `compile`/`compile_with_warnings` (re-exported from `bynk-emit`
below), so it stays as the single item re-export it already effectively was.

**Decision.** Delete the three re-exports. `bynkc`'s eight integration test
files that reached through them move to importing `bynk_syntax`/
`bynk_driver`/`bynk_fmt` directly — the correct import in any case, exactly
T-D1's reasoning for its fourteen. `bynk-fmt`'s own doc comment and README,
and three site docs, described `bynkc` re-exporting `bynk-fmt` as `bynkc::fmt`
(one, backwards: `bynk-fmt` does not re-export from `bynkc`) — corrected to
describe the `bynk-driver`-mediated path `bynkc fmt` actually takes.

No reverse dependent on crates.io was found depending on `bynkc::ast`/
`bynkc::lexer`/`bynkc::fmt`/etc. — the module paths were themselves purely
re-exported plumbing with no independent behaviour, so a consumer that did
depend on them has an unchanged, one-hop fix (import the leaf crate). Treated
as behaviour-preserving in the same sense T-D1 was, with its own changelog row
per R10.4's own instruction to record such a change, not because a break was
found.

**Consequences.** `bynkc::ast`, `bynkc::diagnostics`, `bynkc::error`,
`bynkc::keywords`, `bynkc::lexer`, `bynkc::parser`, `bynkc::span`,
`bynkc::coverage`, `bynkc::test_json`, and `bynkc::fmt` stop resolving for any
external consumer; `bynkc::CompileError` and the rest of `bynkc`'s ~30 item
re-exports are unaffected. `cargo install bynkc` is unaffected — no item
re-export moves and the binary's own behaviour is unchanged.
