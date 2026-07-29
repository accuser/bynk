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
///
/// Only flags the run actually passed are respelled (#972). The child does its
/// own `bynk.toml` `[fmt]` lookup — it inherits this process's working
/// directory and gets the same input paths, so it reaches the same manifest —
/// and forwarding a *resolved* value would defeat that: `--max-line-width 100`
/// synthesised from the default would override a project's `120`, and one argv
/// cannot express the several manifests a multi-project run may resolve.
fn delegated_argv(args: &FmtArgs) -> Vec<OsString> {
    let mut argv: Vec<OsString> = vec!["fmt".into()];
    if args.check {
        argv.push("--check".into());
    }
    if let Some(kind) = args.indent {
        argv.push("--indent".into());
        argv.push(
            match kind {
                IndentKind::Tab => "tab",
                IndentKind::Spaces => "spaces",
            }
            .into(),
        );
    }
    if let Some(width) = args.indent_width {
        argv.push("--indent-width".into());
        argv.push(width.to_string().into());
    }
    if let Some(width) = args.max_line_width {
        argv.push("--max-line-width".into());
        argv.push(width.to_string().into());
    }
    if args.trailing_comma {
        argv.push("--trailing-comma".into());
    }
    if args.no_trailing_comma {
        argv.push("--no-trailing-comma".into());
    }
    if args.no_config {
        argv.push("--no-config".into());
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
            indent: None,
            indent_width: None,
            max_line_width: None,
            trailing_comma: false,
            no_trailing_comma: false,
            no_config: false,
        }
    }

    #[test]
    fn an_unflagged_run_forwards_no_style_at_all() {
        // #972: not even a synthesised `--max-line-width 100`. The child does
        // its own `bynk.toml` lookup, and a resolved default forwarded as a
        // flag would override the project's own `[fmt]`.
        assert_eq!(argv_of(base()), vec!["fmt", "--", "a.bynk"]);
    }

    #[test]
    fn style_overrides_reach_the_pinned_compiler() {
        // The regression this guards: a `BYNK_BYNKC` override that formatted
        // to the canonical style while the developer asked for another one.
        let args = FmtArgs {
            check: true,
            indent: Some(IndentKind::Spaces),
            indent_width: Some(4),
            max_line_width: Some(120),
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
    fn an_explicit_tab_is_forwarded_so_it_can_beat_a_manifest() {
        // `--indent tab` is not a no-op once `[fmt] indent = "spaces"` exists:
        // it is how a run overrides the project back to tabs, so the child must
        // be told, even though tabs are also the spec default.
        let args = FmtArgs {
            indent: Some(IndentKind::Tab),
            ..base()
        };
        let argv = argv_of(args);
        assert!(
            argv.windows(2).any(|w| w == ["--indent", "tab"]),
            "an explicit `--indent tab` must be forwarded: {argv:?}"
        );
    }

    #[test]
    fn trailing_comma_and_no_config_are_forwarded() {
        let args = FmtArgs {
            trailing_comma: true,
            no_config: true,
            ..base()
        };
        let argv = argv_of(args);
        assert!(argv.contains(&"--trailing-comma".to_string()), "{argv:?}");
        assert!(argv.contains(&"--no-config".to_string()), "{argv:?}");
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
