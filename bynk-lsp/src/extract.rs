//! v0.213 (ADR 0239): `textDocument/codeAction` extract-variable —
//! `CodeActionKind::REFACTOR_EXTRACT`. The smallest expression node whose
//! span fully contains the requested selection is bound to a fresh `let`
//! inserted immediately above its enclosing statement (or block tail), and
//! the selection's span is replaced with the new name.
//!
//! `text` is whatever snapshot the caller passes in — for `code_action` that's
//! the last **committed** analysis round's snapshot (ADR 0235's
//! stale-while-revalidate posture), not necessarily the very latest keystroke.
//! Like [`crate::structure`], this reparses that snapshot fresh each call — no
//! cached AST is retained in `Analysis`, and extraction is selection-driven
//! the same way folding/selection ranges are.
//!
//! Track #800 settles as: [`extract_function`] reuses the exact same
//! smallest-containing-expression selection algorithm, but lifts the
//! expression into a new top-level `fn` (threading its free identifiers as
//! parameters) rather than a local `let`. It is capability-free-only — `fn`
//! has no `given` clause to propagate a capability-using body into, unlike
//! `Handler`/`Provider` (that language-change question is deliberately not
//! taken on here) — so it declines whenever the selection's site carries any
//! recorded [`Requirement`], covered or not.

use bynk_check::checker::Ty;
use bynk_check::locals::{LocalBinding, locals_at};
use bynk_check::requirements::Requirement;
use bynk_syntax::ast::*;
use bynk_syntax::lexer::tokenize;
use bynk_syntax::parser::parse_unit_with_recovery;
use bynk_syntax::span::Span;
use std::collections::HashMap;
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

/// Extract-function actions for the requested range (track #800). Locates
/// the same smallest-containing-expression the extract-variable selection
/// algorithm would, but only within a `Commons`/`Context` file's top-level
/// `fn`/`Provider`/`Service`/`Agent` items — a new top-level `fn` needs
/// somewhere top-level to live, so an `Adapter` (no Bynk bodies) or a `Suite`
/// test case (no enclosing top-level item to insert above) offers nothing.
///
/// Declines (offers nothing) rather than guessing whenever:
/// - the selection's site carries any recorded capability [`Requirement`],
///   covered or not — the settled capability-free-only surface; a plain `fn`
///   has no `given` to cover it once lifted.
/// - the selection's expression type, or any free variable's type, isn't
///   available — both `expr_types` and the rendered `locals` types are
///   Ok-path captures (ADR 0063's clean-file ceiling), so a file with an
///   unrelated error elsewhere yields no action rather than a guessed type.
/// - two distinct outer-scope bindings share a free variable's name (a rare
///   nested-shadow collision) — threading either one as the parameter would
///   silently pick the wrong variable for the other occurrence.
///
/// Free variables are the selection's `Ident` references (walked via
/// [`expr_children`], the same exhaustive child iterator `locate` uses) whose
/// nearest enclosing binding — [`locals_at`] at the reference's own offset —
/// sits outside the selection; an identifier with no local binding at all
/// (a top-level `fn`, a capability, a type name) is left alone, already
/// resolvable at the new `fn`'s own top-level scope.
pub fn extract_function(
    text: &str,
    requested: Span,
    uri: &Url,
    version: Option<i32>,
    requirements: &[Requirement],
    locals: &[LocalBinding],
    expr_types: &[(Span, Ty)],
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
    let Some(site) = find_function_site(&unit, requested) else {
        return Vec::new();
    };
    if requirements
        .iter()
        .any(|r| contains(site.expr_span, r.site))
    {
        return Vec::new();
    }
    let Some(ret_ty) = ty_at_span(expr_types, site.expr_span) else {
        return Vec::new();
    };

    let mut idents = Vec::new();
    collect_idents(site.expr, &mut idents);
    let mut free: Vec<(&str, Span, &str)> = Vec::new();
    for id in &idents {
        let Some(binding) = locals_at(locals, id.span.start)
            .into_iter()
            .find(|b| b.name == id.name)
        else {
            continue;
        };
        if contains(site.expr_span, binding.def_span) {
            continue; // bound inside the selection itself, not free
        }
        free.push((id.name.as_str(), binding.def_span, binding.ty.as_str()));
    }
    let mut def_by_name: HashMap<&str, Span> = HashMap::new();
    for (name, def_span, _) in &free {
        match def_by_name.get(name) {
            Some(prev) if *prev != *def_span => return Vec::new(),
            _ => {
                def_by_name.insert(name, *def_span);
            }
        }
    }
    let mut params: Vec<(&str, &str)> = Vec::new();
    for (name, _, ty) in &free {
        if !params.iter().any(|(n, _)| n == name) {
            params.push((name, ty));
        }
    }

    let fn_name = fresh_word(text, "extractedFn");
    let param_list = params
        .iter()
        .map(|(n, t)| format!("{n}: {t}"))
        .collect::<Vec<_>>()
        .join(", ");
    let arg_list = params
        .iter()
        .map(|(n, _)| n.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let selected = &text[site.expr_span.start..site.expr_span.end];
    let new_fn = format!(
        "fn {fn_name}({param_list}) -> {} {{\n  {selected}\n}}\n\n",
        ret_ty.display()
    );
    let insert_pos = crate::position::offset_to_position(text, site.item_start);

    let edits = vec![
        OneOf::Left(TextEdit {
            range: Range::new(insert_pos, insert_pos),
            new_text: new_fn,
        }),
        OneOf::Left(TextEdit {
            range: crate::position::span_to_range(text, site.expr_span),
            new_text: format!("{fn_name}({arg_list})"),
        }),
    ];
    vec![CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("Extract function `{fn_name}`"),
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

/// The smallest expression node fully containing the selection, and the byte
/// offset immediately before its enclosing statement (or block tail) — where
/// the new `let` line is inserted. `expr` is retained (not just its span) so
/// [`extract_function`] can walk it for free identifiers without a second
/// descent through the tree.
struct Site<'a> {
    insertion_offset: usize,
    expr_span: Span,
    expr: &'a Expr,
}

/// Closed containment over half-open spans: `outer` fully contains `inner`.
fn contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// Finds the body (fn / handler / provider op / test case) whose span
/// contains `target`, then hands off to [`find_in_block`]. `None` when the
/// selection sits outside any body (a header, or a file that doesn't parse).
fn find_site(unit: &SourceUnit, target: Span) -> Option<Site<'_>> {
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

fn find_in_items(items: &[CommonsItem], target: Span) -> Option<Site<'_>> {
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
fn find_in_block(block: &Block, target: Span) -> Option<Site<'_>> {
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
fn locate(expr: &Expr, target: Span, insertion_offset: usize) -> Site<'_> {
    match &expr.kind {
        ExprKind::Block(b) => find_in_block(b, target).unwrap_or(Site {
            insertion_offset,
            expr_span: expr.span,
            expr,
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
                expr,
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
                        expr,
                    }),
                };
            }
            Site {
                insertion_offset,
                expr_span: expr.span,
                expr,
            }
        }
        _ => {
            let children = expr_children(expr);
            match children.into_iter().find(|c| contains(c.span, target)) {
                Some(child) => locate(child, target, insertion_offset),
                None => Site {
                    insertion_offset,
                    expr_span: expr.span,
                    expr,
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
    fresh_word(text, "extracted")
}

/// Like [`fresh_name`], generalised to any base word — extract-function uses
/// `extractedFn`/`extractedFn2`/… so its generated name reads distinctly from
/// extract-variable's `extracted` when both actions are offered on the same
/// selection.
fn fresh_word(text: &str, base: &str) -> String {
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
            base.to_string()
        } else {
            format!("{base}{n}")
        };
        if !occurs_as_word(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// The enclosing top-level item's span — where [`find_function_site`] inserts
/// the new `fn` (immediately above it). `None` for a `CommonsItem` extraction
/// can never target (a `type`/`context` declaration carries no body).
fn item_span(item: &CommonsItem) -> Option<Span> {
    match item {
        CommonsItem::Fn(f) => Some(f.span),
        CommonsItem::Provider(p) => Some(p.span),
        CommonsItem::Service(s) => Some(s.span),
        CommonsItem::Agent(a) => Some(a.span),
        _ => None,
    }
}

/// Like [`find_site`], but only within `Commons`/`Context` files, and
/// additionally records the start of the enclosing top-level item — where the
/// new `fn` is inserted, immediately above it.
struct FunctionSite<'a> {
    item_start: usize,
    expr_span: Span,
    expr: &'a Expr,
}

fn find_function_site(unit: &SourceUnit, target: Span) -> Option<FunctionSite<'_>> {
    let items = match unit {
        SourceUnit::Commons(c) => &c.items,
        SourceUnit::Context(c) => &c.items,
        SourceUnit::Adapter(_) | SourceUnit::Suite(_) => return None,
    };
    let item = items
        .iter()
        .find(|it| item_span(it).is_some_and(|s| contains(s, target)))?;
    let site = find_in_items(std::slice::from_ref(item), target)?;
    Some(FunctionSite {
        item_start: item_span(item)?.start,
        expr_span: site.expr_span,
        expr: site.expr,
    })
}

/// `expr` and every `Ident` reference nested inside it — walked via
/// [`expr_children`], the same exhaustive-by-construction iterator `locate`
/// uses, so a future `ExprKind` variant can't silently drop out of either
/// walk without the other noticing.
fn collect_idents<'a>(expr: &'a Expr, out: &mut Vec<&'a Ident>) {
    if let ExprKind::Ident(id) = &expr.kind {
        out.push(id);
    }
    for child in expr_children(expr) {
        collect_idents(child, out);
    }
}

/// The recorded type of the expression whose span is exactly `span` — an
/// exact match, not [`bynk_check::expr_types::type_at_offset`]'s tightest-
/// containing-offset search, since the caller already knows the precise node.
fn ty_at_span(entries: &[(Span, Ty)], span: Span) -> Option<&Ty> {
    entries.iter().find(|(s, _)| *s == span).map(|(_, t)| t)
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
    fn resets_the_insertion_point_inside_a_nested_match_arm_block() {
        let src = concat!(
            "context c\n\n",
            "fn f(n: Int) -> Int {\n",
            "  match n {\n",
            "    0 => {\n",
            "      let z = n * 2\n",
            "      z\n",
            "    }\n",
            "    _ => 0\n",
            "  }\n",
            "}\n",
        );
        let actions = actions_for(src, "n * 2");
        let edits = sole_edit(&actions[0]);
        // Inserted right above `let z = …` inside the arm's block, not above
        // the whole `match`.
        let insert_line = edits[0].range.start.line;
        let let_z_line = crate::position::offset_to_position(src, src.find("let z").unwrap()).line;
        assert_eq!(insert_line, let_z_line);
    }

    #[test]
    fn selection_crossing_a_statement_boundary_offers_nothing() {
        let src = "context c\n\nfn f() -> Int {\n  let a = 1\n  let b = 2\n  a\n}\n";
        // Spans from inside `let a`'s value into the start of `let b` — no
        // single statement/tail value fully contains it.
        let actions = actions_for(src, "1\n  let b");
        assert!(actions.is_empty());
    }

    #[test]
    fn selection_of_the_whole_block_braces_included_offers_nothing() {
        let src = "context c\n\nfn f() -> Int {\n  let a = 1\n  a\n}\n";
        // The full body span, braces included — larger than any single
        // statement/tail value, so no candidate node covers it.
        let actions = actions_for(src, "{\n  let a = 1\n  a\n}");
        assert!(actions.is_empty());
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

    mod extract_function_tests {
        use super::*;
        use bynk_check::locals::LocalKind;
        use bynk_check::requirements::RequirementSource;

        fn function_actions_for(
            text: &str,
            needle: &str,
            requirements: &[Requirement],
            locals: &[LocalBinding],
            expr_types: &[(Span, Ty)],
        ) -> Vec<CodeActionOrCommand> {
            let start = text.find(needle).expect("needle present");
            let requested = Span::new(start, start + needle.len());
            let uri = Url::parse("file:///a.bynk").unwrap();
            extract_function(
                text,
                requested,
                &uri,
                Some(3),
                requirements,
                locals,
                expr_types,
            )
        }

        fn nth_offset(text: &str, needle: &str, n: usize) -> usize {
            text.match_indices(needle)
                .nth(n)
                .expect("occurrence present")
                .0
        }

        // Permissive (whole-file) scope: these tests exercise extract_function's
        // own gating/free-variable logic, not locals_at's scope resolution
        // (already covered in bynk-check::locals's own tests).
        fn param(text: &str, name: &str, ty: &str) -> LocalBinding {
            let def_start = nth_offset(text, name, 0);
            LocalBinding {
                name: name.to_string(),
                def_span: Span::new(def_start, def_start + name.len()),
                kind: LocalKind::Param,
                ty: ty.to_string(),
                scope: Span::new(0, text.len()),
            }
        }

        fn int_type(text: &str, needle: &str) -> (Span, Ty) {
            let start = text.find(needle).expect("needle present");
            (
                Span::new(start, start + needle.len()),
                Ty::Base(BaseType::Int),
            )
        }

        fn capability_use(site_needle_offset: usize, len: usize) -> Requirement {
            Requirement {
                capability: "Clock".to_string(),
                site: Span::new(site_needle_offset, site_needle_offset + len),
                source: RequirementSource::DirectCall {
                    op: "now".to_string(),
                },
                covered: false,
                materialize: None,
            }
        }

        #[test]
        fn extracts_a_free_variable_as_a_parameter() {
            let src = "context c\n\nfn f(num: Int) -> Int {\n  num * 2\n}\n";
            let locals = vec![param(src, "num", "Int")];
            let types = vec![int_type(src, "num * 2")];
            let actions = function_actions_for(src, "num * 2", &[], &locals, &types);
            assert_eq!(actions.len(), 1);
            let edits = sole_edit(&actions[0]);
            assert_eq!(
                edits[0].new_text,
                "fn extractedFn(num: Int) -> Int {\n  num * 2\n}\n\n"
            );
            // Inserted right above the enclosing `fn f`, not at the selection.
            assert_eq!(edits[0].range.start, Position::new(2, 0));
            assert_eq!(edits[1].new_text, "extractedFn(num)");
        }

        #[test]
        fn no_free_variables_yields_a_nullary_call() {
            let src = "context c\n\nfn f() -> Int {\n  1 + 2\n}\n";
            let types = vec![int_type(src, "1 + 2")];
            let actions = function_actions_for(src, "1 + 2", &[], &[], &types);
            let edits = sole_edit(&actions[0]);
            assert_eq!(
                edits[0].new_text,
                "fn extractedFn() -> Int {\n  1 + 2\n}\n\n"
            );
            assert_eq!(edits[1].new_text, "extractedFn()");
        }

        #[test]
        fn capability_using_selection_offers_nothing() {
            let src = "context c\n\nfn f() -> Int {\n  1 + 2\n}\n";
            let site = src.find("1 + 2").unwrap();
            let reqs = vec![capability_use(site, "1 + 2".len())];
            let types = vec![int_type(src, "1 + 2")];
            let actions = function_actions_for(src, "1 + 2", &reqs, &[], &types);
            assert!(actions.is_empty());
        }

        #[test]
        fn bumps_the_suffix_on_a_name_collision() {
            let src =
                "context c\n\nfn extractedFn() -> Int {\n  0\n}\n\nfn f() -> Int {\n  1 + 2\n}\n";
            let types = vec![int_type(src, "1 + 2")];
            let actions = function_actions_for(src, "1 + 2", &[], &[], &types);
            let edits = sole_edit(&actions[0]);
            assert!(edits[0].new_text.starts_with("fn extractedFn2("));
            assert_eq!(edits[1].new_text, "extractedFn2()");
        }

        #[test]
        fn selection_outside_any_body_offers_nothing() {
            let src = "context c\n\nfn f() -> Int {\n  1 + 2\n}\n";
            let types = vec![int_type(src, "1 + 2")];
            let actions = function_actions_for(src, "context c", &[], &[], &types);
            assert!(actions.is_empty());
        }

        #[test]
        fn a_dirty_file_with_no_recorded_type_offers_nothing() {
            let src = "context c\n\nfn f() -> Int {\n  1 + 2\n}\n";
            // No expr_types entries at all — the clean-file ceiling (ADR 0063):
            // a file with an unrelated error elsewhere yields none.
            let actions = function_actions_for(src, "1 + 2", &[], &[], &[]);
            assert!(actions.is_empty());
        }

        #[test]
        fn ambiguous_same_name_shadow_from_two_outer_scopes_declines() {
            let src = "context c\n\nfn f() -> Int {\n  x + x\n}\n";
            let types = vec![int_type(src, "x + x")];
            let first_x = nth_offset(src, "x + x", 0); // the first `x`'s offset
            let second_x = first_x + "x + ".len(); // the second `x`'s offset
            // Two distinct external bindings named `x`, each in scope only at
            // one of the two occurrences — an occurrence-position-dependent
            // resolution a real program could never actually produce, used
            // here purely to force the ambiguous-shadow branch.
            let locals = vec![
                LocalBinding {
                    name: "x".to_string(),
                    def_span: Span::new(0, 1),
                    kind: LocalKind::Let,
                    ty: "Int".to_string(),
                    scope: Span::new(first_x, first_x),
                },
                LocalBinding {
                    name: "x".to_string(),
                    def_span: Span::new(1, 2),
                    kind: LocalKind::Let,
                    ty: "Int".to_string(),
                    scope: Span::new(second_x, second_x),
                },
            ];
            let actions = function_actions_for(src, "x + x", &[], &locals, &types);
            assert!(actions.is_empty());
        }
    }
}
