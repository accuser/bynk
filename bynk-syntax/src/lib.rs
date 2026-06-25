//! Bynk's syntax foundation — the lowest leaf of the compiler crate set.
//!
//! This crate holds the modules every other layer depends *on* and none depend
//! *up* from: the lexer, the parser and its AST, source [`span`]s, the
//! [`keywords`] table, the structured [`CompileError`](error::CompileError)
//! type, and the [`diagnostics`] code registry (the single source of truth for
//! `bynk.*` codes). Diagnostics, positions, and codes therefore cross every
//! crate without an upward edge.
//!
//! Extracted from `bynkc` as slice 1 of the crate-decomposition track (ADRs
//! 0099 layering, 0102 foundation boundary). Behaviour is unchanged from when
//! these modules lived in `bynkc`; `bynkc` now re-exports them so its public
//! API is preserved.

pub mod ast;
pub mod diagnostics;
pub mod error;
pub mod keywords;
pub mod lexer;
pub mod parser;
pub mod span;

pub use error::{CompileError, Severity, partition_by_severity};
