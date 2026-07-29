//! `bynk fmt` — format `.bynk` source files in place.
//!
//! Runs the formatter in-process over `bynk-fmt` (v0.138, #487), mirroring
//! `bynkc fmt` byte-for-byte: each input is formatted and rewritten only when it
//! changes; `--check` reports non-canonical files without writing; `-` reads
//! stdin and writes the formatted result to stdout. The `BYNK_BYNKC` override
//! shells the pinned compiler instead, like `bynk check`.

use std::ffi::OsString;
use std::process::ExitCode;

use bynk_driver::{FmtArgs, IndentKind};

use crate::compiler::{Compiler, Origin};

/// Run `bynk fmt`. `compiler` carries the driver's resolution so a `BYNK_BYNKC`
/// override can be honoured by shelling the pinned `bynkc`.
pub fn run(compiler: &Compiler, args: FmtArgs) -> ExitCode {
    if let (Some(Origin::Override), Some(bynkc)) = (compiler.origin, compiler.path.as_deref()) {
        return crate::shell::delegate(bynkc, delegated_argv(&args));
    }
    fmt_in_process(args)
}

/// The argv for the shelled `bynkc fmt`. Every flag is respelled, style flags
/// included (#968): forwarding only `--check` would have a `BYNK_BYNKC`
/// override quietly format to the canonical style while the developer asked
/// for another one. `bynk` and `bynkc` flatten the same [`FmtArgs`], so the
/// flag names below are the ones the child parses.
fn delegated_argv(args: &FmtArgs) -> Vec<OsString> {
    let mut argv: Vec<OsString> = vec!["fmt".into()];
    if args.check {
        argv.push("--check".into());
    }
    if args.indent == IndentKind::Spaces {
        argv.push("--indent".into());
        argv.push("spaces".into());
    }
    if let Some(width) = args.indent_width {
        argv.push("--indent-width".into());
        argv.push(width.to_string().into());
    }
    argv.push("--max-line-width".into());
    argv.push(args.max_line_width.to_string().into());
    if args.no_trailing_comma {
        argv.push("--no-trailing-comma".into());
    }
    // Flags first, then `--`, then the paths: `bynk` already parsed these as
    // positionals (the user may have used their own `--`), so a path that
    // begins with a dash must not be re-read as a flag by the child. `-`
    // itself still means stdin after the separator.
    argv.push("--".into());
    argv.extend(args.inputs.iter().map(|p| p.as_os_str().to_os_string()));
    argv
}

/// The default path: the shared command body (#521, [`bynk_driver::run_fmt`]).
fn fmt_in_process(args: FmtArgs) -> ExitCode {
    bynk_driver::run_fmt("bynk", &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv_of(args: FmtArgs) -> Vec<String> {
        delegated_argv(&args)
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn base() -> FmtArgs {
        FmtArgs {
            inputs: vec!["a.bynk".into()],
            check: false,
            indent: IndentKind::Tab,
            indent_width: None,
            max_line_width: 100,
            trailing_comma: false,
            no_trailing_comma: false,
        }
    }

    #[test]
    fn defaults_forward_as_the_canonical_style() {
        assert_eq!(
            argv_of(base()),
            vec!["fmt", "--max-line-width", "100", "--", "a.bynk"]
        );
    }

    #[test]
    fn style_overrides_reach_the_pinned_compiler() {
        // The regression this guards: a `BYNK_BYNKC` override that formatted
        // to the canonical style while the developer asked for another one.
        let args = FmtArgs {
            check: true,
            indent: IndentKind::Spaces,
            indent_width: Some(4),
            max_line_width: 120,
            no_trailing_comma: true,
            ..base()
        };
        assert_eq!(
            argv_of(args),
            vec![
                "fmt",
                "--check",
                "--indent",
                "spaces",
                "--indent-width",
                "4",
                "--max-line-width",
                "120",
                "--no-trailing-comma",
                "--",
                "a.bynk",
            ]
        );
    }

    #[test]
    fn every_input_is_forwarded_after_the_separator() {
        // A path beginning with a dash is a path, not a flag — `bynk` already
        // parsed it as a positional, so the child must too. `-` still means
        // stdin after the separator.
        let args = FmtArgs {
            inputs: vec!["-".into(), "b/c.bynk".into(), "-weird.bynk".into()],
            ..base()
        };
        let argv = argv_of(args);
        let sep = argv
            .iter()
            .position(|a| a == "--")
            .expect("a `--` separator");
        assert_eq!(
            &argv[sep + 1..],
            ["-", "b/c.bynk", "-weird.bynk"],
            "every input must follow the separator, in order: {argv:?}"
        );
    }
}
