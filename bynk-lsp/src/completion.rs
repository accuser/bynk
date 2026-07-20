//! Re-exports `bynk-ide`'s completion logic (#808) — moved there so it's
//! reachable from `bynk-wasm` too, which can't depend on this crate's
//! `tower-lsp`/`tokio` stack. See `bynk_ide::completion` for the real module.
pub use bynk_ide::completion::*;
