//! `bynk check` — type-check a `.bynk` file or project without writing output.
//!
//! Runs the linked pipeline in-process (v0.138, #487): a directory routes
//! through [`bynk_emit::project::compile_project`], a single file through
//! [`bynk_emit::compile_with_warnings`] — exactly `bynkc check`'s two branches, so the
//! output is identical. The escape hatch matches `bynk dev`: when the driver
//! resolved `bynkc` via a `BYNK_BYNKC` override, the pinned compiler is shelled
//! instead so an externally-managed toolchain still governs the result.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::cli::CheckFormatArg;
use crate::compiler::{Compiler, Origin};

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

/// The default path: the shared command body (#521, [`bynk_driver::run_check`]).
fn check_in_process(input: &Path, format: CheckFormatArg) -> ExitCode {
    bynk_driver::run_check("bynk", input, format == CheckFormatArg::Short)
}
