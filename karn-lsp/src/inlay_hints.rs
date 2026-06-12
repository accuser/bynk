//! v0.27 (ADR 0056): pure `inlayHint` computation — inferred-type hints
//! from a cached analysis round's harvested hint set.
//!
//! The hints arrive pre-curated from `karnc` (annotation-absent `let` /
//! `let <-` bindings and lambda parameters, labels pre-rendered via
//! `Ty::display()`); this module only filters to the requested visible
//! range and converts positions against the analysed snapshot (the v0.24
//! rule). A hint anchors at the **end** of its binding-name span, so the
//! label's leading `: ` reads as source syntax (`x: Int`) — no padding is
//! requested, the separator is part of the label.

use karnc::span::Span;
use tower_lsp::lsp_types::*;

use crate::position::offset_to_position;

/// The hints whose anchor (binding-name end) falls inside the requested
/// range. `text` is the analysed snapshot the spans are offsets into;
/// `hints` is one file's harvested `(binding-name span, label)` set.
pub fn inlay_hints(text: &str, hints: &[(Span, String)], requested: Span) -> Vec<InlayHint> {
    hints
        .iter()
        .filter(|(span, _)| requested.start <= span.end && span.end <= requested.end)
        .map(|(span, label)| InlayHint {
            position: offset_to_position(text, span.end),
            label: InlayHintLabel::String(label.clone()),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: None,
            data: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label_of(h: &InlayHint) -> &str {
        match &h.label {
            InlayHintLabel::String(s) => s,
            other => panic!("expected a plain string label, got {other:?}"),
        }
    }

    #[test]
    fn hints_in_range_position_after_the_binding_name() {
        //                0123456789
        let text = "let x = 1\nlet y = 2\n";
        let hints = vec![
            (Span::new(4, 5), ": Int".to_string()),
            (Span::new(14, 15), ": Int".to_string()),
        ];
        let got = inlay_hints(text, &hints, Span::new(0, text.len()));
        assert_eq!(got.len(), 2);
        // Anchored at the end of `x` (line 0, col 5) and `y` (line 1, col 5).
        assert_eq!(got[0].position, Position::new(0, 5));
        assert_eq!(label_of(&got[0]), ": Int");
        assert_eq!(got[0].kind, Some(InlayHintKind::TYPE));
        assert_eq!(got[1].position, Position::new(1, 5));
    }

    #[test]
    fn out_of_range_hints_are_filtered() {
        let text = "let x = 1\nlet y = 2\n";
        let hints = vec![
            (Span::new(4, 5), ": Int".to_string()),
            (Span::new(14, 15), ": Int".to_string()),
        ];
        // Only the first line is visible.
        let got = inlay_hints(text, &hints, Span::new(0, 9));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].position, Position::new(0, 5));
    }

    #[test]
    fn empty_hint_set_returns_empty() {
        assert!(inlay_hints("let x = 1\n", &[], Span::new(0, 9)).is_empty());
    }
}
