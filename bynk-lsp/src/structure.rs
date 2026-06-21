//! v0.37 (ADR 0070): structural ranges — `textDocument/foldingRange` and
//! `textDocument/selectionRange`.
//!
//! Both read the per-file **recovered AST** (the document-symbols parse path)
//! and share one span visitor ([`collect`]): every node contributes its
//! `(span, foldable)` pair. **Folding** keeps the multi-line block-like nodes
//! (`foldable`); **selection** keeps every span containing the cursor and
//! nests them. Neither touches the binding index or the analysis round — they
//! parse the live document, so they answer even when the project doesn't check.

use std::collections::HashSet;

use bynk_syntax::ast::*;
use bynk_syntax::lexer::{TokenKind, tokenize};
use bynk_syntax::parser::parse_unit_with_recovery;
use bynk_syntax::span::Span;
use tower_lsp::lsp_types::{FoldingRange, FoldingRangeKind, Position, Range, SelectionRange};

use crate::position::{offset_to_position, position_to_offset, span_to_range};

/// Every AST node's span paired with whether it is a folding candidate (a
/// multi-line block-like construct). Non-candidate spans are still collected —
/// selection chains need the fine-grained leaves. Empty when the file has no
/// recognisable header (recovery returned nothing).
fn collect(source: &str) -> Vec<(Span, bool)> {
    let Ok(tokens) = tokenize(source) else {
        return Vec::new();
    };
    let (Some(unit), _errs) = parse_unit_with_recovery(&tokens, source) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk_unit(&unit, &mut out);
    out
}

fn walk_unit(unit: &SourceUnit, out: &mut Vec<(Span, bool)>) {
    match unit {
        SourceUnit::Commons(c) => {
            out.push((c.span, true));
            c.items.iter().for_each(|i| walk_item(i, out));
        }
        SourceUnit::Context(c) => {
            out.push((c.span, true));
            c.items.iter().for_each(|i| walk_item(i, out));
        }
        SourceUnit::Adapter(a) => {
            out.push((a.span, true));
            a.items.iter().for_each(|i| walk_item(i, out));
        }
        SourceUnit::Test(t) => {
            out.push((t.span, true));
            for case in &t.cases {
                out.push((case.span, true));
                walk_block(&case.body, out);
            }
        }
        SourceUnit::Integration(i) => {
            out.push((i.span, true));
            for case in &i.cases {
                out.push((case.span, true));
            }
        }
    }
}

fn walk_item(item: &CommonsItem, out: &mut Vec<(Span, bool)>) {
    match item {
        CommonsItem::Type(t) => {
            out.push((t.span, true));
            match &t.body {
                TypeBody::Record(r) => out.push((r.span, true)),
                TypeBody::Sum(s) => out.push((s.span, true)),
                TypeBody::Opaque { .. } | TypeBody::Refined { .. } => {}
            }
        }
        CommonsItem::Fn(f) => {
            out.push((f.span, true));
            walk_block(&f.body, out);
        }
        CommonsItem::Capability(c) => out.push((c.span, true)),
        CommonsItem::Provider(p) => {
            out.push((p.span, true));
            for op in &p.ops {
                out.push((op.span, true));
                walk_block(&op.body, out);
            }
        }
        CommonsItem::Service(s) => {
            out.push((s.span, true));
            for h in &s.handlers {
                out.push((h.span, true));
                walk_block(&h.body, out);
            }
        }
        CommonsItem::Agent(a) => {
            out.push((a.span, true));
            for h in &a.handlers {
                out.push((h.span, true));
                walk_block(&h.body, out);
            }
        }
        CommonsItem::Actor(a) => {
            out.push((a.span, true));
        }
    }
}

fn walk_block(b: &Block, out: &mut Vec<(Span, bool)>) {
    out.push((b.span, true));
    for s in &b.statements {
        out.push((s.span(), false));
        match s {
            Statement::Let(l) | Statement::EffectLet(l) => walk_expr(&l.value, out),
            Statement::Commit(c) => walk_expr(&c.value, out),
            Statement::Assert(a) => walk_expr(&a.value, out),
        }
    }
    walk_expr(&b.tail, out);
}

fn walk_expr(e: &Expr, out: &mut Vec<(Span, bool)>) {
    let foldable = matches!(
        e.kind,
        ExprKind::Block(_)
            | ExprKind::If { .. }
            | ExprKind::Match { .. }
            | ExprKind::RecordConstruction { .. }
            | ExprKind::RecordSpread { .. }
            | ExprKind::ListLit(_)
            | ExprKind::Lambda(_)
    );
    out.push((e.span, foldable));
    match &e.kind {
        ExprKind::Block(b) => {
            for s in &b.statements {
                out.push((s.span(), false));
                match s {
                    Statement::Let(l) | Statement::EffectLet(l) => walk_expr(&l.value, out),
                    Statement::Commit(c) => walk_expr(&c.value, out),
                    Statement::Assert(a) => walk_expr(&a.value, out),
                }
            }
            walk_expr(&b.tail, out);
        }
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            walk_expr(cond, out);
            walk_block(then_block, out);
            walk_block(else_block, out);
        }
        ExprKind::Match { discriminant, arms } => {
            walk_expr(discriminant, out);
            for arm in arms {
                out.push((arm.span, true));
                match &arm.body {
                    MatchBody::Expr(ex) => walk_expr(ex, out),
                    MatchBody::Block(bl) => walk_block(bl, out),
                }
            }
        }
        ExprKind::RecordConstruction { fields, .. } => {
            for f in fields {
                if let Some(v) = &f.value {
                    walk_expr(v, out);
                }
            }
        }
        ExprKind::RecordSpread {
            base, overrides, ..
        } => {
            walk_expr(base, out);
            for f in overrides {
                if let Some(v) = &f.value {
                    walk_expr(v, out);
                }
            }
        }
        ExprKind::ListLit(elems) => elems.iter().for_each(|el| walk_expr(el, out)),
        ExprKind::Lambda(l) => walk_expr(&l.body, out),
        ExprKind::BinOp(_, a, b) => {
            walk_expr(a, out);
            walk_expr(b, out);
        }
        ExprKind::UnaryOp(_, x)
        | ExprKind::Paren(x)
        | ExprKind::Ok(x)
        | ExprKind::Err(x)
        | ExprKind::Question(x)
        | ExprKind::Some(x)
        | ExprKind::EffectPure(x)
        | ExprKind::Assert(x) => walk_expr(x, out),
        ExprKind::Call { args, .. } | ExprKind::ConstructorCall { args, .. } => {
            args.iter().for_each(|a| walk_expr(a, out))
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            walk_expr(receiver, out);
            args.iter().for_each(|a| walk_expr(a, out));
        }
        ExprKind::FieldAccess { receiver, .. } => walk_expr(receiver, out),
        ExprKind::Is { value, .. } => walk_expr(value, out),
        ExprKind::Mock { args, .. } => args.iter().for_each(|a| walk_expr(a, out)),
        // v0.43: walk each interpolation hole's expression.
        ExprKind::InterpStr(parts) => parts.iter().for_each(|part| {
            if let InterpPart::Hole(hole) = part {
                walk_expr(hole, out);
            }
        }),
        // Leaves carry no foldable children.
        ExprKind::IntLit(_)
        | ExprKind::FloatLit { .. }
        | ExprKind::StrLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::Ident(_)
        | ExprKind::None
        | ExprKind::UnitLit => {}
    }
}

/// `textDocument/foldingRange` — the structural multi-line constructs plus
/// multi-line comment runs. A range is emitted only when it spans more than
/// one line (LSP folds ≥2 lines); duplicate `(start, end)` line pairs (a decl
/// and its body sharing both lines) collapse to one.
pub fn folding_ranges(source: &str) -> Vec<FoldingRange> {
    let mut out = Vec::new();
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    for (span, foldable) in collect(source) {
        if !foldable {
            continue;
        }
        let start = offset_to_position(source, span.start).line;
        let end = offset_to_position(source, span.end).line;
        if end > start && seen.insert((start, end)) {
            out.push(fold(start, end, None));
        }
    }
    out.extend(comment_folds(source));
    out
}

/// Multi-line runs of consecutive `--` line comments → `Comment` folds. Spans
/// come from the lexer's `Comment` tokens (the trivia table keeps only bodies),
/// grouped while each comment sits on the line immediately after the previous.
fn comment_folds(source: &str) -> Vec<FoldingRange> {
    let Ok(tokens) = tokenize(source) else {
        return Vec::new();
    };
    let comments: Vec<Span> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Comment)
        .map(|t| t.span)
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < comments.len() {
        let start = offset_to_position(source, comments[i].start).line;
        let mut end = offset_to_position(source, comments[i].end).line;
        let mut j = i;
        while j + 1 < comments.len() {
            let next = offset_to_position(source, comments[j + 1].start).line;
            if next == end + 1 {
                j += 1;
                end = offset_to_position(source, comments[j].end).line;
            } else {
                break;
            }
        }
        if end > start {
            out.push(fold(start, end, Some(FoldingRangeKind::Comment)));
        }
        i = j + 1;
    }
    out
}

fn fold(start_line: u32, end_line: u32, kind: Option<FoldingRangeKind>) -> FoldingRange {
    FoldingRange {
        start_line,
        start_character: None,
        end_line,
        end_character: None,
        kind,
        collapsed_text: None,
    }
}

/// `textDocument/selectionRange` — for each position, the chain of enclosing
/// AST node ranges, innermost first (each `.parent` widens outward to the
/// file). Falls back to an empty range at the cursor when no node contains it
/// (e.g. trailing whitespace) or the file doesn't parse.
pub fn selection_ranges(source: &str, positions: &[Position]) -> Vec<SelectionRange> {
    let nodes = collect(source);
    positions
        .iter()
        .map(|pos| selection_at(source, &nodes, *pos))
        .collect()
}

fn selection_at(source: &str, nodes: &[(Span, bool)], pos: Position) -> SelectionRange {
    let empty = SelectionRange {
        range: Range::new(pos, pos),
        parent: None,
    };
    let Some(offset) = position_to_offset(source, pos) else {
        return empty;
    };
    // Spans containing the offset, de-duplicated, smallest first.
    let mut spans: Vec<Span> = nodes
        .iter()
        .map(|(s, _)| *s)
        .filter(|s| s.start <= offset && offset <= s.end)
        .collect();
    spans.sort_by_key(|s| (s.start, s.end));
    spans.dedup();
    spans.sort_by_key(|s| s.end - s.start);
    // Build outermost → innermost so each node's `parent` is the next-larger.
    let mut chain: Option<Box<SelectionRange>> = None;
    for span in spans.into_iter().rev() {
        chain = Some(Box::new(SelectionRange {
            range: span_to_range(source, span),
            parent: chain,
        }));
    }
    chain.map(|b| *b).unwrap_or(empty)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = concat!(
        "context shop\n",
        "\n",
        "-- a comment\n",
        "-- second line\n",
        "\n",
        "type Money = {\n",
        "  cents: Int,\n",
        "  currency: String,\n",
        "}\n",
        "\n",
        "fn total(m: Money) -> Int {\n",
        "  if m.cents > 0 {\n",
        "    m.cents\n",
        "  } else {\n",
        "    0\n",
        "  }\n",
        "}\n",
    );

    /// The 0-based line a substring first appears on.
    fn line_of(needle: &str) -> u32 {
        let off = SRC.find(needle).expect("substring present");
        offset_to_position(SRC, off).line
    }

    #[test]
    fn folds_structural_constructs_and_comment_runs_omitting_single_lines() {
        let folds = folding_ranges(SRC);

        // Every fold spans more than one line.
        assert!(folds.iter().all(|f| f.end_line > f.start_line));

        // The two-line comment run folds as a Comment.
        let comment = folds
            .iter()
            .find(|f| f.kind == Some(FoldingRangeKind::Comment))
            .expect("comment run folds");
        assert_eq!((comment.start_line, comment.end_line), (2, 3));

        // The record body folds (start at the `type` line, end at its `}`).
        assert!(
            folds
                .iter()
                .any(|f| f.start_line == line_of("type Money") && f.end_line == line_of("}\n\nfn")),
            "record body folds"
        );

        // The `if` folds (a structural Region); the single-line then/else tail
        // expression (`m.cents` alone, line 12) does not start a fold.
        assert!(
            folds
                .iter()
                .any(|f| f.start_line == line_of("if m.cents") && f.kind.is_none()),
            "if folds"
        );
        let tail_line = line_of("    m.cents"); // the indented tail, line 12
        assert!(
            !folds.iter().any(|f| f.start_line == tail_line),
            "single-line tail expression is not folded"
        );
    }

    #[test]
    fn selection_chain_widens_from_the_cursor_to_the_file() {
        // Cursor on `cents` of the then-block tail `m.cents` (line 12).
        let off = SRC.find("    m.cents").unwrap() + 6; // onto `cents`
        let pos = offset_to_position(SRC, off);
        let ranges = selection_ranges(SRC, &[pos]);
        assert_eq!(ranges.len(), 1);

        // Walk the parent chain; ranges must strictly widen and stay nested.
        let mut levels = 0;
        let mut cur = Some(&ranges[0]);
        let mut prev: Option<&Range> = None;
        let mut outermost = ranges[0].range;
        while let Some(node) = cur {
            if let Some(p) = prev {
                // Each parent contains the previous (child) range.
                assert!(node.range.start <= p.start && node.range.end >= p.end);
                assert!(node.range != *p, "ranges strictly widen");
            }
            outermost = node.range;
            prev = Some(&node.range);
            levels += 1;
            cur = node.parent.as_deref();
        }
        assert!(levels >= 4, "cursor → … → context is several levels");
        // Outermost is the whole context (starts on line 0).
        assert_eq!(outermost.start.line, 0);
    }

    #[test]
    fn partial_parse_still_folds_what_parsed() {
        // A malformed trailing item must not panic and must still fold the
        // valid context + type above it.
        let src = "context shop\n\ntype Money = {\n  cents: Int,\n}\n\nfn broken(";
        let folds = folding_ranges(src);
        assert!(
            folds.iter().any(|f| f.start_line == 2),
            "the type still folds"
        );
        // Selection at the top of the file is well-formed too.
        let sel = selection_ranges(src, &[Position::new(3, 4)]);
        assert_eq!(sel.len(), 1);
    }
}
