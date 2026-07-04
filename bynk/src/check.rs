//! `bynk check` — type-check a `.bynk` file or project without writing output.
//!
//! Runs the linked pipeline in-process (v0.138, #487): a directory routes
//! through [`compile_project`], a single file through
//! [`compile_with_warnings`] — exactly `bynkc check`'s two branches, so the
//! output is identical. The escape hatch matches `bynk dev`: when the driver
//! resolved `bynkc` via a `BYNK_BYNKC` override, the pinned compiler is shelled
//! instead so an externally-managed toolchain still governs the result.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bynk_emit::project::{CompileOptions, read_project_paths};
use bynk_emit::{compile_with_warnings, project};

use crate::cli::CheckFormatArg;
use crate::compiler::{Compiler, Origin};
use crate::diagnostics;

/// Run `bynk check`. `compiler` carries the driver's resolution so an override
/// can be honoured by shelling the pinned `bynkc`.
pub fn run(compiler: &Compiler, input: PathBuf, format: CheckFormatArg) -> ExitCode {
    // Escape hatch (mirrors `bynk dev`): a `BYNK_BYNKC` override pins an external
    // compiler, so `check` shells *that* `bynkc` rather than the linked pipeline.
    if let (Some(Origin::Override), Some(bynkc)) = (compiler.origin, compiler.path.as_deref()) {
        return crate::shell::delegate(
            bynkc,
            [
                "check".as_ref(),
                input.as_os_str(),
                "--format".as_ref(),
                format.as_bynkc_arg().as_ref(),
            ],
        );
    }
    check_in_process(&input, format)
}

/// The default path: compile in-process and render diagnostics.
fn check_in_process(input: &Path, format: CheckFormatArg) -> ExitCode {
    let short = format == CheckFormatArg::Short;
    if input.is_dir() {
        match project::compile_project(&project_options(input)) {
            Ok(out) => {
                diagnostics::print_project_warnings(&out.warnings);
                ExitCode::SUCCESS
            }
            Err(failure) => {
                if short {
                    diagnostics::render_project_failure_short(&failure);
                } else {
                    diagnostics::render_project_failure(&failure);
                }
                ExitCode::FAILURE
            }
        }
    } else {
        let source = match std::fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("bynk: could not read `{}`: {e}", input.display());
                return ExitCode::FAILURE;
            }
        };
        let filename = input.display().to_string();
        match compile_with_warnings(&source, &filename) {
            Ok(compiled) => {
                if !compiled.warnings.is_empty() {
                    if short {
                        bynk_render::print_errors_short(&compiled.warnings, &source, &filename);
                    } else {
                        bynk_render::print_errors(&compiled.warnings, &source, &filename);
                    }
                }
                ExitCode::SUCCESS
            }
            Err(errors) => {
                if short {
                    bynk_render::print_errors_short(&errors, &source, &filename);
                } else {
                    bynk_render::print_errors(&errors, &source, &filename);
                }
                ExitCode::FAILURE
            }
        }
    }
}

/// Root a directory project the way every project command does (matching
/// `bynkc`'s `project_options` and `bynk dev`'s `dev_compile_options`): a
/// `bynk.toml` or `src/` subdir selects the split-paths layout, else the legacy
/// single-tree where `<input>` is itself the root. Bundle target — `check` never
/// emits, so topology is irrelevant, and this keeps it identical to `bynkc
/// check`.
fn project_options(input: &Path) -> CompileOptions {
    if input.join("bynk.toml").exists() || input.join("src").is_dir() {
        CompileOptions::split(input.to_path_buf(), read_project_paths(input))
    } else {
        CompileOptions::single(input.to_path_buf())
    }
}
