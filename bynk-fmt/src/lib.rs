//! `bynk-fmt` — the Bynk formatter.
//!
//! Thin public-facing crate that re-exports the formatter implementation
//! from [`bynkc::fmt`]. The split exists so that downstream consumers (the
//! LSP server, third-party tools) can depend on a small surface without
//! pulling in the full compiler API; the implementation lives alongside the
//! parser and AST because formatting is fundamentally an AST-walk over the
//! compiler's own types.

pub use bynkc::fmt::{FormatError, FormatOptions, IndentStyle, format_source};
