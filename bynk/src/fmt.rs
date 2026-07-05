//! `bynk fmt` — format `.bynk` source files in place.
//!
//! Runs the formatter in-process over `bynk-fmt` (v0.138, #487), mirroring
//! `bynkc fmt` byte-for-byte: each input is formatted and rewritten only when it
//! changes; `--check` reports non-canonical files without writing; `-` reads
//! stdin and writes the formatted result to stdout. The `BYNK_BYNKC` override
//! shells the pinned compiler instead, like `bynk check`.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::compiler::{Compiler, Origin};

/// Run `bynk fmt`. `compiler` carries the driver's resolution so a `BYNK_BYNKC`
/// override can be honoured by shelling the pinned `bynkc`.
pub fn run(compiler: &Compiler, inputs: Vec<PathBuf>, check: bool) -> ExitCode {
    if let (Some(Origin::Override), Some(bynkc)) = (compiler.origin, compiler.path.as_deref()) {
        let mut args: Vec<OsString> = vec!["fmt".into()];
        args.extend(inputs.iter().map(|p| p.as_os_str().to_os_string()));
        if check {
            args.push("--check".into());
        }
        return crate::shell::delegate(bynkc, args);
    }
    fmt_in_process(inputs, check)
}

/// The default path: the shared command body (#521, [`bynk_driver::run_fmt`]).
fn fmt_in_process(inputs: Vec<PathBuf>, check: bool) -> ExitCode {
    bynk_driver::run_fmt("bynk", &inputs, check)
}
