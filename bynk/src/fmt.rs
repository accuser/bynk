//! `bynk fmt` — format `.bynk` source files in place.
//!
//! Runs the formatter in-process over `bynk-fmt` (v0.138, #487), mirroring
//! `bynkc fmt` byte-for-byte: each input is formatted and rewritten only when it
//! changes; `--check` reports non-canonical files without writing; `-` reads
//! stdin and writes the formatted result to stdout. The `BYNK_BYNKC` override
//! shells the pinned compiler instead, like `bynk check`.

use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use bynk_fmt::{FormatOptions, format_source};

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

/// The default path: format each input with `bynk-fmt`.
fn fmt_in_process(inputs: Vec<PathBuf>, check: bool) -> ExitCode {
    let opts = FormatOptions::default();
    if inputs.is_empty() {
        eprintln!("bynk fmt: no input files (pass file paths or `-` for stdin)");
        return ExitCode::FAILURE;
    }
    let mut had_diff = false;
    let mut had_error = false;
    for input in &inputs {
        if input.as_os_str() == "-" {
            let mut source = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut source) {
                eprintln!("bynk fmt: read from stdin: {e}");
                return ExitCode::FAILURE;
            }
            match format_source(&source, &opts) {
                Ok(formatted) => print!("{formatted}"),
                Err(e) => {
                    bynk_render::print_errors(&e.errors, &source, "<stdin>");
                    return ExitCode::FAILURE;
                }
            }
            continue;
        }
        let source = match std::fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("bynk fmt: read `{}`: {e}", input.display());
                had_error = true;
                continue;
            }
        };
        let filename = input.display().to_string();
        match format_source(&source, &opts) {
            Ok(formatted) => {
                if check {
                    if formatted != source {
                        eprintln!("bynk fmt: {} is not canonically formatted", input.display());
                        had_diff = true;
                    }
                } else if formatted != source
                    && let Err(e) = std::fs::write(input, formatted)
                {
                    eprintln!("bynk fmt: write `{}`: {e}", input.display());
                    had_error = true;
                }
            }
            Err(e) => {
                bynk_render::print_errors(&e.errors, &source, &filename);
                had_error = true;
            }
        }
    }
    if had_error || (check && had_diff) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
