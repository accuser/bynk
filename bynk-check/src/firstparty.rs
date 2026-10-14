//! First-party standard adapters embedded in the toolchain (v0.17 §4.2).
//!
//! The sources, bindings and metadata live in the data-only `bynk-std` crate
//! (standard-library track, #1940). This module re-exports them unchanged, so
//! `bynk_check::firstparty::*` keeps resolving for every consumer. Parsing and
//! injecting the sources is checker logic, and stays here in `project_model`.

pub use bynk_std::*;
