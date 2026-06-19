//! `bynk` — the Bynk driver.
//!
//! A thin orchestrator over the [`bynkc`] compiler and the Node toolchain:
//! `bynk` is to `bynkc` what `cargo` is to `rustc`. The compiler stays pure
//! (compile / check / fmt / test); environment orchestration — "is `wrangler`
//! installed", "is your machine ready" — lives here (ADR: introduce the `bynk`
//! driver).
//!
//! v0.46 ships the first command, [`doctor`], an upfront environment check. The
//! crate is deliberately split into single-concern modules (per ADR 0060):
//!
//! - [`probe`] — the portable detection primitive (presence + version +
//!   provenance), backed by the `which` crate so it is not Unix-only.
//! - [`compiler`] — locate `bynkc` (override → PATH → sibling-of-`bynk`) and
//!   report driver↔compiler version skew.
//! - [`doctor`] — the capability model, the checks, and the exit-code contract.
//! - [`report`] — render a [`doctor::Report`] as a human table, `--format
//!   short`, or `--format json`.

pub mod cli;
pub mod compiler;
pub mod doctor;
pub mod probe;
pub mod report;

/// The driver's own version, from Cargo. Compared against the resolved
/// `bynkc`'s version to detect skew ([`compiler::Skew`]).
pub const DRIVER_VERSION: &str = env!("CARGO_PKG_VERSION");
