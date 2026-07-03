//! `bynk-fmt` — the Bynk formatter.
//!
//! A real leaf crate: the formatter is an AST-walk over the `bynk-syntax`
//! types, so it depends on `bynk-syntax` only and never links the compiler
//! (resolver/checker/emitter). Slice 2 of the crate-decomposition track moved
//! the implementation down here from `bynkc::fmt` and re-pointed it onto the
//! `bynk-syntax` leaf, turning the former cosmetic façade into the home of the
//! formatter itself. `bynkc` now depends on this crate and re-exports it as
//! `bynkc::fmt` for its own `bynkc fmt` command.

mod fmt;

pub use fmt::{FormatError, FormatOptions, IndentStyle, format_source};
// v0.123 (editor-currency slice 2): the surface renderers for an expression and
// a refinement, exposed so `bynk-lsp` hover renders predicates / `where`
// clauses through the formatter's own logic rather than a copy that could drift.
pub use fmt::{expr_to_string, refinement_to_string};
