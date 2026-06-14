//! Karn v0.3 compiler library.
//!
//! Compiles `.karn` commons source into TypeScript modules.
//!
//! Pipeline: lex → parse → resolve → check → emit.
//!
//! v0.3 introduces multi-file commons and the `uses` mechanism. A "project"
//! is a directory containing one or more commons; a commons is either a
//! single `.karn` file or a directory of `.karn` files that share a
//! `commons name` header. See [`compile_project`].
//!
//! The single-string entrypoint [`compile`] remains for v0–v0.2 fixtures
//! and any single-file commons that does not declare `uses` against another
//! commons.

pub mod ast;
pub mod builtin_names;
pub mod checker;
pub mod cli;
pub mod diagnostics;
pub mod emitter;
pub mod error;
pub mod expr_types;
pub mod firstparty;
pub mod fmt;
pub mod hints;
pub mod index;
pub mod keywords;
pub mod lexer;
pub mod parser;
pub mod project;
pub mod resolver;
pub mod span;

use std::path::Path;

use ariadne::Source;

pub use error::CompileError;
pub use firstparty::Platform;
pub use project::{
    AttributedError, BuildTarget, CompileOptions, CompiledFile, ProjectFailure, ProjectOutput,
    ProjectPaths, Roots, compile_project, read_project_paths,
};

/// Severity classification for [`Diagnostic`]. Mirrors LSP severity levels so
/// the LSP server can map diagnostics to the protocol without reinterpreting
/// error categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    /// Classify a [`CompileError`] by its category prefix.
    ///
    /// Categories starting with `karn.parse.orphan_doc_block` or
    /// `karn.given.unused_capability` are warnings; everything else is an
    /// error. Future categories can be added as the diagnostic surface grows.
    pub fn for_error(err: &CompileError) -> Severity {
        match err.category {
            "karn.parse.orphan_doc_block" | "karn.given.unused_capability" => Severity::Warning,
            _ => Severity::Error,
        }
    }
}

/// One diagnostic produced from a recovery-mode compile of a single file.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub error: CompileError,
    pub severity: Severity,
}

/// Best-effort single-file compilation that always returns diagnostics.
///
/// Used by the LSP server: lex → parse-with-recovery → resolve → check, with
/// each phase accumulating its diagnostics. The returned [`SourceUnit`] is
/// `Some` whenever the parser produced one (which is true for any file with a
/// recognisable header, even if individual items failed). Resolve and check
/// run only when both the lexer and parser produced a unit; their errors are
/// added to the same diagnostic list.
///
/// The TypeScript output is intentionally not produced here — the LSP only
/// needs diagnostics; the CLI uses [`compile`] / [`compile_project`].
pub fn diagnose(source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let tokens = match lexer::tokenize(source) {
        Ok(t) => t,
        Err(e) => {
            diagnostics.push(Diagnostic {
                severity: Severity::for_error(&e),
                error: e,
            });
            return diagnostics;
        }
    };
    let (unit_opt, parse_errors) = parser::parse_unit_with_recovery(&tokens, source);
    for e in parse_errors {
        diagnostics.push(Diagnostic {
            severity: Severity::for_error(&e),
            error: e,
        });
    }
    let Some(unit) = unit_opt else {
        return diagnostics;
    };
    // Resolution and checking are only well-defined for self-contained
    // commons units in single-file mode — contexts go through compile_project
    // which has the cross-file machinery. Match the same restriction here.
    if let ast::SourceUnit::Commons(c) = unit {
        match resolver::resolve(c) {
            Ok(resolved) => {
                if let Err(errs) = resolver::resolve_file(&resolved) {
                    for e in errs {
                        diagnostics.push(Diagnostic {
                            severity: Severity::for_error(&e),
                            error: e,
                        });
                    }
                }
                if let Err(errs) = checker::check(resolved) {
                    for e in errs {
                        diagnostics.push(Diagnostic {
                            severity: Severity::for_error(&e),
                            error: e,
                        });
                    }
                }
            }
            Err(errs) => {
                for e in errs {
                    diagnostics.push(Diagnostic {
                        severity: Severity::for_error(&e),
                        error: e,
                    });
                }
            }
        }
    }
    diagnostics
}

/// Compile a single Karn source string to a TypeScript string.
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

/// Render project-level errors to a string (for test assertion).
/// v0.24 (ADR 0052): per-file diagnostics from a whole-project analysis.
/// `text` is the **analysed snapshot** — positions must convert against it,
/// not a newer buffer (the analyse→publish window is real).
pub struct FileDiagnostics {
    /// Project-root-relative source path.
    pub source_path: std::path::PathBuf,
    /// The exact text that was analysed (overlay or disk).
    pub text: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// v0.24: the result of [`diagnose_project`]. Every discovered file appears
/// in `files` — clean files with an empty list — so a consumer can clear
/// stale diagnostics. `unattributed` holds project-level diagnostics with
/// no single owning file (group/cycle/directory validations).
pub struct ProjectDiagnostics {
    pub files: Vec<FileDiagnostics>,
    pub unattributed: Vec<Diagnostic>,
    /// v0.25 (ADR 0053): the project-wide binding index — every in-scope
    /// symbol's definition and reference sites, spans against the analysed
    /// snapshots in `files`.
    pub index: index::ProjectIndex,
    /// v0.27 (ADR 0056): per-file inferred-type inlay hints — `(binding-name
    /// span, label)`, span-ordered, spans against the analysed snapshots.
    pub hints: hints::FileHints,
    /// v0.30.2 (ADR 0063): per-file expression types — `(expr span, Ty)`,
    /// captured on the Ok path, for `.`-member completion's receiver typing.
    /// Empty for files with errors (the clean-file ceiling).
    pub expr_types: expr_types::FileExprTypes,
}

/// v0.24 (ADR 0052): non-bailing, overlay-aware, file-attributed project
/// diagnostics — the LSP analysis entry point, distinct from
/// [`compile_project`] (which bails and emits). `overlay` maps
/// canonicalised absolute paths to buffer text layered over disk reads.
pub fn diagnose_project(
    root: &std::path::Path,
    overlay: &std::collections::HashMap<std::path::PathBuf, String>,
) -> ProjectDiagnostics {
    let analysis = project::analyse_project(root, overlay);
    let mut by_file: std::collections::HashMap<std::path::PathBuf, Vec<Diagnostic>> =
        std::collections::HashMap::new();
    let mut unattributed = Vec::new();
    for ae in analysis.errors {
        let d = Diagnostic {
            severity: Severity::for_error(&ae.error),
            error: ae.error,
        };
        match ae.source_path {
            Some(p) => by_file.entry(p).or_default().push(d),
            None => unattributed.push(d),
        }
    }
    let files = analysis
        .snapshots
        .into_iter()
        .map(|(source_path, text)| FileDiagnostics {
            diagnostics: by_file.remove(&source_path).unwrap_or_default(),
            source_path,
            text,
        })
        .collect();
    // Anything attributed to a path without a snapshot (defensive — should
    // not happen) still surfaces rather than vanishing.
    for (_, ds) in by_file {
        unattributed.extend(ds);
    }
    ProjectDiagnostics {
        files,
        unattributed,
        index: analysis.index,
        hints: analysis.hints,
        expr_types: analysis.expr_types,
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
