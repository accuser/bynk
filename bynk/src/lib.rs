//! `bynk` — the Bynk driver.
//!
//! A thin orchestrator over the `bynkc` compiler and the Node toolchain:
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
//! - [`new`] — scaffold a new project (offline file-writing; no toolchain).
//! - [`dev`] — build a project and serve it locally with `wrangler dev`.
//! - [`check`] / [`fmt`] — type-check / format in-process (v0.138, #487).
//! - [`test`](mod@test) — delegate to the driver-resolved `bynkc` (v0.138, #487).
//! - [`diagnostics`] — shared flatten-then-delegate rendering for in-process
//!   compiles; [`shell`] — shelling the resolved `bynkc`.

pub mod check;
pub mod cli;
pub mod compiler;
pub mod deploy;
pub mod dev;
pub mod diagnostics;
pub mod doctor;
pub mod fmt;
pub mod new;
pub mod probe;
pub mod report;
pub mod shell;
pub mod test;

/// The driver's own version, from Cargo. Compared against the resolved
/// `bynkc`'s version to detect skew ([`compiler::Skew`]).
pub const DRIVER_VERSION: &str = env!("CARGO_PKG_VERSION");
