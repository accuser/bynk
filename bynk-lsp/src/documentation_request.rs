//! #847: `bynk/documentationModel` — the documentation-view custom LSP request.
//!
//! The second custom request in this server (after #846's `bynk/sequenceModel`),
//! and the same posture: no `workspace/*/refresh` nudge exists for a custom
//! method and none is needed — Tier 1 is on-demand, the client re-issues the
//! request each time "Bynk: Show Documentation" fires (Decision D).
//!
//! Unlike `bynk/sequenceModel`, the request carries **no cursor position**: a
//! documentation page is the *whole unit's* declarations, so the params are a
//! bare `TextDocumentIdentifier`. The page was file-scoped (Decision A) until
//! #1885, which merges every file of a multi-file unit
//! ([`documentation_model_for`]). The wire shape is a plain serde mirror of
//! [`bynk_ide::documentation::DocModel`], each `Span` lowered to an LSP `Range`
//! against the committed snapshot text of the file it is in — the same
//! convention `sequence_request`/`SerKey` use — with a per-entry `uri` naming
//! that file (the `{uri, range}` pattern `architecture_request` uses).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bynk_ide::documentation;
use tower_lsp::lsp_types::Url;

/// The `bynk/documentationModel` request payload. A bare text-document
/// identifier — no cursor position (the page is the whole unit).
///
/// `rename_all = "camelCase"` is load-bearing: the client sends the LSP wire
/// name `textDocument`, so the field must deserialize from camelCase, not the
/// Rust `text_document`. (Without it, every request fails with a missing-field
/// error — a wire-shape bug no direct `documentation_model_for` test would
/// catch, only a deserialize test; see `documentation_request.rs`'s params
/// test.)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentationModelParams {
    pub text_document: tower_lsp::lsp_types::TextDocumentIdentifier,
}

// -- Wire shape: a plain serde mirror of `bynk_ide::documentation::DocModel`,
// -- `Span` lowered to `Range` against the committed snapshot text.

#[derive(Debug, Clone, serde::Serialize)]
pub struct WireDocModel {
    #[serde(rename = "unitName")]
    pub unit_name: String,
    #[serde(rename = "unitKind")]
    pub unit_kind: &'static str,
    #[serde(rename = "unitDoc")]
    pub unit_doc: Option<String>,
    #[serde(rename = "unitRange")]
    pub unit_range: tower_lsp::lsp_types::Range,
    pub entries: Vec<WireDocEntry>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WireDocEntry {
    pub name: String,
    pub kind: &'static str,
    pub depth: u32,
    pub markdown: String,
    pub documented: bool,
    pub range: tower_lsp::lsp_types::Range,
    /// #1885: the document `range` is in. Always sent: the page is merged
    /// across a unit's files, so an entry may live in a file other than the
    /// requested one (and a one-file unit's entries simply all name it).
    pub uri: tower_lsp::lsp_types::Url,
}

/// #1885: the wire page for the document at `rel` (project-relative), merged
/// across every file of its unit. The unit's files come from the round's
/// `unit_sources` (qualified name → project-relative paths) and their text
/// from `snapshots`; each entry's range is lowered against *its own* file and
/// carries that file's `file://` URI, so click-to-code opens the right one.
/// An entry whose file has no snapshot or no URI is dropped rather than sent
/// with a wrong location. `None` for a `suite`, a file with no header, or a
/// `rel` with no snapshot.
pub fn documentation_model_for(
    rel: &Path,
    snapshots: &HashMap<PathBuf, String>,
    unit_sources: &HashMap<String, Vec<PathBuf>>,
    project_root: &Path,
) -> Option<WireDocModel> {
    let text = snapshots.get(rel)?;
    let siblings: Vec<(&Path, &str)> = bynk_ide::symbols::own_declaration_name(text)
        .and_then(|(name, _)| unit_sources.get(&name))
        .into_iter()
        .flatten()
        .filter_map(|p| snapshots.get(p).map(|t| (p.as_path(), t.as_str())))
        .collect();
    let model = documentation::documentation_model_merged((rel, text.as_str()), siblings)?;
    // Each entry's range is lowered once, against the snapshot of the file
    // its span is in.
    let entries = model
        .entries
        .iter()
        .filter_map(|e| {
            let file = e.file.as_deref().unwrap_or(rel);
            let file_text = snapshots.get(file)?;
            Some(WireDocEntry {
                name: e.name.clone(),
                kind: e.kind,
                depth: e.depth,
                markdown: e.markdown.clone(),
                documented: e.documented,
                range: crate::position::span_to_range(file_text, e.span),
                uri: Url::from_file_path(project_root.join(file)).ok()?,
            })
        })
        .collect();
    Some(WireDocModel {
        unit_name: model.unit_name,
        unit_kind: model.unit_kind,
        unit_doc: model.unit_doc,
        unit_range: crate::position::span_to_range(text, model.unit_span),
        entries,
    })
}
