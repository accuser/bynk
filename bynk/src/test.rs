//! `bynk test` — discover and run a project's test declarations.
//!
//! Delegates to the `bynkc` the driver resolves (`BYNK_BYNKC` → PATH →
//! sibling-of-`bynk`) and shells `bynkc test`, forwarding every flag verbatim
//! (v0.138, #487). Unlike `check`/`fmt`, `test` delegates *always*, not only
//! under an override: it orchestrates external `tsc`/`node`, so it is a
//! subprocess regardless, and routing it through the driver's resolution is the
//! whole point — an editor or developer inherits that resolution instead of
//! locating `bynkc` themselves (the fix direction for #486). The trade-off (a
//! `bynkc` binary must be present, and the driver↔compiler skew surface stays)
//! is accepted; the driver at least resolves it more richly than any editor.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::cli::TestFormatArg;
use crate::compiler::Compiler;

/// Parsed `bynk test` flags, forwarded to `bynkc test`.
pub struct TestArgs {
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub no_run: bool,
    pub format: TestFormatArg,
    pub inspect: bool,
    pub seed: Option<String>,
    pub case: Option<String>,
}

/// Run `bynk test` by shelling the resolved `bynkc`. When no `bynkc` could be
/// located, point the developer at `bynk doctor` rather than emitting a raw
/// spawn error.
pub fn run(compiler: &Compiler, args: TestArgs) -> ExitCode {
    let Some(bynkc) = compiler.path.as_deref() else {
        eprintln!(
            "bynk test: no `bynkc` compiler found (looked at $BYNK_BYNKC, PATH, and next to `bynk`)."
        );
        eprintln!("  Run `bynk doctor --only test` for the exact remedy.");
        return ExitCode::FAILURE;
    };

    let mut argv: Vec<OsString> = vec!["test".into(), args.input.into_os_string()];
    if let Some(output) = args.output {
        argv.push("--output".into());
        argv.push(output.into_os_string());
    }
    if args.no_run {
        argv.push("--no-run".into());
    }
    argv.push("--format".into());
    argv.push(args.format.as_bynkc_arg().into());
    if args.inspect {
        argv.push("--inspect".into());
    }
    if let Some(seed) = args.seed {
        argv.push("--seed".into());
        argv.push(seed.into());
    }
    if let Some(case) = args.case {
        argv.push("--case".into());
        argv.push(case.into());
    }

    crate::shell::delegate(bynkc, argv)
}
