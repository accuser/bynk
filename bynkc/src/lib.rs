//! Bynk v0.3 compiler library.
//!
//! Compiles `.bynk` commons source into TypeScript modules.
//!
//! Pipeline: lex → parse → resolve → check → emit.
//!
//! v0.3 introduces multi-file commons and the `uses` mechanism. A "project"
//! is a directory containing one or more commons; a commons is either a
//! single `.bynk` file or a directory of `.bynk` files that share a
//! `commons name` header. See [`compile_project`].
//!
//! The single-string entrypoint [`compile`] remains for v0–v0.2 fixtures
//! and any single-file commons that does not declare `uses` against another
//! commons.

pub mod cli;
pub mod test_json;

// The syntax foundation now lives in the `bynk-syntax` leaf crate (slice 1 of
// the crate-decomposition track). Re-export its modules at the crate root so
// `bynkc`'s public API and every internal `crate::ast` / `crate::lexer` path is
// preserved — consumers and the rest of the pipeline see no change.
pub use bynk_syntax::error::Severity;
pub use bynk_syntax::{CompileError, ast, diagnostics, error, keywords, lexer, parser, span};

// The semantic-analysis layer moved down into the `bynk-check` crate (slice 3):
// resolver, checker, the registries, first-party sources, actors, and the
// captured index/hints/expr_types/locals tables. Re-export its modules at the
// crate root so `bynkc`'s public API and every internal `crate::checker` /
// `crate::index` path is preserved — the emitter/project layers above see no
// change.
pub use bynk_check::{
    actors, builtin_names, checker, expr_types, firstparty, hints, index, kernel_methods, locals,
    resolver,
};

// Build orchestration + TS emission moved down into the `bynk-emit` crate
// (slice 4). Re-export its modules at the crate root so `bynkc`'s public API and
// every internal `crate::emitter` / `crate::project` path is preserved — the CLI
// and compile/diagnose glue see no change.
pub use bynk_emit::{emitter, project};

// The IDE/LSP analysis surface moved down into the `bynk-ide` crate (slice 5):
// the non-bailing single-file and project diagnostics. Re-export them so
// `bynkc`'s public API and its index/diagnose integration tests resolve
// unchanged (the binary itself does not use this surface).
pub use bynk_ide::{Diagnostic, FileDiagnostics, ProjectDiagnostics, diagnose, diagnose_project};

// The formatter moved down into the `bynk-fmt` leaf (slice 2). Re-export it as
// `bynkc::fmt` so the `bynkc fmt` command and existing `bynkc::fmt::…` consumers
// (e.g. the LSP's formatting path) keep resolving unchanged.
pub use bynk_fmt as fmt;

use std::path::Path;

use ariadne::Source;

pub use firstparty::Platform;

/// Minimum supported Node.js **major** version for the `node` platform binding
/// and for running Bynk's emitted TypeScript.
///
/// Single source of truth for the Node floor: the `bynk` driver's `doctor`
/// command compares a detected `node` against this, and the
/// [`CliPlatform::Node`](cli::CliPlatform::Node) and `BYNK_NODE_BINDING` doc
/// comments link here rather than restating the number, so the floor is stated
/// once (v0.46 — was duplicated prose at two sites before).
pub const NODE_MAJOR_FLOOR: u32 = 18;
pub use project::{
    AttributedError, BuildTarget, CompileOptions, CompiledFile, ProjectFailure, ProjectOutput,
    ProjectPaths, Roots, compile_project, read_project_paths,
};

/// Compile a single Bynk source string to a TypeScript string.
///
/// This entry point parses the input as a self-contained, single-file commons
/// with no `uses` against other commons. Use [`compile_project`] for
/// multi-file projects or for any source that declares `uses`.
///
/// `filename` is used only for diagnostic rendering.
pub fn compile(source: &str, _filename: &str) -> Result<String, Vec<CompileError>> {
    let tokens = lexer::tokenize(source).map_err(|e| vec![e])?;
    let commons = parser::parse(&tokens, source)?;
    // v0.20a: function types are confined to non-boundary positions — the
    // same rule the project path applies.
    let mut boundary_errors = Vec::new();
    project::check_function_type_boundary_items(&commons.items, &mut boundary_errors);
    if !boundary_errors.is_empty() {
        return Err(boundary_errors);
    }
    let resolved = resolver::resolve(commons)?;
    let typed = checker::check(resolved)?;
    Ok(emitter::emit(&typed))
}

/// Render a list of compile errors to a string (for tests) using the given filename
/// as the diagnostic source label.
pub fn render_errors(errors: &[CompileError], source: &str, filename: &str) -> String {
    let mut out = Vec::new();
    let mut cache = (filename, Source::from(source));
    for err in errors {
        err.report(filename)
            .write(&mut cache, &mut out)
            .expect("write to Vec<u8> cannot fail");
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Render a list of compile errors to a string with colour disabled and the
/// given filename as the source label. Unlike [`render_errors`], the output
/// contains no ANSI escape codes, so it is byte-stable — suitable for the
/// committed diagnostic transcripts under `docs/diagnostics/`.
pub fn render_errors_plain(errors: &[CompileError], source: &str, filename: &str) -> String {
    let mut out = Vec::new();
    let mut cache = (filename, Source::from(source));
    for err in errors {
        err.report_plain(filename)
            .write(&mut cache, &mut out)
            .expect("write to Vec<u8> cannot fail");
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Render to stderr with color, used by the CLI.
pub fn print_errors(errors: &[CompileError], source: &str, filename: &str) {
    let mut cache = (filename, Source::from(source));
    for err in errors {
        let _ = err.report(filename).eprint(&mut cache);
    }
}

/// Render project-level errors as plain `[category] message` lines — the
/// fallback for errors with no file attribution. Rich, source-context
/// rendering lives in [`print_project_failure`] (v0.24).
pub fn print_project_errors(root: &Path, errors: &[CompileError]) {
    let _ = root;
    for err in errors {
        eprintln!("[{}] {}", err.category, err.message);
        for note in &err.notes {
            eprintln!("  note: {note}");
        }
    }
}

/// v0.24 (ADR 0052 rider): render a failed project build with full ariadne
/// source context per file — the attribution built for the LSP, fixing the
/// standing gap where project-mode CLI errors were bare lines while
/// single-file mode had rich rendering. Unattributed (project-level)
/// errors keep the plain form.
pub fn print_project_failure(failure: &project::ProjectFailure) {
    let texts: std::collections::HashMap<&std::path::Path, &str> = failure
        .snapshots
        .iter()
        .map(|(p, t)| (p.as_path(), t.as_str()))
        .collect();
    for ae in &failure.errors {
        match ae
            .source_path
            .as_deref()
            .and_then(|p| texts.get(p).map(|t| (p, *t)))
        {
            Some((path, text)) => {
                let label = path.to_string_lossy().replace('\\', "/");
                print_errors(std::slice::from_ref(&ae.error), text, &label);
            }
            None => {
                eprintln!("[{}] {}", ae.error.category, ae.error.message);
                for note in &ae.error.notes {
                    eprintln!("  note: {note}");
                }
            }
        }
    }
}

/// v0.38 (ADR 0071): one terse line per diagnostic for tooling consumers
/// (`bynkc check --format short`):
/// `path:line:col: <severity>[<category>]: <message>`. Line/column are
/// 1-indexed, computed from the byte span against the source. The VS Code
/// `bynkc` problem-matcher keys off this exact shape — keep it stable.
pub fn print_errors_short(errors: &[CompileError], source: &str, filename: &str) {
    eprint!("{}", render_errors_short(errors, source, filename));
}

/// The string form of [`print_errors_short`] — one `…[category]: message` line
/// per error, each newline-terminated. The renderer behind the CLI's `--format
/// short`, exposed for testing.
pub fn render_errors_short(errors: &[CompileError], source: &str, filename: &str) -> String {
    let mut out = String::new();
    for err in errors {
        out.push_str(&short_line(filename, source, err));
        out.push('\n');
    }
    out
}

/// The project-failure analogue of [`print_errors_short`]: each attributed
/// error is positioned against its file's snapshot; an unattributed
/// (project-level) error falls back to `<severity>[<category>]: <message>`.
pub fn print_project_failure_short(failure: &project::ProjectFailure) {
    for line in project_failure_short_lines(failure) {
        eprintln!("{line}");
    }
}

/// The string form of [`print_project_failure_short`]: one `path:line:col:
/// severity[category]: message` line per attributed error (an unattributed
/// project-level error falls back to `severity[category]: message`). Backs both
/// the printer above and the `bynkc test --format json` compile-error document,
/// whose `diagnostics` the VS Code `bynkc` problem-matcher re-parses.
pub fn project_failure_short_lines(failure: &project::ProjectFailure) -> Vec<String> {
    let texts: std::collections::HashMap<&std::path::Path, &str> = failure
        .snapshots
        .iter()
        .map(|(p, t)| (p.as_path(), t.as_str()))
        .collect();
    failure
        .errors
        .iter()
        .map(|ae| {
            match ae
                .source_path
                .as_deref()
                .and_then(|p| texts.get(p).map(|t| (p, *t)))
            {
                Some((path, text)) => {
                    let label = path.to_string_lossy().replace('\\', "/");
                    short_line(&label, text, &ae.error)
                }
                None => format!(
                    "{}[{}]: {}",
                    severity_word(&ae.error),
                    ae.error.category,
                    ae.error.message
                ),
            }
        })
        .collect()
}

fn short_line(filename: &str, source: &str, err: &CompileError) -> String {
    let (line, col) = bynk_syntax::span::line_col(source, err.span.start);
    format!(
        "{filename}:{line}:{col}: {}[{}]: {}",
        severity_word(err),
        err.category,
        err.message
    )
}

fn severity_word(err: &CompileError) -> &'static str {
    match Severity::for_error(err) {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

pub fn render_project_errors(errors: &[CompileError]) -> String {
    let mut out = String::new();
    for err in errors {
        out.push_str(&format!("[{}] {}\n", err.category, err.message));
        for note in &err.notes {
            out.push_str(&format!("  note: {note}\n"));
        }
    }
    out
}

#[allow(dead_code)]
fn _path_unused(_: &Path) {}
