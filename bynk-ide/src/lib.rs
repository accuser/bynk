//! Bynk's IDE/LSP analysis surface.
//!
//! The non-bailing diagnostics the language server consumes — single-file
//! ([`diagnose`]) and whole-project ([`diagnose_project`]) — plus the result
//! types ([`Diagnostic`], [`FileDiagnostics`], [`ProjectDiagnostics`]). These
//! are *queries* over the captured tables produced during analysis (the binding
//! index, inlay hints, expression types, locals — all in `bynk-check`); the
//! project analysis itself ([`bynk_emit::project::analyse_project`]) is the
//! non-bailing counterpart to `compile_project`.
//!
//! Extracted from `bynkc` as slice 5 of the crate-decomposition track over
//! `bynk-syntax` + `bynk-check` + `bynk-emit`. Behaviour is unchanged; the
//! language server (`bynk-lsp`) depends on this crate directly instead of the
//! whole `bynkc` compiler crate, and `bynkc` re-exports these items so its own
//! tests and public API are unchanged.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bynk_check::{checker, expr_types, hints, index, locals, resolver};
use bynk_syntax::error::{CompileError, Severity};
use bynk_syntax::{ast, lexer, parser};

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
/// needs diagnostics; the CLI uses `compile` / `compile_project`.
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

/// Per-file diagnostics from a whole-project analysis.
/// v0.24 (ADR 0052): `text` is the **analysed snapshot** — positions must
/// convert against it, not a newer buffer (the analyse→publish window is real).
pub struct FileDiagnostics {
    /// Project-root-relative source path.
    pub source_path: PathBuf,
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
    /// v0.31 (ADR 0064): per-file local bindings with scope ranges, for the
    /// scope-at-offset query backing locals completion + navigation.
    pub locals: locals::FileLocals,
    /// Slice 6b (ADR 0095): qualified unit name → its project source file(s),
    /// in discovery order — the unit→file map backing document links and
    /// consumed-context navigation. Synthetic units excluded; empty on a bail.
    pub unit_sources: HashMap<String, Vec<PathBuf>>,
}

/// v0.24 (ADR 0052): non-bailing, overlay-aware, file-attributed project
/// diagnostics — the LSP analysis entry point, distinct from
/// `compile_project` (which bails and emits). `overlay` maps
/// canonicalised absolute paths to buffer text layered over disk reads.
pub fn diagnose_project(root: &Path, overlay: &HashMap<PathBuf, String>) -> ProjectDiagnostics {
    let analysis = bynk_emit::project::analyse_project(root, overlay);
    let mut by_file: HashMap<PathBuf, Vec<Diagnostic>> = HashMap::new();
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
        locals: analysis.locals,
        unit_sources: analysis.unit_sources,
    }
}
