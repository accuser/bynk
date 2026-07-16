//! `bynkc-lsp` — the Bynk Language Server binary.
//!
//! Slice C (the `[lib]` seam): the server implementation lives in the crate's
//! library ([`bynk_lsp`]); this binary is a thin entry point so integration
//! tests can `use bynk_lsp::…` rather than `#[path]`-include source modules.

#[tokio::main]
async fn main() {
    bynk_lsp::run().await;
}
