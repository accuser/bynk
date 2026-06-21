//! v0.26 (ADR 0054): pure `codeAction` computation — quick-fixes from the
//! structured [`bynk_syntax::error::Suggestion`]s riding on a cached analysis
//! round's diagnostics.
//!
//! Keying rule: a diagnostic's suggestions are offered when the requested
//! range intersects the **diagnostic's** span — never the edits' spans,
//! which for both `given` fixes land away from the squiggle (the usage site
//! in the body vs the clause in the signature). Positions convert against
//! the analysed snapshot (the v0.24 rule); edits are **versioned** against
//! the analysed document version, so a drifted buffer rejects the edit
//! rather than mis-applying it.

use bynk_syntax::error::Applicability;
use bynk_syntax::span::Span;
use tower_lsp::lsp_types::*;

/// Quick-fixes for every suggestion whose owning diagnostic intersects the
/// requested range. `text` and `version` are the analysed snapshot and the
/// open-document version captured with it.
pub fn quick_fixes(
    text: &str,
    diagnostics: &[bynk_ide::Diagnostic],
    requested: Span,
    uri: &Url,
    version: Option<i32>,
) -> Vec<CodeActionOrCommand> {
    let mut out = Vec::new();
    for d in diagnostics {
        if !intersects(d.error.span, requested) {
            continue;
        }
        for s in &d.error.suggestions {
            // Only `MachineApplicable` fixes are offered as one-click edits;
            // `HasPlaceholders` has no concrete replacement to apply.
            if s.applicability != Applicability::MachineApplicable {
                continue;
            }
            let edits: Vec<OneOf<TextEdit, AnnotatedTextEdit>> = s
                .edits
                .iter()
                .map(|(span, replacement)| {
                    OneOf::Left(TextEdit {
                        range: crate::position::span_to_range(text, *span),
                        new_text: replacement.clone(),
                    })
                })
                .collect();
            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: s.message.clone(),
                kind: Some(CodeActionKind::QUICKFIX),
                edit: Some(WorkspaceEdit {
                    changes: None,
                    document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                        text_document: OptionalVersionedTextDocumentIdentifier {
                            uri: uri.clone(),
                            version,
                        },
                        edits,
                    }])),
                    change_annotations: None,
                }),
                ..Default::default()
            }));
        }
    }
    out
}

/// Closed intersection over half-open spans: a cursor request (an empty
/// range) sitting on either boundary of the diagnostic still matches.
fn intersects(a: Span, b: Span) -> bool {
    a.start <= b.end && b.start <= a.end
}

#[cfg(test)]
mod tests {
    use super::*;
    use bynk_syntax::error::CompileError;

    fn diag_with_suggestion() -> bynk_ide::Diagnostic {
        // text: "-> T given Cap { Used.op() }" — diagnostic on the usage at
        // 17..21, fix inserting at the clause (14, far from the squiggle).
        bynk_ide::Diagnostic {
            severity: bynk_syntax::Severity::Error,
            error: CompileError::new(
                "bynk.given.undeclared_capability",
                Span::new(17, 21),
                "capability `Used` is used but not listed",
            )
            .with_suggestion(
                "add `Used` to the `given` clause",
                vec![(Span::new(14, 14), ", Used".to_string())],
                Applicability::MachineApplicable,
            ),
        }
    }

    #[test]
    fn keyed_on_the_diagnostic_span_not_the_edit_span() {
        let text = "-> T given Cap { Used.op() }";
        let uri = Url::parse("file:///a.bynk").unwrap();
        // Cursor on the squiggle (the usage site): the fix is offered even
        // though its edit lands elsewhere.
        let on_diag = quick_fixes(
            text,
            &[diag_with_suggestion()],
            Span::new(18, 18),
            &uri,
            Some(7),
        );
        assert_eq!(on_diag.len(), 1);
        // Cursor away from the diagnostic (even on the edit's own span):
        // nothing is offered.
        let on_edit = quick_fixes(
            text,
            &[diag_with_suggestion()],
            Span::new(14, 14),
            &uri,
            Some(7),
        );
        assert!(on_edit.is_empty());
    }

    #[test]
    fn action_carries_a_versioned_quickfix_edit() {
        let text = "-> T given Cap { Used.op() }";
        let uri = Url::parse("file:///a.bynk").unwrap();
        let actions = quick_fixes(
            text,
            &[diag_with_suggestion()],
            Span::new(17, 21),
            &uri,
            Some(7),
        );
        let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
            panic!("expected a CodeAction");
        };
        assert_eq!(action.title, "add `Used` to the `given` clause");
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        let Some(DocumentChanges::Edits(doc_edits)) =
            &action.edit.as_ref().unwrap().document_changes
        else {
            panic!("expected versioned document edits");
        };
        assert_eq!(doc_edits[0].text_document.version, Some(7));
        assert_eq!(doc_edits[0].text_document.uri, uri);
        let OneOf::Left(edit) = &doc_edits[0].edits[0] else {
            panic!("expected a plain TextEdit");
        };
        assert_eq!(edit.new_text, ", Used");
        // The insertion converts to an empty range at the clause position.
        assert_eq!(edit.range.start, edit.range.end);
        assert_eq!(edit.range.start.character, 14);
    }

    #[test]
    fn placeholder_suggestions_are_not_offered() {
        let text = "x";
        let uri = Url::parse("file:///a.bynk").unwrap();
        let d = bynk_ide::Diagnostic {
            severity: bynk_syntax::Severity::Error,
            error: CompileError::new("bynk.test", Span::new(0, 1), "msg").with_suggestion(
                "fill in <T>",
                vec![(Span::new(0, 1), "<T>".to_string())],
                Applicability::HasPlaceholders,
            ),
        };
        assert!(quick_fixes(text, &[d], Span::new(0, 1), &uri, None).is_empty());
    }
}
