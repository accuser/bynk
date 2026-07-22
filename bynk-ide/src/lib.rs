//! Bynk's IDE/LSP analysis surface.
//!
//! The non-bailing diagnostics the language server consumes — single-file
//! ([`diagnose`]) and whole-project ([`diagnose_project_with`], with
//! [`diagnose_project`] the single-tree convenience) — plus the result
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

use bynk_check::{checker, expr_types, hints, index, locals, requirements, resolver};
use bynk_syntax::error::{CompileError, Severity};
use bynk_syntax::{ast, lexer, parser};

/// #846: re-exported rather than left as a raw `bynk_emit::project` path —
/// `bynk-lsp` links `bynk-ide`/`bynk-check`/`bynk-syntax` directly and
/// deliberately does not depend on `bynk-emit` (the whole-compiler crate);
/// see `bynk-lsp/Cargo.toml`'s dependency comment. Unlike `Roots`/
/// `AnalysisRoots` (which the IDE layer re-shapes because the raw type
/// carries build-only concerns), `ContextSequenceInfo`'s fields are already
/// IDE-appropriate as-is, so this is a plain re-export rather than a lowering.
pub use bynk_emit::project::ContextSequenceInfo;

pub mod completion;
pub mod locals_nav;
pub mod sequence;
pub mod signature_help;
pub mod symbols;

/// One diagnostic produced from a recovery-mode compile of a single file.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub error: CompileError,
    pub severity: Severity,
}

/// Best-effort single-file compilation that always returns diagnostics.
///
/// Used by the LSP server: lex → parse-with-recovery → resolve → check, with
/// each phase accumulating its diagnostics. The returned `SourceUnit` is
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
                // ADR 0117: a clean check may still carry non-failing warnings
                // (`Ok` now), so surface those too — not only the `Err` path.
                match checker::check(resolved) {
                    Ok(typed) => {
                        for e in typed.warnings {
                            diagnostics.push(Diagnostic {
                                severity: Severity::for_error(&e),
                                error: e,
                            });
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
    /// v0.99: per-file capability-requirement ledger — every capability-consuming
    /// site with its provenance, driving the ghost `given` inlay hint and hover.
    pub requirements: requirements::FileRequirements,
    /// Slice 6b (ADR 0095): qualified unit name → its project source file(s),
    /// in discovery order — the unit→file map backing document links and
    /// consumed-context navigation. Synthetic units excluded; empty on a bail.
    pub unit_sources: HashMap<String, Vec<PathBuf>>,
    /// #846: qualified context/adapter unit name → the cross-context/agent
    /// tables the sequence-diagram query classifies handler calls against.
    /// See `bynk_emit::project::ProjectAnalysis::sequence_info`.
    pub sequence_info: HashMap<String, ContextSequenceInfo>,
}

/// Slice A: which trees a project's analysis walks.
///
/// `bynk-ide` owns this rather than re-exporting `bynk_emit::project::Roots`:
/// this crate is the IDE-facing published surface, and `Roots` carries
/// `tests_prefix` semantics an IDE caller has no business knowing. The lowering
/// is a few lines and it is the seam where the LSP's needs and the compiler's
/// can diverge later without a break.
#[derive(Debug, Clone)]
pub enum AnalysisRoots {
    /// One tree, walked as a single root, with no manifest consulted — the
    /// pre-slice-A behaviour and what [`diagnose_project`] still means.
    SingleTree(PathBuf),
    /// A manifest-backed project rooted here: `bynk.toml`'s `[paths]
    /// include`/`exclude` decide the trees, exactly as `bynkc` reads them.
    /// Mirrors `bynk-driver`'s `project_options` — the compiler's own choice.
    Project(PathBuf),
}

impl AnalysisRoots {
    fn lower(&self) -> bynk_emit::project::Roots {
        match self {
            AnalysisRoots::SingleTree(root) => bynk_emit::project::Roots::Single(root.clone()),
            AnalysisRoots::Project(root) => bynk_emit::project::Roots::Split {
                project_root: root.clone(),
                paths: bynk_emit::project::read_project_paths(root),
            },
        }
    }

    /// The project root every analysed `source_path` is relative to. For
    /// `SingleTree` that is the tree itself (identity ≡ tree-relative, ADR
    /// 0198).
    pub fn project_root(&self) -> &Path {
        match self {
            AnalysisRoots::SingleTree(r) | AnalysisRoots::Project(r) => r,
        }
    }
}

/// Slice A: the `.bynk` files these roots contain — the same discovery
/// `compile_project` performs, `exclude` and the `out`/`node_modules` caches
/// honoured. For enumerating a project's units without analysing it.
pub fn discover_files(roots: &AnalysisRoots) -> Vec<PathBuf> {
    bynk_emit::project::discover_project_files(&roots.lower())
}

/// #302: the qualified name a file moved from `old_rel` to `new_rel` should
/// now declare, preserving whichever single-file/multi-file arrangement
/// `old_rel` used to satisfy against `old_name` — for the LSP's
/// `workspace/willRenameFiles` handler.
pub fn renamed_unit_name(old_rel: &Path, old_name: &str, new_rel: &Path) -> Option<String> {
    bynk_emit::project::renamed_unit_name(old_rel, old_name, new_rel)
}

/// v0.24 (ADR 0052): non-bailing, overlay-aware, file-attributed project
/// diagnostics — the LSP analysis entry point, distinct from
/// `compile_project` (which bails and emits). `overlay` maps
/// canonicalised absolute paths to buffer text layered over disk reads.
///
/// Slice A: this is the **single-tree convenience** over
/// [`diagnose_project_with`] — it walks `root` as one tree and consults no
/// manifest, which is what every caller handing in a fixture root already
/// means. A manifest-backed project wants
/// `diagnose_project_with(&AnalysisRoots::Project(root), …)`.
pub fn diagnose_project(root: &Path, overlay: &HashMap<PathBuf, String>) -> ProjectDiagnostics {
    diagnose_project_with(&AnalysisRoots::SingleTree(root.to_path_buf()), overlay)
}

/// Slice A: project diagnostics over manifest-resolved roots — the LSP analyses
/// exactly the files `bynkc` compiles, from the same manifest, through the same
/// discovery.
///
/// Every path in the result is **project-relative** (ADR 0198), so a file is
/// named uniquely across `include` roots.
pub fn diagnose_project_with(
    roots: &AnalysisRoots,
    overlay: &HashMap<PathBuf, String>,
) -> ProjectDiagnostics {
    let analysis = bynk_emit::project::analyse_project_with(&roots.lower(), overlay);
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
        requirements: analysis.requirements,
        expr_types: analysis.expr_types,
        locals: analysis.locals,
        unit_sources: analysis.unit_sources,
        sequence_info: analysis.sequence_info,
    }
}
