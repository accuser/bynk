# 0246 — Completion/symbols/locals-nav/signature-help logic moves from bynk-lsp to bynk-ide

- **Status:** Accepted (v0.216.1)

**Context.** ADR 0242 shipped the playground's hover via a `bynk_hover` wasm
entry, and split completion (the other half of #397) into #808: hover only
needed the checker's `expr_types` sink exposed through the wasm boundary, a
small change, while completion's pure logic — `bynk-lsp/src/completion.rs`
(~2500 lines), plus `symbols.rs`, `locals_nav.rs`, `signature_help.rs` — has
zero `tower-lsp`/`tokio` dependency but is trapped inside `bynk-lsp`, which
depends on both. Neither is viable on `wasm32-unknown-unknown`, so
`bynk-wasm` cannot add `bynk-lsp` as a dependency to reach this logic; the
crate boundary is the blocker, not the algorithm.

`bynk-ide` already sits between `bynk-check`/`bynk-emit` and `bynk-lsp`, has
no `tower-lsp`/`tokio` dependency, and is already proven to build for
`wasm32-unknown-unknown` (transitively, via `bynk-emit`).

**Decision.** Move `completion.rs`, `symbols.rs`, `locals_nav.rs`, and
`signature_help.rs` from `bynk-lsp/src/` into `bynk-ide/src/`, unchanged in
behaviour. All four move together (not just the two files with the least
`bynk-lsp`-specific coupling) because `signature_help.rs` imports
`crate::completion::{BUILTIN_STATICS, for_each_unit}` and
`crate::symbols::type_ref_str` — keeping all four in the same crate means
those imports need no path changes.

`bynk-lsp` keeps a same-named module at each old path
(`bynk-lsp/src/completion.rs` etc.) that is now just `pub use
bynk_ide::completion::*;` (and the equivalent for the other three), so the
~55 call sites across `bynk-lsp/src/lib.rs` and `hover.rs` need no edits —
`crate::completion::complete(...)` keeps resolving exactly as before, now via
the re-export. The ~20 `pub(crate)` items these call sites reach into
directly (`CORS_FIELDS`, `in_cors_field_position`, `sum_type_variants`,
`variants_for_ty`, `for_each_unit`, `type_ref_str`, and others) are promoted
to `pub`, since cross-module `pub(crate)` visibility doesn't survive a crate
boundary.

`symbols.rs`'s one non-pure corner — `CrossFileSymbol`,
`find_declaration_cross_file`, `describe_symbol_cross_file` — used
`tower_lsp::lsp_types::Url` only for two things: comparing against the
current file (identity) and tagging the result. Both are `PathBuf`
operations in disguise, so the moved `bynk_ide::symbols` versions are
`PathBuf`-keyed, and `bynk-lsp/src/symbols.rs`'s shim re-declares the
`Url`-typed originals as thin wrappers over the `PathBuf` versions — legal
because Rust's shadowing rule lets a module's own item definitions win over
a glob import of the same name.

**Consequences.** No LSP-observable behaviour changes; the ~97 inline unit
tests that lived in these four files move with them and pass unchanged
(bar six `symbols.rs` fixtures rewritten from `Url::from_file_path(...)` to
plain `PathBuf`s). `bynk-ide` gains a new dependency on `bynk-fmt` (for
`symbols.rs`'s signature-rendering helpers), which only depends on
`bynk-syntax` and so stays wasm-safe. This unblocks — but does not itself
add — a `bynk_complete` wasm entry and playground completion support,
tracked as a follow-up.
