//! `karnc-lsp` — Karn Language Server.
//!
//! Implements the LSP capabilities listed in `design/karn-lsp-spec.md` §4.3:
//! synchronisation (Full), diagnostics, hover, go-to-definition, formatting,
//! range formatting, document symbols, references, rename, code actions,
//! workspace symbols, document highlights, and file watching. Built on
//! `tower-lsp`.
//!
//! Architecture:
//! - [`Backend`] holds the project state: root path (the directory
//!   containing `karn.toml`), parsed configuration, and an in-memory map of
//!   open files. State is guarded by a `tokio::sync::RwLock`.
//! - Document changes trigger `recompile_and_publish` which re-runs the
//!   compiler (via [`karnc::diagnose`]) and publishes resulting diagnostics.
//! - Hover and definition consult the parsed AST for the file under the
//!   cursor; both are best-effort (return None for unrecognised positions).
//! - Formatting delegates to [`karn_fmt::format_source`].

mod code_actions;
mod completion;
mod document_symbols;
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

const SERVER_NAME: &str = "karnc-lsp";
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
    /// Canonicalised source root the snapshots' relative paths resolve
    /// against.
    src_root: PathBuf,
    index: karnc::index::ProjectIndex,
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
    diagnostics: std::collections::HashMap<PathBuf, Vec<karnc::Diagnostic>>,
    /// v0.27 (ADR 0056): project-relative path → the round's harvested
    /// inferred-type hints, spans against the analysed snapshots.
    hints: karnc::hints::FileHints,
    /// v0.31 (ADR 0064): project-relative path → the round's local bindings
    /// with scope ranges, for locals navigation (references/definition/
    /// highlight), spans against the analysed snapshots.
    locals: karnc::locals::FileLocals,
    /// Slice 6: project-relative path → the round's expression types, spans
    /// against the analysed snapshots — backs go-to-type-definition.
    expr_types: karnc::expr_types::FileExprTypes,
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
    /// Path to the project root (the directory containing `karn.toml`). If
    /// no project root is found, this is None and the server operates in
    /// single-file mode for any open file.
    project_root: Option<PathBuf>,
    /// Parsed `karn.toml` configuration. Defaults applied for missing fields.
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
}

#[derive(Clone)]
struct Backend {
    client: Client,
    state: Arc<RwLock<State>>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(RwLock::new(State::default())),
        }
    }

    /// Locate `karn.toml` walking upward from the given path. Returns the
    /// project root (the directory containing `karn.toml`) on success.
    fn find_project_root(start: &std::path::Path) -> Option<PathBuf> {
        let mut current = if start.is_file() {
            start.parent()?.to_path_buf()
        } else {
            start.to_path_buf()
        };
        loop {
            let candidate = current.join("karn.toml");
            if candidate.is_file() {
                return Some(current);
            }
            current = current.parent()?.to_path_buf();
        }
    }

    /// Re-run the compiler on the document at `uri` and publish diagnostics.
    /// Best-effort: a malformed file produces diagnostics rather than a
    /// hard failure.
    async fn recompile_and_publish(&self, uri: &Url) {
        // v0.24 (ADR 0052): with a project root, diagnostics are
        // project-wide (every file, contexts included) on a debounce.
        // Single-file mode (no karn.toml) keeps the per-buffer path below.
        if self.state.read().await.project_root.is_some() {
            self.schedule_project_diagnostics().await;
            return;
        }
        let text = {
            let state = self.state.read().await;
            state.docs.get(uri).map(|d| d.text.clone())
        };
        let Some(text) = text else { return };
        let diagnostics = karnc::diagnose(&text);
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
        let (root, src_root, overlay, versions, previously_dirty) = {
            let state = self.state.read().await;
            let Some(root) = state.project_root.clone() else {
                return;
            };
            let src_root = root.join(&state.config.src_dir);
            let canonical_src_root = src_root.canonicalize().unwrap_or_else(|_| src_root.clone());
            let mut overlay = std::collections::HashMap::new();
            let mut versions = std::collections::HashMap::new();
            for (uri, doc) in &state.docs {
                if let Ok(p) = uri.to_file_path() {
                    let canonical = p.canonicalize().unwrap_or(p);
                    // v0.25: capture the version the overlay snapshot came
                    // from, keyed project-relative like the analysis output.
                    if let Ok(rel) = canonical.strip_prefix(&canonical_src_root) {
                        versions.insert(rel.to_path_buf(), doc.version);
                    }
                    overlay.insert(canonical, doc.text.clone());
                }
            }
            (root, src_root, overlay, versions, state.published.clone())
        };

        let analysis_root = src_root.clone();
        let Ok(result) =
            tokio::task::spawn_blocking(move || karnc::diagnose_project(&analysis_root, &overlay))
                .await
        else {
            return;
        };

        let mut new_by_uri: std::collections::HashMap<Url, Vec<Diagnostic>> =
            std::collections::HashMap::new();
        let mut snapshots = std::collections::HashMap::new();
        let mut diagnostics: std::collections::HashMap<PathBuf, Vec<karnc::Diagnostic>> =
            std::collections::HashMap::new();
        for file in &result.files {
            let abs = src_root.join(&file.source_path);
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
            new_by_uri.insert(uri, diags);
            diagnostics.insert(file.source_path.clone(), file.diagnostics.clone());
            snapshots.insert(file.source_path.clone(), file.text.clone());
        }
        // v0.25: retain the round's index + snapshots for references/rename
        // and the binding-correct definition/hover. v0.26: plus the raw
        // diagnostics, for `codeAction` (the suggestions ride on them).
        {
            let analysis = Arc::new(Analysis {
                src_root: src_root.canonicalize().unwrap_or_else(|_| src_root.clone()),
                index: result.index.clone(),
                snapshots,
                versions,
                diagnostics,
                hints: result.hints,
                locals: result.locals,
                expr_types: result.expr_types,
                unit_sources: result.unit_sources,
            });
            self.state.write().await.analysis = Some(analysis);
        }
        // Project-level diagnostics with no single owning file surface on
        // karn.toml (position 0:0) rather than vanishing.
        if !result.unattributed.is_empty()
            && let Ok(toml_uri) = Url::from_file_path(root.join("karn.toml"))
        {
            let entry = new_by_uri.entry(toml_uri).or_default();
            for d in &result.unattributed {
                entry.push(Diagnostic {
                    range: Default::default(),
                    severity: Some(match d.severity {
                        karnc::Severity::Error => DiagnosticSeverity::ERROR,
                        karnc::Severity::Warning => DiagnosticSeverity::WARNING,
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
            self.client.publish_diagnostics(uri, diags, None).await;
        }
        self.state.write().await.published = dirty;
    }

    /// Project source root resolved against the active `karn.toml`'s
    /// `[paths].src`. Returns `None` when no project root is known (single-
    /// file mode), in which case cross-file lookups are skipped.
    async fn project_src_root(&self) -> Option<PathBuf> {
        let state = self.state.read().await;
        let root = state.project_root.as_ref()?;
        Some(root.join(&state.config.src_dir))
    }

    /// v0.31: the def + use spans of the local under the cursor (def first), or
    /// `None` if the cursor is not on a local.
    fn local_sites(
        &self,
        analysis: &Analysis,
        rel: &std::path::Path,
        offset: usize,
    ) -> Option<Vec<karnc::span::Span>> {
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
        let analysis = self.state.read().await.analysis.clone();
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
        karnc::locals::locals_at(locals, offset)
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
        spans: &[karnc::span::Span],
    ) -> Vec<Location> {
        let Some(text) = analysis.snapshots.get(rel) else {
            return Vec::new();
        };
        let Ok(uri) = Url::from_file_path(analysis.src_root.join(rel)) else {
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
        let src_root = self.project_src_root().await;
        completion::value_member_candidates(&ty, text, src_root.as_deref())
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
    ) -> Option<karnc::checker::Ty> {
        let src_root = self.project_src_root().await?;
        let canonical_src_root = src_root.canonicalize().unwrap_or_else(|_| src_root.clone());
        let cur = uri.to_file_path().ok()?;
        let cur = cur.canonicalize().unwrap_or(cur);
        let rel = cur.strip_prefix(&canonical_src_root).ok()?.to_path_buf();
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
        let result =
            tokio::task::spawn_blocking(move || karnc::diagnose_project(&src_root, &overlay))
                .await
                .ok()?;
        let (_, entries) = result.expr_types.iter().find(|(p, _)| **p == rel)?;
        karnc::expr_types::type_at_offset(entries, recv_offset).cloned()
    }

    /// v0.25: the latest analysis, running one synchronously if none has
    /// completed yet (a request can arrive before the first debounced
    /// round).
    async fn ensure_analysis(&self) -> Option<Arc<Analysis>> {
        if let Some(a) = self.state.read().await.analysis.clone() {
            return Some(a);
        }
        self.run_project_diagnostics().await;
        self.state.read().await.analysis.clone()
    }

    /// v0.25: a fresh analysis of the current buffers — rename plans against
    /// live state, not the last debounced round.
    async fn fresh_analysis(&self) -> Option<Arc<Analysis>> {
        self.run_project_diagnostics().await;
        self.state.read().await.analysis.clone()
    }

    /// Map a request URI to the analysis' project-relative path.
    fn uri_to_rel(analysis: &Analysis, uri: &Url) -> Option<PathBuf> {
        let p = uri.to_file_path().ok()?;
        let canonical = p.canonicalize().unwrap_or(p);
        canonical
            .strip_prefix(&analysis.src_root)
            .ok()
            .map(|r| r.to_path_buf())
    }

    /// Slice 6a follow-up (ADR 0095): if `pos` sits on a `uses`/`consumes` unit
    /// name, the location of that unit's source (its first file, at the top —
    /// units aren't index symbols, so there is no finer def span to land on).
    /// Spans come from the live buffer; the target from the round's unit→source
    /// map. `None` for a first-party/unresolved unit or a non-unit position.
    async fn unit_reference_definition(&self, uri: &Url, pos: Position) -> Option<Location> {
        let (text, analysis) = {
            let s = self.state.read().await;
            (s.docs.get(uri).map(|d| d.text.clone()), s.analysis.clone())
        };
        let (text, analysis) = (text?, analysis?);
        let offset = cursor_byte_offset(&text, pos);
        for (unit, span) in crate::symbols::unit_reference_spans(&text) {
            if span.start <= offset && offset <= span.end {
                let rel = analysis.unit_sources.get(&unit)?.first()?;
                let target = Url::from_file_path(analysis.src_root.join(rel)).ok()?;
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
    fn site_to_location(analysis: &Analysis, site: &karnc::index::SiteRef) -> Option<Location> {
        let text = analysis.snapshots.get(&site.path)?;
        let abs = analysis.src_root.join(&site.path);
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
        key: &karnc::index::SymbolKey,
        def: &karnc::index::SiteRef,
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
    fn call_ranges(analysis: &Analysis, sites: &[&karnc::index::SiteRef]) -> Vec<Range> {
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
        let analysis = { self.state.read().await.analysis.clone() };
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
                Some(karnc::span::Span::new(start, end))
            }
        };
        let lt = analysis
            .locals
            .get(&rel)
            .map(|l| crate::locals_nav::local_token_sites(l, text))
            .unwrap_or_default();
        crate::index_queries::semantic_tokens(&analysis.index, &lt, &rel, text, span)
    }

    /// The (analysis, rel-path, snapshot byte offset) for a request
    /// position — the shared front half of every index-backed handler.
    async fn index_position(
        &self,
        uri: &Url,
        position: Position,
        fresh: bool,
    ) -> Option<(Arc<Analysis>, PathBuf, usize)> {
        let analysis = if fresh {
            self.fresh_analysis().await?
        } else {
            self.ensure_analysis().await?
        };
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
    ) -> Option<(String, karnc::span::Span, String)> {
        let text = {
            let state = self.state.read().await;
            state.docs.get(uri)?.text.clone()
        };
        let offset = crate::position::position_to_offset(&text, position)?;
        let tokens = karnc::lexer::tokenize(&text).ok()?;
        // Find the token whose span covers `offset`.
        for t in &tokens {
            if t.span.start <= offset
                && offset < t.span.end
                && matches!(
                    t.kind,
                    karnc::lexer::TokenKind::Ident
                        | karnc::lexer::TokenKind::Int
                        | karnc::lexer::TokenKind::String
                        | karnc::lexer::TokenKind::Bool
                        | karnc::lexer::TokenKind::Float
                        | karnc::lexer::TokenKind::Result
                        | karnc::lexer::TokenKind::Option
                        | karnc::lexer::TokenKind::Effect
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
            if let Some(root) = Self::find_project_root(&path) {
                state.config = project::load_config(&root).unwrap_or_default();
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
                        format!("karnc-lsp: project root at {}", root.display()),
                    )
                    .await;
            }
            None => {
                self.client
                    .log_message(
                        MessageType::INFO,
                        "karnc-lsp: no karn.toml found; single-file mode",
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
            // First open in a single-file context may need to set project root.
            if state.project_root.is_none()
                && let Ok(path) = uri.to_file_path()
                && let Some(root) = Self::find_project_root(&path)
            {
                state.config = project::load_config(&root).unwrap_or_default();
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
        // Debounce: use the configured value. For simplicity, sleep then
        // recompile. Multiple rapid changes effectively coalesce because
        // each tasks reads the latest text at recompile time.
        let debounce_ms = {
            let s = self.state.read().await;
            s.config.diagnostics_debounce_ms
        };
        let backend = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(debounce_ms)).await;
            backend.recompile_and_publish(&uri).await;
        });
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let mut state = self.state.write().await;
        state.docs.remove(&uri);
    }

    async fn hover(&self, params: HoverParams) -> JsonRpcResult<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        // v0.25 rider: binding-correct hover — find the definition through
        // the index, then describe it in its defining file (names are unique
        // per file, so the per-file lookup is exact). Falls back to the
        // legacy name-matching path for not-yet-indexed symbol kinds.
        if let Some((analysis, rel, offset)) = self.index_position(&uri, pos, false).await
            && let Some((key, def)) =
                crate::index_queries::definition_at(&analysis.index, &rel, offset)
            && let Some(def_text) = analysis.snapshots.get(&def.path)
            && let Some(content) = crate::symbols::describe_symbol(def_text, &key.name)
        {
            return Ok(Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: content,
                }),
                range: None,
            }));
        }
        let Some((name, _span, text)) = self.identifier_at(&uri, pos).await else {
            return Ok(None);
        };
        // Local lookup first (fast path).
        let content = match crate::symbols::describe_symbol(&text, &name) {
            Some(local) => local,
            None => {
                // Fall back to a project-wide scan (v1.1), then the embedded
                // first-party sources (slice 9) — so `uses` / `consumes` names
                // resolve across file boundaries (§3.4) and stdlib/surface
                // symbols surface their signature + doc too.
                let src_root = self.project_src_root().await;
                match src_root
                    .and_then(|root| crate::symbols::describe_symbol_cross_file(&root, &uri, &name))
                    .map(|(_other_uri, desc)| desc)
                    .or_else(|| crate::symbols::describe_firstparty_symbol(&name))
                {
                    Some(desc) => desc,
                    None => return Ok(None),
                }
            }
        };
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: content,
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
        let offset = cursor_byte_offset(&text, pos);
        let Some(ctx) = crate::signature_help::call_context(&text, offset) else {
            return Ok(None);
        };
        let src_root = self.project_src_root().await;
        // Name callees (free fns, statics, capability ops, of/unsafe) — lexical.
        let label =
            match crate::signature_help::resolve_label(&ctx.callee, &text, src_root.as_deref()) {
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
        let analysis = { self.state.read().await.analysis.clone() };
        let Some(analysis) = analysis else {
            return Ok(Some(Vec::new()));
        };
        let Some(rel) = Self::uri_to_rel(&analysis, &uri) else {
            return Ok(Some(Vec::new()));
        };
        let Some(text) = analysis.snapshots.get(&rel) else {
            return Ok(Some(Vec::new()));
        };
        let lenses: Vec<CodeLens> = crate::index_queries::code_lenses(&analysis.index, &rel)
            .into_iter()
            .map(|(def, refs)| {
                let range = crate::position::span_to_range(text, def.span);
                let locations: Vec<Location> = refs
                    .iter()
                    .filter_map(|r| Self::site_to_location(&analysis, r))
                    .collect();
                let n = refs.len();
                CodeLens {
                    range,
                    command: Some(Command {
                        title: format!("{n} reference{}", if n == 1 { "" } else { "s" }),
                        // Peek the references on click — a standard client command,
                        // so no extension support is required.
                        command: "editor.action.showReferences".to_string(),
                        arguments: Some(vec![
                            serde_json::to_value(&uri).unwrap_or_default(),
                            serde_json::to_value(range.start).unwrap_or_default(),
                            serde_json::to_value(&locations).unwrap_or_default(),
                        ]),
                    }),
                    data: None,
                }
            })
            .collect();
        Ok(Some(lenses))
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> JsonRpcResult<Option<Vec<CallHierarchyItem>>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some((analysis, rel, offset)) = self.index_position(&uri, pos, false).await else {
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
        let analysis = { self.state.read().await.analysis.clone() };
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
        let analysis = { self.state.read().await.analysis.clone() };
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
        let Some((analysis, rel, offset)) = self.index_position(&uri, pos, false).await else {
            return Ok(None);
        };
        let Some((key, _)) = analysis.index.symbol_at(&rel, offset) else {
            return Ok(None);
        };
        if key.kind != karnc::index::SymbolKind::Capability {
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
        let Some((analysis, rel, offset)) = self.index_position(&uri, pos, false).await else {
            return Ok(None);
        };
        let Some(entries) = analysis.expr_types.get(&rel) else {
            return Ok(None);
        };
        let Some(ty) = karnc::expr_types::type_at_offset(entries, offset) else {
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
        let (text, analysis) = {
            let s = self.state.read().await;
            (s.docs.get(&uri).map(|d| d.text.clone()), s.analysis.clone())
        };
        let (Some(text), Some(analysis)) = (text, analysis) else {
            return Ok(None);
        };
        let links: Vec<DocumentLink> = crate::symbols::unit_reference_spans(&text)
            .into_iter()
            .filter_map(|(unit, span)| {
                let rel = analysis.unit_sources.get(&unit)?.first()?;
                let target = Url::from_file_path(analysis.src_root.join(rel)).ok()?;
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
        // The line up to the cursor — the context the completion keys off.
        let line_prefix = text
            .lines()
            .nth(pos.line as usize)
            .map(|l| {
                let end = (pos.character as usize).min(l.len());
                l.get(..end).unwrap_or(l)
            })
            .unwrap_or("")
            .to_string();
        let src_root = self.project_src_root().await;
        let candidates = completion::complete(&line_prefix, &text, src_root.as_deref());
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
        if items.is_empty() {
            // Slice 3: a lowercase `receiver.` is a value receiver — type it by
            // re-analysing the rewritten buffer and offer its members. (Value
            // members name no declared symbol, so they carry no resolve data.)
            let offset = cursor_byte_offset(&text, pos);
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
                .project_src_root()
                .await
                .and_then(|root| {
                    crate::symbols::describe_symbol_cross_file(&root, &uri, &item.label)
                })
                .map(|(_uri, md)| md)
                // Slice 9: stdlib/surface symbols (e.g. a `uses karn.list` combinator)
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
        if let Some((analysis, rel, offset)) = self.index_position(&uri, pos, false).await {
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
        if let Some(root) = self.project_src_root().await
            && let Some(found) = crate::symbols::find_declaration_cross_file(&root, &uri, &name)
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
        match karn_fmt::format_source(&text, &opts) {
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
        // v1.1 — outline view + Cmd-Shift-O. See `design/karn-lsp-spec.md` §3.7.
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
        let Some((analysis, rel, offset)) = self.index_position(&uri, pos, false).await else {
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
        let analysis = { self.state.read().await.analysis.clone() };
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
            karnc::span::Span::new(start, end),
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
        let analysis = { self.state.read().await.analysis.clone() };
        let Some(analysis) = analysis else {
            return Ok(Some(Vec::new()));
        };
        let Some(rel) = Self::uri_to_rel(&analysis, &uri) else {
            return Ok(Some(Vec::new()));
        };
        let (Some(text), Some(hints)) = (analysis.snapshots.get(&rel), analysis.hints.get(&rel))
        else {
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
        Ok(Some(crate::inlay_hints::inlay_hints(
            text,
            hints,
            karnc::span::Span::new(start, end),
        )))
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
        let Some((analysis, rel, offset)) = self.index_position(&uri, pos, false).await else {
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
        let Some((analysis, rel, offset)) = self.index_position(&uri, pos, false).await else {
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
        let Some((analysis, rel, offset)) = self.index_position(&uri, pos, true).await else {
            return Err(refused("rename requires a project (karn.toml)".into()));
        };
        let plan = crate::index_queries::plan_rename(&analysis.index, &rel, offset, &new_name)
            .map_err(refused)?;

        // Validator 1 + 2 input: re-analyse with the edits applied. Every
        // snapshot is pinned via the overlay so the re-analysis differs from
        // the plan's baseline only by the edits themselves.
        let mut overlay = std::collections::HashMap::new();
        for (rel_path, text) in &analysis.snapshots {
            let edited = match plan.edits.get(rel_path) {
                Some(spans) => crate::index_queries::apply_edits(text, spans, &plan.new_name),
                None => text.clone(),
            };
            let abs = analysis.src_root.join(rel_path);
            let abs = abs.canonicalize().unwrap_or(abs);
            overlay.insert(abs, edited);
        }
        let analysis_root = analysis.src_root.clone();
        let Ok(post) =
            tokio::task::spawn_blocking(move || karnc::diagnose_project(&analysis_root, &overlay))
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
            let abs = analysis.src_root.join(rel_path);
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
        // For every changed `.karn` file we have open, refresh diagnostics.
        let mut uris_to_refresh = Vec::new();
        {
            let state = self.state.read().await;
            for ev in &params.changes {
                if state.docs.contains_key(&ev.uri) {
                    uris_to_refresh.push(ev.uri.clone());
                }
            }
        }
        for uri in uris_to_refresh {
            self.recompile_and_publish(&uri).await;
        }
    }
}

/// The advertised capability set — `design/karn-lsp-spec.md` §4.3. Split out
/// of `initialize` so the advertisement is unit-testable without transport.
fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
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

/// The byte offset of an LSP `(line, character)` position in `text`. Mirrors
/// the `line_prefix` computation (character as a byte index — ASCII-faithful).
fn cursor_byte_offset(text: &str, pos: Position) -> usize {
    let mut offset = 0;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if i == pos.line as usize {
            let bare = line.strip_suffix('\n').unwrap_or(line);
            return offset + (pos.character as usize).min(bare.len());
        }
        offset += line.len();
    }
    offset.min(text.len())
}

/// v0.34 (ADR 0067): a serializable mirror of [`karnc::index::SymbolKey`] for
/// round-tripping through `CallHierarchyItem.data` — the index kind isn't
/// `Serialize`, so the kind travels as its `display()` string.
#[derive(serde::Serialize, serde::Deserialize)]
struct SerKey {
    unit: String,
    kind: String,
    name: String,
}

impl From<&karnc::index::SymbolKey> for SerKey {
    fn from(k: &karnc::index::SymbolKey) -> Self {
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
    fn read(data: &Option<serde_json::Value>) -> Option<karnc::index::SymbolKey> {
        let sk: SerKey = serde_json::from_value(data.as_ref()?.clone()).ok()?;
        let kind = match sk.kind.as_str() {
            "type" => karnc::index::SymbolKind::Type,
            "fn" => karnc::index::SymbolKind::Fn,
            "capability" => karnc::index::SymbolKind::Capability,
            "service" => karnc::index::SymbolKind::Service,
            "agent" => karnc::index::SymbolKind::Agent,
            "provider" => karnc::index::SymbolKind::Provider,
            _ => return None,
        };
        Some(karnc::index::SymbolKey {
            unit: sk.unit,
            kind,
            name: sk.name,
        })
    }
}

fn lsp_symbol_kind(kind: karnc::index::SymbolKind) -> SymbolKind {
    match kind {
        karnc::index::SymbolKind::Type => SymbolKind::STRUCT,
        karnc::index::SymbolKind::Fn => SymbolKind::FUNCTION,
        karnc::index::SymbolKind::Capability => SymbolKind::INTERFACE,
        karnc::index::SymbolKind::Service | karnc::index::SymbolKind::Agent => SymbolKind::CLASS,
        karnc::index::SymbolKind::Provider => SymbolKind::OBJECT,
        karnc::index::SymbolKind::Method => SymbolKind::METHOD,
        karnc::index::SymbolKind::CapabilityOp => SymbolKind::METHOD,
        karnc::index::SymbolKind::Field => SymbolKind::FIELD,
        karnc::index::SymbolKind::Actor => SymbolKind::INTERFACE,
    }
}

fn make_diagnostic(d: &karnc::Diagnostic, text: &str, uri: &Url) -> Diagnostic {
    let range = crate::position::span_to_range(text, d.error.span);
    let severity = match d.severity {
        karnc::Severity::Error => DiagnosticSeverity::ERROR,
        karnc::Severity::Warning => DiagnosticSeverity::WARNING,
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
    // Logging to ~/.karn-lsp.log. Default level: warn; tunable via
    // RUST_LOG or the LSP client's trace setting.
    if let Some(home) = std::env::var_os("HOME") {
        let path: PathBuf = PathBuf::from(home).join(".karn-lsp.log");
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            use tracing_subscriber::prelude::*;
            let env_filter = tracing_subscriber::EnvFilter::try_from_env("KARN_LSP_LOG")
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
    tracing::info!("karnc-lsp v{} starting", SERVER_VERSION);
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use super::*;

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
