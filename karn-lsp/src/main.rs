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
mod symbols;

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as JsonRpcResult;
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
        let Some(src_root) = self.project_src_root().await else {
            return Vec::new();
        };
        let canonical_src_root = src_root.canonicalize().unwrap_or_else(|_| src_root.clone());
        let Ok(cur) = uri.to_file_path() else {
            return Vec::new();
        };
        let cur = cur.canonicalize().unwrap_or(cur);
        let Ok(rel) = cur.strip_prefix(&canonical_src_root) else {
            return Vec::new();
        };
        let rel = rel.to_path_buf();
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
        let root = src_root.clone();
        let Ok(result) =
            tokio::task::spawn_blocking(move || karnc::diagnose_project(&root, &overlay)).await
        else {
            return Vec::new();
        };
        let Some((_, entries)) = result.expr_types.iter().find(|(p, _)| **p == rel) else {
            return Vec::new(); // the file didn't check clean (the ceiling)
        };
        let Some(ty) = karnc::expr_types::type_at_offset(entries, recv_offset) else {
            return Vec::new();
        };
        completion::value_member_candidates(ty, text, Some(src_root.as_path()))
            .into_iter()
            .map(to_completion_item)
            .collect()
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
                // Fall back to a project-wide scan (v1.1). Required so
                // `uses` / `consumes` names resolve across file boundaries
                // per `design/karn-lsp-spec.md` §3.4.
                let src_root = self.project_src_root().await;
                match src_root
                    .and_then(|root| crate::symbols::describe_symbol_cross_file(&root, &uri, &name))
                {
                    Some((_other_uri, desc)) => desc,
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
        if candidates.is_empty() {
            // Slice 3: a lowercase `receiver.` is a value receiver — type it by
            // re-analysing the rewritten buffer and offer its members.
            let offset = cursor_byte_offset(&text, pos);
            let value_items = self.value_member_completions(&uri, &text, offset).await;
            return Ok((!value_items.is_empty()).then_some(CompletionResponse::Array(value_items)));
        }
        let items: Vec<CompletionItem> = candidates.into_iter().map(to_completion_item).collect();
        Ok(Some(CompletionResponse::Array(items)))
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
        // keyword, the `{` of a selected-capability list, and `,`.
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![" ".to_string(), "{".to_string(), ",".to_string()]),
            ..Default::default()
        }),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
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

fn lsp_symbol_kind(kind: karnc::index::SymbolKind) -> SymbolKind {
    match kind {
        karnc::index::SymbolKind::Type => SymbolKind::STRUCT,
        karnc::index::SymbolKind::Fn => SymbolKind::FUNCTION,
        karnc::index::SymbolKind::Capability => SymbolKind::INTERFACE,
        karnc::index::SymbolKind::Service | karnc::index::SymbolKind::Agent => SymbolKind::CLASS,
        karnc::index::SymbolKind::Provider => SymbolKind::OBJECT,
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
