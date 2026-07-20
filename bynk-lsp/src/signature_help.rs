//! Re-exports `bynk-ide`'s signature-help logic (#808) — moved there so it's
//! reachable from `bynk-wasm` too, which can't depend on this crate's
//! `tower-lsp`/`tokio` stack. See `bynk_ide::signature_help` for the real module.
pub use bynk_ide::signature_help::*;
