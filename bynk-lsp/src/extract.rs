//! v0.213 (ADR 0239): `textDocument/codeAction` extract-variable —
//! `CodeActionKind::REFACTOR_EXTRACT`. The smallest expression node whose
//! span fully contains the requested selection is bound to a fresh `let`
//! inserted immediately above its enclosing statement (or block tail), and
//! the selection's span is replaced with the new name.
//!
//! Like [`crate::structure`], this reparses the live document — no cached AST
//! is retained in `Analysis`, and extraction is selection-driven the same way
//! folding/selection ranges are.

use bynk_syntax::ast::*;
use bynk_syntax::lexer::tokenize;
use bynk_syntax::parser::parse_unit_with_recovery;
use bynk_syntax::span::Span;
use tower_lsp::lsp_types::*;

/// Extract-variable actions for the requested range. Only offered for a
/// genuine selection (`requested.start != requested.end`) — a bare cursor
/// offers nothing, the refactor-vs-quickfix posture other editors take.
pub fn extract_variable(
    text: &str,
    requested: Span,
    uri: &Url,
    version: Option<i32>,
) -> Vec<CodeActionOrCommand> {
    if requested.start == requested.end {
        return Vec::new();
    }
    let Ok(tokens) = tokenize(text) else {
        return Vec::new();
    };
    let (Some(unit), _errs) = parse_unit_with_recovery(&tokens, text) else {
        return Vec::new();
    };
    let Some(site) = find_site(&unit, requested) else {
        return Vec::new();
    };

    let selected = &text[site.expr_span.start..site.expr_span.end];
    let name = fresh_name(text);
    let indent = line_indent(text, site.insertion_offset);
    let insert_pos = crate::position::offset_to_position(text, site.insertion_offset);

    let edits = vec![
        OneOf::Left(TextEdit {
            range: Range::new(insert_pos, insert_pos),
            new_text: format!("let {name} = {selected}\n{indent}"),
        }),
        OneOf::Left(TextEdit {
            range: crate::position::span_to_range(text, site.expr_span),
            new_text: name.clone(),
        }),
    ];
    vec![CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("Extract variable `{name}`"),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
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
    })]
}

/// The smallest expression node's span, and the byte offset immediately
/// before its enclosing statement (or block tail) — where the new `let`
/// line is inserted.
struct Site {
    insertion_offset: usize,
    expr_span: Span,
}

/// Closed containment over half-open spans: `outer` fully contains `inner`.
fn contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// Finds the body (fn / handler / provider op / test case) whose span
/// contains `target`, then hands off to [`find_in_block`]. `None` when the
/// selection sits outside any body (a header, or a file that doesn't parse).
fn find_site(unit: &SourceUnit, target: Span) -> Option<Site> {
    match unit {
        SourceUnit::Commons(c) => find_in_items(&c.items, target),
        SourceUnit::Context(c) => find_in_items(&c.items, target),
        // Adapters carry no Bynk-bodied operations to extract from.
        SourceUnit::Adapter(_) => None,
        SourceUnit::Suite(t) => t
            .cases
            .iter()
            .find(|case| contains(case.body.span, target))
            .and_then(|case| find_in_block(&case.body, target)),
    }
}

fn find_in_items(items: &[CommonsItem], target: Span) -> Option<Site> {
    for item in items {
        match item {
            CommonsItem::Fn(f) if contains(f.body.span, target) => {
                return find_in_block(&f.body, target);
            }
            CommonsItem::Provider(p) => {
                if let Some(op) = p.ops.iter().find(|op| contains(op.body.span, target)) {
                    return find_in_block(&op.body, target);
                }
            }
            CommonsItem::Service(s) => {
                if let Some(h) = s.handlers.iter().find(|h| contains(h.body.span, target)) {
                    return find_in_block(&h.body, target);
                }
            }
            CommonsItem::Agent(a) => {
                if let Some(h) = a.handlers.iter().find(|h| contains(h.body.span, target)) {
                    return find_in_block(&h.body, target);
                }
            }
            _ => {}
        }
    }
    None
}

/// Finds the statement (or the tail) whose value fully contains `target`,
/// then narrows within it via [`locate`]. `None` when `target` doesn't sit
/// fully inside any single statement/tail (e.g. it spans the whole block,
/// braces included, or crosses a statement boundary).
fn find_in_block(block: &Block, target: Span) -> Option<Site> {
    for stmt in &block.statements {
        let mut values = Vec::new();
        statement_exprs(stmt, &mut values);
        let Some(value) = values.first() else {
            continue;
        };
        if contains(value.span, target) {
            return Some(locate(value, target, stmt.span().start));
        }
    }
    if contains(block.tail.span, target) {
        return Some(locate(&block.tail, target, block.tail.span.start));
    }
    None
}

/// Descends into `expr` for the smallest node whose span fully contains
/// `target`. `Block`/`If`/`Match` carry their own nested statement/tail
/// slots, so descending into one resets `insertion_offset` to that slot's
/// start; every other kind bubbles the offset through unchanged, walked via
/// [`expr_children`] — `bynk_syntax`'s exhaustive-by-construction child
/// iterator, reused here rather than a second hand-rolled `ExprKind` match
/// (an `ExprKind` variant this doesn't handle would otherwise silently fall
/// through the extraction path instead of failing to compile).
fn locate(expr: &Expr, target: Span, insertion_offset: usize) -> Site {
    match &expr.kind {
        ExprKind::Block(b) => find_in_block(b, target).unwrap_or(Site {
            insertion_offset,
            expr_span: expr.span,
        }),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            if contains(cond.span, target) {
                return locate(cond, target, insertion_offset);
            }
            if contains(then_block.span, target)
                && let Some(site) = find_in_block(then_block, target)
            {
                return site;
            }
            if contains(else_block.span, target)
                && let Some(site) = find_in_block(else_block, target)
            {
                return site;
            }
            Site {
                insertion_offset,
                expr_span: expr.span,
            }
        }
        ExprKind::Match { discriminant, arms } => {
            if contains(discriminant.span, target) {
                return locate(discriminant, target, insertion_offset);
            }
            for arm in arms {
                if !contains(arm.body.span(), target) {
                    continue;
                }
                return match &arm.body {
                    MatchBody::Expr(e) => locate(e, target, insertion_offset),
                    MatchBody::Block(b) => find_in_block(b, target).unwrap_or(Site {
                        insertion_offset,
                        expr_span: expr.span,
                    }),
                };
            }
            Site {
                insertion_offset,
                expr_span: expr.span,
            }
        }
        _ => {
            let children = expr_children(expr);
            match children.into_iter().find(|c| contains(c.span, target)) {
                Some(child) => locate(child, target, insertion_offset),
                None => Site {
                    insertion_offset,
                    expr_span: expr.span,
                },
            }
        }
    }
}

/// `extracted`, `extracted2`, … — the first candidate that doesn't already
/// appear as a whole word anywhere in the file. A collision-avoiding
/// placeholder, not a scope-aware binder: the client's rename-on-extract is
/// the expected next step for a better name.
fn fresh_name(text: &str) -> String {
    let is_ident_char = |c: char| c.is_alphanumeric() || c == '_';
    let occurs_as_word = |candidate: &str| {
        text.match_indices(candidate).any(|(i, _)| {
            let before_ok = text[..i]
                .chars()
                .next_back()
                .is_none_or(|c| !is_ident_char(c));
            let after_ok = text[i + candidate.len()..]
                .chars()
                .next()
                .is_none_or(|c| !is_ident_char(c));
            before_ok && after_ok
        })
    };
    let mut n = 1;
    loop {
        let candidate = if n == 1 {
            "extracted".to_string()
        } else {
            format!("extracted{n}")
        };
        if !occurs_as_word(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// The whitespace-only run from `offset`'s line start up to `offset` — empty
/// if that run isn't pure whitespace (e.g. a single-line body has no
/// indentation to mirror; `bynk fmt` cleans up the result, the same
/// tolerance the quick-fix path takes).
fn line_indent(text: &str, offset: usize) -> String {
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let prefix = &text[line_start..offset];
    if prefix.chars().all(|c| c == ' ' || c == '\t') {
        prefix.to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actions_for(text: &str, needle: &str) -> Vec<CodeActionOrCommand> {
        let start = text.find(needle).expect("needle present");
        let requested = Span::new(start, start + needle.len());
        let uri = Url::parse("file:///a.bynk").unwrap();
        extract_variable(text, requested, &uri, Some(3))
    }

    fn sole_edit(action: &CodeActionOrCommand) -> Vec<TextEdit> {
        let CodeActionOrCommand::CodeAction(a) = action else {
            panic!("expected a CodeAction");
        };
        assert_eq!(a.kind, Some(CodeActionKind::REFACTOR_EXTRACT));
        let Some(DocumentChanges::Edits(doc_edits)) = &a.edit.as_ref().unwrap().document_changes
        else {
            panic!("expected versioned document edits");
        };
        assert_eq!(doc_edits[0].text_document.version, Some(3));
        doc_edits[0]
            .edits
            .iter()
            .map(|e| {
                let OneOf::Left(edit) = e else {
                    panic!("expected a plain TextEdit")
                };
                edit.clone()
            })
            .collect()
    }

    #[test]
    fn extracts_a_sub_expression_of_a_let_value() {
        let src = "context c\n\nfn f() -> Int {\n  let y = 1 + 2\n  y\n}\n";
        let actions = actions_for(src, "1 + 2");
        assert_eq!(actions.len(), 1);
        let edits = sole_edit(&actions[0]);
        assert_eq!(edits.len(), 2);
        assert!(edits[0].new_text.starts_with("let extracted = 1 + 2\n"));
        assert_eq!(edits[1].new_text, "extracted");
        // The insertion is a zero-length range right at the start of the
        // `let y = …` line — same line the replacement (later on that line)
        // sits on, since positions are computed against the original text.
        assert_eq!(edits[0].range.start, edits[0].range.end);
        assert_eq!(edits[0].range.start.line, edits[1].range.start.line);
        assert!(edits[0].range.start.character < edits[1].range.start.character);
    }

    #[test]
    fn extracts_the_whole_tail_expression() {
        let src = "context c\n\nfn f() -> Int {\n  let x = 1\n  x + 1\n}\n";
        let actions = actions_for(src, "x + 1");
        let edits = sole_edit(&actions[0]);
        assert!(edits[0].new_text.starts_with("let extracted = x + 1\n"));
        assert_eq!(edits[1].new_text, "extracted");
    }

    #[test]
    fn resets_the_insertion_point_inside_a_nested_if_block() {
        let src = concat!(
            "context c\n\n",
            "fn f(n: Int) -> Int {\n",
            "  if n > 0 {\n",
            "    let z = n * 2\n",
            "    z\n",
            "  } else {\n",
            "    0\n",
            "  }\n",
            "}\n",
        );
        let actions = actions_for(src, "n * 2");
        let edits = sole_edit(&actions[0]);
        // Inserted right above `let z = …` inside the `if` branch, not above
        // the whole `if` statement.
        let insert_line = edits[0].range.start.line;
        let let_z_line = crate::position::offset_to_position(src, src.find("let z").unwrap()).line;
        assert_eq!(insert_line, let_z_line);
    }

    #[test]
    fn extracts_a_call_argument() {
        let src = "context c\n\nfn f() -> Int {\n  g(1 + 2, 3)\n}\n";
        let actions = actions_for(src, "1 + 2");
        let edits = sole_edit(&actions[0]);
        assert_eq!(edits[1].new_text, "extracted");
        assert!(edits[0].new_text.contains("let extracted = 1 + 2"));
    }

    #[test]
    fn bumps_the_suffix_on_a_name_collision() {
        let src = "context c\n\nfn f() -> Int {\n  let extracted = 0\n  1 + 2\n}\n";
        let actions = actions_for(src, "1 + 2");
        let edits = sole_edit(&actions[0]);
        assert!(edits[0].new_text.starts_with("let extracted2 = 1 + 2\n"));
        assert_eq!(edits[1].new_text, "extracted2");
    }

    #[test]
    fn empty_selection_offers_nothing() {
        let src = "context c\n\nfn f() -> Int {\n  1 + 2\n}\n";
        let off = src.find("1 + 2").unwrap();
        let uri = Url::parse("file:///a.bynk").unwrap();
        let actions = extract_variable(src, Span::new(off, off), &uri, Some(1));
        assert!(actions.is_empty());
    }

    #[test]
    fn selection_outside_any_body_offers_nothing() {
        let src = "context c\n\nfn f() -> Int {\n  1 + 2\n}\n";
        let actions = actions_for(src, "context c");
        assert!(actions.is_empty());
    }

    #[test]
    fn preserves_indentation_on_a_multi_statement_block() {
        let src = "context c\n\nfn f() -> Int {\n    let a = 1\n    let b = a + 2\n    b\n}\n";
        let actions = actions_for(src, "a + 2");
        let edits = sole_edit(&actions[0]);
        assert_eq!(edits[0].new_text, "let extracted = a + 2\n    ");
    }
}
