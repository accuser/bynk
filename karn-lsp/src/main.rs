//! `karnc-lsp` — Karn Language Server.
//!
//! Implements the LSP capabilities listed in `design/karn-lsp-spec.md` §4.3:
//! synchronisation (Full), diagnostics, hover, go-to-definition, formatting,
//! range formatting, and file watching. Built on `tower-lsp`.
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

mod completion;
mod document_symbols;
mod position;
mod project;
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

    /// Project source root resolved against the active `karn.toml`'s
    /// `[paths].src`. Returns `None` when no project root is known (single-
    /// file mode), in which case cross-file lookups are skipped.
    async fn project_src_root(&self) -> Option<PathBuf> {
        let state = self.state.read().await;
        let root = state.project_root.as_ref()?;
        Some(root.join(&state.config.src_dir))
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
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                // v0.17: completion for `consumes` units and `given` /
                // `consumes U { … }` capabilities. Trigger on the space after a
                // keyword, the `{` of a selected-capability list, and `,`.
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        " ".to_string(),
                        "{".to_string(),
                        ",".to_string(),
                    ]),
                    ..Default::default()
                }),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_range_formatting_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: None,
                }),
                ..Default::default()
            },
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
            return Ok(None);
        }
        let items: Vec<CompletionItem> = candidates
            .into_iter()
            .map(|c| CompletionItem {
                label: c.label,
                kind: Some(match c.kind {
                    completion::CompletionKind::Unit => CompletionItemKind::MODULE,
                    completion::CompletionKind::Capability => CompletionItemKind::INTERFACE,
                }),
                detail: c.detail,
                ..Default::default()
            })
            .collect();
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
