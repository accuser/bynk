//! Rust binding for the `tree-sitter-bynk` grammar.
//!
//! This crate is **not** a product crate. It exists so the cross-parser
//! conformance test (`tests/conformance.rs`) can parse Bynk source with the
//! tree-sitter grammar and diff the accept/reject decision against the
//! hand-written `bynk-syntax` recursive-descent parser — the only guard that
//! ties the editor grammar to the compiler parser. It is unpublished and kept
//! off the release workflows' hardcoded crate list.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_bynk() -> *const ();
}

/// The tree-sitter [`LanguageFn`] for the Bynk grammar.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_bynk) };

/// The generated `node-types.json` for the grammar.
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");
