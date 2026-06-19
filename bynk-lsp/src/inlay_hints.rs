//! v0.27 (ADR 0056): pure `inlayHint` computation — inferred-type hints
//! from a cached analysis round's harvested hint set.
//!
//! The hints arrive pre-curated from `bynkc` (annotation-absent `let` /
//! `let <-` bindings and lambda parameters, labels pre-rendered via
//! `Ty::display()`); this module only filters to the requested visible
//! range and converts positions against the analysed snapshot (the v0.24
//! rule). v0.39 (ADR 0072): the harvested set now carries a [`HintKind`] —
//! **`Type`** hints anchor at the span's **end** (the label's leading `: ` /
//! `[` reads as source: `x: Int`, `identity[Int]`, no padding); **`Parameter`**
//! hints anchor at the argument span's **start** with trailing padding, so the
//! label reads `count: 5`.

use bynkc::hints::{Hint, HintKind};
use bynkc::span::Span;
use tower_lsp::lsp_types::*;

use crate::position::offset_to_position;

/// The hints whose anchor falls inside the requested range. `text` is the
/// analysed snapshot the spans are offsets into; `hints` is one file's
/// harvested [`Hint`] set.
pub fn inlay_hints(text: &str, hints: &[Hint], requested: Span) -> Vec<InlayHint> {
    hints
        .iter()
        .filter_map(|h| {
            let anchor = match h.kind {
                HintKind::Type => h.span.end,
                HintKind::Parameter => h.span.start,
            };
            (requested.start <= anchor && anchor <= requested.end).then(|| {
                let (kind, padding_right) = match h.kind {
                    HintKind::Type => (InlayHintKind::TYPE, None),
                    HintKind::Parameter => (InlayHintKind::PARAMETER, Some(true)),
                };
                InlayHint {
                    position: offset_to_position(text, anchor),
                    label: InlayHintLabel::String(h.label.clone()),
                    kind: Some(kind),
                    text_edits: None,
                    tooltip: None,
                    padding_left: None,
                    padding_right,
                    data: None,
                }
            })
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

    fn type_hint(start: usize, end: usize, label: &str) -> Hint {
        Hint {
            span: Span::new(start, end),
            label: label.to_string(),
            kind: HintKind::Type,
        }
    }

    #[test]
    fn type_hints_anchor_after_the_name_with_no_padding() {
        //                0123456789
        let text = "let x = 1\nlet y = 2\n";
        let hints = vec![type_hint(4, 5, ": Int"), type_hint(14, 15, ": Int")];
        let got = inlay_hints(text, &hints, Span::new(0, text.len()));
        assert_eq!(got.len(), 2);
        // Anchored at the end of `x` (line 0, col 5) and `y` (line 1, col 5).
        assert_eq!(got[0].position, Position::new(0, 5));
        assert_eq!(label_of(&got[0]), ": Int");
        assert_eq!(got[0].kind, Some(InlayHintKind::TYPE));
        assert_eq!(got[0].padding_right, None);
        assert_eq!(got[1].position, Position::new(1, 5));
    }

    #[test]
    fn parameter_hints_anchor_before_the_argument_with_trailing_padding() {
        // text: `f(5)` — the argument `5` is at offset 2..3.
        let text = "f(5)\n";
        let hints = vec![Hint {
            span: Span::new(2, 3),
            label: "count:".to_string(),
            kind: HintKind::Parameter,
        }];
        let got = inlay_hints(text, &hints, Span::new(0, text.len()));
        assert_eq!(got.len(), 1);
        // Anchored at the *start* of the argument (col 2), with trailing space.
        assert_eq!(got[0].position, Position::new(0, 2));
        assert_eq!(label_of(&got[0]), "count:");
        assert_eq!(got[0].kind, Some(InlayHintKind::PARAMETER));
        assert_eq!(got[0].padding_right, Some(true));
    }

    #[test]
    fn out_of_range_hints_are_filtered() {
        let text = "let x = 1\nlet y = 2\n";
        let hints = vec![type_hint(4, 5, ": Int"), type_hint(14, 15, ": Int")];
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
