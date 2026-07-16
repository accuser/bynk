//! `bynkc-lsp` — Bynk Language Server.
//!
//! Implements the LSP capabilities listed in `design/bynk-lsp-spec.md` §4.3:
//! synchronisation (Full), diagnostics, hover, go-to-definition, formatting,
//! range formatting, document symbols, references, rename, code actions,
//! workspace symbols, document highlights, and file watching. Built on
//! `tower-lsp`.
//!
//! Architecture:
//! - [`Backend`] holds the project state: root path (the directory
//!   containing `bynk.toml`), parsed configuration, and an in-memory map of
//!   open files. State is guarded by a `tokio::sync::RwLock`.
//! - Document changes trigger `recompile_and_publish` which re-runs the
//!   compiler (via [`bynk_ide::diagnose`]) and publishes resulting diagnostics.
//! - Hover and definition consult the parsed AST for the file under the
//!   cursor; both are best-effort (return None for unrecognised positions).
//! - Formatting delegates to [`bynk_fmt::format_source`].

mod code_actions;
mod completion;
mod document_symbols;
mod hover;
mod index_queries;
mod inlay_hints;
mod locals_nav;
mod position;
mod project;
mod publish;
mod signature_help;
mod structure;
mod symbols;

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as JsonRpcResult;
use tower_lsp::lsp_types::request::{
    GotoImplementationParams, GotoImplementationResponse, GotoTypeDefinitionParams,
    GotoTypeDefinitionResponse,
};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::project::ProjectConfig;

const SERVER_NAME: &str = "bynkc-lsp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// In-memory document state.
#[derive(Debug, Clone)]
struct DocumentState {
    text: String,
    version: i32,
}

/// v0.25 (ADR 0053): one analysis round's retained outputs — the binding
/// index plus the snapshots its spans are offsets into, and the open-doc
/// versions captured when the overlay was built (rename emits versioned
/// edits against exactly these versions).
#[derive(Debug)]
struct Analysis {
    /// Slice A: the canonicalised **project root** every path in this round
    /// resolves against. Was the single `src` directory; the round now covers
    /// every `include` tree, and ADR 0198 makes each file's path
    /// project-relative — so this is the one base that resolves all of them.
    project_root: PathBuf,
    index: bynk_check::index::ProjectIndex,
    /// Project-relative path → the analysed text.
    snapshots: std::collections::HashMap<PathBuf, String>,
    /// Project-relative path → the open document's version at analysis
    /// time (absent for files read from disk).
    versions: std::collections::HashMap<PathBuf, i32>,
    /// v0.26 (ADR 0054): project-relative path → the round's diagnostics,
    /// full `CompileError`s included — the suggestions `codeAction` serves
    /// ride on them. Every analysed file has an entry (clean files an empty
    /// one). Replaces the v0.25 categories-only field; the rename baseline
    /// derives from these via [`Self::diag_categories`].
    diagnostics: std::collections::HashMap<PathBuf, Vec<bynk_ide::Diagnostic>>,
    /// v0.27 (ADR 0056): project-relative path → the round's harvested
    /// inferred-type hints, spans against the analysed snapshots.
    hints: bynk_check::hints::FileHints,
    /// v0.99: project-relative path → the round's capability-requirement ledger,
    /// driving the materializable ghost `given` inlay hint, spans against the
    /// analysed snapshots.
    requirements: bynk_check::requirements::FileRequirements,
    /// v0.31 (ADR 0064): project-relative path → the round's local bindings
    /// with scope ranges, for locals navigation (references/definition/
    /// highlight), spans against the analysed snapshots.
    locals: bynk_check::locals::FileLocals,
    /// Slice 6: project-relative path → the round's expression types, spans
    /// against the analysed snapshots — backs go-to-type-definition.
    expr_types: bynk_check::expr_types::FileExprTypes,
    /// Slice 6b (ADR 0095): qualified unit name → its project source file(s),
    /// project-relative — backs document links (`uses`/`consumes` → source).
    unit_sources: std::collections::HashMap<String, Vec<PathBuf>>,
}

impl Analysis {
    /// Per-file diagnostic categories — the rename validator's baseline,
    /// derived from the retained diagnostics.
    fn diag_categories(&self) -> Vec<(PathBuf, String)> {
        self.diagnostics
            .iter()
            .flat_map(|(path, diags)| {
                diags
                    .iter()
                    .map(|d| (path.clone(), d.error.category.to_string()))
            })
            .collect()
    }
}

/// Mutable project state.
#[derive(Debug, Default)]
struct State {
    /// Path to the project root (the directory containing `bynk.toml`). If
    /// no project root is found, this is None and the server operates in
    /// single-file mode for any open file.
    project_root: Option<PathBuf>,
    /// Parsed `bynk.toml` configuration. Defaults applied for missing fields.
    config: ProjectConfig,
    /// Open documents keyed by URI.
    docs: std::collections::HashMap<Url, DocumentState>,
    /// v0.24: URIs that currently carry published project diagnostics — the
    /// previous round's dirty set, so newly-clean files get a clearing
    /// (empty) publish.
    published: std::collections::HashSet<Url>,
    /// v0.24: debounce generation. Each change bumps it; a scheduled
    /// analysis runs only if it is still the latest when the delay elapses.
    analysis_generation: u64,
    /// v0.25: the latest analysis round's index + snapshots. References,
    /// rename, and the re-pointed definition/hover read this; positions
    /// convert against the analysed snapshots (v0.24 rule).
    analysis: Option<Arc<Analysis>>,
    /// Monotonic id handed to each analysis round as it *starts*. Together
    /// with `analysis_round_committed` this orders round completions: an old
    /// slow round must never overwrite a newer round's results (#513).
    analysis_round_started: u64,
    /// The id of the newest round whose results have been committed.
    analysis_round_committed: u64,
}

#[derive(Clone)]
struct Backend {
    client: Client,
    state: Arc<RwLock<State>>,
    /// Slice B (the freshness contract): serialises request-driven refreshes so
    /// concurrent index-backed requests after one edit coalesce onto a single
    /// round instead of each spawning its own. Held only across `analysis_for`'s
    /// refresh; never across a `state` lock.
    refresh_lock: Arc<tokio::sync::Mutex<()>>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(RwLock::new(State::default())),
            refresh_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Locate `bynk.toml` walking upward from the given path. Returns the
    /// project root (the directory containing `bynk.toml`) on success.
    fn find_project_root(start: &std::path::Path) -> Option<PathBuf> {
        let mut current = if start.is_file() {
            start.parent()?.to_path_buf()
        } else {
            start.to_path_buf()
        };
        loop {
            let candidate = current.join("bynk.toml");
            if candidate.is_file() {
                return Some(current);
            }
            current = current.parent()?.to_path_buf();
        }
    }

    /// Locate the nearest ancestor directory named `src`, walking upward from
    /// `start`. This is the implicit source root of a *rootless* tree — the
    /// same `src/`-without-`bynk.toml` layout `bynkc` compiles in its legacy
    /// single-tree mode (`bynkc/tests/e2e.rs` `compile_fixture`), which the
    /// compiler fixtures use. Returns that `src` directory.
    fn find_source_root(start: &std::path::Path) -> Option<PathBuf> {
        let mut current = if start.is_file() {
            start.parent()?.to_path_buf()
        } else {
            start.to_path_buf()
        };
        loop {
            if current.file_name().and_then(|n| n.to_str()) == Some("src") {
                return Some(current);
            }
            current = current.parent()?.to_path_buf();
        }
    }

    /// Resolve the analysis root for a path, with its config. A real
    /// `bynk.toml` project (config loaded from disk) takes precedence;
    /// otherwise (#485) fall back to the nearest enclosing `src/` as an
    /// implicit project so a multi-file commons in a rootless tree still
    /// analyses cross-file instead of dropping to sibling-blind single-file
    /// mode. `None` when neither is found — the caller stays single-file.
    fn resolve_root(start: &std::path::Path) -> Option<(PathBuf, project::ProjectConfig)> {
        if let Some(root) = Self::find_project_root(start) {
            let config = project::load_config(&root).unwrap_or_default();
            return Some((root, config));
        }
        // The implicit project root is the parent of `src`: with the default
        // `src_dir` ("src"), `run_project_diagnostics` re-derives exactly this
        // `src` tree as the analysis root, so every project-mode feature works
        // with no further plumbing.
        let src = Self::find_source_root(start)?;
        let root = src.parent()?.to_path_buf();
        Some((root, project::ProjectConfig::default()))
    }

    /// Re-run the compiler on the document at `uri` and publish diagnostics.
    /// Best-effort: a malformed file produces diagnostics rather than a
    /// hard failure.
    async fn recompile_and_publish(&self, uri: &Url) {
        // v0.24 (ADR 0052): with a project root, diagnostics are
        // project-wide (every file, contexts included) on a debounce.
        // Single-file mode (no bynk.toml) keeps the per-buffer path below.
        if self.state.read().await.project_root.is_some() {
            self.schedule_project_diagnostics().await;
            return;
        }
        let text = {
            let state = self.state.read().await;
            state.docs.get(uri).map(|d| d.text.clone())
        };
        let Some(text) = text else { return };
        let diagnostics = bynk_ide::diagnose(&text);
        let lsp_diags: Vec<Diagnostic> = diagnostics
            .into_iter()
            .map(|d| make_diagnostic(&d, &text, uri))
            .collect();
        let version = {
            let state = self.state.read().await;
            state.docs.get(uri).map(|d| d.version)
        };
        self.client
            .publish_diagnostics(uri.clone(), lsp_diags, version)
            .await;
    }

    /// Reload `bynk.toml` for the active project root after an external edit,
    /// so the format options, the diagnostics mode/debounce, and the source
    /// root take effect without restarting the server. The config's consumers
    /// (`formatting`, `did_change`, `run_project_diagnostics`) all read it live
    /// off `state`, so refreshing the stored config is enough — the caller
    /// schedules the re-analysis. A no-op in single-file mode (no root).
    async fn reload_config(&self) {
        let mut state = self.state.write().await;
        let Some(root) = state.project_root.clone() else {
            return;
        };
        state.config = project::load_config(&root).unwrap_or_default();
    }

    /// v0.24: debounce a project-wide analysis — each call bumps the
    /// generation; the spawned task runs only if still the latest after the
    /// delay, so a typing burst produces one analysis.
    async fn schedule_project_diagnostics(&self) {
        let generation = {
            let mut state = self.state.write().await;
            state.analysis_generation += 1;
            state.analysis_generation
        };
        let this = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            if this.state.read().await.analysis_generation != generation {
                return;
            }
            this.run_project_diagnostics().await;
        });
    }

    /// v0.24 (ADR 0052): one project-wide diagnostics round — overlay the
    /// open buffers over disk, analyse off the async runtime, convert spans
    /// against the **analysed snapshots**, and publish via the pure
    /// publish-plan (clears included).
    async fn run_project_diagnostics(&self) {
        let (round, root, canonical_root, overlay, versions, previously_dirty) = {
            let mut state = self.state.write().await;
            let Some(root) = state.project_root.clone() else {
                return;
            };
            state.analysis_round_started += 1;
            let round = state.analysis_round_started;
            // Slice A: the analysis is rooted at the *project*, not at one
            // `include` tree, and every path it returns is project-relative
            // (ADR 0198) — so this is the base the overlay keys against too.
            let canonical_root = root.canonicalize().unwrap_or_else(|_| root.clone());
            let mut overlay = std::collections::HashMap::new();
            let mut versions = std::collections::HashMap::new();
            for (uri, doc) in &state.docs {
                if let Ok(p) = uri.to_file_path() {
                    let canonical = p.canonicalize().unwrap_or(p);
                    // v0.25: capture the version the overlay snapshot came
                    // from, keyed project-relative like the analysis output.
                    if let Ok(rel) = canonical.strip_prefix(&canonical_root) {
                        versions.insert(rel.to_path_buf(), doc.version);
                    }
                    overlay.insert(canonical, doc.text.clone());
                }
            }
            let published = state.published.clone();
            (round, root, canonical_root, overlay, versions, published)
        };

        // Slice A: manifest-aware, multi-root — the same trees `bynkc` compiles.
        let roots = bynk_ide::AnalysisRoots::Project(root.clone());
        let Ok(result) =
            tokio::task::spawn_blocking(move || bynk_ide::diagnose_project_with(&roots, &overlay))
                .await
        else {
            return;
        };

        let mut new_by_uri: std::collections::HashMap<Url, Vec<Diagnostic>> =
            std::collections::HashMap::new();
        // Slice B (DECISION C): the document version each file was analysed at,
        // keyed by URI — so the publish can carry it and the client can drop a
        // range computed against a buffer it has already edited past. `None` for
        // a file read from disk (no open buffer, no version).
        let mut version_by_uri: std::collections::HashMap<Url, Option<i32>> =
            std::collections::HashMap::new();
        let mut snapshots = std::collections::HashMap::new();
        let mut diagnostics: std::collections::HashMap<PathBuf, Vec<bynk_ide::Diagnostic>> =
            std::collections::HashMap::new();
        for file in &result.files {
            let abs = canonical_root.join(&file.source_path);
            let abs = abs.canonicalize().unwrap_or(abs);
            let Ok(uri) = Url::from_file_path(&abs) else {
                continue;
            };
            // Spans convert against the snapshot the analysis saw — never a
            // newer buffer (Settled, v0.24 proposal).
            let diags: Vec<Diagnostic> = file
                .diagnostics
                .iter()
                .map(|d| make_diagnostic(d, &file.text, &uri))
                .collect();
            version_by_uri.insert(uri.clone(), versions.get(&file.source_path).copied());
            new_by_uri.insert(uri, diags);
            diagnostics.insert(file.source_path.clone(), file.diagnostics.clone());
            snapshots.insert(file.source_path.clone(), file.text.clone());
        }
        // v0.25: retain the round's index + snapshots for references/rename
        // and the binding-correct definition/hover. v0.26: plus the raw
        // diagnostics, for `codeAction` (the suggestions ride on them).
        {
            let analysis = Arc::new(Analysis {
                project_root: canonical_root.clone(),
                index: result.index.clone(),
                snapshots,
                versions,
                diagnostics,
                hints: result.hints,
                requirements: result.requirements,
                locals: result.locals,
                expr_types: result.expr_types,
                unit_sources: result.unit_sources,
            });
            let mut state = self.state.write().await;
            // Completion order is not start order: a slow old round finishing
            // after a newer one must be dropped, not committed (#513).
            if state.analysis_round_committed >= round {
                return;
            }
            state.analysis_round_committed = round;
            state.analysis = Some(analysis);
        }
        // Project-level diagnostics with no single owning file surface at
        // position 0:0 rather than vanishing — on `bynk.toml` when it exists,
        // else (#485, implicit `src/` mode has no manifest) on the first
        // analysed file, so they anchor to a real, openable document.
        let unattributed_anchor = {
            let toml = root.join("bynk.toml");
            if toml.is_file() {
                Url::from_file_path(toml).ok()
            } else {
                result.files.first().and_then(|f| {
                    let abs = canonical_root.join(&f.source_path);
                    let abs = abs.canonicalize().unwrap_or(abs);
                    Url::from_file_path(abs).ok()
                })
            }
        };
        if !result.unattributed.is_empty()
            && let Some(anchor_uri) = unattributed_anchor
        {
            let entry = new_by_uri.entry(anchor_uri).or_default();
            for d in &result.unattributed {
                entry.push(Diagnostic {
                    range: Default::default(),
                    severity: Some(match d.severity {
                        bynk_syntax::Severity::Error => DiagnosticSeverity::ERROR,
                        bynk_syntax::Severity::Warning => DiagnosticSeverity::WARNING,
                    }),
                    code: Some(tower_lsp::lsp_types::NumberOrString::String(
                        d.error.category.to_string(),
                    )),
                    message: d.error.message.clone(),
                    ..Default::default()
                });
            }
        }

        let (publishes, dirty) = publish::publish_plan(&previously_dirty, new_by_uri);
        for (uri, diags) in publishes {
            // Slice B (DECISION C): stamp the publish with the version the round
            // analysed this file at (was `None`), so a client can reject a range
            // its buffer has moved past. A clearing publish for a now-absent file
            // carries no version — it has no entry in `version_by_uri`.
            let version = version_by_uri.get(&uri).copied().flatten();
            self.client.publish_diagnostics(uri, diags, version).await;
        }
        let mut state = self.state.write().await;
        if state.analysis_round_committed == round {
            state.published = dirty;
        }
    }

    /// Slice A: the analysis roots for the active project — the manifest's,
    /// resolved by the compiler's own discovery. `None` in single-file mode (no
    /// project root), where cross-file lookups are skipped.
    ///
    /// Replaces `project_src_root`, which returned `root.join(config.src_dir)`:
    /// one tree, chosen by reducing `[paths] include` to its first entry and
    /// ignoring `exclude`. That reduction is the defect this slice removes.
    async fn analysis_roots(&self) -> Option<bynk_ide::AnalysisRoots> {
        let state = self.state.read().await;
        let root = state.project_root.as_ref()?;
        Some(bynk_ide::AnalysisRoots::Project(root.clone()))
    }

    /// The project's `.bynk` files, from the compiler's discovery — `exclude`
    /// and the `out`/`node_modules` caches honoured. Backs the unit enumeration
    /// completion does; `None` in single-file mode.
    async fn project_files(&self) -> Option<Vec<PathBuf>> {
        let roots = self.analysis_roots().await?;
        tokio::task::spawn_blocking(move || bynk_ide::discover_files(&roots))
            .await
            .ok()
    }

    /// v0.31: the def + use spans of the local under the cursor (def first), or
    /// `None` if the cursor is not on a local.
    fn local_sites(
        &self,
        analysis: &Analysis,
        rel: &std::path::Path,
        offset: usize,
    ) -> Option<Vec<bynk_syntax::span::Span>> {
        let text = analysis.snapshots.get(rel)?;
        let locals = analysis.locals.get(rel)?;
        crate::locals_nav::local_sites_at(locals, text, offset)
    }

    /// v0.31 (ADR 0064): the in-scope local bindings at the cursor, as
    /// `variable` completions, read from the **cached** analysis — so they
    /// survive the mid-edit buffer the current keystroke produced (the last
    /// good round's bindings around the cursor are what's wanted). Positions
    /// convert against the cached snapshot, like the other cached-round reads.
    async fn locals_completions(&self, uri: &Url, pos: Position) -> Vec<CompletionItem> {
        // Slice B: completion's locals sub-path resolves `pos` against the
        // round's snapshot (like `index_position`), so it refreshes too — the
        // one exposed reader the §4.2 table missed.
        let analysis = self.analysis_for(uri).await;
        let Some(analysis) = analysis else {
            return Vec::new();
        };
        let Some(rel) = Self::uri_to_rel(&analysis, uri) else {
            return Vec::new();
        };
        let (Some(text), Some(locals)) = (analysis.snapshots.get(&rel), analysis.locals.get(&rel))
        else {
            return Vec::new();
        };
        let Some(offset) = crate::position::position_to_offset(text, pos) else {
            return Vec::new();
        };
        bynk_check::locals::locals_at(locals, offset)
            .into_iter()
            .map(|b| CompletionItem {
                label: b.name.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(b.ty.clone()),
                ..Default::default()
            })
            .collect()
    }

    /// Convert same-file local spans to LSP `Location`s.
    fn local_locations(
        &self,
        analysis: &Analysis,
        rel: &std::path::Path,
        spans: &[bynk_syntax::span::Span],
    ) -> Vec<Location> {
        let Some(text) = analysis.snapshots.get(rel) else {
            return Vec::new();
        };
        let Ok(uri) = Url::from_file_path(analysis.project_root.join(rel)) else {
            return Vec::new();
        };
        spans
            .iter()
            .map(|s| Location {
                uri: uri.clone(),
                range: crate::position::span_to_range(text, *s),
            })
            .collect()
    }

    /// Slice 3 (ADR 0063): complete the members of a typed **value** receiver.
    /// Re-analyses the buffer rewritten so the receiver parses (the trailing
    /// `.partial` dropped), types the receiver via the retained `expr_types`,
    /// and maps its type to kernel methods + record fields. Empty when the
    /// receiver can't be typed (the file has errors — the clean-file ceiling).
    async fn value_member_completions(
        &self,
        uri: &Url,
        text: &str,
        offset: usize,
    ) -> Vec<CompletionItem> {
        let Some((rewritten, recv_offset)) = completion::value_receiver_rewrite(text, offset)
        else {
            return Vec::new();
        };
        let Some(ty) = self.type_receiver(uri, rewritten, recv_offset).await else {
            return Vec::new();
        };
        let files = self.project_files().await;
        completion::value_member_candidates(&ty, text, files.as_deref())
            .into_iter()
            .map(to_completion_item)
            .collect()
    }

    /// v0.124 (slice 3): at `<expr> is <cursor>`, the scrutinee sum type's
    /// variants. The scrutinee is typed via `expr_types` (re-analysing through
    /// `type_receiver`, the value-member path), so it is subject to the clean-
    /// file ceiling and goes silent — never wrong — on a broken buffer.
    async fn is_pattern_completions(
        &self,
        uri: &Url,
        text: &str,
        offset: usize,
    ) -> Vec<CompletionItem> {
        let Some(scrut_off) = is_scrutinee_offset(text, offset) else {
            return Vec::new();
        };
        self.scrutinee_variant_completions(uri, text, scrut_off)
            .await
    }

    /// v0.128: at an arm-pattern-start inside a `match <expr> { … }`, the
    /// scrutinee sum type's variants — the deferred half of slice 3's
    /// `is`-pattern completion, sharing its scrutinee typing and candidate set.
    async fn match_arm_completions(
        &self,
        uri: &Url,
        text: &str,
        offset: usize,
    ) -> Vec<CompletionItem> {
        let Some(scrut_off) = match_scrutinee_offset(text, offset) else {
            return Vec::new();
        };
        self.scrutinee_variant_completions(uri, text, scrut_off)
            .await
    }

    /// The variants of the scrutinee whose last character is at `scrut_off` — the
    /// shared tail of `is`/`match` pattern completion. Types the scrutinee via
    /// `expr_types` (the clean-file ceiling; silent, never wrong, on a broken
    /// buffer) and offers its variants; empty for a non-sum, non-`Result`/`Option`
    /// scrutinee. v0.145 (ADR 0169): `Result`/`Option` scrutinees now fire too
    /// (`variants_for_ty`), not only user-declared sums.
    async fn scrutinee_variant_completions(
        &self,
        uri: &Url,
        text: &str,
        scrut_off: usize,
    ) -> Vec<CompletionItem> {
        let Some(ty) = self.type_receiver(uri, text.to_string(), scrut_off).await else {
            return Vec::new();
        };
        let files = self.project_files().await;
        completion::variants_for_ty(&ty, text, files.as_deref())
            .into_iter()
            .map(to_completion_item)
            .collect()
    }

    /// v0.145 (ADR 0169): at `OuterVariant(‸` inside a match arm-pattern, the
    /// payload field type's variants — e.g. `Ok`/`Err` inside `Some(‸)` on an
    /// `Option[Result[…]]` scrutinee. `match_scrutinee_offset` deliberately bails
    /// on a nested constructor; `nested_pattern_offset` targets exactly it,
    /// yielding the scrutinee offset and the outer variant. Types the scrutinee
    /// via the same clean-file ceiling and resolves the payload type.
    async fn nested_pattern_completions(
        &self,
        uri: &Url,
        text: &str,
        offset: usize,
    ) -> Vec<CompletionItem> {
        let Some((scrut_off, variant)) = nested_pattern_offset(text, offset) else {
            return Vec::new();
        };
        let Some(ty) = self.type_receiver(uri, text.to_string(), scrut_off).await else {
            return Vec::new();
        };
        let files = self.project_files().await;
        completion::nested_variant_completions(&ty, &variant, text, files.as_deref())
            .into_iter()
            .map(to_completion_item)
            .collect()
    }

    /// v0.32 (ADR 0065): the type of a receiver expression at `recv_offset` in a
    /// buffer `rewritten` so it parses — re-analyse the overlay and query the
    /// retained `expr_types`. Shared by value-member completion and signature
    /// help; `None` when the file doesn't check clean (the clean-file ceiling).
    async fn type_receiver(
        &self,
        uri: &Url,
        rewritten: String,
        recv_offset: usize,
    ) -> Option<bynk_check::checker::Ty> {
        let roots = self.analysis_roots().await?;
        let project_root = roots.project_root().to_path_buf();
        let canonical_root = project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.clone());
        let cur = uri.to_file_path().ok()?;
        let cur = cur.canonicalize().unwrap_or(cur);
        // Slice A: project-relative, matching the round's identity (ADR 0198).
        let rel = cur.strip_prefix(&canonical_root).ok()?.to_path_buf();
        // Overlay every open doc, with this one rewritten so it parses.
        let overlay = {
            let state = self.state.read().await;
            let mut ov = std::collections::HashMap::new();
            for (u, doc) in &state.docs {
                if let Ok(p) = u.to_file_path() {
                    let canonical = p.canonicalize().unwrap_or(p);
                    let t = if u == uri {
                        rewritten.clone()
                    } else {
                        doc.text.clone()
                    };
                    ov.insert(canonical, t);
                }
            }
            ov
        };
        // Fast path (#513): completion fires on every `.` keystroke, and the
        // rewritten buffer (the trailing `.`-segment removed so it parses) is
        // usually byte-identical to the snapshot the last debounced round
        // analysed. Reuse that round's expression types instead of running a
        // synchronous whole-project re-analysis on the request path.
        if let Some(analysis) = self.state.read().await.analysis.clone()
            && analysis.snapshots.get(&rel).map(String::as_str) == Some(rewritten.as_str())
            && let Some((_, entries)) = analysis.expr_types.iter().find(|(p, _)| **p == rel)
        {
            return bynk_check::expr_types::type_at_offset(entries, recv_offset).cloned();
        }
        let result =
            tokio::task::spawn_blocking(move || bynk_ide::diagnose_project_with(&roots, &overlay))
                .await
                .ok()?;
        let (_, entries) = result.expr_types.iter().find(|(p, _)| **p == rel)?;
        bynk_check::expr_types::type_at_offset(entries, recv_offset).cloned()
    }

    /// v0.25: the latest analysis, running one synchronously if none has
    /// completed yet (a request can arrive before the first debounced
    /// round).
    async fn ensure_analysis(&self) -> Option<Arc<Analysis>> {
        if let Some(a) = self.state.read().await.analysis.clone() {
            return Some(a);
        }
        self.refresh_now().await;
        self.state.read().await.analysis.clone()
    }

    /// Slice B — the freshness contract (Q3, settled #663). The analysis a
    /// request must answer from, **current for `uri`**: cold start triggers a
    /// round; a round that predates `uri`'s buffer triggers a refresh.
    ///
    /// The client's request position refers to `uri`'s current document
    /// version — messages are ordered, so `docs[uri].version` reflects every
    /// `didChange` sent before the request. The returned analysis is guaranteed
    /// to have analysed *that* version of `uri`, so `position_to_offset` against
    /// its snapshot is never resolved against text the user edited past.
    ///
    /// Returns `None` — decline, per Q3 — only when the request cannot be
    /// answered at the version the client holds: single-file mode (no project),
    /// a file outside every `include` root (never a snapshot key), or a
    /// concurrent edit that moved past the refresh (rare; the next request is
    /// current). Never returns an analysis whose snapshot for `uri` is stale.
    async fn analysis_for(&self, uri: &Url) -> Option<Arc<Analysis>> {
        // The version the request's position is stated against. `None` when the
        // file is not an open buffer — then any round is as authoritative as it
        // gets (nothing newer to be stale against), so the freshness gate is a
        // no-op and only cold start matters.
        let want = self.state.read().await.docs.get(uri).map(|d| d.version);
        let current = |a: &Arc<Analysis>| {
            let Some(rel) = Self::uri_to_rel(a, uri) else {
                return false; // unmappable URI — cannot be answered
            };
            // The file must actually be *analysed* (a snapshot key), not merely
            // have a version entry: `versions` is built from open docs, so a
            // file open but outside every `include` root has a version and no
            // snapshot. Such a file is never answerable — decline.
            if !a.snapshots.contains_key(&rel) {
                return false;
            }
            match want {
                // Open buffer: the analysed snapshot must be at the client's
                // version, or the position resolves against text edited past.
                Some(v) => a.versions.get(&rel) == Some(&v),
                // Not an open buffer (a closed/disk file, e.g. a goto target):
                // the analysed round is authoritative — nothing newer to lag.
                None => true,
            }
        };

        if let Some(a) = self.state.read().await.analysis.clone()
            && current(&a)
        {
            return Some(a);
        }

        // Refresh. The lock serialises concurrent requests: the first runs the
        // round, the rest wait and then find it already current below — so N
        // requests after one edit share one round, not N.
        let _guard = self.refresh_lock.lock().await;
        if let Some(a) = self.state.read().await.analysis.clone()
            && current(&a)
        {
            return Some(a);
        }
        self.refresh_now().await;
        let a = self.state.read().await.analysis.clone()?;
        // Strict: only answer if the fresh round is actually current for `uri`.
        // An edit that landed during the round leaves us behind — decline, and
        // the next request refreshes again. Never a position against stale text.
        current(&a).then_some(a)
    }

    /// Run a round now, superseding any pending debounced one. Bumping the
    /// generation makes a scheduled round (which checks it before running) bail,
    /// so a request-driven refresh does not race a redundant debounce round that
    /// would produce the same result 200 ms later.
    async fn refresh_now(&self) {
        self.state.write().await.analysis_generation += 1;
        self.run_project_diagnostics().await;
    }

    /// Map a request URI to the analysis' project-relative path.
    fn uri_to_rel(analysis: &Analysis, uri: &Url) -> Option<PathBuf> {
        let p = uri.to_file_path().ok()?;
        let canonical = p.canonicalize().unwrap_or(p);
        // Slice A: one `strip_prefix` still, but against the *project* root —
        // which is total across `include` trees, where the old `src` base could
        // only ever name files in one of them. A file under no root strips fine
        // and simply misses every lookup, which is correct: it was not analysed.
        canonical
            .strip_prefix(&analysis.project_root)
            .ok()
            .map(|r| r.to_path_buf())
    }

    /// Slice 6a follow-up (ADR 0095): if `pos` sits on a `uses`/`consumes` unit
    /// name, the location of that unit's source (its first file, at the top —
    /// units aren't index symbols, so there is no finer def span to land on).
    /// Spans come from the live buffer; the target from the round's unit→source
    /// map. `None` for a first-party/unresolved unit or a non-unit position.
    async fn unit_reference_definition(&self, uri: &Url, pos: Position) -> Option<Location> {
        // Slice B: the position is resolved against *live* text (no stale-offset
        // risk), but the `uses`/`consumes` → source lookup reads the round's
        // `unit_sources`, so route that through the gate — fresh or decline,
        // never a stale unit map. Cheap here: `goto_definition` already
        // refreshed via `index_position`, so this hits the current-round path.
        let analysis = self.analysis_for(uri).await;
        let text = self
            .state
            .read()
            .await
            .docs
            .get(uri)
            .map(|d| d.text.clone());
        let (text, analysis) = (text?, analysis?);
        let offset = cursor_offset(&text, pos);
        for (unit, span) in crate::symbols::unit_reference_spans(&text) {
            if span.start <= offset && offset <= span.end {
                let rel = analysis.unit_sources.get(&unit)?.first()?;
                let target = Url::from_file_path(analysis.project_root.join(rel)).ok()?;
                return Some(Location {
                    uri: target,
                    range: Range::default(),
                });
            }
        }
        None
    }

    /// Convert an index site to an LSP location, spans against the analysed
    /// snapshot (v0.24 rule).
    fn site_to_location(
        analysis: &Analysis,
        site: &bynk_check::index::SiteRef,
    ) -> Option<Location> {
        let text = analysis.snapshots.get(&site.path)?;
        let abs = analysis.project_root.join(&site.path);
        let uri = Url::from_file_path(abs).ok()?;
        Some(Location {
            uri,
            range: crate::position::span_to_range(text, site.span),
        })
    }

    /// v0.34 (ADR 0067): build a `CallHierarchyItem` for an index symbol from
    /// its key + definition site. The key is round-tripped through `data` so
    /// the incoming/outgoing follow-ups resolve straight off it, never
    /// re-inferring from a position.
    fn call_hierarchy_item(
        analysis: &Analysis,
        key: &bynk_check::index::SymbolKey,
        def: &bynk_check::index::SiteRef,
    ) -> Option<CallHierarchyItem> {
        let location = Self::site_to_location(analysis, def)?;
        Some(CallHierarchyItem {
            name: key.name.clone(),
            kind: lsp_symbol_kind(key.kind),
            tags: None,
            detail: Some(key.unit.clone()),
            uri: location.uri,
            range: location.range,
            selection_range: location.range,
            data: serde_json::to_value(SerKey::from(key)).ok(),
        })
    }

    /// The call-site ranges (`fromRanges`) for a call relation, each converted
    /// against its file's analysed snapshot.
    fn call_ranges(analysis: &Analysis, sites: &[&bynk_check::index::SiteRef]) -> Vec<Range> {
        sites
            .iter()
            .filter_map(|s| {
                let text = analysis.snapshots.get(&s.path)?;
                Some(crate::position::span_to_range(text, s.span))
            })
            .collect()
    }

    /// v0.28 (ADR 0057): the shared body of both semantic-tokens requests —
    /// resolve the cached round, convert the optional range against the
    /// analysed snapshot, and run the pure producer. Empty when no round is
    /// cached or the file is outside the project.
    async fn semantic_tokens_for(&self, uri: &Url, range: Option<Range>) -> Vec<SemanticToken> {
        let analysis = self.analysis_for(uri).await;
        let Some(analysis) = analysis else {
            return Vec::new();
        };
        let Some(rel) = Self::uri_to_rel(&analysis, uri) else {
            return Vec::new();
        };
        let Some(text) = analysis.snapshots.get(&rel) else {
            return Vec::new();
        };
        let span = match range {
            None => None,
            // The requested range converts against the analysed snapshot,
            // like the spans it is intersected with.
            Some(r) => {
                let (Some(start), Some(end)) = (
                    crate::position::position_to_offset(text, r.start),
                    crate::position::position_to_offset(text, r.end),
                ) else {
                    return Vec::new();
                };
                Some(bynk_syntax::span::Span::new(start, end))
            }
        };
        let lt = analysis
            .locals
            .get(&rel)
            .map(|l| crate::locals_nav::local_token_sites(l, text))
            .unwrap_or_default();
        // v0.140 (ADR 0163): handler-annotation spans (`@cache` name + argument
        // labels), classified as `decorator`. Parsed from the snapshot here, off
        // the index-read path (mirroring how locals are precomputed).
        let dt = crate::symbols::handler_annotation_token_spans(text);
        crate::index_queries::semantic_tokens(&analysis.index, &lt, &dt, &rel, text, span)
    }

    /// The (analysis, rel-path, snapshot byte offset) for a request
    /// position — the shared front half of every index-backed handler.
    async fn index_position(
        &self,
        uri: &Url,
        position: Position,
    ) -> Option<(Arc<Analysis>, PathBuf, usize)> {
        // Slice B: `analysis_for` guarantees the round analysed `uri`'s current
        // version, so `position_to_offset` resolves against the same text the
        // client's position refers to — the `fresh` flag every caller used to
        // pass is gone (freshness is the contract now, not a per-call choice).
        let analysis = self.analysis_for(uri).await?;
        let rel = Self::uri_to_rel(&analysis, uri)?;
        let text = analysis.snapshots.get(&rel)?;
        let offset = crate::position::position_to_offset(text, position)?;
        Some((analysis, rel, offset))
    }

    /// Locate the AST node at the given cursor position by re-parsing the
    /// document. Returns the textual identifier (if any) and its span.
    /// Used by hover and definition handlers.
    async fn identifier_at(
        &self,
        uri: &Url,
        position: Position,
    ) -> Option<(String, bynk_syntax::span::Span, String)> {
        let text = {
            let state = self.state.read().await;
            state.docs.get(uri)?.text.clone()
        };
        let offset = crate::position::position_to_offset(&text, position)?;
        // Hole-aware (issue #473): interpolation holes are expanded so a cursor
        // inside `"… \(name) …"` lands on the hole's identifier token, not the
        // opaque `InterpStr` token.
        let tokens = bynk_syntax::lexer::tokenize_expanding_holes(&text).ok()?;
        // Find the token whose span covers `offset`.
        for t in &tokens {
            if t.span.start <= offset
                && offset < t.span.end
                && matches!(
                    t.kind,
                    bynk_syntax::lexer::TokenKind::Ident
                        | bynk_syntax::lexer::TokenKind::Int
                        | bynk_syntax::lexer::TokenKind::String
                        | bynk_syntax::lexer::TokenKind::Bool
                        | bynk_syntax::lexer::TokenKind::Float
                        | bynk_syntax::lexer::TokenKind::Result
                        | bynk_syntax::lexer::TokenKind::Option
                        | bynk_syntax::lexer::TokenKind::Effect
                )
            {
                let name = text[t.span.start..t.span.end].to_string();
                return Some((name, t.span, text));
            }
        }
        None
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> JsonRpcResult<InitializeResult> {
        // Resolve project root from workspace folders or the first folder URI.
        if let Some(folders) = &params.workspace_folders
            && let Some(first) = folders.first()
            && let Ok(path) = first.uri.to_file_path()
        {
            let mut state = self.state.write().await;
            if let Some((root, config)) = Self::resolve_root(&path) {
                state.config = config;
                state.project_root = Some(root);
            }
        }
        Ok(InitializeResult {
            capabilities: server_capabilities(),
            server_info: Some(ServerInfo {
                name: SERVER_NAME.into(),
                version: Some(SERVER_VERSION.into()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        let root = { self.state.read().await.project_root.clone() };
        match root {
            Some(root) => {
                self.client
                    .log_message(
                        MessageType::INFO,
                        format!("bynkc-lsp: project root at {}", root.display()),
                    )
                    .await;
            }
            None => {
                self.client
                    .log_message(
                        MessageType::INFO,
                        "bynkc-lsp: no bynk.toml found; single-file mode",
                    )
                    .await;
            }
        }
    }

    async fn shutdown(&self) -> JsonRpcResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        {
            let mut state = self.state.write().await;
            // First open in a single-file context may need to set project root
            // — a `bynk.toml` project, or (#485) an implicit `src/` tree.
            if state.project_root.is_none()
                && let Ok(path) = uri.to_file_path()
                && let Some((root, config)) = Self::resolve_root(&path)
            {
                state.config = config;
                state.project_root = Some(root);
            }
            state.docs.insert(
                uri.clone(),
                DocumentState {
                    text: params.text_document.text,
                    version: params.text_document.version,
                },
            );
        }
        self.recompile_and_publish(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        {
            let mut state = self.state.write().await;
            if let Some(doc) = state.docs.get_mut(&uri)
                && let Some(change) = params.content_changes.into_iter().next_back()
            {
                doc.text = change.text;
                doc.version = params.text_document.version;
            }
        }
        // `[lsp] diagnostics_mode = "on_save"`: no per-keystroke rounds — the
        // buffer state is updated above and diagnosis waits for `didSave`.
        let (mode, debounce_ms) = {
            let s = self.state.read().await;
            (s.config.diagnostics_mode, s.config.diagnostics_debounce_ms)
        };
        if mode == crate::project::DiagnosticsMode::OnSave {
            return;
        }
        // Debounce: use the configured value. For simplicity, sleep then
        // recompile. Multiple rapid changes effectively coalesce because
        // each tasks reads the latest text at recompile time.
        let backend = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(debounce_ms)).await;
            backend.recompile_and_publish(&uri).await;
        });
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        // The live path already diagnosed on change; this matters for
        // `diagnostics_mode = "on_save"`, where saves are the only trigger.
        self.recompile_and_publish(&params.text_document.uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let mut state = self.state.write().await;
        state.docs.remove(&uri);
    }

    /// Transport only: resolve the position, gather the round's tables and the
    /// live buffer, and package the result. The resolution *order* — which is the
    /// behaviour — lives in [`crate::hover::hover_content`], so it has one
    /// definition a test can pin (ADR 0190; #611's gap B was a fall-through bug).
    async fn hover(&self, params: HoverParams) -> JsonRpcResult<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        // The analysed round, positioned — absent for a file outside it.
        let positioned = self.index_position(&uri, pos).await;
        // The live buffer — absent when the document is not open. Distinct from
        // the snapshot above, which lags while the user types.
        let doc_text = {
            let state = self.state.read().await;
            state.docs.get(&uri).map(|d| d.text.clone())
        };
        let doc = doc_text
            .as_deref()
            .and_then(|t| Some((t, crate::position::position_to_offset(t, pos)?)));
        let files = self.project_files().await;
        let analysis = positioned
            .as_ref()
            .map(|(a, rel, offset)| crate::hover::HoverAnalysis {
                index: &a.index,
                snapshots: &a.snapshots,
                locals: &a.locals,
                expr_types: &a.expr_types,
                rel,
                offset: *offset,
            });
        let content = crate::hover::hover_content(&crate::hover::HoverInput {
            analysis,
            doc,
            uri: &uri,
            files: files.as_deref(),
        });
        Ok(content.map(|value| Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: None,
        }))
    }

    /// v0.32 (ADR 0065): signature help for the call under the cursor.
    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> JsonRpcResult<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let text = {
            let s = self.state.read().await;
            s.docs.get(&uri).map(|d| d.text.clone())
        };
        let Some(text) = text else { return Ok(None) };
        let offset = cursor_offset(&text, pos);
        let Some(ctx) = crate::signature_help::call_context(&text, offset) else {
            return Ok(None);
        };
        let files = self.project_files().await;
        // Name callees (free fns, statics, capability ops, of/unsafe) — lexical.
        let label = match crate::signature_help::resolve_label(&ctx.callee, &text, files.as_deref())
        {
            Some(l) => Some(l),
            // v0.32 slice 2: a value-receiver method (`xs.fold(`) — type the
            // receiver via the rewrite + re-analyse, then the kernel signature.
            None => match crate::signature_help::value_receiver_method(&ctx.callee) {
                Some((_, method)) => {
                    if let Some((rewritten, recv_offset)) =
                        crate::signature_help::value_receiver_rewrite(
                            &text,
                            &ctx.callee,
                            ctx.open_paren,
                            offset,
                        )
                        && let Some(ty) = self.type_receiver(&uri, rewritten, recv_offset).await
                    {
                        crate::signature_help::kernel_method_signature(&ty, method)
                    } else {
                        None
                    }
                }
                None => None,
            },
        };
        let Some(label) = label else { return Ok(None) };
        let active = ctx.active_param as u32;
        let parameters: Vec<ParameterInformation> = crate::signature_help::param_ranges(&label)
            .into_iter()
            .map(|(s, e)| ParameterInformation {
                label: ParameterLabel::LabelOffsets([s as u32, e as u32]),
                documentation: None,
            })
            .collect();
        Ok(Some(SignatureHelp {
            signatures: vec![SignatureInformation {
                label,
                documentation: None,
                parameters: Some(parameters),
                active_parameter: Some(active),
            }],
            active_signature: Some(0),
            active_parameter: Some(active),
        }))
    }

    /// v0.33 (ADR 0066): a reference-count lens above each top-level definition,
    /// clickable to peek the references. Served from the cached round.
    async fn code_lens(&self, params: CodeLensParams) -> JsonRpcResult<Option<Vec<CodeLens>>> {
        let uri = params.text_document.uri;
        let analysis = self.analysis_for(&uri).await;
        let Some(analysis) = analysis else {
            return Ok(Some(Vec::new()));
        };
        let Some(rel) = Self::uri_to_rel(&analysis, &uri) else {
            return Ok(Some(Vec::new()));
        };
        let Some(text) = analysis.snapshots.get(&rel) else {
            return Ok(Some(Vec::new()));
        };
        // Peek the references/providers on click — a standard client command,
        // so no extension support is required (the client middleware hydrates the
        // three-argument shape). Shared by both the reference and provider lenses.
        let show_references = |range: Range, locations: Vec<Location>, title: String| CodeLens {
            range,
            command: Some(Command {
                title,
                command: "editor.action.showReferences".to_string(),
                arguments: Some(vec![
                    serde_json::to_value(&uri).unwrap_or_default(),
                    serde_json::to_value(range.start).unwrap_or_default(),
                    serde_json::to_value(&locations).unwrap_or_default(),
                ]),
            }),
            data: None,
        };
        let mut lenses: Vec<CodeLens> = crate::index_queries::code_lenses(&analysis.index, &rel)
            .into_iter()
            .map(|(def, refs)| {
                let range = crate::position::span_to_range(text, def.span);
                let locations: Vec<Location> = refs
                    .iter()
                    .filter_map(|r| Self::site_to_location(&analysis, r))
                    .collect();
                let n = refs.len();
                show_references(
                    range,
                    locations,
                    format!("{n} reference{}", if n == 1 { "" } else { "s" }),
                )
            })
            .collect();
        // v0.127 (editor-currency slice 6): a `N provider(s)` lens on each
        // capability, listing the services that `provides` it. Stacks below the
        // reference lens, as a referenced test stacks a reference + test lens.
        lenses.extend(
            crate::index_queries::capability_provider_lenses(&analysis.index, &rel)
                .into_iter()
                .map(|(def, providers)| {
                    let range = crate::position::span_to_range(text, def.span);
                    let locations: Vec<Location> = providers
                        .iter()
                        .filter_map(|r| Self::site_to_location(&analysis, r))
                        .collect();
                    let n = providers.len();
                    show_references(
                        range,
                        locations,
                        format!("{n} provider{}", if n == 1 { "" } else { "s" }),
                    )
                }),
        );
        // v0.129 (#259): a `N refinements of <Base>` lens on each refined/opaque
        // type, listing its family — every type over the same builtin base. Stacks
        // below the reference lens, like the provider lens on a capability.
        lenses.extend(
            crate::index_queries::refinement_family_lenses(&analysis.index, &rel)
                .into_iter()
                .map(|(def, base, family)| {
                    let range = crate::position::span_to_range(text, def.span);
                    let locations: Vec<Location> = family
                        .iter()
                        .filter_map(|r| Self::site_to_location(&analysis, r))
                        .collect();
                    let n = family.len();
                    show_references(
                        range,
                        locations,
                        format!("{n} refinements of {}", base.name()),
                    )
                }),
        );
        Ok(Some(lenses))
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> JsonRpcResult<Option<Vec<CallHierarchyItem>>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some((analysis, rel, offset)) = self.index_position(&uri, pos).await else {
            return Ok(None);
        };
        let Some((key, def)) =
            crate::index_queries::prepare_call_hierarchy(&analysis.index, &rel, offset)
        else {
            return Ok(None);
        };
        Ok(Self::call_hierarchy_item(&analysis, key, def).map(|item| vec![item]))
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> JsonRpcResult<Option<Vec<CallHierarchyIncomingCall>>> {
        let analysis = self.analysis_for(&params.item.uri).await;
        let Some(analysis) = analysis else {
            return Ok(Some(Vec::new()));
        };
        let Some(key) = SerKey::read(&params.item.data) else {
            return Ok(Some(Vec::new()));
        };
        let calls = crate::index_queries::incoming_calls(&analysis.index, &key)
            .into_iter()
            .filter_map(|rel| {
                let from = Self::call_hierarchy_item(&analysis, rel.key, rel.def)?;
                let from_ranges = Self::call_ranges(&analysis, &rel.sites);
                Some(CallHierarchyIncomingCall { from, from_ranges })
            })
            .collect();
        Ok(Some(calls))
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> JsonRpcResult<Option<Vec<CallHierarchyOutgoingCall>>> {
        let analysis = self.analysis_for(&params.item.uri).await;
        let Some(analysis) = analysis else {
            return Ok(Some(Vec::new()));
        };
        let Some(key) = SerKey::read(&params.item.data) else {
            return Ok(Some(Vec::new()));
        };
        let calls = crate::index_queries::outgoing_calls(&analysis.index, &key)
            .into_iter()
            .filter_map(|rel| {
                let to = Self::call_hierarchy_item(&analysis, rel.key, rel.def)?;
                let from_ranges = Self::call_ranges(&analysis, &rel.sites);
                Some(CallHierarchyOutgoingCall { to, from_ranges })
            })
            .collect();
        Ok(Some(calls))
    }

    /// v0.35 (ADR 0068): `textDocument/implementation` — on a capability
    /// symbol (its declaration, a `given Cap` use, or a `provides Cap` use),
    /// the providers that implement it. `None` for any other symbol (the
    /// reverse, provider → capability, is served by goto-definition).
    async fn goto_implementation(
        &self,
        params: GotoImplementationParams,
    ) -> JsonRpcResult<Option<GotoImplementationResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some((analysis, rel, offset)) = self.index_position(&uri, pos).await else {
            return Ok(None);
        };
        let Some((key, _)) = analysis.index.symbol_at(&rel, offset) else {
            return Ok(None);
        };
        if key.kind != bynk_check::index::SymbolKind::Capability {
            return Ok(None);
        }
        let locations: Vec<Location> = crate::index_queries::implementations(&analysis.index, key)
            .into_iter()
            .filter_map(|d| Self::site_to_location(&analysis, d))
            .collect();
        if locations.is_empty() {
            return Ok(None);
        }
        Ok(Some(GotoDefinitionResponse::Array(locations)))
    }

    /// Slice 6: `textDocument/typeDefinition` — from a value at the cursor to the
    /// definition of its (user-declared) type. Reads the value's type from the
    /// round's `expr_types`, unwraps it to a `Named` target, and returns that
    /// type's definition site(s). `None` for a built-in/function/actor type, or
    /// a cursor not on a typed expression in a clean round.
    async fn goto_type_definition(
        &self,
        params: GotoTypeDefinitionParams,
    ) -> JsonRpcResult<Option<GotoTypeDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some((analysis, rel, offset)) = self.index_position(&uri, pos).await else {
            return Ok(None);
        };
        let Some(entries) = analysis.expr_types.get(&rel) else {
            return Ok(None);
        };
        let Some(ty) = bynk_check::expr_types::type_at_offset(entries, offset) else {
            return Ok(None);
        };
        let Some(name) = crate::index_queries::named_type_target(ty) else {
            return Ok(None);
        };
        let locations: Vec<Location> =
            crate::index_queries::type_definitions_named(&analysis.index, name)
                .into_iter()
                .filter_map(|d| Self::site_to_location(&analysis, d))
                .collect();
        if locations.is_empty() {
            return Ok(None);
        }
        Ok(Some(GotoDefinitionResponse::Array(locations)))
    }

    /// Slice 6b (ADR 0095): `textDocument/documentLink` — `uses`/`consumes` unit
    /// names are clickable to the unit's source. Spans come from parsing the live
    /// buffer; the target is the unit's first source file from the round's
    /// unit→source map. A first-party `uses` (embedded, no on-disk file) or an
    /// unresolved unit yields no link.
    async fn document_link(
        &self,
        params: DocumentLinkParams,
    ) -> JsonRpcResult<Option<Vec<DocumentLink>>> {
        let uri = params.text_document.uri;
        let analysis = self.analysis_for(&uri).await;
        let text = self
            .state
            .read()
            .await
            .docs
            .get(&uri)
            .map(|d| d.text.clone());
        let (Some(text), Some(analysis)) = (text, analysis) else {
            return Ok(None);
        };
        let links: Vec<DocumentLink> = crate::symbols::unit_reference_spans(&text)
            .into_iter()
            .filter_map(|(unit, span)| {
                let rel = analysis.unit_sources.get(&unit)?.first()?;
                let target = Url::from_file_path(analysis.project_root.join(rel)).ok()?;
                Some(DocumentLink {
                    range: crate::position::span_to_range(&text, span),
                    target: Some(target),
                    tooltip: Some(format!("Open unit `{unit}`")),
                    data: None,
                })
            })
            .collect();
        Ok((!links.is_empty()).then_some(links))
    }

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> JsonRpcResult<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let text = {
            let s = self.state.read().await;
            s.docs.get(&uri).map(|d| d.text.clone())
        };
        let Some(text) = text else { return Ok(None) };
        let offset = cursor_offset(&text, pos);
        // The line up to the cursor — the context the completion keys off.
        // Derived from the converted offset (always a char boundary), not by
        // slicing the line at `pos.character` bytes.
        let line_prefix = text[..offset].rsplit('\n').next().unwrap_or("").to_string();
        let files = self.project_files().await;
        let candidates = completion::complete(&line_prefix, &text, files.as_deref());
        let mut items: Vec<CompletionItem> =
            candidates.into_iter().map(to_completion_item).collect();
        // ADR 0064/0093 D3: offer in-scope locals/params at keyword position
        // (alongside keywords) and at expression position (alongside the
        // constructors + type names `complete()` now yields there). Both are
        // places a value or name can begin; the two positions are disjoint.
        if completion::is_keyword_position(&line_prefix)
            || completion::is_expression_position(&line_prefix)
        {
            items.extend(self.locals_completions(&uri, pos).await);
        }
        // v0.124 (slice 3): inside a `requires`/`ensures` predicate, offer the
        // enclosing function's parameters (and `result` in an `ensures`),
        // merged with whatever the lexical cell yields there — the same
        // append-in-scope-names posture as locals above.
        items.extend(contract_param_completions(&text, offset, &line_prefix));
        // v0.131: inside a `cors { }` block, offer the policy field names; at a
        // service-body item start, offer the `cors` section keyword alongside the
        // handler-kind keywords the keyword-position cell already yields.
        items.extend(cors_completions(&text, offset, &line_prefix));
        // v0.141 (ADR 0164): inside a `security { }` block, offer the policy field
        // names; at a service-body item start, offer the `security` section keyword.
        items.extend(security_completions(&text, offset, &line_prefix));
        // v0.140 (ADR 0163): inside `@cache( … )`, offer the annotation argument
        // names; at a service-body item start, offer the `@cache` snippet alongside
        // the `cors` keyword and handler kinds.
        items.extend(cache_completions(&text, offset, &line_prefix));
        // v0.142 (ADR 0165): inside a `limits { }` block, offer the policy field
        // names; at a service-body item start, offer the `limits` section keyword.
        items.extend(limits_completions(&text, offset, &line_prefix));
        // v0.142 (ADR 0165): inside `@limit( … )`, offer the annotation argument
        // names; at a service-body item start, offer the `@limit` snippet.
        items.extend(limit_completions(&text, offset, &line_prefix));
        // v0.128: at a `match` arm-pattern-start, prepend the scrutinee's
        // variants — the most relevant candidate there. Unlike an `is` position, a
        // fresh-line or after-comma arm already looks like a keyword/expression
        // position (so `items` is non-empty and the `is_empty` path below never
        // fires), hence the merge. The expensive scrutinee typing is gated behind
        // the cheap lexical `match_scrutinee_offset` check inside, so ordinary
        // keyword-position completion pays only a string scan.
        // v0.145 (ADR 0169): a nested constructor position (`Some(‸`) offers the
        // payload type's variants; it and the arm-start position are mutually
        // exclusive (one is inside a `(`, the other before any), so the two lists
        // never overlap. Nested is the more specific position, so it leads.
        let mut pattern_items = self.nested_pattern_completions(&uri, &text, offset).await;
        pattern_items.extend(self.match_arm_completions(&uri, &text, offset).await);
        if !pattern_items.is_empty() {
            let mut merged = pattern_items;
            merged.extend(items);
            stamp_resolve_data(&mut merged, &uri);
            return Ok(Some(CompletionResponse::Array(merged)));
        }
        if items.is_empty() {
            // Slice 3: `<expr> is <cursor>` — offer the scrutinee sum type's
            // variants, resolved from `expr_types` (the ADR 0063 ceiling).
            let is_items = self.is_pattern_completions(&uri, &text, offset).await;
            if !is_items.is_empty() {
                return Ok(Some(CompletionResponse::Array(is_items)));
            }
            // A lowercase `receiver.` is a value receiver — type it by
            // re-analysing the rewritten buffer and offer its members. (Value
            // members name no declared symbol, so they carry no resolve data.)
            let value_items = self.value_member_completions(&uri, &text, offset).await;
            return Ok((!value_items.is_empty()).then_some(CompletionResponse::Array(value_items)));
        }
        // Slice 5: stash the doc URI so `completion_resolve` can attach lazy docs.
        stamp_resolve_data(&mut items, &uri);
        Ok(Some(CompletionResponse::Array(items)))
    }

    /// Slice 5: fill in hover-quality `documentation` for the focused completion
    /// item, reusing the hover renderer (`symbols::describe_symbol`, local then
    /// cross-file — §3.4). The originating doc URI is read from the item's
    /// `data` (a resolve request carries only the item, not a position). A no-op
    /// for an item that names no declared symbol (a keyword, kernel method, or
    /// local) — its one-line `detail` already suffices.
    async fn completion_resolve(&self, mut item: CompletionItem) -> JsonRpcResult<CompletionItem> {
        if item.documentation.is_some() {
            return Ok(item);
        }
        let Some(uri) = item
            .data
            .as_ref()
            .and_then(|d| d.get("uri"))
            .and_then(serde_json::Value::as_str)
            .and_then(|s| Url::parse(s).ok())
        else {
            return Ok(item);
        };
        let local = {
            let s = self.state.read().await;
            s.docs.get(&uri).map(|d| d.text.clone())
        };
        let doc = match local
            .as_deref()
            .and_then(|t| crate::symbols::describe_symbol(t, &item.label))
        {
            Some(md) => Some(md),
            None => self
                .project_files()
                .await
                .and_then(|files| {
                    crate::symbols::describe_symbol_cross_file(&files, &uri, &item.label)
                })
                .map(|(_uri, md)| md)
                // Slice 9: stdlib/surface symbols (e.g. a `uses bynk.list` combinator)
                // live in the embedded first-party sources, not the project's files.
                .or_else(|| crate::symbols::describe_firstparty_symbol(&item.label)),
        };
        if let Some(md) = doc {
            item.documentation = Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: md,
            }));
        }
        Ok(item)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> JsonRpcResult<Option<GotoDefinitionResponse>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
        let pos = params.text_document_position_params.position;
        // v0.25 rider: binding-correct definition via the index (fixes the
        // name-collision mis-navigation of the string-matching path). The
        // legacy path remains as fallback for not-yet-indexed symbol kinds
        // (locals, methods, fields, ops).
        if let Some((analysis, rel, offset)) = self.index_position(&uri, pos).await {
            if let Some((_, def)) =
                crate::index_queries::definition_at(&analysis.index, &rel, offset)
                && let Some(location) = Self::site_to_location(&analysis, def)
            {
                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }
            // v0.31: a local binding — scope-correct definition (before the
            // string-matching fallback, which can't tell scopes apart).
            if let Some(text) = analysis.snapshots.get(&rel)
                && let Some(locals) = analysis.locals.get(&rel)
                && let Some(def) = crate::locals_nav::local_definition_at(locals, text, offset)
                && let Some(location) = self
                    .local_locations(&analysis, &rel, &[def])
                    .into_iter()
                    .next()
            {
                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }
        }
        // Slice 6a follow-up (ADR 0095): the cursor on a `uses`/`consumes` unit
        // name jumps to that unit's source. Units aren't index symbols, so the
        // unit→source map resolves them; runs before the name-matching path so a
        // unit segment can't be mistaken for a like-named type.
        if let Some(location) = self.unit_reference_definition(&uri, pos).await {
            return Ok(Some(GotoDefinitionResponse::Scalar(location)));
        }
        let Some((name, _span, text)) = self.identifier_at(&uri, pos).await else {
            return Ok(None);
        };
        if let Some(decl_span) = crate::symbols::find_declaration_span(&text, &name) {
            let range = crate::position::span_to_range(&text, decl_span);
            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                uri,
                range,
            })));
        }
        // Cross-file fallback (v1.1; LSP spec §3.4).
        if let Some(files) = self.project_files().await
            && let Some(found) = crate::symbols::find_declaration_cross_file(&files, &uri, &name)
        {
            let range = crate::position::span_to_range(&found.source, found.span);
            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                uri: found.uri,
                range,
            })));
        }
        Ok(None)
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> JsonRpcResult<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let text = {
            let s = self.state.read().await;
            s.docs.get(&uri).map(|d| d.text.clone())
        };
        let Some(text) = text else { return Ok(None) };
        let opts = {
            let s = self.state.read().await;
            s.config.format_options()
        };
        match bynk_fmt::format_source(&text, &opts) {
            Ok(formatted) => {
                if formatted == text {
                    Ok(Some(Vec::new()))
                } else {
                    // Replace the entire document.
                    let end_pos = crate::position::end_position(&text);
                    Ok(Some(vec![TextEdit {
                        range: Range {
                            start: Position::new(0, 0),
                            end: end_pos,
                        },
                        new_text: formatted,
                    }]))
                }
            }
            Err(_) => {
                // Formatting failed (parse error). Return no edits; the
                // diagnostics flow will surface the parse error.
                Ok(Some(Vec::new()))
            }
        }
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> JsonRpcResult<Option<Vec<TextEdit>>> {
        // Best-effort: format the whole document. Per spec, range
        // formatting may return edits wider than the requested range.
        self.formatting(DocumentFormattingParams {
            text_document: params.text_document,
            options: params.options,
            work_done_progress_params: params.work_done_progress_params,
        })
        .await
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> JsonRpcResult<Option<DocumentSymbolResponse>> {
        // v1.1 — outline view + Cmd-Shift-O. See `design/bynk-lsp-spec.md` §3.7.
        let uri = params.text_document.uri;
        let text = {
            let s = self.state.read().await;
            s.docs.get(&uri).map(|d| d.text.clone())
        };
        let Some(text) = text else { return Ok(None) };
        let syms = crate::document_symbols::outline(&text);
        if syms.is_empty() {
            return Ok(None);
        }
        Ok(Some(DocumentSymbolResponse::Nested(syms)))
    }

    /// v0.37 (ADR 0070): `textDocument/foldingRange` — structural folds + comment
    /// runs from the recovered AST (no analysis round).
    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> JsonRpcResult<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri;
        let text = {
            let s = self.state.read().await;
            s.docs.get(&uri).map(|d| d.text.clone())
        };
        let Some(text) = text else { return Ok(None) };
        Ok(Some(crate::structure::folding_ranges(&text)))
    }

    /// v0.37 (ADR 0070): `textDocument/selectionRange` — the enclosing-node
    /// chain (innermost first) for each requested position.
    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> JsonRpcResult<Option<Vec<SelectionRange>>> {
        let uri = params.text_document.uri;
        let text = {
            let s = self.state.read().await;
            s.docs.get(&uri).map(|d| d.text.clone())
        };
        let Some(text) = text else { return Ok(None) };
        Ok(Some(crate::structure::selection_ranges(
            &text,
            &params.positions,
        )))
    }

    async fn references(&self, params: ReferenceParams) -> JsonRpcResult<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let Some((analysis, rel, offset)) = self.index_position(&uri, pos).await else {
            return Ok(None);
        };
        let include_decl = params.context.include_declaration;
        if let Some(sites) =
            crate::index_queries::sites_for(&analysis.index, &rel, offset, include_decl)
        {
            let locations: Vec<Location> = sites
                .into_iter()
                .filter_map(|site| Self::site_to_location(&analysis, site))
                .collect();
            return Ok(Some(locations));
        }
        // v0.31: a local binding — its def + uses, resolved from the snapshot.
        if let Some(spans) = self.local_sites(&analysis, &rel, offset) {
            let spans = if include_decl {
                &spans[..]
            } else {
                &spans[1..]
            }; // def first
            let locations = self.local_locations(&analysis, &rel, spans);
            return Ok(Some(locations));
        }
        Ok(None)
    }

    /// v0.26 (ADR 0054): quick-fixes from structured suggestions. Served
    /// from the **cached** analysis round only (never a fresh run — slow,
    /// and it could disagree with the squiggles the client is showing): a
    /// request before the first round, or for a file outside the project,
    /// returns the empty list.
    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> JsonRpcResult<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let analysis = self.analysis_for(&uri).await;
        let Some(analysis) = analysis else {
            return Ok(Some(Vec::new()));
        };
        let Some(rel) = Self::uri_to_rel(&analysis, &uri) else {
            return Ok(Some(Vec::new()));
        };
        let (Some(text), Some(diags)) =
            (analysis.snapshots.get(&rel), analysis.diagnostics.get(&rel))
        else {
            return Ok(Some(Vec::new()));
        };
        // The request range converts against the analysed snapshot (the
        // v0.24 rule), like the spans it is intersected with.
        let (Some(start), Some(end)) = (
            crate::position::position_to_offset(text, params.range.start),
            crate::position::position_to_offset(text, params.range.end),
        ) else {
            return Ok(Some(Vec::new()));
        };
        let actions = crate::code_actions::quick_fixes(
            text,
            diags,
            bynk_syntax::span::Span::new(start, end),
            &uri,
            analysis.versions.get(&rel).copied(),
        );
        Ok(Some(actions))
    }

    /// v0.27 (ADR 0056): inferred-type inlay hints for the visible range,
    /// served from the cached round only — no cached round (pre-first-
    /// analysis, non-project file) returns the empty list. Positions
    /// convert against the analysed snapshot (the v0.24 rule).
    async fn inlay_hint(&self, params: InlayHintParams) -> JsonRpcResult<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri;
        let analysis = self.analysis_for(&uri).await;
        let Some(analysis) = analysis else {
            return Ok(Some(Vec::new()));
        };
        let Some(rel) = Self::uri_to_rel(&analysis, &uri) else {
            return Ok(Some(Vec::new()));
        };
        let Some(text) = analysis.snapshots.get(&rel) else {
            return Ok(Some(Vec::new()));
        };
        // The visible range converts against the analysed snapshot, like
        // the hint spans it is intersected with.
        let (Some(start), Some(end)) = (
            crate::position::position_to_offset(text, params.range.start),
            crate::position::position_to_offset(text, params.range.end),
        ) else {
            return Ok(Some(Vec::new()));
        };
        let visible = bynk_syntax::span::Span::new(start, end);
        // v0.27: inferred-type hints. v0.99: plus the materializable ghost
        // `given` hints for uncovered capability requirements. A file may carry
        // one set without the other, so each defaults to empty independently.
        let mut hints = analysis
            .hints
            .get(&rel)
            .map(|h| crate::inlay_hints::inlay_hints(text, h, visible))
            .unwrap_or_default();
        if let Some(reqs) = analysis.requirements.get(&rel) {
            hints.extend(crate::inlay_hints::given_hints(text, reqs, visible));
        }
        Ok(Some(hints))
    }

    /// v0.28 (ADR 0057): semantic tokens for the whole document, served
    /// from the cached round only (no cached round / non-project file →
    /// empty), positions against the analysed snapshot (the v0.24 rule).
    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> JsonRpcResult<Option<SemanticTokensResult>> {
        let data = self
            .semantic_tokens_for(&params.text_document.uri, None)
            .await;
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        })))
    }

    /// v0.28 (ADR 0057): the `…/range` variant — the same pure read,
    /// filtered to tokens overlapping the requested range.
    async fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> JsonRpcResult<Option<SemanticTokensRangeResult>> {
        let data = self
            .semantic_tokens_for(&params.text_document.uri, Some(params.range))
            .await;
        Ok(Some(SemanticTokensRangeResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        })))
    }

    /// v0.26 rider (ADR 0055): project-wide symbol search — the index's
    /// definitions, filtered by the query.
    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> JsonRpcResult<Option<Vec<SymbolInformation>>> {
        let Some(analysis) = self.ensure_analysis().await else {
            return Ok(None);
        };
        let matches = crate::index_queries::workspace_symbols(&analysis.index, &params.query);
        let symbols: Vec<SymbolInformation> = matches
            .into_iter()
            .filter_map(|(key, def)| {
                let location = Self::site_to_location(&analysis, def)?;
                #[allow(deprecated)]
                Some(SymbolInformation {
                    name: key.name.clone(),
                    kind: lsp_symbol_kind(key.kind),
                    tags: None,
                    deprecated: None,
                    location,
                    container_name: Some(key.unit.clone()),
                })
            })
            .collect();
        Ok(Some(symbols))
    }

    /// v0.26 rider (ADR 0055): the symbol-at-cursor's occurrences in the
    /// active file. `kind` is omitted — the index does not distinguish read
    /// from write references.
    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> JsonRpcResult<Option<Vec<DocumentHighlight>>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some((analysis, rel, offset)) = self.index_position(&uri, pos).await else {
            return Ok(None);
        };
        let Some(text) = analysis.snapshots.get(&rel) else {
            return Ok(None);
        };
        if let Some(sites) =
            crate::index_queries::document_highlights(&analysis.index, &rel, offset)
        {
            let highlights: Vec<DocumentHighlight> = sites
                .into_iter()
                .map(|s| DocumentHighlight {
                    range: crate::position::span_to_range(text, s.span),
                    kind: None,
                })
                .collect();
            return Ok(Some(highlights));
        }
        // v0.31: a local binding's occurrences (def + uses) in the file.
        if let Some(spans) = self.local_sites(&analysis, &rel, offset) {
            let highlights = spans
                .iter()
                .map(|s| DocumentHighlight {
                    range: crate::position::span_to_range(text, *s),
                    kind: None,
                })
                .collect();
            return Ok(Some(highlights));
        }
        Ok(None)
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> JsonRpcResult<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri;
        let pos = params.position;
        // Refuse (None) for anything the index does not cover — locals,
        // methods, record fields, capability ops, unit names — rather than
        // falling through to a partial or name-matched rename.
        let Some((analysis, rel, offset)) = self.index_position(&uri, pos).await else {
            return Ok(None);
        };
        let Some((key, site)) = crate::index_queries::prepare_rename(&analysis.index, &rel, offset)
        else {
            return Ok(None);
        };
        let Some(text) = analysis.snapshots.get(&rel) else {
            return Ok(None);
        };
        Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
            range: crate::position::span_to_range(text, site.span),
            placeholder: key.name.clone(),
        }))
    }

    async fn rename(&self, params: RenameParams) -> JsonRpcResult<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let new_name = params.new_name;
        let refused = |msg: String| tower_lsp::jsonrpc::Error {
            code: tower_lsp::jsonrpc::ErrorCode::InvalidParams,
            message: msg.into(),
            data: None,
        };
        // Plan against a *fresh* analysis of the current buffers, so the
        // edits and the captured versions describe live state.
        let Some((analysis, rel, offset)) = self.index_position(&uri, pos).await else {
            return Err(refused("rename requires a project (bynk.toml)".into()));
        };
        let plan = crate::index_queries::plan_rename(&analysis.index, &rel, offset, &new_name)
            .map_err(refused)?;

        // Validator 1 + 2 input: re-analyse with the edits applied. Every
        // snapshot is pinned via the overlay so the re-analysis differs from
        // the plan's baseline only by the edits themselves.
        //
        // Slice A: this must re-analyse over the **same roots** the baseline
        // round used (`AnalysisRoots::Project`, manifest-aware), not the
        // single-tree `diagnose_project`. `diagnose_project(project_root)`
        // resolves to `Roots::Single`, which walks the whole tree with **no
        // `exclude`** and no `out`/`node_modules` skip — so `post` would cover a
        // superset of the baseline's files, and validators 1 and 2 (which
        // compare `post` against baselines from the manifest-aware round) would
        // read a diagnostic or index site in an excluded tree as *new* and
        // refuse a valid rename.
        let mut overlay = std::collections::HashMap::new();
        for (rel_path, text) in &analysis.snapshots {
            let edited = match plan.edits.get(rel_path) {
                Some(spans) => crate::index_queries::apply_edits(text, spans, &plan.new_name),
                None => text.clone(),
            };
            let abs = analysis.project_root.join(rel_path);
            let abs = abs.canonicalize().unwrap_or(abs);
            overlay.insert(abs, edited);
        }
        let roots = bynk_ide::AnalysisRoots::Project(analysis.project_root.clone());
        let Ok(post) =
            tokio::task::spawn_blocking(move || bynk_ide::diagnose_project_with(&roots, &overlay))
                .await
        else {
            return Err(refused("rename validation failed to run".into()));
        };

        // Validator 1 — collisions: refuse on any new diagnostic.
        let post_diags: Vec<(PathBuf, String)> = post
            .files
            .iter()
            .flat_map(|f| {
                f.diagnostics
                    .iter()
                    .map(|d| (f.source_path.clone(), d.error.category.to_string()))
            })
            .collect();
        crate::index_queries::no_new_diagnostics(&analysis.diag_categories(), &post_diags)
            .map_err(refused)?;

        // Validator 2 — capture/escape: the re-built index must be the old
        // index modulo the rename; a silent re-binding has no diagnostic.
        if !crate::index_queries::index_unchanged_modulo_rename(&analysis.index, &post.index, &plan)
        {
            return Err(refused(format!(
                "renaming `{}` to `{new_name}` would silently re-bind another name — refused",
                plan.key.name
            )));
        }

        // Versioned edits: the client rejects the rename if a buffer drifted
        // past the analysed version rather than mis-applying it.
        let mut document_edits: Vec<TextDocumentEdit> = Vec::new();
        for (rel_path, spans) in &plan.edits {
            let Some(text) = analysis.snapshots.get(rel_path) else {
                continue;
            };
            let abs = analysis.project_root.join(rel_path);
            let Ok(file_uri) = Url::from_file_path(&abs) else {
                continue;
            };
            let edits: Vec<OneOf<TextEdit, AnnotatedTextEdit>> = spans
                .iter()
                .map(|span| {
                    OneOf::Left(TextEdit {
                        range: crate::position::span_to_range(text, *span),
                        new_text: plan.new_name.clone(),
                    })
                })
                .collect();
            document_edits.push(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: file_uri,
                    version: analysis.versions.get(rel_path).copied(),
                },
                edits,
            });
        }
        Ok(Some(WorkspaceEdit {
            changes: None,
            document_changes: Some(DocumentChanges::Edits(document_edits)),
            change_annotations: None,
        }))
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        // For every changed `.bynk` file we have open, refresh diagnostics.
        // Changes to files we do *not* have open (a git checkout, an external
        // edit) still invalidate the project index — schedule a project round
        // so cross-file state doesn't go stale (#513).
        let mut uris_to_refresh = Vec::new();
        let mut non_open_change = false;
        // A `bynk.toml` edit changes the formatting style, the diagnostics
        // mode/debounce, and the source root — none of which were re-read after
        // the initial load, so the settings only took effect on an LSP restart.
        // Detect the change here and reload the config before re-analysing.
        let mut config_changed = false;
        {
            let state = self.state.read().await;
            for ev in &params.changes {
                if is_bynk_toml(&ev.uri) {
                    config_changed = true;
                } else if state.docs.contains_key(&ev.uri) {
                    uris_to_refresh.push(ev.uri.clone());
                } else if ev.uri.path().ends_with(".bynk") {
                    non_open_change = true;
                }
            }
        }
        if config_changed {
            self.reload_config().await;
        }
        for uri in uris_to_refresh {
            self.recompile_and_publish(&uri).await;
        }
        // A reloaded config re-derives the analysis root and diagnostics
        // behaviour, so re-analyse the whole project against it — same
        // debounced round the non-open `.bynk` change schedules.
        if config_changed || non_open_change {
            self.schedule_project_diagnostics().await;
        }
    }
}

/// The advertised capability set — `design/bynk-lsp-spec.md` §4.3. Split out
/// of `initialize` so the advertisement is unit-testable without transport.
fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        // Full-text sync, with save notifications explicitly opted in — the
        // `on_save` diagnostics mode is driven by `didSave` (#513).
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                ..Default::default()
            },
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        // v0.17: completion for `consumes` units and `given` /
        // `consumes U { … }` capabilities. Trigger on the space after a
        // keyword, the `{` of a selected-capability list, and `,`. The `.`
        // auto-fires the name- and value-receiver member contexts (ADR 0093 D1).
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![
                " ".to_string(),
                "{".to_string(),
                ",".to_string(),
                ".".to_string(),
            ]),
            // Slice 5: resolve fills in hover-quality `documentation` lazily, on
            // the focused item only, so the initial list stays cheap.
            resolve_provider: Some(true),
            ..Default::default()
        }),
        // v0.32 (ADR 0065): signature help while typing a call's arguments.
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
            retrigger_characters: Some(vec![",".to_string()]),
            ..Default::default()
        }),
        // v0.33 (ADR 0066): reference-count lenses above top-level definitions.
        code_lens_provider: Some(CodeLensOptions {
            resolve_provider: Some(false),
        }),
        // v0.34 (ADR 0067): call hierarchy over the binding index's call graph.
        call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
        // v0.35 (ADR 0068): implementation nav — capability → its providers.
        implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
        // Slice 6: go-to-type-definition (value → its type's declaration).
        type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
        // Slice 6b: `uses`/`consumes` unit names link to their source.
        document_link_provider: Some(DocumentLinkOptions {
            resolve_provider: Some(false),
            work_done_progress_options: Default::default(),
        }),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        // v0.37 (ADR 0070): structural folding + selection ranges (AST-driven).
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
        // v0.25 (ADR 0053): references + rename over the binding
        // index; prepareRename refuses out-of-scope symbols.
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        // v0.26 (ADR 0054): quick-fixes from the diagnostics' structured
        // suggestions.
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
            ..Default::default()
        })),
        // v0.27 (ADR 0056): inferred-type inlay hints from the retained
        // analysis round's harvested hint set.
        inlay_hint_provider: Some(OneOf::Left(true)),
        // v0.28 (ADR 0057): semantic tokens over the frozen legend — a
        // pure read of the cached index (`symbols` + `foreign_refs`),
        // additive over the client's syntactic layer. `delta` deferred.
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: crate::index_queries::semantic_tokens_legend(),
                full: Some(SemanticTokensFullOptions::Bool(true)),
                range: Some(true),
                ..Default::default()
            },
        )),
        // v0.26 riders (ADR 0055): both are `ProjectIndex` queries.
        workspace_symbol_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        workspace: Some(WorkspaceServerCapabilities {
            workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                supported: Some(true),
                change_notifications: Some(OneOf::Left(true)),
            }),
            file_operations: None,
        }),
        ..Default::default()
    }
}

/// Index symbol kind → LSP symbol kind, aligned with the document-symbol
/// outline's choices (capability=INTERFACE, service/agent=CLASS,
/// provider=OBJECT). The index does not distinguish type shapes, so every
/// type maps to STRUCT.
/// Map a `completion::Completion` to an LSP `CompletionItem`.
/// Stash the document URI in each item's `data` so `completion_resolve` can look
/// the symbol up — a resolve request carries only the item, not a position.
fn stamp_resolve_data(items: &mut [CompletionItem], uri: &Url) {
    let data = serde_json::json!({ "uri": uri.to_string() });
    for item in items.iter_mut() {
        item.data = Some(data.clone());
    }
}

/// v0.124 (slice 3): the enclosing function's parameters (and `result` for an
/// `ensures`) as completions, when `offset` sits in a `requires`/`ensures`
/// predicate. Empty when not in a contract clause or no enclosing `fn` is
/// found. A pure parse — the params are read straight off the recovered AST.
/// v0.131: the CORS completion cells. Inside a `cors { }` block at a field-name
/// position, offer the closed field set; at a service-body item start, offer the
/// `cors` section keyword. Both are lexical (offset-based), matching the
/// `contract_param_completions` posture.
fn cors_completions(text: &str, offset: usize, line: &str) -> Vec<CompletionItem> {
    if completion::in_cors_field_position(text, offset) {
        return completion::CORS_FIELDS
            .iter()
            .map(|(name, doc)| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some((*doc).to_string()),
                insert_text: Some(format!("{name}: ")),
                ..Default::default()
            })
            .collect();
    }
    if completion::in_service_body_item_position(text, offset, line) {
        return vec![CompletionItem {
            label: "cors".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("a cross-origin (CORS) policy for this HTTP service".to_string()),
            insert_text: Some("cors {\n\torigins: [$0],\n}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        }];
    }
    Vec::new()
}

/// v0.141 (ADR 0164): the security-headers completion cells. Inside a
/// `security { }` block at a field-name position, offer the closed field set
/// (`nosniff`/`hsts`); at a service-body item start, offer the `security` section
/// keyword. Both are lexical (offset-based), mirroring `cors_completions`.
fn security_completions(text: &str, offset: usize, line: &str) -> Vec<CompletionItem> {
    if completion::in_security_field_position(text, offset) {
        return completion::SECURITY_FIELDS
            .iter()
            .map(|(name, doc)| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some((*doc).to_string()),
                insert_text: Some(format!("{name}: ")),
                ..Default::default()
            })
            .collect();
    }
    if completion::in_service_body_item_position(text, offset, line) {
        return vec![CompletionItem {
            label: "security".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("security response headers for this HTTP service".to_string()),
            insert_text: Some("security {\n\tnosniff: $0,\n}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        }];
    }
    Vec::new()
}

/// v0.140 (ADR 0163): the `@cache` completion cells. Inside `@cache( … )` at an
/// argument-name position, offer the closed argument set (`maxAge`/`scope`); at a
/// service-body item start, offer the `@cache` annotation snippet. Both are lexical
/// (offset-based), mirroring `cors_completions`.
fn cache_completions(text: &str, offset: usize, line: &str) -> Vec<CompletionItem> {
    if completion::in_cache_arg_position(text, offset) {
        return completion::CACHE_ARGS
            .iter()
            .map(|(name, doc)| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some((*doc).to_string()),
                insert_text: Some(format!("{name}: ")),
                ..Default::default()
            })
            .collect();
    }
    if completion::in_service_body_item_position(text, offset, line) {
        return vec![CompletionItem {
            label: "@cache".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some(
                "cache a GET read — a synthesised ETag/304 revalidation with a freshness window"
                    .to_string(),
            ),
            insert_text: Some("@cache(maxAge: ${1:5.minutes})".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        }];
    }
    Vec::new()
}

/// v0.142 (ADR 0165): the request-limits completion cells. Inside a `limits { }`
/// block at a field-name position, offer the closed field set (`maxBody`); at a
/// service-body item start, offer the `limits` section keyword. Both are lexical
/// (offset-based), mirroring `security_completions`.
fn limits_completions(text: &str, offset: usize, line: &str) -> Vec<CompletionItem> {
    if completion::in_limits_field_position(text, offset) {
        return completion::LIMITS_FIELDS
            .iter()
            .map(|(name, doc)| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some((*doc).to_string()),
                insert_text: Some(format!("{name}: ")),
                ..Default::default()
            })
            .collect();
    }
    if completion::in_service_body_item_position(text, offset, line) {
        return vec![CompletionItem {
            label: "limits".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("request limits for this HTTP service".to_string()),
            insert_text: Some("limits {\n\tmaxBody: $0,\n}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        }];
    }
    Vec::new()
}

/// v0.142 (ADR 0165): the `@limit` completion cells. Inside `@limit( … )` at an
/// argument-name position, offer the closed argument set (`maxBody`); at a
/// service-body item start, offer the `@limit` annotation snippet. Both are lexical
/// (offset-based), mirroring `cache_completions`.
fn limit_completions(text: &str, offset: usize, line: &str) -> Vec<CompletionItem> {
    if completion::in_limit_arg_position(text, offset) {
        return completion::LIMIT_ARGS
            .iter()
            .map(|(name, doc)| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some((*doc).to_string()),
                insert_text: Some(format!("{name}: ")),
                ..Default::default()
            })
            .collect();
    }
    if completion::in_service_body_item_position(text, offset, line) {
        return vec![CompletionItem {
            label: "@limit".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some(
                "cap the request body size — a `413` synthesised before the body is read"
                    .to_string(),
            ),
            insert_text: Some("@limit(maxBody: ${1:1048576})".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        }];
    }
    Vec::new()
}

fn contract_param_completions(text: &str, offset: usize, line: &str) -> Vec<CompletionItem> {
    use bynk_syntax::ast::{CommonsItem, SourceUnit};
    let Some(is_ensures) = completion::contract_clause_kind(line) else {
        return Vec::new();
    };
    let Ok(tokens) = bynk_syntax::lexer::tokenize(text) else {
        return Vec::new();
    };
    let (Some(unit), _) = bynk_syntax::parser::parse_unit_with_recovery(&tokens, text) else {
        return Vec::new();
    };
    let items = match &unit {
        SourceUnit::Commons(c) => &c.items,
        SourceUnit::Context(c) => &c.items,
        SourceUnit::Adapter(a) => &a.items,
        _ => return Vec::new(),
    };
    for item in items {
        // The cursor sits in a fn's signature/contract region: between the fn's
        // start and the `{` that opens its body.
        if let CommonsItem::Fn(f) = item
            && f.span.start <= offset
            && offset <= f.body.span.start
        {
            // Built directly as VARIABLE items, matching `locals_completions`
            // (in-scope names carry no resolve data).
            let mut out: Vec<CompletionItem> = f
                .params
                .iter()
                .filter(|p| p.name.name != "_")
                .map(|p| CompletionItem {
                    label: p.name.name.clone(),
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail: Some(format!(
                        "parameter: {}",
                        crate::symbols::type_ref_str(&p.type_ref)
                    )),
                    ..Default::default()
                })
                .collect();
            if is_ensures {
                out.push(CompletionItem {
                    label: "result".to_string(),
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail: Some("the function's return value".to_string()),
                    ..Default::default()
                });
            }
            return out;
        }
    }
    Vec::new()
}

/// v0.124 (slice 3): the byte offset of the scrutinee's last character in
/// `<scrutinee> is <partial>` ending at `cursor`, or `None` if the cursor is
/// not at an `is`-pattern position. `is` must be a standalone word (so `basis`
/// does not trigger it).
fn is_scrutinee_offset(text: &str, cursor: usize) -> Option<usize> {
    let before = text.get(..cursor)?;
    // Drop the partial variant being typed, then the whitespace before it.
    let before = before
        .trim_end_matches(|c: char| c.is_alphanumeric() || c == '_')
        .trim_end();
    let before = before.strip_suffix("is")?;
    if !before.ends_with(char::is_whitespace) {
        return None;
    }
    let before = before.trim_end();
    (!before.is_empty()).then(|| before.len() - 1)
}

/// v0.128: the byte offset of the scrutinee's last character in a
/// `match <scrutinee> { … <partial>` whose cursor sits at an **arm-pattern-start**
/// position, or `None` otherwise — the deferred half of slice 3's `is`-pattern
/// completion. Conservative: it fires only at the *start* of an arm's pattern
/// (after the `{` or a top-level `,`, before any `=>`), never inside an arm body
/// or a nested constructor pattern, so it stays honest mid-edit.
fn match_scrutinee_offset(text: &str, cursor: usize) -> Option<usize> {
    let before = text.get(..cursor)?;
    // The innermost `{` still open at the cursor — the block the cursor is in.
    let brace = innermost_open_brace(before)?;
    // The current arm: from the last top-level `,` after the brace (or the brace
    // itself) to the cursor. A `=>` in it means the cursor is in the arm body.
    let arm_start = arm_start_offset(before, brace);
    let arm = before.get(arm_start..)?;
    if arm.contains("=>") {
        return None;
    }
    // Only at the pattern's *start*: nothing but the partial pattern being typed
    // sits between the arm boundary and the cursor.
    if !arm
        .trim_end_matches(|c: char| c.is_alphanumeric() || c == '_')
        .trim()
        .is_empty()
    {
        return None;
    }
    match_head_scrutinee_offset(before, brace)
}

/// v0.145 (ADR 0169): the scrutinee offset and outer variant name at a
/// `match <scrutinee> { … OuterVariant(<partial>` position — the cursor inside a
/// variant's payload parens within an arm-pattern (before `=>`), the one place
/// `match_scrutinee_offset` bails. Conservative: the payload `(` must be still
/// open, the token before it an uppercase-led variant constructor, and only the
/// partial nested pattern may sit between the `(` and the cursor.
fn nested_pattern_offset(text: &str, cursor: usize) -> Option<(usize, String)> {
    let before = text.get(..cursor)?;
    let brace = innermost_open_brace(before)?;
    let arm_start = arm_start_offset(before, brace);
    let arm = before.get(arm_start..)?;
    if arm.contains("=>") {
        return None; // in the arm body, not its pattern
    }
    // The innermost `(` still open in the arm — the outer variant's payload.
    let paren = innermost_open_paren(arm)?;
    // The identifier immediately before that `(` is the outer variant; only an
    // uppercase-led constructor opens a nested pattern (a binding never does).
    let head = arm.get(..paren)?.trim_end();
    let variant: String = head
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if !variant.chars().next().is_some_and(char::is_uppercase) {
        return None;
    }
    // Between the payload `(` and the cursor, only the partial nested pattern
    // being typed (an identifier, optionally a `Type.` qualifier) may sit.
    let after = arm.get(paren + 1..)?;
    if !after
        .trim_start_matches(|c: char| c.is_alphanumeric() || c == '_' || c == '.')
        .trim()
        .is_empty()
    {
        return None;
    }
    let scrut_off = match_head_scrutinee_offset(before, brace)?;
    Some((scrut_off, variant))
}

/// The byte offset of the scrutinee's last character for the `match <scrutinee>`
/// whose body brace is at `brace`, or `None` if `brace` does not head a
/// `match`: a standalone `match` keyword, then a scrutinee expression with no
/// nested block or arrow between it and the brace. Shared by
/// `match_scrutinee_offset` and `nested_pattern_offset`.
fn match_head_scrutinee_offset(before: &str, brace: usize) -> Option<usize> {
    let head = before.get(..brace)?.trim_end();
    let m = head.rfind("match")?;
    if head[..m]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
    {
        return None; // part of a longer identifier (`rematch`), not the keyword
    }
    let after = head.get(m + "match".len()..)?;
    if !after.starts_with(char::is_whitespace) {
        return None;
    }
    let scrut = after.trim();
    if scrut.is_empty() || scrut.contains(['{', '}']) || scrut.contains("=>") {
        return None;
    }
    // `head` was trimmed to end at the scrutinee's last char (the brace followed).
    Some(head.len() - 1)
}

/// The offset (relative to `arm`) of the innermost `(` left unclosed in `arm` — a
/// `(`-only balance scan, the payload paren the cursor sits in — or `None` if
/// every `(` is closed.
fn innermost_open_paren(arm: &str) -> Option<usize> {
    let mut stack: Vec<usize> = Vec::new();
    for (i, c) in arm.char_indices() {
        match c {
            '(' => stack.push(i),
            ')' => {
                stack.pop();
            }
            _ => {}
        }
    }
    stack.pop()
}

/// The byte offset of the innermost `{` left unclosed in `before` (a `{`-only
/// balance scan — the block the cursor sits in), or `None` if every `{` is closed.
fn innermost_open_brace(before: &str) -> Option<usize> {
    let mut stack: Vec<usize> = Vec::new();
    for (i, c) in before.char_indices() {
        match c {
            '{' => stack.push(i),
            '}' => {
                stack.pop();
            }
            _ => {}
        }
    }
    stack.pop()
}

/// The offset just past the last top-level `,` inside the block opened at `brace`
/// (depth 0 relative to that brace), or just past the brace itself if the block
/// holds no top-level comma yet — the start of the arm the cursor is editing.
fn arm_start_offset(before: &str, brace: usize) -> usize {
    let mut depth = 0i32;
    let mut start = brace + 1; // just after the `{`
    for (rel, c) in before[brace + 1..].char_indices() {
        match c {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => start = brace + 1 + rel + c.len_utf8(),
            _ => {}
        }
    }
    start
}

fn to_completion_item(c: completion::Completion) -> CompletionItem {
    CompletionItem {
        kind: Some(match c.kind {
            completion::CompletionKind::Unit => CompletionItemKind::MODULE,
            completion::CompletionKind::Capability => CompletionItemKind::INTERFACE,
            completion::CompletionKind::Type => CompletionItemKind::STRUCT,
            completion::CompletionKind::Keyword => CompletionItemKind::KEYWORD,
            completion::CompletionKind::Snippet => CompletionItemKind::SNIPPET,
            completion::CompletionKind::Variant => CompletionItemKind::ENUM_MEMBER,
            completion::CompletionKind::Member => CompletionItemKind::METHOD,
            completion::CompletionKind::Field => CompletionItemKind::FIELD,
            completion::CompletionKind::Constructor => CompletionItemKind::CONSTRUCTOR,
            completion::CompletionKind::Function => CompletionItemKind::FUNCTION,
        }),
        // Snippet items carry `${n:…}` tab stops; everything else inserts its
        // label verbatim (the default).
        insert_text_format: c.insert_text.as_ref().map(|_| InsertTextFormat::SNIPPET),
        insert_text: c.insert_text,
        label: c.label,
        detail: c.detail,
        ..Default::default()
    }
}

/// The byte offset of an LSP `(line, character)` position in `text`,
/// clamped to the end of the document when the position lies past it.
/// LSP positions count UTF-16 code units, so this goes through the shared
/// converter — a byte-faithful reading misplaces the cursor on any line
/// with non-ASCII text before it.
fn cursor_offset(text: &str, pos: Position) -> usize {
    crate::position::position_to_offset(text, pos).unwrap_or(text.len())
}

/// v0.34 (ADR 0067): a serializable mirror of [`bynk_check::index::SymbolKey`] for
/// round-tripping through `CallHierarchyItem.data` — the index kind isn't
/// `Serialize`, so the kind travels as its `display()` string.
#[derive(serde::Serialize, serde::Deserialize)]
struct SerKey {
    unit: String,
    kind: String,
    name: String,
}

impl From<&bynk_check::index::SymbolKey> for SerKey {
    fn from(k: &bynk_check::index::SymbolKey) -> Self {
        SerKey {
            unit: k.unit.clone(),
            kind: k.kind.display().to_string(),
            name: k.name.clone(),
        }
    }
}

impl SerKey {
    /// Recover a `SymbolKey` from a `CallHierarchyItem`'s `data`. `None` for a
    /// missing/garbled payload or an unknown kind — the follow-up then returns
    /// no calls rather than guessing.
    fn read(data: &Option<serde_json::Value>) -> Option<bynk_check::index::SymbolKey> {
        let sk: SerKey = serde_json::from_value(data.as_ref()?.clone()).ok()?;
        let kind = match sk.kind.as_str() {
            "type" => bynk_check::index::SymbolKind::Type,
            "fn" => bynk_check::index::SymbolKind::Fn,
            "capability" => bynk_check::index::SymbolKind::Capability,
            "service" => bynk_check::index::SymbolKind::Service,
            "agent" => bynk_check::index::SymbolKind::Agent,
            "provider" => bynk_check::index::SymbolKind::Provider,
            _ => return None,
        };
        Some(bynk_check::index::SymbolKey {
            unit: sk.unit,
            kind,
            name: sk.name,
        })
    }
}

fn lsp_symbol_kind(kind: bynk_check::index::SymbolKind) -> SymbolKind {
    match kind {
        bynk_check::index::SymbolKind::Type => SymbolKind::STRUCT,
        bynk_check::index::SymbolKind::Fn => SymbolKind::FUNCTION,
        bynk_check::index::SymbolKind::Capability => SymbolKind::INTERFACE,
        bynk_check::index::SymbolKind::Service | bynk_check::index::SymbolKind::Agent => {
            SymbolKind::CLASS
        }
        bynk_check::index::SymbolKind::Provider => SymbolKind::OBJECT,
        bynk_check::index::SymbolKind::Method => SymbolKind::METHOD,
        bynk_check::index::SymbolKind::CapabilityOp => SymbolKind::METHOD,
        bynk_check::index::SymbolKind::Field => SymbolKind::FIELD,
        bynk_check::index::SymbolKind::Actor => SymbolKind::INTERFACE,
    }
}

/// Whether a watched-file URI names a `bynk.toml` manifest — the trigger for a
/// live config reload. Matches on the file-name component (not a path suffix),
/// so a file like `notbynk.toml` doesn't spuriously fire.
fn is_bynk_toml(uri: &Url) -> bool {
    let Ok(path) = uri.to_file_path() else {
        return false;
    };
    path.file_name().and_then(|n| n.to_str()) == Some("bynk.toml")
}

fn make_diagnostic(d: &bynk_ide::Diagnostic, text: &str, uri: &Url) -> Diagnostic {
    let range = crate::position::span_to_range(text, d.error.span);
    let severity = match d.severity {
        bynk_syntax::Severity::Error => DiagnosticSeverity::ERROR,
        bynk_syntax::Severity::Warning => DiagnosticSeverity::WARNING,
    };
    let related_information: Vec<DiagnosticRelatedInformation> = d
        .error
        .labels
        .iter()
        .map(|(span, msg)| DiagnosticRelatedInformation {
            location: Location {
                // Secondary-label spans are offsets into this same document's
                // `text`, so they belong to the document's own URI — not a
                // placeholder. (Cross-file related info is not yet modelled.)
                uri: uri.clone(),
                range: crate::position::span_to_range(text, *span),
            },
            message: msg.clone(),
        })
        .collect();
    let mut message = d.error.message.clone();
    for note in &d.error.notes {
        message.push_str("\n\n");
        message.push_str("note: ");
        message.push_str(note);
    }
    Diagnostic {
        range,
        severity: Some(severity),
        code: Some(NumberOrString::String(d.error.category.to_string())),
        code_description: None,
        source: Some(SERVER_NAME.to_string()),
        message,
        related_information: if related_information.is_empty() {
            None
        } else {
            Some(related_information)
        },
        tags: None,
        data: None,
    }
}

#[tokio::main]
async fn main() {
    // Answer `--version`/`-V` and exit before entering the stdio LSP loop, so
    // tooling (e.g. the VS Code status bar) can query the version without the
    // server blocking on stdin.
    if std::env::args()
        .skip(1)
        .any(|a| a == "--version" || a == "-V")
    {
        println!("{SERVER_NAME} {SERVER_VERSION}");
        return;
    }
    // Logging to ~/.bynk-lsp.log. Default level: warn; tunable via
    // RUST_LOG or the LSP client's trace setting.
    if let Some(home) = std::env::var_os("HOME") {
        let path: PathBuf = PathBuf::from(home).join(".bynk-lsp.log");
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            use tracing_subscriber::prelude::*;
            let env_filter = tracing_subscriber::EnvFilter::try_from_env("BYNK_LSP_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
            let file_layer = tracing_subscriber::fmt::layer()
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false);
            tracing_subscriber::registry()
                .with(env_filter)
                .with(file_layer)
                .try_init()
                .ok();
        }
    }
    tracing::info!("bynkc-lsp v{} starting", SERVER_VERSION);
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Slice A: the project model, driven through `Backend` ---------------
    //
    // These are the crate's first *behaviour-over-time* tests: they drive the
    // real `Backend` — the layer the track doc (§4.1) notes has always been
    // testable in-crate via `LspService::new(Backend::new)`, and never was.
    // Everything else in this module asserts *static* shape.
    //
    // Hermetic on purpose. `bynk-lsp` is published and `Cargo.toml`'s `exclude`
    // list can only drop `tests/*.rs`, never this file — so an in-crate test
    // reading a sibling directory would fail `cargo test` on the released
    // tarball. (`find_source_root_walks_up_to_the_nearest_src` below already
    // does exactly that; not this slice's to fix.) The sibling-reading fixtures
    // live in `tests/project_model.rs`, which *is* excluded.

    /// A throwaway project, removed on drop — including on panic.
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch_project(tag: &str, files: &[(&str, &str)]) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "bynk_lsp_sliceA_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        for (rel, body) in files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, body).unwrap();
        }
        Scratch(dir)
    }

    /// Build a `Backend` over a real `LspService`, rooted at `root`.
    ///
    /// `LspService::new(Backend::new)` is what `main` itself calls — the
    /// `Client` it hands back is the only thing `Backend` needed, and it has
    /// been available for this since the server was written.
    async fn backend_at(root: &std::path::Path) -> Backend {
        let (service, _socket) = tower_lsp::LspService::new(Backend::new);
        let backend = service.inner().clone();
        {
            let mut state = backend.state.write().await;
            state.project_root = Some(root.to_path_buf());
            state.config = project::load_config(root).unwrap_or_default();
        }
        backend
    }

    /// The slice, end to end through the server: a round covers **every**
    /// `include` tree, and each file keeps a distinct project-relative identity
    /// (ADR 0198). Before slice A the round was handed `<root>/src` and the
    /// `tests/` tree did not exist as far as the LSP was concerned.
    #[tokio::test]
    async fn a_round_covers_every_include_tree() {
        let s = scratch_project(
            "round",
            &[
                ("bynk.toml", "[project]\nname = \"round\"\n"),
                ("src/thing.bynk", "context thing\n"),
                // Same basename, second root — the ADR 0198 collision.
                ("tests/thing.bynk", "suite thing\n"),
            ],
        );
        let backend = backend_at(&s.0).await;
        backend.run_project_diagnostics().await;

        let analysis = backend
            .state
            .read()
            .await
            .analysis
            .clone()
            .expect("a round committed");
        let mut keys: Vec<String> = analysis
            .snapshots
            .keys()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        keys.sort();
        assert_eq!(
            keys,
            vec!["src/thing.bynk", "tests/thing.bynk"],
            "the round must cover both include trees, with distinct identities",
        );
    }

    /// The identity a request resolves through. `uri_to_rel` is one
    /// `strip_prefix` against the project root — total across `include` trees,
    /// where the old `src` base could only ever name files in one of them.
    #[tokio::test]
    async fn a_uri_in_any_include_tree_resolves_to_its_analysed_file() {
        let s = scratch_project(
            "uri",
            &[
                ("bynk.toml", "[project]\nname = \"uri\"\n"),
                ("src/thing.bynk", "context thing\n"),
                ("tests/thing.bynk", "suite thing\n"),
            ],
        );
        let backend = backend_at(&s.0).await;
        backend.run_project_diagnostics().await;
        let analysis = backend.state.read().await.analysis.clone().expect("round");

        for (rel, label) in [
            ("src/thing.bynk", "primary"),
            ("tests/thing.bynk", "secondary"),
        ] {
            let abs = s.0.join(rel);
            let uri = Url::from_file_path(abs.canonicalize().unwrap_or(abs)).unwrap();
            let resolved = Backend::uri_to_rel(&analysis, &uri)
                .unwrap_or_else(|| panic!("{label} root URI must resolve"));
            assert_eq!(
                resolved.to_string_lossy().replace('\\', "/"),
                rel,
                "a {label}-tree URI must name its own analysed file",
            );
            assert!(
                analysis.snapshots.contains_key(&resolved),
                "…and that file must be in the round",
            );
        }
    }

    /// `exclude` reaches the server, not just the compiler. `project.rs` used to
    /// parse it and throw it away — its own comment said the analyse walk "does
    /// not yet prune by `exclude`".
    #[tokio::test]
    async fn an_excluded_tree_is_not_analysed() {
        let s = scratch_project(
            "excl",
            &[
                (
                    "bynk.toml",
                    "[project]\nname = \"excl\"\n\n[paths]\ninclude = [\".\"]\nexclude = [\"generated\"]\n",
                ),
                ("a.bynk", "context a\n"),
                ("generated/gen.bynk", "context gen\n"),
            ],
        );
        let backend = backend_at(&s.0).await;
        backend.run_project_diagnostics().await;
        let analysis = backend.state.read().await.analysis.clone().expect("round");
        let keys: Vec<String> = analysis
            .snapshots
            .keys()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(keys, vec!["a.bynk"], "excluded trees stay out of the round");
    }

    /// CI repro (#653): the VS Code extension's fixture workspace — a legacy
    /// `[paths] src`/`tests` manifest (keys ADR 0147 retired, so
    /// `read_project_paths` ignores them → `conventional()` → `["src"]`) with a
    /// dotted commons in `src/`. Drives the real `references` handler.
    #[tokio::test]
    async fn references_resolve_in_the_vscode_fixture_layout() {
        let s = scratch_project(
            "vsc",
            &[
                (
                    "bynk.toml",
                    "[project]\nname = \"fixture\"\nversion = \"0.1.0\"\n\n[paths]\nsrc = \"src\"\ntests = \"tests\"\n",
                ),
                (
                    "src/text.bynk",
                    "commons fixture.text\n\nfn shout(s: String) -> String {\n  s\n}\n\nfn greet(name: String) -> String {\n  \"Hi, \\(shout(name))!\"\n}\n",
                ),
            ],
        );
        // Root exactly as `initialize` does: resolve from the workspace folder.
        let (root, config) = Backend::resolve_root(&s.0).expect("bynk.toml is present");
        assert_eq!(root, s.0, "the manifest's directory is the project root");

        let (service, _socket) = tower_lsp::LspService::new(Backend::new);
        let backend = service.inner().clone();
        {
            let mut st = backend.state.write().await;
            st.project_root = Some(root.clone());
            st.config = config;
        }
        backend.run_project_diagnostics().await;

        let analysis = backend
            .state
            .read()
            .await
            .analysis
            .clone()
            .expect("a round committed");
        let keys: Vec<String> = analysis
            .snapshots
            .keys()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(keys, vec!["src/text.bynk"], "the fixture file is analysed");

        // The URI the editor sends, mapped through the round's identity.
        let abs = s.0.join("src/text.bynk");
        let uri = Url::from_file_path(abs.canonicalize().unwrap_or(abs)).unwrap();
        let rel = Backend::uri_to_rel(&analysis, &uri).expect("URI resolves into the round");
        assert_eq!(rel, PathBuf::from("src/text.bynk"));

        // `shout`'s declaration site: line 2 (0-based), at `fn shout`.
        let text = analysis.snapshots.get(&rel).expect("snapshot present");
        let decl = text.find("shout").expect("`shout` in source");
        let pos = crate::position::offset_to_position(text, decl);

        let refs = backend
            .references(ReferenceParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: pos,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: ReferenceContext {
                    include_declaration: true,
                },
            })
            .await
            .expect("references must not error");
        let found = refs.unwrap_or_default();
        assert!(
            !found.is_empty(),
            "`shout` is referenced by `greet` — references must resolve; got none",
        );
    }

    // -- Slice B: the freshness contract, driven through a real Backend -------
    //
    // These are behaviour-over-time tests (§4.1): they edit a buffer and then
    // make a request, asserting the request answers against the *new* text.
    // The static tests above can't see this — the defect lives between the
    // edit and the request, which only a driven sequence exercises.

    /// Open `src/a.bynk`, round it, then edit and drive `did_change`. `uri`,
    /// the round-1 relative path, and the edited version are returned.
    async fn open_round_edit(
        backend: &Backend,
        root: &std::path::Path,
        v1_text: &str,
        v2_text: &str,
    ) -> (Url, PathBuf) {
        let abs = root.join("src/a.bynk");
        let uri = Url::from_file_path(abs.canonicalize().unwrap_or(abs)).unwrap();

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "bynk".into(),
                    version: 1,
                    text: v1_text.to_string(),
                },
            })
            .await;
        backend.run_project_diagnostics().await;

        // The round exists and is version 1.
        let a1 = backend
            .state
            .read()
            .await
            .analysis
            .clone()
            .expect("round 1");
        let rel = Backend::uri_to_rel(&a1, &uri).expect("uri resolves");
        assert_eq!(a1.versions.get(&rel), Some(&1), "round 1 is version 1");

        // Edit: the buffer becomes `v2_text` at version 2. The debounce this
        // schedules is superseded by the request-driven refresh below.
        backend
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: v2_text.to_string(),
                }],
            })
            .await;
        (uri, rel)
    }

    /// The headline. After an edit that inserts a line above a symbol, a
    /// position request at the symbol's *new* location resolves to the symbol —
    /// because the gate refreshes to the edited buffer first. Under the old
    /// behaviour the new position was resolved against the round-1 snapshot,
    /// landing on the wrong text.
    #[tokio::test]
    async fn a_position_after_an_edit_resolves_against_the_new_text() {
        let v1 = "commons q.a\n\nfn target(x: Int) -> Int {\n  x\n}\n";
        // Prepend a blank line: `target` moves from line 2 to line 3.
        let v2 = format!("\n{v1}");
        let s = scratch_project(
            "fresh_hd",
            &[("bynk.toml", "[project]\nname=\"q\"\n"), ("src/a.bynk", v1)],
        );
        let backend = backend_at(&s.0).await;
        let (uri, rel) = open_round_edit(&backend, &s.0, v1, &v2).await;

        // The gate refreshes to the edited version and text.
        let a = backend.analysis_for(&uri).await.expect("current analysis");
        assert_eq!(a.versions.get(&rel), Some(&2), "gate refreshed to the edit");
        assert_eq!(a.snapshots.get(&rel).map(String::as_str), Some(v2.as_str()));

        // `target`'s new position resolves to `target` in the refreshed snapshot.
        let off_v2 = v2.find("target").unwrap();
        let new_pos = crate::position::offset_to_position(&v2, off_v2);
        let (a2, rel2, off) = backend
            .index_position(&uri, new_pos)
            .await
            .expect("position resolves");
        assert!(
            a2.snapshots.get(&rel2).unwrap()[off..].starts_with("target"),
            "the new position must land on `target` in the current snapshot — \
             the whole point of refreshing",
        );
    }

    /// The gate never returns a round whose snapshot for the file predates the
    /// buffer. A cached round at version 1 with the buffer at version 2 must be
    /// refreshed, not served.
    #[tokio::test]
    async fn a_stale_round_is_never_served() {
        let v1 = "commons q.a\n\nfn f(x: Int) -> Int {\n  x\n}\n";
        let v2 = format!("{v1}\nfn g(y: Int) -> Int {{\n  y\n}}\n");
        let s = scratch_project(
            "fresh_stale",
            &[("bynk.toml", "[project]\nname=\"q\"\n"), ("src/a.bynk", v1)],
        );
        let backend = backend_at(&s.0).await;
        let (uri, rel) = open_round_edit(&backend, &s.0, v1, &v2).await;

        // Precondition: the *cached* round is still version 1 (no refresh yet).
        let cached = backend.state.read().await.analysis.clone().unwrap();
        assert_eq!(cached.versions.get(&rel), Some(&1), "cached round is stale");

        // The gate must not hand back that stale round.
        let a = backend.analysis_for(&uri).await.unwrap();
        assert_eq!(
            a.versions.get(&rel),
            Some(&2),
            "analysis_for must refresh past a stale cached round, never serve it",
        );
    }

    /// DECISION B: concurrent requests after one edit coalesce onto a single
    /// round, not one each. The refresh lock serialises them; the second finds
    /// the first's round already current.
    #[tokio::test]
    async fn concurrent_requests_after_one_edit_share_one_round() {
        let v1 = "commons q.a\n\nfn f(x: Int) -> Int {\n  x\n}\n";
        let v2 = format!("\n{v1}");
        let s = scratch_project(
            "fresh_coal",
            &[("bynk.toml", "[project]\nname=\"q\"\n"), ("src/a.bynk", v1)],
        );
        let backend = backend_at(&s.0).await;
        let (uri, _rel) = open_round_edit(&backend, &s.0, v1, &v2).await;

        let started_before = backend.state.read().await.analysis_round_started;

        // Fire several gate calls concurrently.
        let calls = (0..5).map(|_| {
            let b = backend.clone();
            let u = uri.clone();
            tokio::spawn(async move { b.analysis_for(&u).await.is_some() })
        });
        for c in calls {
            assert!(
                c.await.unwrap(),
                "each concurrent request must get an analysis"
            );
        }

        let started_after = backend.state.read().await.analysis_round_started;
        assert_eq!(
            started_after - started_before,
            1,
            "five concurrent requests after one edit must share ONE round, not run five",
        );
    }

    /// DECISION D: a file outside every `include` root cannot be answered at the
    /// client's version — the gate declines rather than serving something.
    #[tokio::test]
    async fn a_file_outside_the_project_is_declined() {
        let v1 = "commons q.a\n\nfn f(x: Int) -> Int {\n  x\n}\n";
        let s = scratch_project(
            "fresh_out",
            &[("bynk.toml", "[project]\nname=\"q\"\n"), ("src/a.bynk", v1)],
        );
        let backend = backend_at(&s.0).await;
        // Round the project so a cached analysis exists.
        backend.run_project_diagnostics().await;

        // A URI for a file the project does not contain, opened as a buffer.
        let outside = s.0.join("elsewhere.bynk");
        std::fs::write(&outside, v1).unwrap();
        let uri = Url::from_file_path(outside.canonicalize().unwrap()).unwrap();
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "bynk".into(),
                    version: 1,
                    text: v1.to_string(),
                },
            })
            .await;

        assert!(
            backend.analysis_for(&uri).await.is_none(),
            "a file outside the include roots is never a snapshot key — decline, \
             don't serve a round that doesn't cover it",
        );
    }

    /// #485: a rootless multi-file-commons file (a `src/` tree with no
    /// `bynk.toml`, the layout the compiler fixtures use) resolves its
    /// implicit source root — the nearest ancestor `src/` — so project-mode
    /// analysis kicks in instead of sibling-blind single-file `diagnose`.
    #[test]
    fn find_source_root_walks_up_to_the_nearest_src() {
        let ws = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root");
        let make = ws.join(
            "bynkc/tests/fixtures/positive/\
             252_multi_file_commons_dotted_test/src/shipping/rates/make.bynk",
        );
        assert!(make.is_file(), "fixture present: {}", make.display());

        let src = Backend::find_source_root(&make).expect("an ancestor src/");
        assert!(
            src.ends_with("252_multi_file_commons_dotted_test/src"),
            "nearest ancestor src, got {}",
            src.display()
        );

        // No `bynk.toml` on the path, so resolution falls back to the implicit
        // src tree, and the project root is `src`'s parent.
        //
        // Slice A: the old invariant here was `root.join(config.src_dir) == src`
        // — the analysis root re-derived by reducing the manifest to one
        // directory. That reduction is gone: the round is rooted at the project
        // and `bynk_ide::AnalysisRoots::Project` resolves the trees from the
        // manifest (here, absent → `ProjectPaths::conventional`, which picks up
        // exactly this `src/`). So what must hold is that the root is `src`'s
        // parent, and that conventional discovery finds this file from it.
        let (root, _config) = Backend::resolve_root(&make).expect("implicit project");
        assert_eq!(root, src.parent().expect("src has a parent"));

        let found = bynk_ide::discover_files(&bynk_ide::AnalysisRoots::Project(root.clone()));
        let make_canon = make.canonicalize().unwrap_or(make.clone());
        assert!(
            found
                .iter()
                .any(|p| p.canonicalize().unwrap_or_else(|_| p.clone()) == make_canon),
            "the compiler's own discovery must reach {} from the project root {}; got {found:?}",
            make.display(),
            root.display(),
        );
    }

    /// A file with no `bynk.toml` and no ancestor `src/` stays in single-file
    /// mode — resolution returns `None`, so the caller keeps the per-buffer
    /// `diagnose` path.
    #[test]
    fn resolve_root_is_none_without_toml_or_src() {
        // The crate manifest sits under `bynk-lsp/`, not inside any `src/`.
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(p.is_file());
        assert!(Backend::find_source_root(&p).is_none());
        assert!(Backend::resolve_root(&p).is_none());
    }

    // v0.124 (slice 3): the `<expr> is <cursor>` scrutinee-offset detection that
    // feeds `is`-pattern completion.
    #[test]
    fn is_scrutinee_offset_locates_the_scrutinee() {
        let text = "  order.status is Pen";
        let off = is_scrutinee_offset(text, text.len()).expect("at an is-position");
        // Lands on the last char of `order.status` (the `s` of `status`).
        assert_eq!(&text[off..off + 1], "s");
        assert!(off < text.find(" is ").unwrap());
        // No trailing partial, cursor right after `is `.
        let text2 = "  x is ";
        let off2 = is_scrutinee_offset(text2, text2.len()).expect("at an is-position");
        assert_eq!(&text2[off2..off2 + 1], "x");
        // `basis` is not a standalone `is`.
        assert!(is_scrutinee_offset("  basis ", "  basis ".len()).is_none());
        // Not an is-position at all.
        assert!(is_scrutinee_offset("  let x = ", "  let x = ".len()).is_none());
    }

    // v0.128: the `match <expr> { <arm-start>` scrutinee-offset detection that
    // feeds match-arm variant completion.
    #[test]
    fn match_scrutinee_offset_locates_the_scrutinee() {
        // First arm, cursor right after the opening brace.
        let t = "match order.status {\n  ";
        let off = match_scrutinee_offset(t, t.len()).expect("at an arm-start");
        assert_eq!(&t[off..off + 1], "s"); // last char of `order.status`
        assert!(off < t.find(" {").unwrap());

        // First arm with a partial pattern typed.
        let t = "match color { Re";
        let off = match_scrutinee_offset(t, t.len()).expect("at an arm-start");
        assert_eq!(&t[off..off + 1], "r"); // last char of `color`

        // A later arm after a top-level comma, mid-partial.
        let t = "match c {\n  Red => 1,\n  Gr";
        let off = match_scrutinee_offset(t, t.len()).expect("at a later arm-start");
        assert_eq!(&t[off..off + 1], "c");

        // A top-level comma inside a preceding arm body does not confuse the
        // header (the nested call's comma is at depth > 0).
        let t = "match c {\n  Red => f(a, b),\n  ";
        assert!(match_scrutinee_offset(t, t.len()).is_some());

        // Inside an arm *body* (after `=>`) — not a pattern position.
        assert!(
            match_scrutinee_offset("match c {\n  Red => ", "match c {\n  Red => ".len()).is_none()
        );

        // A non-`match` block offers nothing.
        assert!(match_scrutinee_offset("fn f() {\n  ", "fn f() {\n  ".len()).is_none());

        // A nested constructor position (`Ok(<cursor>`) is not an arm-start.
        assert!(match_scrutinee_offset("match c {\n  Ok(", "match c {\n  Ok(".len()).is_none());

        // No open brace / no scrutinee → nothing.
        assert!(match_scrutinee_offset("match c ", "match c ".len()).is_none());
        assert!(match_scrutinee_offset("match {\n  ", "match {\n  ".len()).is_none());
    }

    // v0.145 (ADR 0169): the `match <expr> { … Variant(<partial>` nested-pattern
    // detection that feeds payload-variant completion — the position
    // `match_scrutinee_offset` deliberately bails on.
    #[test]
    fn nested_pattern_offset_locates_the_scrutinee_and_variant() {
        // Cursor right inside a variant's payload parens.
        let t = "match res {\n  Some(";
        let (off, variant) = nested_pattern_offset(t, t.len()).expect("inside a nested pattern");
        assert_eq!(&t[off..off + 1], "s"); // last char of `res`
        assert_eq!(variant, "Some");

        // With a partial nested pattern typed, and a qualifier.
        let t = "match res {\n  Ok(Po";
        let (off, variant) = nested_pattern_offset(t, t.len()).expect("mid partial");
        assert_eq!(&t[off..off + 1], "s");
        assert_eq!(variant, "Ok");

        // A later arm after a top-level comma.
        let t = "match r {\n  Ok(n) => n,\n  Err(";
        let (off, variant) = nested_pattern_offset(t, t.len()).expect("later arm");
        assert_eq!(&t[off..off + 1], "r");
        assert_eq!(variant, "Err");

        // A lowercase-led token before `(` is a binding/call, not a variant
        // constructor — no nested completion (there is no inner type to open).
        assert!(nested_pattern_offset("match r {\n  ok(", "match r {\n  ok(".len()).is_none());

        // An arm-start (no open paren) is the flat position, not a nested one.
        assert!(nested_pattern_offset("match c {\n  ", "match c {\n  ".len()).is_none());
        assert!(nested_pattern_offset("match c {\n  Ok", "match c {\n  Ok".len()).is_none());

        // Inside an arm body (after `=>`) is not a pattern position.
        let t = "match c {\n  Ok(n) => g(";
        assert!(nested_pattern_offset(t, t.len()).is_none());

        // A non-`match` block offers nothing.
        assert!(nested_pattern_offset("fn f() {\n  h(", "fn f() {\n  h(".len()).is_none());
    }

    /// A watched-file change on `bynk.toml` is recognised (so the config can be
    /// reloaded live), while a sibling `.bynk` file or a merely `…bynk.toml`-
    /// suffixed name is not — the name-component match, not a path suffix.
    #[test]
    fn is_bynk_toml_matches_only_the_manifest() {
        // Build URIs from a host-absolute base so `from_file_path` succeeds on
        // Windows too (a Unix-style `/proj` path is not absolute there — no
        // drive letter — and would fail to convert). Mirrors the sibling
        // `find_source_root` test's `CARGO_MANIFEST_DIR` base.
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let toml = Url::from_file_path(base.join("bynk.toml")).expect("abs path");
        assert!(is_bynk_toml(&toml));
        let nested = Url::from_file_path(base.join("sub").join("bynk.toml")).expect("abs path");
        assert!(is_bynk_toml(&nested));

        // A source file is not the manifest.
        let src = Url::from_file_path(base.join("src").join("main.bynk")).expect("abs path");
        assert!(!is_bynk_toml(&src));
        // A file whose name merely *ends with* `bynk.toml` must not fire.
        let decoy = Url::from_file_path(base.join("notbynk.toml")).expect("abs path");
        assert!(!is_bynk_toml(&decoy));
        // A non-file URI never matches.
        let remote = Url::parse("https://example.com/bynk.toml").expect("url");
        assert!(!is_bynk_toml(&remote));
    }

    /// The v0.26 capability advertisements — the "trivial unit check" the
    /// proposal scopes in place of a transport round-trip.
    #[test]
    fn advertises_code_actions_and_the_index_riders() {
        let caps = server_capabilities();
        let Some(CodeActionProviderCapability::Options(opts)) = caps.code_action_provider else {
            panic!("codeActionProvider not advertised with options");
        };
        assert_eq!(opts.code_action_kinds, Some(vec![CodeActionKind::QUICKFIX]));
        assert!(matches!(
            caps.workspace_symbol_provider,
            Some(OneOf::Left(true))
        ));
        assert!(matches!(
            caps.document_highlight_provider,
            Some(OneOf::Left(true))
        ));
    }

    /// The v0.27 capability advertisement — the "trivial unit check" the
    /// proposal scopes in place of a transport round-trip.
    #[test]
    fn advertises_save_notifications() {
        // `diagnostics_mode = "on_save"` is driven by `didSave`; the sync
        // options must opt in explicitly or clients may not send it (#513).
        let caps = server_capabilities();
        let Some(TextDocumentSyncCapability::Options(opts)) = caps.text_document_sync else {
            panic!("textDocumentSync not advertised with options");
        };
        assert_eq!(opts.change, Some(TextDocumentSyncKind::FULL));
        assert!(matches!(
            opts.save,
            Some(TextDocumentSyncSaveOptions::Supported(true))
        ));
    }

    #[test]
    fn advertises_inlay_hints() {
        let caps = server_capabilities();
        assert!(matches!(caps.inlay_hint_provider, Some(OneOf::Left(true))));
    }

    /// Slice 6: go-to-type-definition (value → its type's declaration).
    #[test]
    fn advertises_type_definition() {
        let caps = server_capabilities();
        assert!(matches!(
            caps.type_definition_provider,
            Some(TypeDefinitionProviderCapability::Simple(true))
        ));
    }

    /// Slice 6b: `uses`/`consumes` document links.
    #[test]
    fn advertises_document_links() {
        let caps = server_capabilities();
        assert!(caps.document_link_provider.is_some());
    }

    /// Slice 5: completion advertises `.` triggers and lazy doc resolution.
    #[test]
    fn advertises_completion_with_dot_trigger_and_resolve() {
        let caps = server_capabilities();
        let opts = caps.completion_provider.expect("completion advertised");
        assert_eq!(opts.resolve_provider, Some(true), "resolve_provider");
        assert!(
            opts.trigger_characters
                .as_deref()
                .is_some_and(|t| t.iter().any(|c| c == ".")),
            "`.` trigger char"
        );
    }

    /// The v0.28 capability advertisement: full + range with the frozen
    /// legend (the legend's content is pinned in `index_queries`).
    #[test]
    fn advertises_semantic_tokens() {
        let caps = server_capabilities();
        let Some(SemanticTokensServerCapabilities::SemanticTokensOptions(opts)) =
            caps.semantic_tokens_provider
        else {
            panic!("semanticTokensProvider not advertised with options");
        };
        assert_eq!(opts.full, Some(SemanticTokensFullOptions::Bool(true)));
        assert_eq!(opts.range, Some(true));
        assert_eq!(opts.legend, crate::index_queries::semantic_tokens_legend());
    }
}
