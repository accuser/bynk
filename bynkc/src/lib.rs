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
    requirements, resolver,
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

// The diagnostic renderers moved down into the `bynk-render` crate (slice 6):
// ariadne human + the short/json line forms over `CompileError`. Re-export them
// so `bynkc`'s binary, the diagnostic transcripts, and the tests resolve
// unchanged. The `ProjectFailure` flatteners (below) stay here and delegate.
pub use bynk_render::{
    print_errors, print_errors_short, print_project_errors, render_errors, render_errors_plain,
    render_errors_short, render_project_errors,
};

pub use firstparty::Platform;

// The Node floor moved to `bynk-emit` (slice 7) so the `bynk` driver can read it
// without depending on the `bynkc` crate. Re-export it so `bynkc::NODE_MAJOR_FLOOR`
// and the `cli.rs` doc-links resolve unchanged.
pub use bynk_emit::{
    Compiled, NODE_MAJOR_FLOOR, compile, compile_with_warnings, write_compiled_file, write_output,
};
pub use project::{
    AttributedError, BuildTarget, CompileOptions, CompiledFile, DiscoveredCase, DiscoveredSuite,
    ImportExt, ProjectFailure, ProjectOutput, ProjectPaths, Roots, TestLocation, compile_project,
    read_project_paths,
};

// In-browser track (ADR 0137): strip-only TS→JS, re-exported so the CLI, the API,
// and tests share one entry point. `strip_project_to_js` moved into `bynk-strip`
// in slice 3 so the wasm entry can reuse it without depending on `bynkc`.
pub use bynk_strip::{StripError, strip_project_to_js, strip_types};

/// v0.24 (ADR 0052 rider): render a failed project build with full ariadne
/// source context per file — the attribution built for the LSP, fixing the
/// standing gap where project-mode CLI errors were bare lines while
/// single-file mode had rich rendering. Unattributed (project-level)
/// errors keep the plain form.
///
/// This is the **flattening layer** (ADR 0100): it attributes each
/// `AttributedError` to its file snapshot and delegates the actual rendering to
/// [`bynk_render::print_errors`]. The `ProjectFailure → CompileError` flattening
/// stays here, above `bynk-render`, so there is no `render → emit` edge.
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
                bynk_render::print_errors(std::slice::from_ref(&ae.error), text, &label);
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

/// v0.89 (ADR 0117): print a successful build's non-failing warnings. A
/// successful build keeps no per-file snapshots, so warnings render in the
/// plain `warning[<category>]: <message>` form (with the owning file, when
/// known) rather than ariadne source context.
pub fn print_project_warnings(warnings: &[project::AttributedError]) {
    for w in warnings {
        let where_ = w
            .source_path
            .as_deref()
            .map(|p| format!("{}: ", p.to_string_lossy().replace('\\', "/")))
            .unwrap_or_default();
        eprintln!("{where_}warning[{}]: {}", w.error.category, w.error.message);
        for note in &w.error.notes {
            eprintln!("  note: {note}");
        }
    }
}

/// The project-failure analogue of [`bynk_render::print_errors_short`]: each
/// attributed error is positioned against its file's snapshot; an unattributed
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
///
/// The flattening layer (ADR 0100): it delegates the per-error formatting to
/// [`bynk_render::short_line`] / [`bynk_render::severity_word`].
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
                    bynk_render::short_line(&label, text, &ae.error)
                }
                None => format!(
                    "{}[{}]: {}",
                    bynk_render::severity_word(&ae.error),
                    ae.error.category,
                    ae.error.message
                ),
            }
        })
        .collect()
}
