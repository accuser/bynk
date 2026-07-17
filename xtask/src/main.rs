//! `cargo xtask <command>` — Bynk repo automation.
//!
//! Slice 0 of the increment-allocation track ships one command, `check-pending`,
//! which validates the files under `design/pending/`. Later slices add the
//! merge-time stamp (version + ADR-number assignment) as further commands here.

use std::process::ExitCode;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("check-pending") => check_pending(),
        Some(other) => {
            eprintln!("xtask: unknown command {other:?}");
            usage();
            ExitCode::from(2)
        }
        None => {
            usage();
            ExitCode::from(2)
        }
    }
}

fn check_pending() -> ExitCode {
    match xtask::check_all() {
        Ok(n) => {
            println!("check-pending: {n} pending file(s) valid");
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for e in &errors {
                eprintln!("{e}");
            }
            eprintln!("check-pending: {} error(s)", errors.len());
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    eprintln!("usage: cargo xtask check-pending");
}
