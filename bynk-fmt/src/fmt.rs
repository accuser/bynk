//! Bynk source formatter.
//!
//! Re-parses the source into an AST and re-prints it in canonical form per
//! the style rules in `design/bynk-lsp-spec.md` §3.5:
//!
//! - Tabs by default (one tab per nesting level).
//! - K&R brace style: opening brace on the same line as the construct header.
//! - Trailing commas in multi-line record / sum / parameter / argument lists.
//! - One blank line between top-level declarations.
//! - No blank lines between fields within a record or arms within a match.
//! - Doc blocks immediately above their declaration, no blank line between.
//! - One space around binary operators, after commas, no space inside parens.
//! - Soft 100-column line width — long parameter lists wrap across lines.
//!
//! The formatter is idempotent: format → format yields the same text.
//!
//! Comments (v1.1): line comments are preserved through the lexer-to-parser
//! trivia pipeline (lexer emits `Comment` tokens, parser attaches them to
//! AST declarations and statements). The formatter re-emits leading
//! comments above each node and a trailing comment, if any, on the same
//! line as the node's last token. Comments inside expression sub-trees
//! are not yet attached to individual operands; they are folded into the
//! enclosing statement's leading trivia. When even that would lose a
//! comment, [`format_source`] refuses with a `bynk.fmt.comment_loss`
//! diagnostic instead of dropping user text (#523) — the file is left
//! unchanged. See `design/bynk-lsp-spec.md` §3.5 for the canonical
//! comment-placement rules.

use bynk_syntax::ast::*;
use bynk_syntax::error::CompileError;
use bynk_syntax::lexer::{Token, TokenKind, tokenize};
use bynk_syntax::parser::parse_units;
use bynk_syntax::span::Span;

/// Indentation style: tabs or spaces. Mirrors the LSP spec's `[fmt].indent`
/// setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndentStyle {
    #[default]
    Tab,
    Spaces(u8),
}

/// Formatter options. All fields have spec-defined defaults.
#[derive(Debug, Clone)]
pub struct FormatOptions {
    pub indent: IndentStyle,
    pub max_line_width: u32,
    pub trailing_comma: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            indent: IndentStyle::Tab,
            max_line_width: 100,
            trailing_comma: true,
        }
    }
}

/// Error returned when formatting fails. The formatter cannot format code
/// that does not parse, so all failure modes here surface as parse errors.
#[derive(Debug, Clone)]
pub struct FormatError {
    pub errors: Vec<CompileError>,
}

/// Format a Bynk source string. On parse failure, returns the original
/// source unchanged is *not* this function's responsibility — callers (LSP,
/// CLI) decide how to handle parse failure. Here we surface the errors so
/// the caller can do so.
pub fn format_source(source: &str, opts: &FormatOptions) -> Result<String, FormatError> {
    let tokens = tokenize(source).map_err(|e| FormatError { errors: vec![e] })?;
    // v0.113: a file may hold more than one top-level unit (an atomic
    // `commons` + `suite` file, DECISION S). Format each and join with a blank
    // line. Each unit's output already ends in exactly one newline, so joining
    // with `"\n"` inserts one blank line between units and leaves a single-unit
    // file byte-identical.
    let units = parse_units(&tokens, source).map_err(|errors| FormatError { errors })?;
    let output = render_units(&units, opts);
    // #523 guard: trivia is only attached at declaration/statement
    // granularity, so a comment inside an expression subtree can be silently
    // dropped. Losing user text is worse than leaving a file unformatted —
    // when the output holds fewer comments than the input, refuse with a
    // diagnostic pointing at the first comment that would vanish.
    if let Some(error) = comment_loss(source, &tokens, &output) {
        return Err(FormatError {
            errors: vec![error],
        });
    }
    // #735 guard: the printer is hand-written and dodges several parse traps by
    // convention (a tail `()` re-attaching as a call, a trailing comma making a
    // param list unparseable). A shape the corpus misses that the printer
    // mis-renders would otherwise be written straight over the user's file with
    // exit 0. Before returning, re-parse the output and compare its *code*
    // structure — every comment stripped from both sides, so trivia re-flow is
    // ignored — against the input. When the output fails to re-parse, or a
    // shape round-trips to a different AST, refuse rather than corrupt.
    if let Some(error) = roundtrip_divergence(source, &output, opts) {
        return Err(FormatError {
            errors: vec![error],
        });
    }
    Ok(output)
}

/// Format every top-level unit and join with a blank line. A file may hold more
/// than one top-level unit (v0.113, an atomic `commons` + `suite` file,
/// DECISION S). Each unit's output already ends in exactly one newline, so
/// joining with `"\n"` inserts one blank line between units and leaves a
/// single-unit file byte-identical.
fn render_units(units: &[SourceUnit], opts: &FormatOptions) -> String {
    let parts: Vec<String> = units
        .iter()
        .map(|unit| {
            let mut f = Formatter::new(opts);
            f.format_unit(unit);
            f.finish()
        })
        .collect();
    parts.join("\n")
}

/// #735: the comment-free canonical rendering of `source` — every `Comment`
/// token dropped before parsing, so the result carries no trivia and reflects
/// only the code structure. Because the parser strips comments the same way
/// ([`split_trivia`]), pre-filtering them changes nothing structural; it just
/// leaves the trivia fields empty so the formatter emits pure code. Returns the
/// parse errors when `source` does not tokenize or parse.
fn code_only_canonical(source: &str, opts: &FormatOptions) -> Result<String, Vec<CompileError>> {
    let tokens = tokenize(source).map_err(|e| vec![e])?;
    let code: Vec<Token> = tokens
        .into_iter()
        .filter(|t| t.kind != TokenKind::Comment)
        .collect();
    let units = parse_units(&code, source)?;
    Ok(render_units(&units, opts))
}

/// #735: refuse when the formatter's `output` does not round-trip to the same
/// code as `source`. Both sides are reduced to their comment-free canonical
/// form ([`code_only_canonical`]) and compared: the formatter only re-flows
/// whitespace and trivia, so for a faithful render the two canonical strings
/// are byte-identical. A mismatch means either the output no longer parses (the
/// data-loss vector) or the printer altered the AST. `None` when the output is
/// safe to write.
///
/// Note: this guard assumes the formatter is idempotent on comment-free code —
/// i.e. `render(parse(strip(x)))` is a stable canonical form. That invariant is
/// held by the corpus/property idempotency tests. Were a future formatter
/// change to break it on some shape, this guard would *refuse* an otherwise
/// valid file rather than corrupt it — it fails safe (file unchanged + a
/// diagnostic), but the surprise would be a formatter bug to fix upstream.
fn roundtrip_divergence(source: &str, output: &str, opts: &FormatOptions) -> Option<CompileError> {
    // The output MUST re-parse to the same structure; a failure here is the
    // core corruption vector this guard exists to stop.
    let canon_out = match code_only_canonical(output, opts) {
        Ok(canon) => canon,
        Err(_) => {
            return Some(roundtrip_error(
                "the formatter produced output that no longer parses",
            ));
        }
    };
    // The input already tokenized and parsed in `format_source`, so its
    // canonical form should always compute. If it unexpectedly does not, do not
    // block a valid format on our own guard failing — leave the file writable.
    let canon_in = code_only_canonical(source, opts).ok()?;
    (canon_in != canon_out)
        .then(|| roundtrip_error("the formatter's output does not round-trip to the same code"))
}

/// Build the `bynk.fmt.roundtrip` diagnostic shared by both failure modes of
/// [`roundtrip_divergence`]. The span is deliberately `Span::default()` (the
/// start of the file): the message points at neither the source nor the
/// output — it is a generic "this is a formatter bug" — and the failing branch
/// carries an *output*-relative span, which the caller renders against the
/// *source* string. When the mis-rendered output is longer than the source,
/// that span is out of range for the buffer ariadne is given (a misplaced
/// caret, or a byte-index panic in the very formatter-bug path this guard
/// exists to handle gracefully). A zero span is always in range and buys the
/// message nothing to lose.
fn roundtrip_error(what: &str) -> CompileError {
    CompileError {
        category: "bynk.fmt.roundtrip",
        span: Span::default(),
        message: format!("{what} — the file was left unchanged"),
        labels: Vec::new(),
        notes: vec![
            "this is a formatter bug, not a problem with your source; please report it \
             with the file that triggered it"
                .to_string(),
        ],
        suggestions: Vec::new(),
    }
}

/// #523: compare the comment population of `source` (already tokenized as
/// `tokens`) against `output`. Returns a `bynk.fmt.comment_loss` error naming
/// the first lost comment when the output would hold fewer comments, `None`
/// when every comment survives. Comments may legitimately *move* (expression
/// trivia folds into the enclosing statement's leading block), so the
/// comparison is by body multiset, not position.
fn comment_loss(source: &str, tokens: &[Token], output: &str) -> Option<CompileError> {
    use bynk_syntax::lexer::comment_body;
    let in_comments: Vec<Span> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Comment)
        .map(|t| t.span)
        .collect();
    if in_comments.is_empty() {
        return None;
    }
    // The formatter's own output must tokenize; treat a failure as "all
    // comments lost" rather than silently accepting the write.
    let mut out_bodies: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    if let Ok(out_tokens) = tokenize(output) {
        for t in &out_tokens {
            if t.kind == TokenKind::Comment {
                *out_bodies
                    .entry(comment_body(output, t.span).trim().to_string())
                    .or_insert(0) += 1;
            }
        }
    }
    let mut lost = 0usize;
    let mut first_lost: Option<Span> = None;
    for span in &in_comments {
        let body = comment_body(source, *span).trim().to_string();
        match out_bodies.get_mut(&body) {
            Some(n) if *n > 0 => *n -= 1,
            _ => {
                lost += 1;
                first_lost.get_or_insert(*span);
            }
        }
    }
    let span = first_lost?;
    Some(CompileError {
        category: "bynk.fmt.comment_loss",
        span,
        message: format!(
            "formatting would lose {lost} comment{} — the file was left unchanged",
            if lost == 1 { "" } else { "s" }
        ),
        labels: vec![(
            span,
            "this comment sits where the formatter cannot yet re-attach it".to_string(),
        )],
        notes: vec![
            "comments inside expression subtrees are not yet preserved; move the comment onto \
             its own line before the enclosing statement to format this file"
                .to_string(),
        ],
        suggestions: Vec::new(),
    })
}

// -- Internal formatter state --

struct Formatter<'a> {
    opts: &'a FormatOptions,
    out: String,
    indent_level: u32,
    /// True when the formatter has just emitted a newline and is at the
    /// start of a fresh line. Used to gate indent emission.
    at_line_start: bool,
}

impl<'a> Formatter<'a> {
    fn new(opts: &'a FormatOptions) -> Self {
        Self {
            opts,
            out: String::new(),
            indent_level: 0,
            at_line_start: true,
        }
    }

    fn finish(mut self) -> String {
        // Single trailing newline.
        while self.out.ends_with("\n\n") {
            self.out.pop();
        }
        if !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        self.out
    }

    fn indent_unit(&self) -> String {
        match self.opts.indent {
            IndentStyle::Tab => "\t".to_string(),
            IndentStyle::Spaces(n) => " ".repeat(n as usize),
        }
    }

    fn emit_indent(&mut self) {
        let unit = self.indent_unit();
        for _ in 0..self.indent_level {
            self.out.push_str(&unit);
        }
    }

    fn push(&mut self, s: &str) {
        if self.at_line_start && !s.starts_with('\n') {
            self.emit_indent();
            self.at_line_start = false;
        }
        if s.contains('\n') {
            self.push_reindented(s);
        } else {
            self.out.push_str(s);
        }
    }

    /// Append a multi-line string, re-applying the current indent to every
    /// continuation line. Multi-line strings come from the single-line
    /// expression renderers (`expr_to_string` and friends), which build their
    /// internal structure assuming column zero — they embed `\n` plus relative
    /// tabs but know nothing about the current nesting depth. Without this an
    /// argument-position `match` (or any embedded multi-line expression) would
    /// print its arms and trailing brace at column one regardless of how deeply
    /// it is nested. The first line is emitted as-is (its indent, if any, was
    /// handled by `push`); blank lines are left empty rather than padded.
    fn push_reindented(&mut self, s: &str) {
        let prefix = self.indent_unit().repeat(self.indent_level as usize);
        for (i, line) in s.split('\n').enumerate() {
            if i > 0 {
                self.out.push('\n');
                if !line.is_empty() {
                    self.out.push_str(&prefix);
                }
            }
            self.out.push_str(line);
        }
    }

    fn newline(&mut self) {
        self.out.push('\n');
        self.at_line_start = true;
    }

    #[allow(dead_code)]
    fn blank_line(&mut self) {
        if !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        if !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
        self.at_line_start = true;
    }

    fn indented<F: FnOnce(&mut Self)>(&mut self, f: F) {
        self.indent_level += 1;
        f(self);
        self.indent_level -= 1;
    }

    // -- Doc block --

    /// Emit a doc block immediately above a declaration. The content is
    /// already normalised (common leading indent stripped) when stored in
    /// the AST; we re-emit with the current indent applied per line.
    fn emit_doc(&mut self, doc: &str) {
        self.push("---");
        self.newline();
        for line in doc.lines() {
            if line.is_empty() {
                self.newline();
            } else {
                self.push(line);
                self.newline();
            }
        }
        self.push("---");
        self.newline();
    }

    // -- Line-comment trivia (v1.1) --

    /// Emit a sequence of leading line-comments, each on its own line at
    /// the current indent. Group has no blank lines between entries.
    fn emit_leading_comments(&mut self, comments: &[String]) {
        for body in comments {
            self.push("--");
            self.push(body);
            self.newline();
        }
    }

    /// Emit a trailing comment on the same line as the just-emitted token.
    /// The spec uses two spaces between code and comment for readability.
    fn emit_trailing_comment(&mut self, body: Option<&str>) {
        if let Some(body) = body {
            // Ensure we're on the same line as the preceding tokens —
            // strip any newline we just emitted.
            while self.out.ends_with('\n') {
                self.out.pop();
            }
            self.out.push_str("  --");
            self.out.push_str(body);
            self.newline();
        }
    }

    // -- Top level --

    fn format_unit(&mut self, unit: &SourceUnit) {
        match unit {
            SourceUnit::Commons(c) => self.format_commons(c),
            SourceUnit::Context(c) => self.format_context(c),
            SourceUnit::Suite(t) => self.format_test(t),
            SourceUnit::Adapter(a) => self.format_adapter(a),
        }
    }

    fn format_adapter(&mut self, a: &AdapterDecl) {
        self.emit_leading_comments(&a.trivia.leading);
        if let Some(doc) = &a.documentation {
            self.emit_doc(doc);
        }
        let header = format!("adapter {}", a.name.joined());
        match a.form {
            CommonsForm::Brace => {
                self.push(&header);
                self.push(" {");
                self.newline();
                self.indented(|f| {
                    f.format_adapter_body(a);
                });
                self.push("}");
                self.newline();
            }
            CommonsForm::Fragment => {
                self.push(&header);
                self.newline();
                self.newline();
                self.format_adapter_body(a);
            }
        }
    }

    fn format_adapter_body(&mut self, a: &AdapterDecl) {
        let mut any_header = false;
        if let Some(b) = &a.binding {
            self.emit_leading_comments(&b.trivia.leading);
            self.push(&format!("binding {:?}", b.module));
            if !b.requires.is_empty() {
                let entries: Vec<String> = b
                    .requires
                    .iter()
                    .map(|r| format!("{:?}: {:?}", r.package, r.range))
                    .collect();
                self.push(&format!(" requires {{ {} }}", entries.join(", ")));
            }
            self.emit_trailing_comment(b.trivia.trailing.as_deref());
            if b.trivia.trailing.is_none() {
                self.newline();
            }
            any_header = true;
        }
        for u in &a.uses {
            self.emit_leading_comments(&u.trivia.leading);
            self.push(&format!("uses {}", u.target.joined()));
            self.emit_trailing_comment(u.trivia.trailing.as_deref());
            if u.trivia.trailing.is_none() {
                self.newline();
            }
            any_header = true;
        }
        for c in &a.consumes {
            self.format_consumes(c);
            any_header = true;
        }
        for e in &a.exports {
            self.emit_leading_comments(&e.trivia.leading);
            self.format_exports(e);
            if e.trivia.trailing.is_some() {
                self.emit_trailing_comment(e.trivia.trailing.as_deref());
            }
            any_header = true;
        }
        if any_header && !a.items.is_empty() {
            self.newline();
        }
        let mut first = true;
        for item in &a.items {
            if !first {
                self.newline();
            }
            self.format_item(item);
            first = false;
        }
        if !a.trailing_comments.is_empty() {
            if !a.items.is_empty() || any_header {
                self.newline();
            }
            self.emit_leading_comments(&a.trailing_comments);
        }
    }

    fn format_test(&mut self, t: &SuiteDecl) {
        self.emit_leading_comments(&t.trivia.leading);
        if let Some(doc) = &t.documentation {
            self.emit_doc(doc);
        }
        let mut header = format!("suite {}", t.target.joined());
        if let Some(tier) = t.tier {
            header.push_str(&format!(" as {}", tier.as_str()));
        }
        match t.form {
            CommonsForm::Brace => {
                self.push(&header);
                self.push(" {");
                self.newline();
                self.indented(|f| {
                    f.format_test_body(
                        &t.uses,
                        &t.stubs,
                        &t.cases,
                        &t.properties,
                        &t.trailing_comments,
                    );
                });
                self.push("}");
                self.newline();
            }
            CommonsForm::Fragment => {
                self.push(&header);
                self.newline();
                self.format_test_body(
                    &t.uses,
                    &t.stubs,
                    &t.cases,
                    &t.properties,
                    &t.trailing_comments,
                );
            }
        }
    }

    fn format_test_body(
        &mut self,
        uses: &[UsesDecl],
        stubs: &[StubClause],
        cases: &[Case],
        properties: &[PropertyDecl],
        trailing_comments: &[String],
    ) {
        let mut first = true;
        for u in uses {
            if !first {
                self.newline();
            }
            self.emit_leading_comments(&u.trivia.leading);
            self.push(&format!("uses {}", u.target.joined()));
            self.emit_trailing_comment(u.trivia.trailing.as_deref());
            self.newline();
            first = false;
        }
        for pv in stubs {
            if !first {
                self.newline();
            }
            self.format_stub_clause(pv);
            first = false;
        }
        for c in cases {
            if !first {
                self.newline();
            }
            self.emit_leading_comments(&c.trivia.leading);
            if let Some(doc) = &c.documentation {
                self.emit_doc(doc);
            }
            let mut ch = format!("case \"{}\"", escape_string(&c.name));
            if let Some(tier) = c.tier {
                ch.push_str(&format!(" as {}", tier.as_str()));
            }
            ch.push(' ');
            self.push(&ch);
            self.format_case_block(&c.body, &c.stubs);
            self.newline();
            first = false;
        }
        for p in properties {
            if !first {
                self.newline();
            }
            self.emit_leading_comments(&p.trivia.leading);
            if let Some(doc) = &p.documentation {
                self.emit_doc(doc);
            }
            self.push(&format!("property \"{}\" {{", escape_string(&p.name)));
            self.newline();
            self.indented(|f| f.format_for_all(&p.forall));
            self.push("}");
            self.newline();
            first = false;
        }
        for comment in trailing_comments {
            self.push(&format!("--{comment}"));
            self.newline();
        }
    }

    /// v0.118: format a `stub` clause as a suite- or case-body line, with
    /// its leading comments / doc and a terminating newline (testing track
    /// slice 6).
    fn format_stub_clause(&mut self, pv: &StubClause) {
        self.emit_leading_comments(&pv.trivia.leading);
        if let Some(doc) = &pv.documentation {
            self.emit_doc(doc);
        }
        self.push(&stub_clause_to_string(pv));
        self.emit_trailing_comment(pv.trivia.trailing.as_deref());
        if pv.trivia.trailing.is_none() {
            self.newline();
        }
    }

    /// v0.118: format a `case` body, emitting its case-scoped `stub` clauses
    /// as the leading lines inside the block, before the statements and tail.
    /// With no case-scoped `stub` this is exactly [`Self::format_block`].
    fn format_case_block(&mut self, b: &Block, stubs: &[StubClause]) {
        if stubs.is_empty() {
            self.format_block(b);
            return;
        }
        self.push("{");
        self.newline();
        self.indented(|f| {
            for pv in stubs {
                f.format_stub_clause(pv);
            }
            for stmt in &b.statements {
                let trivia = statement_trivia(stmt);
                f.emit_leading_comments(&trivia.leading);
                f.format_statement(stmt);
                f.emit_trailing_comment(trivia.trailing.as_deref());
                if trivia.trailing.is_none() {
                    f.newline();
                }
            }
            f.emit_leading_comments(&b.tail_leading_comments);
            // See `format_block`: an implicit `()` tail synthesised after a
            // trailing `expect` is not re-printed, to preserve idempotency.
            let implicit_unit_after_assert = matches!(b.tail.kind, ExprKind::UnitLit)
                && matches!(b.statements.last(), Some(Statement::Expect(_)))
                && b.tail_leading_comments.is_empty();
            if !implicit_unit_after_assert {
                f.format_expr(&b.tail);
                f.newline();
            }
        });
        self.push("}");
    }

    /// v0.114: format a `for all <bindings> [where <pred>] { … }` binder — the
    /// sole body of a `property`.
    fn format_for_all(&mut self, fa: &ForAll) {
        let bindings = fa
            .bindings
            .iter()
            .map(|b| format!("{}: {}", b.name.name, type_ref_to_string(&b.type_ref)))
            .collect::<Vec<_>>()
            .join(", ");
        let mut header = format!("for all {bindings}");
        if let Some(w) = &fa.where_pred {
            header.push_str(&format!(" where {}", expr_to_string(w)));
        }
        self.push(&format!("{header} "));
        self.format_block(&fa.body);
        self.newline();
    }

    fn format_commons(&mut self, c: &Commons) {
        self.emit_leading_comments(&c.trivia.leading);
        if let Some(doc) = &c.documentation {
            self.emit_doc(doc);
        }
        let header = format!("commons {}", c.name.joined());
        match c.form {
            CommonsForm::Brace => {
                self.push(&header);
                self.push(" {");
                self.newline();
                self.indented(|f| {
                    f.format_commons_body(&c.uses, &c.items, &c.trailing_comments);
                });
                self.push("}");
                self.newline();
            }
            CommonsForm::Fragment => {
                self.push(&header);
                self.newline();
                self.newline();
                self.format_commons_body(&c.uses, &c.items, &c.trailing_comments);
            }
        }
    }

    fn format_commons_body(
        &mut self,
        uses: &[UsesDecl],
        items: &[CommonsItem],
        trailing_comments: &[String],
    ) {
        let mut any_uses = false;
        for u in uses {
            self.emit_leading_comments(&u.trivia.leading);
            self.push(&format!("uses {}", u.target.joined()));
            self.emit_trailing_comment(u.trivia.trailing.as_deref());
            if u.trivia.trailing.is_none() {
                self.newline();
            }
            any_uses = true;
        }
        if any_uses && !items.is_empty() {
            self.newline();
        }
        let mut first = true;
        for item in items {
            if !first {
                self.newline();
            }
            self.format_item(item);
            first = false;
        }
        if !trailing_comments.is_empty() {
            // One blank line before trailing-file comments if anything
            // came before them.
            if !items.is_empty() || any_uses {
                self.newline();
            }
            self.emit_leading_comments(trailing_comments);
        }
    }

    fn format_context(&mut self, c: &Context) {
        self.emit_leading_comments(&c.trivia.leading);
        if let Some(doc) = &c.documentation {
            self.emit_doc(doc);
        }
        let header = format!("context {}", c.name.joined());
        match c.form {
            CommonsForm::Brace => {
                self.push(&header);
                self.push(" {");
                self.newline();
                self.indented(|f| {
                    f.format_context_body(
                        &c.uses,
                        &c.consumes,
                        &c.exports,
                        &c.items,
                        &c.trailing_comments,
                    );
                });
                self.push("}");
                self.newline();
            }
            CommonsForm::Fragment => {
                self.push(&header);
                self.newline();
                self.newline();
                self.format_context_body(
                    &c.uses,
                    &c.consumes,
                    &c.exports,
                    &c.items,
                    &c.trailing_comments,
                );
            }
        }
    }

    /// Print one `consumes` clause in any of its three forms: whole-unit,
    /// aliased, or braced capability selection (v0.17 §3.3 — previously the
    /// braced form was silently dropped, a semantic-changing format).
    fn format_consumes(&mut self, c: &ConsumesDecl) {
        self.emit_leading_comments(&c.trivia.leading);
        match (&c.alias, &c.selected) {
            (Some(alias), _) => {
                self.push(&format!("consumes {} as {}", c.target.joined(), alias.name))
            }
            (None, Some(selected)) if selected.is_empty() => {
                self.push(&format!("consumes {} {{ }}", c.target.joined()));
            }
            (None, Some(selected)) => {
                let names: Vec<&str> = selected.iter().map(|i| i.name.as_str()).collect();
                self.push(&format!(
                    "consumes {} {{ {} }}",
                    c.target.joined(),
                    names.join(", ")
                ));
            }
            (None, None) => self.push(&format!("consumes {}", c.target.joined())),
        }
        self.emit_trailing_comment(c.trivia.trailing.as_deref());
        if c.trivia.trailing.is_none() {
            self.newline();
        }
    }

    fn format_context_body(
        &mut self,
        uses: &[UsesDecl],
        consumes: &[ConsumesDecl],
        exports: &[ExportsDecl],
        items: &[CommonsItem],
        trailing_comments: &[String],
    ) {
        let mut any_header = false;
        for u in uses {
            self.emit_leading_comments(&u.trivia.leading);
            self.push(&format!("uses {}", u.target.joined()));
            self.emit_trailing_comment(u.trivia.trailing.as_deref());
            if u.trivia.trailing.is_none() {
                self.newline();
            }
            any_header = true;
        }
        for c in consumes {
            self.format_consumes(c);
            any_header = true;
        }
        for e in exports {
            self.emit_leading_comments(&e.trivia.leading);
            self.format_exports(e);
            // exports may emit multi-line — the trailing comment goes on
            // its last line. Since format_exports already terminates with
            // a newline, splice the comment before it if present.
            if e.trivia.trailing.is_some() {
                self.emit_trailing_comment(e.trivia.trailing.as_deref());
            }
            any_header = true;
        }
        if any_header && !items.is_empty() {
            self.newline();
        }
        let mut first = true;
        for item in items {
            if !first {
                self.newline();
            }
            self.format_item(item);
            first = false;
        }
        if !trailing_comments.is_empty() {
            if !items.is_empty() || any_header {
                self.newline();
            }
            self.emit_leading_comments(trailing_comments);
        }
    }

    fn format_exports(&mut self, e: &ExportsDecl) {
        let vis = match e.kind {
            ExportKind::Type(Visibility::Opaque) => "opaque",
            ExportKind::Type(Visibility::Transparent) => "transparent",
            ExportKind::Capability => "capability",
        };
        if e.names.is_empty() {
            self.push(&format!("exports {} {{}}", vis));
            self.newline();
            return;
        }
        // Single-line form if it fits.
        let oneline = format!(
            "exports {} {{ {} }}",
            vis,
            e.names
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        if self.line_fits(&oneline) {
            self.push(&oneline);
            self.newline();
            return;
        }
        // Multi-line form.
        self.push(&format!("exports {} {{", vis));
        self.newline();
        self.indented(|f| {
            for (i, n) in e.names.iter().enumerate() {
                f.push(&n.name);
                if i + 1 < e.names.len() || f.opts.trailing_comma {
                    f.push(",");
                }
                f.newline();
            }
        });
        self.push("}");
        self.newline();
    }

    fn line_fits(&self, candidate: &str) -> bool {
        let unit_len = match self.opts.indent {
            IndentStyle::Tab => 4, // Approximate tab width for width estimation.
            IndentStyle::Spaces(n) => n as usize,
        };
        let column = self.indent_level as usize * unit_len + candidate.len();
        column as u32 <= self.opts.max_line_width
    }

    fn format_item(&mut self, item: &CommonsItem) {
        match item {
            CommonsItem::Type(t) => self.format_type_decl(t),
            CommonsItem::Fn(f) => self.format_fn_decl(f),
            CommonsItem::Capability(c) => self.format_capability(c),
            CommonsItem::Provider(p) => self.format_provider(p),
            CommonsItem::Service(s) => self.format_service(s),
            CommonsItem::Agent(a) => self.format_agent(a),
            CommonsItem::Actor(a) => self.format_actor(a),
        }
    }

    // -- Type declarations --

    fn format_type_decl(&mut self, t: &TypeDecl) {
        self.emit_leading_comments(&t.trivia.leading);
        if let Some(doc) = &t.documentation {
            self.emit_doc(doc);
        }
        // v0.157 (ADR 0183): `[A, B]` type parameters, spelled as on a function.
        let params = if t.type_params.is_empty() {
            String::new()
        } else {
            let names: Vec<&str> = t
                .type_params
                .iter()
                .map(|tp| tp.name.name.as_str())
                .collect();
            format!("[{}]", names.join(", "))
        };
        self.push(&format!("type {}{} = ", t.name.name, params));
        self.format_type_body(&t.body);
        self.emit_trailing_comment(t.trivia.trailing.as_deref());
        if t.trivia.trailing.is_none() {
            self.newline();
        }
    }

    fn format_type_body(&mut self, body: &TypeBody) {
        match body {
            TypeBody::Refined {
                base, refinement, ..
            } => {
                self.push(base.name());
                if let Some(r) = refinement {
                    self.push(" where ");
                    self.format_refinement(r);
                }
            }
            TypeBody::Opaque {
                base, refinement, ..
            } => {
                self.push("opaque ");
                self.push(base.name());
                if let Some(r) = refinement {
                    self.push(" where ");
                    self.format_refinement(r);
                }
            }
            TypeBody::Record(r) => self.format_record_body(r),
            TypeBody::Sum(s) => self.format_sum_body(s),
        }
    }

    fn format_refinement(&mut self, r: &Refinement) {
        for (i, p) in r.predicates.iter().enumerate() {
            if i > 0 {
                self.push(" && ");
            }
            self.format_pred(p);
        }
    }

    fn format_pred(&mut self, p: &RefinementPred) {
        match &p.kind {
            PredKind::Matches(re) => self.push(&format!("Matches(\"{}\")", escape_string(re))),
            PredKind::InRange(a, b) => self.push(&format!("InRange({}, {})", a.value, b.value)),
            PredKind::InRangeF(a, b) => self.push(&format!("InRange({}, {})", a.lexeme, b.lexeme)),
            PredKind::MinLength(n) => self.push(&format!("MinLength({n})")),
            PredKind::MaxLength(n) => self.push(&format!("MaxLength({n})")),
            PredKind::Length(n) => self.push(&format!("Length({n})")),
            PredKind::NonNegative => self.push("NonNegative"),
            PredKind::Positive => self.push("Positive"),
            PredKind::NonEmpty => self.push("NonEmpty"),
        }
    }

    fn format_record_body(&mut self, r: &RecordBody) {
        if r.fields.is_empty() {
            self.push("{}");
            return;
        }
        // Try single-line first.
        let oneline_fields: Vec<String> = r
            .fields
            .iter()
            .map(|f| self.format_record_field_oneline(f))
            .collect();
        let oneline = format!("{{ {} }}", oneline_fields.join(", "));
        if self.line_fits(&oneline) && !oneline.contains('\n') {
            self.push(&oneline);
            return;
        }
        // Multi-line.
        self.push("{");
        self.newline();
        self.indented(|f| {
            for (i, field) in r.fields.iter().enumerate() {
                f.format_record_field(field);
                if i + 1 < r.fields.len() || f.opts.trailing_comma {
                    f.push(",");
                }
                f.newline();
            }
        });
        self.push("}");
    }

    fn format_record_field(&mut self, field: &RecordField) {
        self.push(&format!("{}: ", field.name.name));
        self.format_type_ref(&field.type_ref);
        if let Some(r) = &field.refinement {
            self.push(" where ");
            self.format_refinement(r);
        }
        if let Some(init) = &field.init {
            self.push(" = ");
            self.format_expr(init);
        }
    }

    fn format_record_field_oneline(&self, field: &RecordField) -> String {
        let mut out = format!("{}: ", field.name.name);
        out.push_str(&type_ref_to_string(&field.type_ref));
        if let Some(r) = &field.refinement {
            out.push_str(" where ");
            out.push_str(&refinement_to_string(r));
        }
        if let Some(init) = &field.init {
            out.push_str(" = ");
            out.push_str(&expr_to_string(init));
        }
        out
    }

    fn format_sum_body(&mut self, s: &SumBody) {
        // Two surface forms exist; we render the pipe form (clearest for both
        // variants with and without payload). enum form is only meaningful for
        // payloadless variants — round-trip preserves semantics either way.
        let any_payload = s.variants.iter().any(|v| !v.payload.is_empty());
        if !any_payload {
            // Enum-style.
            let names: Vec<&str> = s.variants.iter().map(|v| v.name.name.as_str()).collect();
            let oneline = format!("enum {{ {} }}", names.join(", "));
            if self.line_fits(&oneline) {
                self.push(&oneline);
                return;
            }
            self.push("enum {");
            self.newline();
            self.indented(|f| {
                for (i, v) in s.variants.iter().enumerate() {
                    f.push(&v.name.name);
                    if i + 1 < s.variants.len() || f.opts.trailing_comma {
                        f.push(",");
                    }
                    f.newline();
                }
            });
            self.push("}");
            return;
        }
        // Pipe form, multi-line.
        for (i, v) in s.variants.iter().enumerate() {
            if i > 0 {
                self.newline();
            }
            self.push("| ");
            self.push(&v.name.name);
            if !v.payload.is_empty() {
                self.push("(");
                let parts: Vec<String> = v
                    .payload
                    .iter()
                    .map(|p| format!("{}: {}", p.name.name, type_ref_to_string(&p.type_ref)))
                    .collect();
                self.push(&parts.join(", "));
                self.push(")");
            }
        }
        // v0.154 (ADR 0178): the trailing `embeds E as V, …` clause, on its own
        // line under the variants.
        if !s.embeds.is_empty() {
            self.newline();
            let parts: Vec<String> = s
                .embeds
                .iter()
                .map(|e| {
                    format!(
                        "{} as {}",
                        type_ref_to_string(&e.source_type),
                        e.variant.name
                    )
                })
                .collect();
            self.push(&format!("embeds {}", parts.join(", ")));
        }
    }

    fn format_type_ref(&mut self, t: &TypeRef) {
        self.push(&type_ref_to_string(t));
    }

    // -- Function declarations --

    fn format_fn_decl(&mut self, f: &FnDecl) {
        self.emit_leading_comments(&f.trivia.leading);
        if let Some(doc) = &f.documentation {
            self.emit_doc(doc);
        }
        self.push("fn ");
        self.push(&f.name.display());
        // v0.20a: `[A, B]` type parameters.
        if !f.type_params.is_empty() {
            let names: Vec<&str> = f
                .type_params
                .iter()
                .map(|tp| tp.name.name.as_str())
                .collect();
            self.push(&format!("[{}]", names.join(", ")));
        }
        self.format_params(&f.params, f.has_self);
        self.push(" -> ");
        self.format_type_ref(&f.return_type);
        // v0.115: contract clauses on their own indented lines between the
        // return type and the body (`requires`/`ensures <name>: <pred>`).
        if f.requires.is_empty() && f.ensures.is_empty() {
            self.push(" ");
        } else {
            self.newline();
            self.indented(|f2| {
                for c in &f.requires {
                    f2.push(&format!(
                        "requires {}: {}",
                        c.name.name,
                        expr_to_string(&c.predicate)
                    ));
                    f2.newline();
                }
                for c in &f.ensures {
                    f2.push(&format!(
                        "ensures {}: {}",
                        c.name.name,
                        expr_to_string(&c.predicate)
                    ));
                    f2.newline();
                }
            });
        }
        self.format_block(&f.body);
        self.emit_trailing_comment(f.trivia.trailing.as_deref());
        if f.trivia.trailing.is_none() {
            self.newline();
        }
    }

    fn format_params(&mut self, params: &[Param], has_self: bool) {
        let mut rendered: Vec<String> = Vec::new();
        if has_self {
            rendered.push("self".to_string());
        }
        // `params` never includes `self` — it is tracked separately via the
        // `has_self` flag (see parser.rs parse_fn_decl).
        for p in params {
            rendered.push(format!(
                "{}: {}",
                p.name.name,
                type_ref_to_string(&p.type_ref)
            ));
        }
        let oneline = format!("({})", rendered.join(", "));
        if self.line_fits(&oneline) || rendered.len() <= 1 {
            self.push(&oneline);
            return;
        }
        // Multi-line params.
        self.push("(");
        self.newline();
        self.indented(|f| {
            for (i, r) in rendered.iter().enumerate() {
                f.push(r);
                // Parameter lists — unlike records, enum/sum variants, agent
                // state fields and exports — do NOT accept a trailing comma in
                // the grammar, so never emit one here regardless of the
                // `trailing_comma` option, or the wrapped output fails to
                // re-parse.
                if i + 1 < rendered.len() {
                    f.push(",");
                }
                f.newline();
            }
        });
        self.push(")");
    }

    // -- Capability / provider / service / agent (v0.5) --

    fn format_capability(&mut self, c: &CapabilityDecl) {
        self.emit_leading_comments(&c.trivia.leading);
        if let Some(doc) = &c.documentation {
            self.emit_doc(doc);
        }
        self.push(&format!("capability {} {{", c.name.name));
        self.newline();
        self.indented(|f| {
            for op in &c.ops {
                f.emit_leading_comments(&op.trivia.leading);
                if let Some(doc) = &op.documentation {
                    f.emit_doc(doc);
                }
                f.push("fn ");
                f.push(&op.name.name);
                f.format_params(&op.params, false);
                f.push(" -> ");
                f.format_type_ref(&op.return_type);
                f.emit_trailing_comment(op.trivia.trailing.as_deref());
                if op.trivia.trailing.is_none() {
                    f.newline();
                }
            }
        });
        self.push("}");
        self.emit_trailing_comment(c.trivia.trailing.as_deref());
        if c.trivia.trailing.is_none() {
            self.newline();
        }
    }

    fn format_provider(&mut self, p: &ProviderDecl) {
        self.emit_leading_comments(&p.trivia.leading);
        if let Some(doc) = &p.documentation {
            self.emit_doc(doc);
        }
        self.push(&format!(
            "provides {} = {}",
            p.capability.name, p.provider_name.name
        ));
        if !p.given.is_empty() {
            self.push(" given ");
            let names: Vec<String> = p.given.iter().map(cap_ref_src).collect();
            self.push(&names.join(", "));
        }
        // v0.17: an external provider (inside an adapter) has no body.
        if p.external {
            self.emit_trailing_comment(p.trivia.trailing.as_deref());
            if p.trivia.trailing.is_none() {
                self.newline();
            }
            return;
        }
        self.push(" {");
        self.newline();
        self.indented(|f| {
            for (i, op) in p.ops.iter().enumerate() {
                if i > 0 {
                    f.newline();
                }
                f.emit_leading_comments(&op.trivia.leading);
                f.push("fn ");
                f.push(&op.name.name);
                f.format_params(&op.params, false);
                f.push(" -> ");
                f.format_type_ref(&op.return_type);
                f.push(" ");
                f.format_block(&op.body);
                f.emit_trailing_comment(op.trivia.trailing.as_deref());
                if op.trivia.trailing.is_none() {
                    f.newline();
                }
            }
        });
        self.push("}");
        self.emit_trailing_comment(p.trivia.trailing.as_deref());
        if p.trivia.trailing.is_none() {
            self.newline();
        }
    }

    fn format_service(&mut self, s: &ServiceDecl) {
        self.emit_leading_comments(&s.trivia.leading);
        if let Some(doc) = &s.documentation {
            self.emit_doc(doc);
        }
        let from = match &s.protocol {
            ServiceProtocol::Call => String::new(),
            ServiceProtocol::Http => " from http".to_string(),
            ServiceProtocol::Cron => " from cron".to_string(),
            ServiceProtocol::Queue { name } => {
                format!(" from queue(\"{}\")", escape_string(name))
            }
            ServiceProtocol::WebSocket { in_type, out_type } => {
                format!(
                    " from websocket(in: {}, out: {})",
                    type_ref_to_string(in_type),
                    type_ref_to_string(out_type)
                )
            }
        };
        // v0.155: the optional service-level `by`/`given` defaults follow the
        // protocol on the header, `by` first — the ambient contract every handler
        // inherits unless it declares its own.
        let mut header = format!("service {}{}", s.name.name, from);
        if let Some(by) = &s.default_by {
            header.push_str(&format!(" {}", by_clause_src(by)));
        }
        if !s.default_given.is_empty() {
            let names: Vec<String> = s.default_given.iter().map(cap_ref_src).collect();
            header.push_str(&format!(" given {}", names.join(", ")));
        }
        self.push(&format!("{header} {{"));
        self.newline();
        self.indented(|f| {
            // v0.131/v0.141/v0.142: the CORS, security, and limits policies are
            // header-position sections, before the handlers (mirroring the agent
            // phase order). A canonical order — `cors`, then `security`, then
            // `limits` — with a blank line between each section.
            if let Some(cors) = &s.cors {
                f.format_cors_policy(cors);
                if s.security.is_some() || s.limits.is_some() || !s.handlers.is_empty() {
                    f.newline();
                }
            }
            if let Some(security) = &s.security {
                f.format_security_policy(security);
                if s.limits.is_some() || !s.handlers.is_empty() {
                    f.newline();
                }
            }
            if let Some(limits) = &s.limits {
                f.format_limits_policy(limits);
                if !s.handlers.is_empty() {
                    f.newline();
                }
            }
            for (i, h) in s.handlers.iter().enumerate() {
                if i > 0 {
                    f.newline();
                }
                f.format_handler(h);
            }
        });
        self.push("}");
        self.emit_trailing_comment(s.trivia.trailing.as_deref());
        if s.trivia.trailing.is_none() {
            self.newline();
        }
    }

    /// Format a `cors { }` policy section (v0.131). One `name: value` field per
    /// line, with a trailing comma, mirroring a record construction.
    fn format_cors_policy(&mut self, cors: &CorsPolicy) {
        self.emit_leading_comments(&cors.trivia.leading);
        self.push("cors {");
        self.newline();
        self.indented(|f| {
            for field in &cors.fields {
                f.push(&format!(
                    "{}: {},",
                    field.name.name,
                    expr_to_string(&field.value)
                ));
                f.newline();
            }
        });
        self.push("}");
        self.newline();
    }

    /// Format a `security { }` policy section (v0.141). One `name: value` field
    /// per line, with a trailing comma, mirroring `format_cors_policy`.
    fn format_security_policy(&mut self, security: &SecurityPolicy) {
        self.emit_leading_comments(&security.trivia.leading);
        self.push("security {");
        self.newline();
        self.indented(|f| {
            for field in &security.fields {
                f.push(&format!(
                    "{}: {},",
                    field.name.name,
                    expr_to_string(&field.value)
                ));
                f.newline();
            }
        });
        self.push("}");
        self.newline();
    }

    /// Format a `limits { }` policy section (v0.142). One `name: value` field per
    /// line, with a trailing comma, mirroring `format_cors_policy`. A `maxBody`
    /// value keeps its as-written `_` digit separators (the `IntLit` lexeme).
    fn format_limits_policy(&mut self, limits: &LimitsPolicy) {
        self.emit_leading_comments(&limits.trivia.leading);
        self.push("limits {");
        self.newline();
        self.indented(|f| {
            for field in &limits.fields {
                f.push(&format!(
                    "{}: {},",
                    field.name.name,
                    expr_to_string(&field.value)
                ));
                f.newline();
            }
        });
        self.push("}");
        self.newline();
    }

    fn format_agent(&mut self, a: &AgentDecl) {
        self.emit_leading_comments(&a.trivia.leading);
        if let Some(doc) = &a.documentation {
            self.emit_doc(doc);
        }
        self.push(&format!("agent {} {{", a.name.name));
        self.newline();
        self.indented(|f| {
            // key
            f.push(&format!(
                "key {}: {}",
                a.key_name.name,
                type_ref_to_string(&a.key_type)
            ));
            f.newline();
            f.newline();
            // storage (v0.81, storage track): the agent's `store` fields.
            for sf in &a.store_fields {
                f.format_store_field(sf);
                f.newline();
            }
            // v0.80: invariants form a phase between the storage fields and the
            // handlers.
            for inv in &a.invariants {
                f.newline();
                f.format_invariant(inv);
            }
            // v0.116: step invariants form part of the same phase, beside the
            // snapshot invariants.
            for tr in &a.transitions {
                f.newline();
                f.format_transition(tr);
            }
            // handlers
            for h in &a.handlers {
                f.newline();
                f.format_handler(h);
            }
        });
        self.push("}");
        self.emit_trailing_comment(a.trivia.trailing.as_deref());
        if a.trivia.trailing.is_none() {
            self.newline();
        }
    }

    /// Format a `store` field (v0.81): `store <name>: <Kind> [= <init>]`, with
    /// its leading comments / doc and trailing comment. The enclosing loop adds
    /// the line break.
    fn format_store_field(&mut self, sf: &StoreField) {
        self.emit_leading_comments(&sf.trivia.leading);
        if let Some(doc) = &sf.documentation {
            self.emit_doc(doc);
        }
        self.push(&format!(
            "store {}: {}",
            sf.name.name,
            store_kind_to_string(&sf.kind)
        ));
        // v0.85 (ADR 0111): annotations follow the kind, one space-separated each.
        for ann in &sf.annotations {
            self.push(&format!(" {}", annotation_to_string(ann)));
        }
        if let Some(init) = &sf.init {
            self.push(&format!(" = {}", expr_with_prec(init, 0)));
        }
        self.emit_trailing_comment(sf.trivia.trailing.as_deref());
    }

    /// Format an agent invariant (v0.80): the name on one line, the predicate
    /// indented beneath, matching the §14 worked examples.
    fn format_invariant(&mut self, inv: &Invariant) {
        self.emit_leading_comments(&inv.trivia.leading);
        if let Some(doc) = &inv.documentation {
            self.emit_doc(doc);
        }
        self.push(&format!("invariant {}:", inv.name.name));
        self.newline();
        self.indented(|f| {
            f.push(&expr_to_string(&inv.predicate));
        });
        self.emit_trailing_comment(inv.trivia.trailing.as_deref());
        if inv.trivia.trailing.is_none() {
            self.newline();
        }
    }

    /// Format an agent step invariant (v0.116): `transition <name>:` with the
    /// `old`/`new` predicate indented beneath, mirroring [`format_invariant`].
    fn format_transition(&mut self, tr: &Transition) {
        self.emit_leading_comments(&tr.trivia.leading);
        if let Some(doc) = &tr.documentation {
            self.emit_doc(doc);
        }
        self.push(&format!("transition {}:", tr.name.name));
        self.newline();
        self.indented(|f| {
            f.push(&expr_to_string(&tr.predicate));
        });
        self.emit_trailing_comment(tr.trivia.trailing.as_deref());
        if tr.trivia.trailing.is_none() {
            self.newline();
        }
    }

    fn format_actor(&mut self, a: &ActorDecl) {
        self.emit_leading_comments(&a.trivia.leading);
        if let Some(doc) = &a.documentation {
            self.emit_doc(doc);
        }
        if let Some(r) = &a.refinement {
            // Reserved refinement form: `actor Name = Base where <predicate>`.
            self.push(&format!(
                "actor {} = {} where {}",
                a.name.name,
                r.base.name,
                expr_to_string(&r.predicate)
            ));
        } else {
            // Normal form: `actor Name { auth = Scheme(, identity = Type)? }`.
            let auth = a.auth.as_ref().map(|i| i.name.as_str()).unwrap_or("None");
            self.push(&format!("actor {} {{ auth = {auth}", a.name.name));
            if !a.auth_config.is_empty() {
                let args: Vec<String> = a
                    .auth_config
                    .iter()
                    .map(|arg| match &arg.value {
                        bynk_syntax::ast::SchemeArgValue::Str(s) => {
                            format!("{} = \"{}\"", arg.key.name, escape_string(s))
                        }
                        bynk_syntax::ast::SchemeArgValue::Int(n) => {
                            format!("{} = {n}", arg.key.name)
                        }
                    })
                    .collect();
                self.push(&format!("({})", args.join(", ")));
            }
            if let Some(id) = &a.identity {
                self.push(&format!(", identity = {}", type_ref_to_string(id)));
            }
            self.push(" }");
        }
        self.emit_trailing_comment(a.trivia.trailing.as_deref());
        if a.trivia.trailing.is_none() {
            self.newline();
        }
    }

    fn format_handler(&mut self, h: &Handler) {
        self.emit_leading_comments(&h.trivia.leading);
        if let Some(doc) = &h.documentation {
            self.emit_doc(doc);
        }
        // v0.140 (ADR 0163): handler-position annotations (`@cache(…)`) print one
        // per line above the `on`, mirroring how decorators read in source. Each is
        // rendered by the shared `annotation_to_string` used for `store` fields.
        for ann in &h.annotations {
            self.push(&annotation_to_string(ann));
            self.newline();
        }
        // The handler kind prefix: `on call`, `on http METHOD "path"`, or
        // `on cron("expr")`. Agent `on call` handlers carry a method name.
        match &h.kind {
            HandlerKind::Call => {
                self.push("on call");
                if let Some(m) = &h.method_name {
                    self.push(&format!(" {}", m.name));
                }
            }
            HandlerKind::Http { method, path } => {
                // Trailing space: the path string is followed by the param list,
                // which reads better separated (`… "/path" (params)`).
                self.push(&format!(
                    "on {}(\"{}\") ",
                    method.as_str(),
                    escape_string(path)
                ));
            }
            HandlerKind::Cron { expr } => {
                self.push(&format!("on schedule(\"{}\") ", escape_string(expr)));
            }
            HandlerKind::Message => {
                self.push("on message");
            }
            HandlerKind::Open => {
                self.push("on open");
            }
            HandlerKind::Close => {
                self.push("on close");
            }
        }
        // The param list follows the kind prefix directly — `on call(params)`,
        // `on open(params)` — while the Http/Cron prefixes already emit a trailing
        // space (`on GET("/x") (params)`). (v0.155: the `by` clause no longer sits
        // here, so no separating space is needed.)
        self.format_params(&h.params, false);
        self.push(" -> ");
        self.format_type_ref(&h.return_type);
        // v0.155: the ambient `by`/`given` clauses follow the return type, `by`
        // first — relocated from before the parameter list to end the
        // `by Actor (params)` call illusion.
        if let Some(by) = &h.by_clause {
            self.push(&format!(" {}", by_clause_src(by)));
        }
        if !h.given.is_empty() {
            self.push(" given ");
            let names: Vec<String> = h.given.iter().map(cap_ref_src).collect();
            self.push(&names.join(", "));
        }
        self.push(" ");
        self.format_block(&h.body);
        self.emit_trailing_comment(h.trivia.trailing.as_deref());
        if h.trivia.trailing.is_none() {
            self.newline();
        }
    }

    // -- Blocks, statements, expressions --

    fn format_block(&mut self, b: &Block) {
        // A block with no statements, no trivia, and a simple tail
        // expression can be emitted inline if it fits; otherwise multi-line.
        let tail_oneline = expr_to_string(&b.tail);
        let any_stmt_trivia = b.statements.iter().any(|s| !statement_trivia(s).is_empty());
        if b.statements.is_empty()
            && b.tail_leading_comments.is_empty()
            && !any_stmt_trivia
            && self.line_fits(&format!("{{ {tail_oneline} }}"))
            && !tail_oneline.contains('\n')
        {
            self.push("{ ");
            self.format_expr(&b.tail);
            self.push(" }");
            return;
        }
        self.push("{");
        self.newline();
        self.indented(|f| {
            for stmt in &b.statements {
                let trivia = statement_trivia(stmt);
                f.emit_leading_comments(&trivia.leading);
                f.format_statement(stmt);
                f.emit_trailing_comment(trivia.trailing.as_deref());
                if trivia.trailing.is_none() {
                    f.newline();
                }
            }
            f.emit_leading_comments(&b.tail_leading_comments);
            // v0.7 / v0.146 (ADR 0170): a block written with no explicit tail
            // carries an implicit `()` tail that the parser synthesises. Don't
            // print it — Bynk has no statement terminators, so a printed `()` on
            // the next line would re-attach to the last statement on re-parse
            // (`x == y` `()` → `x == y()`), breaking idempotency. The parser
            // re-derives the implicit unit tail, so omitting it is loss-free.
            let omit_implicit_tail = b.implicit_tail && b.tail_leading_comments.is_empty();
            if !omit_implicit_tail {
                f.format_expr(&b.tail);
                f.newline();
            }
        });
        self.push("}");
    }

    fn format_statement(&mut self, s: &Statement) {
        match s {
            Statement::Let(l) => {
                self.push("let ");
                self.push(&l.name.name);
                if let Some(t) = &l.type_annot {
                    self.push(": ");
                    self.format_type_ref(t);
                }
                self.push(" = ");
                self.format_expr(&l.value);
            }
            Statement::EffectLet(l) => {
                self.push("let ");
                self.push(&l.name.name);
                if let Some(t) = &l.type_annot {
                    self.push(": ");
                    self.format_type_ref(t);
                }
                self.push(" <- ");
                self.format_expr(&l.value);
                if let Some(p) = &l.principal {
                    self.push(&format!(" {}", call_site_actor_src(p)));
                }
            }
            Statement::Expect(a) => {
                self.push("expect ");
                self.format_expr(&a.value);
            }
            Statement::Send(s) => {
                self.push("~> ");
                self.format_expr(&s.value);
            }
            Statement::Do(d) => {
                self.push("do ");
                self.format_expr(&d.value);
            }
            Statement::Assign(a) => {
                self.push(&a.target.name);
                self.push(" := ");
                self.format_expr(&a.value);
            }
        }
    }

    fn format_expr(&mut self, e: &Expr) {
        // `match` renders multi-line, so it must go through the indent-aware
        // emitter rather than `expr_to_string` — the latter builds a flat
        // string with hardcoded single-tab arms that ignores the current
        // nesting depth (the closing brace and every arm would land at column
        // one regardless of how deeply the `match` is nested). Everything else
        // is single-line and renders fine as a string.
        match &e.kind {
            ExprKind::Match { discriminant, arms } => self.format_match(discriminant, arms),
            _ => self.push(&expr_to_string(e)),
        }
    }

    /// Emit a `match` expression at the current indent level. Arms sit one
    /// level deeper than the `match`/`}`; block-bodied arms recurse through
    /// `format_block` so their statements indent correctly in turn.
    fn format_match(&mut self, discriminant: &Expr, arms: &[MatchArm]) {
        self.push("match ");
        self.format_expr(discriminant);
        self.push(" {");
        self.newline();
        self.indented(|f| {
            for arm in arms {
                f.push(&pattern_to_string(&arm.pattern));
                // ADR 0169: render an optional `if <guard>` before `=>`.
                if let Some(guard) = &arm.guard {
                    f.push(" if ");
                    f.push(&expr_with_prec(guard, 0));
                }
                f.push(" => ");
                match &arm.body {
                    MatchBody::Expr(e) => f.format_expr(e),
                    MatchBody::Block(b) => f.format_block(b),
                }
                f.push(",");
                f.newline();
            }
        });
        self.push("}");
    }
}

/// Borrow the trivia attached to a statement variant.
/// Render a `given`-clause capability reference back to source: a bare name
/// for a local capability, or `prefix.Name` for a cross-context one (v0.15).
fn cap_ref_src(c: &CapRef) -> String {
    match &c.context {
        Some(prefix) => format!("{}.{}", prefix.joined(), c.name.name),
        None => c.name.name.clone(),
    }
}

/// Render a `by` clause back to source: `by <Actor>` (binder-less), `by <b>: <Actor>`
/// (captured identity), or an ordered sum `by <b>: A | B` (v0.52). Shared by handler
/// and service-header (v0.155) formatting.
fn by_clause_src(by: &ByClause) -> String {
    let actors = by
        .actors
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    match &by.binder {
        Some(b) => format!("by {}: {actors}", b.name),
        None => format!("by {actors}"),
    }
}

/// v0.182 (#664): render a call-site actor clause — `by User("bob")` or the
/// unit-identity `by Visitor`.
fn call_site_actor_src(p: &CallSiteActor) -> String {
    match &p.identity {
        Some(id) => format!("by {}({})", p.actor.name, expr_with_prec(id, 0)),
        None => format!("by {}", p.actor.name),
    }
}

/// Render a storage kind: `Cell[Int]`, `Map[K, V]`, or a bare head (v0.81).
fn store_kind_to_string(k: &StoreKind) -> String {
    if k.args.is_empty() {
        k.head.name.clone()
    } else {
        format!(
            "{}[{}]",
            k.head.name,
            k.args
                .iter()
                .map(type_ref_to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// Render a storage annotation (v0.85; ADR 0111): `@name`, or `@name(arg, …)`
/// where each argument is an optional `label: ` then the value expression.
/// Render a storage annotation as a single source-syntax token: `@indexed(by:
/// id)`, `@bounded(10000)`, `@ttl(5.minutes)`, or a bare `@retain`. Public so
/// the LSP's agent-state hover (ADR 0161) can render a `store` field's
/// annotations without re-deriving them.
pub fn annotation_to_string(ann: &Annotation) -> String {
    if ann.args.is_empty() {
        return format!("@{}", ann.name.name);
    }
    let args = ann
        .args
        .iter()
        .map(|a| match &a.label {
            Some(l) => format!("{}: {}", l.name, expr_with_prec(&a.value, 0)),
            None => expr_with_prec(&a.value, 0),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("@{}({})", ann.name.name, args)
}

/// v0.118: render a `stub` clause head-to-tail as a single source line:
/// `stub <capability>.<method>(<args>) <rhs>` (testing track slice 6).
fn stub_clause_to_string(pv: &StubClause) -> String {
    let args = pv
        .args
        .iter()
        .map(|a| match a {
            ArgPattern::Any(_) => "_".to_string(),
            ArgPattern::Value(e) => expr_to_string(e),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let rhs = match &pv.rhs {
        StubRhs::Returns(e) => format!("returns {}", expr_to_string(e)),
        StubRhs::Fails(_) => "fails".to_string(),
        StubRhs::ReturnsEach(outcomes, _) => {
            let items = outcomes
                .iter()
                .map(|o| match o {
                    SeqOutcome::Value(e) => expr_to_string(e),
                    SeqOutcome::Fails(_) => "fails".to_string(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("returns each [{items}]")
        }
    };
    format!(
        "stub {}.{}({}) {}",
        pv.capability.name, pv.method.name, args, rhs
    )
}

fn statement_trivia(s: &Statement) -> &Trivia {
    match s {
        Statement::Let(l) | Statement::EffectLet(l) => &l.trivia,
        Statement::Expect(a) => &a.trivia,
        Statement::Send(s) => &s.trivia,
        Statement::Do(d) => &d.trivia,
        Statement::Assign(a) => &a.trivia,
    }
}

// -- String-rendering helpers (used by inline single-line emission) --

fn type_ref_to_string(t: &TypeRef) -> String {
    match t {
        TypeRef::Base(b, _) => b.name().to_string(),
        TypeRef::Named(id) => id.name.clone(),
        TypeRef::Result(a, b, _) => format!(
            "Result[{}, {}]",
            type_ref_to_string(a),
            type_ref_to_string(b)
        ),
        TypeRef::Option(t, _) => format!("Option[{}]", type_ref_to_string(t)),
        TypeRef::Effect(t, _) => format!("Effect[{}]", type_ref_to_string(t)),
        TypeRef::HttpResult(t, _) => format!("HttpResult[{}]", type_ref_to_string(t)),
        TypeRef::QueueResult(_) => "QueueResult".to_string(),
        TypeRef::List(t, _) => format!("List[{}]", type_ref_to_string(t)),
        TypeRef::Query(t, _) => format!("Query[{}]", type_ref_to_string(t)),
        TypeRef::Stream(t, _) => format!("Stream[{}]", type_ref_to_string(t)),
        TypeRef::Connection(t, _) => format!("Connection[{}]", type_ref_to_string(t)),
        TypeRef::History(t, _) => format!("History[{}]", type_ref_to_string(t)),
        TypeRef::Map(k, v, _) => {
            format!("Map[{}, {}]", type_ref_to_string(k), type_ref_to_string(v))
        }
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::JsonError(_) => "JsonError".to_string(),
        TypeRef::Unit(_) => "()".to_string(),
        // v0.157 (ADR 0183): a user generic-type application, as written.
        TypeRef::App { name, args, .. } => format!(
            "{}[{}]",
            name.name,
            args.iter()
                .map(type_ref_to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeRef::Fn(params, ret, _) => {
            let lhs = match params.len() {
                0 => "()".to_string(),
                1 if !matches!(params[0], TypeRef::Fn(..)) => type_ref_to_string(&params[0]),
                _ => format!(
                    "({})",
                    params
                        .iter()
                        .map(type_ref_to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            };
            format!("{lhs} -> {}", type_ref_to_string(ret))
        }
    }
}

pub fn refinement_to_string(r: &Refinement) -> String {
    let mut s = String::new();
    for (i, p) in r.predicates.iter().enumerate() {
        if i > 0 {
            s.push_str(" && ");
        }
        s.push_str(&pred_to_string(p));
    }
    s
}

fn pred_to_string(p: &RefinementPred) -> String {
    match &p.kind {
        PredKind::Matches(re) => format!("Matches(\"{}\")", escape_string(re)),
        PredKind::InRange(a, b) => format!("InRange({}, {})", a.value, b.value),
        PredKind::InRangeF(a, b) => format!("InRange({}, {})", a.lexeme, b.lexeme),
        PredKind::MinLength(n) => format!("MinLength({n})"),
        PredKind::MaxLength(n) => format!("MaxLength({n})"),
        PredKind::Length(n) => format!("Length({n})"),
        PredKind::NonNegative => "NonNegative".to_string(),
        PredKind::Positive => "Positive".to_string(),
        PredKind::NonEmpty => "NonEmpty".to_string(),
    }
}

pub fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

pub fn expr_to_string(e: &Expr) -> String {
    expr_with_prec(e, 0)
}

// Operator precedences (smaller = binds looser):
//   1: || 2: && 3: == != 4: < <= > >= 5: + - 6: * / 7: unary ! - 8: postfix . () ?
fn binop_prec(op: BinOp) -> u8 {
    match op {
        // v0.80: `implies` is the lowest-precedence binary operator (below `||`).
        BinOp::Implies => 0,
        BinOp::Or => 1,
        BinOp::And => 2,
        BinOp::Eq | BinOp::NotEq => 3,
        BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => 4,
        BinOp::Add | BinOp::Sub => 5,
        BinOp::Mul | BinOp::Div => 6,
    }
}

fn expr_with_prec(e: &Expr, parent_prec: u8) -> String {
    match &e.kind {
        // v0.142 (ADR 0166): the stored lexeme verbatim — formatting must not
        // normalise away the author's `_` digit separators.
        ExprKind::IntLit { lexeme, .. } => lexeme.clone(),
        // v0.21: the stored lexeme verbatim — formatting must not normalise.
        ExprKind::FloatLit { lexeme, .. } => lexeme.clone(),
        // v0.86 (ADR 0112): a duration literal `<value>.<unit>`.
        ExprKind::DurationLit { value, unit, .. } => format!("{value}.{}", unit.name()),
        ExprKind::StrLit(s) => format!("\"{}\"", escape_string(s)),
        // v0.43: re-emit the interpolated string — chunks re-escaped, each
        // hole as `\(expr)`. Re-escaping a chunk's literal `\` to `\\` keeps a
        // source `\\(` (an escaped `\(`) round-tripping as text, not a hole.
        ExprKind::InterpStr(parts) => {
            let mut out = String::from("\"");
            for part in parts {
                match part {
                    InterpPart::Chunk(text) => out.push_str(&escape_string(text)),
                    InterpPart::Hole(hole) => {
                        out.push_str(&format!("\\({})", expr_with_prec(hole, 0)));
                    }
                }
            }
            out.push('"');
            out
        }
        ExprKind::BoolLit(b) => b.to_string(),
        ExprKind::UnitLit => "()".to_string(),
        ExprKind::Ident(id) => id.name.clone(),
        ExprKind::ListLit(elems) => format!(
            "[{}]",
            elems
                .iter()
                .map(expr_to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExprKind::Call {
            name,
            type_args,
            args,
        } => {
            let targs = if type_args.is_empty() {
                String::new()
            } else {
                format!(
                    "[{}]",
                    type_args
                        .iter()
                        .map(type_ref_to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let parts: Vec<String> = args.iter().map(|a| expr_with_prec(a, 0)).collect();
            format!("{}{}({})", name.name, targs, parts.join(", "))
        }
        ExprKind::BinOp(op, l, r) => {
            let prec = binop_prec(*op);
            let inner = format!(
                "{} {} {}",
                expr_with_prec(l, prec),
                op.name(),
                expr_with_prec(r, prec + 1)
            );
            if prec < parent_prec {
                format!("({inner})")
            } else {
                inner
            }
        }
        ExprKind::UnaryOp(op, inner) => {
            // Unary binds tightly (prec 7).
            let s = format!("{}{}", op.name(), expr_with_prec(inner, 7));
            if parent_prec > 7 { format!("({s})") } else { s }
        }
        ExprKind::Paren(inner) => format!("({})", expr_with_prec(inner, 0)),
        // v0.20a: a lambda prints as `(params) => body`.
        ExprKind::Lambda(lambda) => {
            let params: Vec<String> = lambda
                .params
                .iter()
                .map(|p| match &p.type_ref {
                    Some(tr) => format!("{}: {}", p.name.name, type_ref_to_string(tr)),
                    None => p.name.name.clone(),
                })
                .collect();
            let body = match &lambda.body.kind {
                ExprKind::Block(b) => format_block_oneline(b),
                _ => expr_with_prec(&lambda.body, 0),
            };
            format!("({}) => {}", params.join(", "), body)
        }
        ExprKind::Block(b) => format_block_oneline(b),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            // v0.146 (ADR 0170): an `if` with no `else` carries a synthesised
            // unit else-branch — omit it so the else-less form round-trips.
            if else_block.is_synth_unit() {
                format!(
                    "if {} {}",
                    expr_with_prec(cond, 0),
                    format_block_oneline(then_block),
                )
            } else {
                format!(
                    "if {} {} else {}",
                    expr_with_prec(cond, 0),
                    format_block_oneline(then_block),
                    format_block_oneline(else_block),
                )
            }
        }
        ExprKind::Ok(v) => format!("Ok({})", expr_with_prec(v, 0)),
        ExprKind::Err(v) => format!("Err({})", expr_with_prec(v, 0)),
        ExprKind::Some(v) => format!("Some({})", expr_with_prec(v, 0)),
        ExprKind::None => "None".to_string(),
        ExprKind::Question(v) => format!("{}?", expr_with_prec(v, 8)),
        ExprKind::ConstructorCall {
            type_name,
            method,
            args,
        } => {
            let parts: Vec<String> = args.iter().map(|a| expr_with_prec(a, 0)).collect();
            format!("{}.{}({})", type_name.name, method.name, parts.join(", "))
        }
        ExprKind::RecordConstruction { type_name, fields } => {
            let parts: Vec<String> = fields
                .iter()
                .map(|f| match &f.value {
                    Some(v) => format!("{}: {}", f.name.name, expr_with_prec(v, 0)),
                    None => f.name.name.clone(),
                })
                .collect();
            if parts.is_empty() {
                format!("{} {{}}", type_name.name)
            } else {
                format!("{} {{ {} }}", type_name.name, parts.join(", "))
            }
        }
        ExprKind::FieldAccess { receiver, field } => {
            format!("{}.{}", expr_with_prec(receiver, 8), field.name)
        }
        ExprKind::MethodCall {
            receiver,
            method,
            type_args,
            args,
        } => {
            let targs = if type_args.is_empty() {
                String::new()
            } else {
                format!(
                    "[{}]",
                    type_args
                        .iter()
                        .map(type_ref_to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let parts: Vec<String> = args.iter().map(|a| expr_with_prec(a, 0)).collect();
            format!(
                "{}.{}{targs}({})",
                expr_with_prec(receiver, 8),
                method.name,
                parts.join(", ")
            )
        }
        ExprKind::Match { discriminant, arms } => {
            let mut out = String::new();
            out.push_str("match ");
            out.push_str(&expr_with_prec(discriminant, 0));
            out.push_str(" {\n");
            for arm in arms {
                out.push('\t');
                out.push_str(&pattern_to_string(&arm.pattern));
                if let Some(guard) = &arm.guard {
                    out.push_str(" if ");
                    out.push_str(&expr_with_prec(guard, 0));
                }
                out.push_str(" => ");
                match &arm.body {
                    MatchBody::Expr(e) => out.push_str(&expr_with_prec(e, 0)),
                    MatchBody::Block(b) => out.push_str(&format_block_oneline(b)),
                }
                out.push_str(",\n");
            }
            out.push('}');
            out
        }
        ExprKind::Is { value, pattern } => {
            format!(
                "{} is {}",
                expr_with_prec(value, 4),
                pattern_to_string(pattern)
            )
        }
        ExprKind::RecordSpread {
            type_name,
            base,
            overrides,
        } => {
            let mut parts = vec![format!("...{}", expr_with_prec(base, 0))];
            for f in overrides {
                if let Some(v) = &f.value {
                    parts.push(format!("{}: {}", f.name.name, expr_with_prec(v, 0)));
                } else {
                    parts.push(f.name.name.clone());
                }
            }
            let body = parts.join(", ");
            match type_name {
                Some(tn) => format!("{} {{ {} }}", tn.name, body),
                None => format!("{{ {} }}", body),
            }
        }
        ExprKind::EffectPure(v) => format!("Effect.pure({})", expr_with_prec(v, 0)),
        ExprKind::Expect(v) => format!("expect {}", expr_with_prec(v, 0)),
        ExprKind::Val { type_ref, args } => {
            let t = type_ref_to_string(type_ref);
            if args.is_empty() {
                format!("Val[{t}]")
            } else {
                let a = args
                    .iter()
                    .map(|x| expr_with_prec(x, 0))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Val[{t}]({a})")
            }
        }
        ExprKind::Wire(inner) => format!("Wire({})", expr_with_prec(inner, 0)),
        ExprKind::Trace { cap, op } => format!("trace({}.{})", cap.name, op.name),
        ExprKind::Observation(o) => {
            let subject = format!("{}.{}", o.cap.name, o.op.name);
            match &o.matcher {
                ObservationMatcher::NeverCalled => format!("{subject} never called"),
                ObservationMatcher::Before { cap, op } => {
                    format!("{subject} before {}.{}", cap.name, op.name)
                }
                ObservationMatcher::Called { count, with_pred } => {
                    let mut s = format!("{subject} called");
                    if let Some(c) = count {
                        if matches!(c.kind, ExprKind::IntLit { value: 1, .. }) {
                            s.push_str(" once");
                        } else {
                            s.push_str(&format!(" {} times", expr_with_prec(c, 0)));
                        }
                    }
                    if let Some(p) = with_pred {
                        s.push_str(&format!(" with {}", expr_with_prec(p, 0)));
                    }
                    s
                }
            }
        }
    }
}

fn pattern_to_string(p: &Pattern) -> String {
    match p {
        Pattern::Wildcard(_) => "_".to_string(),
        // ADR 0169: a bare name binding renders as its identifier.
        Pattern::Binding(id) => id.name.clone(),
        // v0.130: literal patterns render as their source literal.
        Pattern::Literal { value, .. } => match value {
            LiteralValue::Int(n) => n.to_string(),
            LiteralValue::Str(s) => format!("\"{}\"", escape_string(s)),
            LiteralValue::Bool(b) => b.to_string(),
        },
        Pattern::Variant {
            type_name,
            variant,
            bindings,
            ..
        } => {
            let name_part = match type_name {
                Some(t) => format!("{}.{}", t.name, variant.name),
                None => variant.name.clone(),
            };
            if bindings.is_empty() {
                name_part
            } else {
                // ADR 0169: each payload binding is a full sub-pattern.
                let parts: Vec<String> = bindings
                    .iter()
                    .map(|b| match &b.kind {
                        PatternBindingKind::Positional { pattern } => pattern_to_string(pattern),
                        PatternBindingKind::Named { field, pattern } => {
                            format!("{}: {}", field.name, pattern_to_string(pattern))
                        }
                    })
                    .collect();
                format!("{}({})", name_part, parts.join(", "))
            }
        }
    }
}

fn format_block_oneline(b: &Block) -> String {
    if b.statements.is_empty() {
        // v0.146 (ADR 0170): an empty block with a synthesised `()` tail prints
        // as `{}` — printing `{ () }` would not round-trip against the parser's
        // implicit-tail synthesis.
        if b.implicit_tail {
            "{}".to_string()
        } else {
            format!("{{ {} }}", expr_with_prec(&b.tail, 0))
        }
    } else {
        // Multi-line block — render with newlines and tab indentation.
        let mut out = String::from("{\n");
        for stmt in &b.statements {
            out.push('\t');
            out.push_str(&stmt_to_string(stmt));
            out.push('\n');
        }
        // Omit the implicit `()` tail (see `format_block`) — printing it breaks
        // round-trip idempotency.
        if !b.implicit_tail {
            out.push('\t');
            out.push_str(&expr_with_prec(&b.tail, 0));
            out.push('\n');
        }
        out.push('}');
        out
    }
}

fn stmt_to_string(s: &Statement) -> String {
    match s {
        Statement::Let(l) => {
            let mut out = format!("let {}", l.name.name);
            if let Some(t) = &l.type_annot {
                out.push_str(&format!(": {}", type_ref_to_string(t)));
            }
            out.push_str(&format!(" = {}", expr_with_prec(&l.value, 0)));
            out
        }
        Statement::EffectLet(l) => {
            let mut out = format!("let {}", l.name.name);
            if let Some(t) = &l.type_annot {
                out.push_str(&format!(": {}", type_ref_to_string(t)));
            }
            out.push_str(&format!(" <- {}", expr_with_prec(&l.value, 0)));
            if let Some(p) = &l.principal {
                out.push_str(&format!(" {}", call_site_actor_src(p)));
            }
            out
        }
        Statement::Expect(a) => format!("expect {}", expr_with_prec(&a.value, 0)),
        Statement::Send(s) => format!("~> {}", expr_with_prec(&s.value, 0)),
        Statement::Do(d) => format!("do {}", expr_with_prec(&d.value, 0)),
        Statement::Assign(a) => format!("{} := {}", a.target.name, expr_with_prec(&a.value, 0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str) -> String {
        format_source(src, &FormatOptions::default()).expect("format failed")
    }

    #[test]
    fn formats_minimal_commons() {
        let src = "commons fitness.units {}";
        let out = fmt(src);
        assert!(out.starts_with("commons fitness.units"));
        // Idempotency.
        let out2 = fmt(&out);
        assert_eq!(out, out2);
    }

    #[test]
    fn formats_refined_type() {
        let src = "commons x { type Metres = Int where NonNegative }";
        let out = fmt(src);
        assert!(out.contains("type Metres = Int where NonNegative"));
        let out2 = fmt(&out);
        assert_eq!(out, out2);
    }

    #[test]
    fn formats_function_decl() {
        let src = "commons x { fn add(a: Int, b: Int) -> Int { a + b } }";
        let out = fmt(src);
        assert!(out.contains("fn add(a: Int, b: Int) -> Int"));
        let out2 = fmt(&out);
        assert_eq!(out, out2);
    }

    #[test]
    fn formats_record() {
        let src = "commons x { type Pt = { x: Int, y: Int } }";
        let out = fmt(src);
        let out2 = fmt(&out);
        assert_eq!(out, out2, "formatter not idempotent: {out}");
    }

    #[test]
    fn formats_doc_block() {
        let src = "commons x {\n---\nA descriptive doc.\n---\ntype T = Int where Positive\n}";
        let out = fmt(src);
        assert!(out.contains("A descriptive doc."));
        let out2 = fmt(&out);
        assert_eq!(out, out2);
    }

    // -- v1.1 comment preservation --

    #[test]
    fn preserves_leading_line_comment_on_decl() {
        let src = "commons x {\n-- explain T\ntype T = Int where NonNegative\n}";
        let out = fmt(src);
        assert!(out.contains("-- explain T"), "comment dropped: {out}");
        // Idempotent.
        assert_eq!(out, fmt(&out));
    }

    #[test]
    fn preserves_trailing_line_comment_on_decl() {
        let src = "commons x {\ntype T = Int where NonNegative  -- short\n}";
        let out = fmt(src);
        assert!(out.contains("-- short"));
        // The trailing comment must remain on the same line as the decl.
        assert!(
            out.lines()
                .any(|l| l.contains("type T") && l.contains("-- short")),
            "trailing comment not on same line: {out}"
        );
        assert_eq!(out, fmt(&out));
    }

    #[test]
    fn preserves_grouped_leading_comments() {
        let src = "commons x {\n-- one\n-- two\ntype T = Int where Positive\n}";
        let out = fmt(src);
        assert!(out.contains("-- one"));
        assert!(out.contains("-- two"));
        // Adjacent — no blank line between the comments.
        let i1 = out.find("-- one").unwrap();
        let i2 = out.find("-- two").unwrap();
        let between = &out[i1..i2];
        assert_eq!(
            between.matches('\n').count(),
            1,
            "blank line inserted: {out}"
        );
        assert_eq!(out, fmt(&out));
    }

    #[test]
    fn preserves_comment_before_block_tail() {
        let src = "commons x {\nfn f(n: Int) -> Int {\nlet y = n + 1\n-- result\ny\n}\n}";
        let out = fmt(src);
        assert!(out.contains("-- result"), "tail comment dropped: {out}");
        assert_eq!(out, fmt(&out));
    }

    #[test]
    fn preserves_comment_with_doc_block_above_decl() {
        let src = "commons x {\n-- TODO: rename\n---\nThe canonical T.\n---\ntype T = Int where Positive\n}";
        let out = fmt(src);
        assert!(out.contains("-- TODO: rename"));
        assert!(out.contains("The canonical T."));
        // Spec layout: comment, then doc block, then declaration.
        let ic = out.find("-- TODO: rename").unwrap();
        let id = out.find("The canonical T.").unwrap();
        let it = out.find("type T").unwrap();
        assert!(ic < id && id < it, "ordering wrong: {out}");
        assert_eq!(out, fmt(&out));
    }

    #[test]
    fn preserves_trailing_file_comment() {
        let src = "commons x.y\n\ntype T = Int where Positive\n-- TODO\n";
        let out = fmt(src);
        assert!(out.contains("-- TODO"));
        assert_eq!(out, fmt(&out));
    }

    // -- #735 round-trip guard --

    #[test]
    fn code_only_canonical_ignores_comments() {
        // Two sources whose only difference is comments must reduce to the same
        // comment-free canonical form — this is what lets the round-trip guard
        // compare structure while the formatter re-flows trivia freely.
        let opts = FormatOptions::default();
        let bare = "commons x { type T = Int where Positive }";
        let commented = "commons x {\n-- a note\ntype T = Int where Positive  -- trailing\n}";
        assert_eq!(
            code_only_canonical(bare, &opts).unwrap(),
            code_only_canonical(commented, &opts).unwrap(),
        );
    }

    #[test]
    fn roundtrip_guard_accepts_faithful_output() {
        // The formatter's own output over a real source must round-trip.
        let opts = FormatOptions::default();
        let src = "commons x { fn add(a: Int, b: Int) -> Int { a + b } }";
        let out = format_source(src, &opts).unwrap();
        assert!(roundtrip_divergence(src, &out, &opts).is_none());
    }

    #[test]
    fn roundtrip_guard_rejects_non_parsing_output() {
        // Simulate a printer that emitted garbage: the output no longer parses,
        // so the guard must fire rather than let it be written. The output here
        // is *longer* than the source and its parse error lands near its end —
        // the error's span must nonetheless stay within the source, because the
        // caller renders it against `source`, not the output (an out-of-range
        // primary span misplaces the caret or panics ariadne). See
        // `roundtrip_error`.
        let opts = FormatOptions::default();
        let src = "commons x { type T = Int where Positive }";
        let corrupt = "commons x { type T = Int where Positive } fn f(a: Int) -> Int { a +";
        assert!(
            corrupt.len() > src.len(),
            "output must be the longer buffer"
        );
        let err =
            roundtrip_divergence(src, corrupt, &opts).expect("must reject non-parsing output");
        assert_eq!(err.category, "bynk.fmt.roundtrip");
        assert!(
            err.span.end <= src.len(),
            "roundtrip error span {:?} escapes the source it is rendered against (len {})",
            err.span,
            src.len(),
        );
    }

    #[test]
    fn roundtrip_guard_rejects_structural_divergence() {
        // Simulate a printer that emitted parseable-but-wrong code: the output
        // parses, but to a different AST than the input. The guard must catch it.
        let opts = FormatOptions::default();
        let src = "commons x { type T = Int where Positive }";
        let wrong = "commons x { type T = Bool }";
        let err =
            roundtrip_divergence(src, wrong, &opts).expect("must reject structural divergence");
        assert_eq!(err.category, "bynk.fmt.roundtrip");
        assert!(err.span.end <= src.len(), "span escapes the source buffer");
    }

    #[test]
    fn roundtrip_error_renders_against_source_without_panicking() {
        // The guard's error is rendered against the *source* (`run_fmt` calls
        // `print_errors(&e.errors, &source, …)`). ariadne uses a primary span
        // unconditionally, so a span outside the source buffer misplaces the
        // caret or panics — exactly in the formatter-bug path this guard must
        // handle gracefully. Render the real diagnostic against a short source
        // and assert it produces output without panicking.
        let err = roundtrip_error("the formatter produced output that no longer parses");
        let source = "commons x {}";
        let rendered = bynk_render::render_errors(std::slice::from_ref(&err), source, "<test>");
        assert!(
            rendered.contains("bynk.fmt.roundtrip"),
            "diagnostic did not render: {rendered}"
        );
    }

    #[test]
    fn unchanged_files_without_comments_format_identically() {
        let src = "commons x { type T = Int where NonNegative }";
        let out = fmt(src);
        // Sanity: the formatter still produces the canonical output for
        // existing fixtures (no spurious comment rendering).
        assert!(!out.contains("--"), "unexpected comment in output: {out}");
    }

    // -- v0.81 storage track: `store` fields and the `:=` write --

    #[test]
    fn formats_store_field_and_cell_write() {
        let src = "context shop {\nagent Counter {\nkey id: String\nstore count: Cell[Int] = 0\non call bump() -> Effect[()] {\ncount := count + 1\n()\n}\n}\n}";
        let out = fmt(src);
        assert!(
            out.contains("store count: Cell[Int] = 0"),
            "store field not formatted: {out}"
        );
        assert!(
            out.contains("count := count + 1"),
            "cell write not formatted: {out}"
        );
        assert_eq!(out, fmt(&out), "formatter not idempotent: {out}");
    }

    #[test]
    fn formats_store_only_agent_without_state_block() {
        let src = "context shop {\nagent Counter {\nkey id: String\nstore count: Cell[Int] = 0\non call get() -> Effect[Int] {\ncount\n}\n}\n}";
        let out = fmt(src);
        // A `store`-only agent emits no empty `state { }` block.
        assert!(!out.contains("state {"), "spurious state block: {out}");
        assert!(out.contains("store count: Cell[Int] = 0"), "{out}");
        assert_eq!(out, fmt(&out), "not idempotent: {out}");
    }
}
