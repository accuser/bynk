//! #847: `bynk/documentationModel` — the documentation-view custom LSP request.
//!
//! The second custom request in this server (after #846's `bynk/sequenceModel`),
//! and the same posture: no `workspace/*/refresh` nudge exists for a custom
//! method and none is needed — Tier 1 is on-demand, the client re-issues the
//! request each time "Bynk: Show Documentation" fires (Decision D).
//!
//! Unlike `bynk/sequenceModel`, the request carries **no cursor position**: a
//! documentation page is the *whole file's* declarations (Decision A,
//! file-scoped), so the params are a bare `TextDocumentIdentifier`. The wire
//! shape is a plain serde mirror of [`bynk_ide::documentation::DocModel`], each
//! `Span` lowered to an LSP `Range` against the committed snapshot text the
//! caller already holds — the same convention `sequence_request`/`SerKey` use.

use bynk_ide::documentation::{self, DocModel};

/// Build the documentation model for `text` (a committed snapshot). `None` for
/// a unit with no doc page — a `suite`, or a file with no recognisable header.
pub fn documentation_model_at(text: &str) -> Option<DocModel> {
    documentation::documentation_model(text)
}

/// The `bynk/documentationModel` request payload. A bare text-document
/// identifier — no cursor position (the page is the whole file, Decision A).
///
/// `rename_all = "camelCase"` is load-bearing: the client sends the LSP wire
/// name `textDocument`, so the field must deserialize from camelCase, not the
/// Rust `text_document`. (Without it, every request fails with a missing-field
/// error — a wire-shape bug no direct `documentation_model_at` test would catch,
/// only a deserialize test; see `documentation_request.rs`'s params test.)
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
}

pub fn to_wire(model: &DocModel, text: &str) -> WireDocModel {
    WireDocModel {
        unit_name: model.unit_name.clone(),
        unit_kind: model.unit_kind,
        unit_doc: model.unit_doc.clone(),
        unit_range: crate::position::span_to_range(text, model.unit_span),
        entries: model
            .entries
            .iter()
            .map(|e| WireDocEntry {
                name: e.name.clone(),
                kind: e.kind,
                depth: e.depth,
                markdown: e.markdown.clone(),
                documented: e.documented,
                range: crate::position::span_to_range(text, e.span),
            })
            .collect(),
    }
}
