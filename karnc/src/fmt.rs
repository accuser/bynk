//! Karn source formatter.
//!
//! Re-parses the source into an AST and re-prints it in canonical form per
//! the style rules in `design/karn-lsp-spec.md` §3.5:
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
//! enclosing statement's leading trivia (or dropped if no such enclosing
//! statement exists). See `design/karn-lsp-spec.md` §3.5 for the canonical
//! comment-placement rules.

use crate::ast::*;
use crate::error::CompileError;
use crate::lexer::tokenize;
use crate::parser::parse_unit;

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

/// Format a Karn source string. On parse failure, returns the original
/// source unchanged is *not* this function's responsibility — callers (LSP,
/// CLI) decide how to handle parse failure. Here we surface the errors so
/// the caller can do so.
pub fn format_source(source: &str, opts: &FormatOptions) -> Result<String, FormatError> {
    let tokens = tokenize(source).map_err(|e| FormatError { errors: vec![e] })?;
    let unit = parse_unit(&tokens, source).map_err(|errors| FormatError { errors })?;
    let mut f = Formatter::new(opts);
    f.format_unit(&unit);
    Ok(f.finish())
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
        self.out.push_str(s);
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
            SourceUnit::Test(t) => self.format_test(t),
            SourceUnit::Integration(i) => self.format_integration(i),
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

    fn format_integration(&mut self, i: &IntegrationDecl) {
        self.emit_leading_comments(&i.trivia.leading);
        if let Some(doc) = &i.documentation {
            self.emit_doc(doc);
        }
        let header = format!("test integration \"{}\"", escape_string(&i.suite));
        match i.form {
            CommonsForm::Brace => {
                self.push(&header);
                self.push(" {");
                self.newline();
                self.indented(|f| {
                    f.format_integration_body(i);
                });
                self.push("}");
                self.newline();
            }
            CommonsForm::Fragment => {
                self.push(&header);
                self.newline();
                self.newline();
                self.format_integration_body(i);
            }
        }
    }

    fn format_integration_body(&mut self, i: &IntegrationDecl) {
        let wires = i
            .participants
            .iter()
            .map(|p| p.joined())
            .collect::<Vec<_>>()
            .join(", ");
        self.push(&format!("wires {wires}"));
        self.newline();
        for u in &i.uses {
            self.newline();
            self.emit_leading_comments(&u.trivia.leading);
            self.push(&format!("uses {}", u.target.joined()));
            self.emit_trailing_comment(u.trivia.trailing.as_deref());
            self.newline();
        }
        for c in &i.cases {
            self.newline();
            self.emit_leading_comments(&c.trivia.leading);
            if let Some(doc) = &c.documentation {
                self.emit_doc(doc);
            }
            self.push(&format!("test \"{}\" ", escape_string(&c.name)));
            self.format_block(&c.body);
            self.newline();
        }
        for comment in &i.trailing_comments {
            self.push(&format!("--{comment}"));
            self.newline();
        }
    }

    fn format_test(&mut self, t: &TestDecl) {
        self.emit_leading_comments(&t.trivia.leading);
        if let Some(doc) = &t.documentation {
            self.emit_doc(doc);
        }
        let header = format!("test {}", t.target.joined());
        match t.form {
            CommonsForm::Brace => {
                self.push(&header);
                self.push(" {");
                self.newline();
                self.indented(|f| {
                    f.format_test_body(&t.uses, &t.mocks, &t.cases, &t.trailing_comments);
                });
                self.push("}");
                self.newline();
            }
            CommonsForm::Fragment => {
                self.push(&header);
                self.newline();
                self.format_test_body(&t.uses, &t.mocks, &t.cases, &t.trailing_comments);
            }
        }
    }

    fn format_test_body(
        &mut self,
        uses: &[UsesDecl],
        mocks: &[MockDecl],
        cases: &[TestCase],
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
        for m in mocks {
            if !first {
                self.newline();
            }
            self.emit_leading_comments(&m.trivia.leading);
            if let Some(doc) = &m.documentation {
                self.emit_doc(doc);
            }
            self.push(&format!(
                "mocks {} = {} {{",
                m.target_name.name, m.impl_name.name
            ));
            self.newline();
            self.indented(|f| {
                let mut first_op = true;
                for op in &m.ops {
                    if !first_op {
                        f.newline();
                    }
                    let params = op
                        .params
                        .iter()
                        .map(|p| format!("{}: {}", p.name.name, type_ref_to_string(&p.type_ref)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    f.push(&format!(
                        "fn {}({params}) -> {} ",
                        op.name.name,
                        type_ref_to_string(&op.return_type)
                    ));
                    f.format_block(&op.body);
                    f.newline();
                    first_op = false;
                }
            });
            self.push("}");
            self.newline();
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
            self.push(&format!("test \"{}\" ", escape_string(&c.name)));
            self.format_block(&c.body);
            self.newline();
            first = false;
        }
        for comment in trailing_comments {
            self.push(&format!("--{comment}"));
            self.newline();
        }
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
        }
    }

    // -- Type declarations --

    fn format_type_decl(&mut self, t: &TypeDecl) {
        self.emit_leading_comments(&t.trivia.leading);
        if let Some(doc) = &t.documentation {
            self.emit_doc(doc);
        }
        self.push(&format!("type {} = ", t.name.name));
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
                self.push(" and ");
            }
            self.format_pred(p);
        }
    }

    fn format_pred(&mut self, p: &RefinementPred) {
        match &p.kind {
            PredKind::Matches(re) => self.push(&format!("Matches(\"{}\")", escape_string(re))),
            PredKind::InRange(a, b) => self.push(&format!("InRange({a}, {b})")),
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
        self.push(" ");
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
                if i + 1 < rendered.len() || f.opts.trailing_comma {
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
        self.push(&format!("service {} {{", s.name.name));
        self.newline();
        self.indented(|f| {
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
            // state
            f.push("state {");
            f.newline();
            f.indented(|f| {
                for (i, field) in a.state_fields.iter().enumerate() {
                    f.format_record_field(field);
                    if i + 1 < a.state_fields.len() || f.opts.trailing_comma {
                        f.push(",");
                    }
                    f.newline();
                }
            });
            f.push("}");
            f.newline();
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

    fn format_handler(&mut self, h: &Handler) {
        self.emit_leading_comments(&h.trivia.leading);
        if let Some(doc) = &h.documentation {
            self.emit_doc(doc);
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
                    "on http {} \"{}\" ",
                    method.as_str(),
                    escape_string(path)
                ));
            }
            HandlerKind::Cron { expr } => {
                self.push(&format!("on cron \"{}\" ", escape_string(expr)));
            }
            HandlerKind::Queue { name } => {
                self.push(&format!("on queue \"{}\" ", escape_string(name)));
            }
        }
        self.format_params(&h.params, false);
        self.push(" -> ");
        self.format_type_ref(&h.return_type);
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
            // v0.7: a block whose last statement is `assert` carries an implicit
            // `()` tail that the parser synthesises. Don't print it — Karn has
            // no statement terminators, so a printed `()` on the next line would
            // re-attach to the assert's expression on re-parse (`x == y` `()` →
            // `x == y()`), breaking idempotency. The parser re-derives the
            // implicit unit tail, so omitting it is loss-free.
            let implicit_unit_after_assert = matches!(b.tail.kind, ExprKind::UnitLit)
                && matches!(b.statements.last(), Some(Statement::Assert(_)))
                && b.tail_leading_comments.is_empty();
            if !implicit_unit_after_assert {
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
            }
            Statement::Commit(c) => {
                self.push("commit ");
                self.format_expr(&c.value);
            }
            Statement::Assert(a) => {
                self.push("assert ");
                self.format_expr(&a.value);
            }
        }
    }

    fn format_expr(&mut self, e: &Expr) {
        self.push(&expr_to_string(e));
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

fn statement_trivia(s: &Statement) -> &Trivia {
    match s {
        Statement::Let(l) | Statement::EffectLet(l) => &l.trivia,
        Statement::Commit(c) => &c.trivia,
        Statement::Assert(a) => &a.trivia,
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
        TypeRef::List(t, _) => format!("List[{}]", type_ref_to_string(t)),
        TypeRef::Map(k, v, _) => {
            format!("Map[{}, {}]", type_ref_to_string(k), type_ref_to_string(v))
        }
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::Unit(_) => "()".to_string(),
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

fn refinement_to_string(r: &Refinement) -> String {
    let mut s = String::new();
    for (i, p) in r.predicates.iter().enumerate() {
        if i > 0 {
            s.push_str(" and ");
        }
        s.push_str(&pred_to_string(p));
    }
    s
}

fn pred_to_string(p: &RefinementPred) -> String {
    match &p.kind {
        PredKind::Matches(re) => format!("Matches(\"{}\")", escape_string(re)),
        PredKind::InRange(a, b) => format!("InRange({a}, {b})"),
        PredKind::MinLength(n) => format!("MinLength({n})"),
        PredKind::MaxLength(n) => format!("MaxLength({n})"),
        PredKind::Length(n) => format!("Length({n})"),
        PredKind::NonNegative => "NonNegative".to_string(),
        PredKind::Positive => "Positive".to_string(),
        PredKind::NonEmpty => "NonEmpty".to_string(),
    }
}

fn escape_string(s: &str) -> String {
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

fn expr_to_string(e: &Expr) -> String {
    expr_with_prec(e, 0)
}

// Operator precedences (smaller = binds looser):
//   1: || 2: && 3: == != 4: < <= > >= 5: + - 6: * / 7: unary ! - 8: postfix . () ?
fn binop_prec(op: BinOp) -> u8 {
    match op {
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
        ExprKind::IntLit(n) => n.to_string(),
        ExprKind::StrLit(s) => format!("\"{}\"", escape_string(s)),
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
            format!(
                "if {} {} else {}",
                expr_with_prec(cond, 0),
                format_block_oneline(then_block),
                format_block_oneline(else_block),
            )
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
            args,
        } => {
            let parts: Vec<String> = args.iter().map(|a| expr_with_prec(a, 0)).collect();
            format!(
                "{}.{}({})",
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
        ExprKind::Assert(v) => format!("assert {}", expr_with_prec(v, 0)),
        ExprKind::Mock { type_ref, args } => {
            let t = type_ref_to_string(type_ref);
            if args.is_empty() {
                format!("Mock[{t}]")
            } else {
                let a = args
                    .iter()
                    .map(|x| expr_with_prec(x, 0))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Mock[{t}]({a})")
            }
        }
    }
}

fn pattern_to_string(p: &Pattern) -> String {
    match p {
        Pattern::Wildcard(_) => "_".to_string(),
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
                let parts: Vec<String> = bindings
                    .iter()
                    .map(|b| match &b.kind {
                        PatternBindingKind::Positional { name } => name.name.clone(),
                        PatternBindingKind::Named { field, name } => {
                            format!("{}: {}", field.name, name.name)
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
        format!("{{ {} }}", expr_with_prec(&b.tail, 0))
    } else {
        // Multi-line block — render with newlines and tab indentation.
        let mut out = String::from("{\n");
        for stmt in &b.statements {
            out.push('\t');
            out.push_str(&stmt_to_string(stmt));
            out.push('\n');
        }
        // Omit the implicit `()` tail after a trailing `assert` (see
        // `format_block`) — printing it breaks round-trip idempotency.
        let implicit_unit_after_assert = matches!(b.tail.kind, ExprKind::UnitLit)
            && matches!(b.statements.last(), Some(Statement::Assert(_)));
        if !implicit_unit_after_assert {
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
            out
        }
        Statement::Commit(c) => format!("commit {}", expr_with_prec(&c.value, 0)),
        Statement::Assert(a) => format!("assert {}", expr_with_prec(&a.value, 0)),
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

    #[test]
    fn unchanged_files_without_comments_format_identically() {
        let src = "commons x { type T = Int where NonNegative }";
        let out = fmt(src);
        // Sanity: the formatter still produces the canonical output for
        // existing fixtures (no spurious comment rendering).
        assert!(!out.contains("--"), "unexpected comment in output: {out}");
    }
}
