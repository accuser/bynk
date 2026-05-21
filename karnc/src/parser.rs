//! Hand-written recursive-descent parser for Karn v0.
//!
//! Token grammar in spec §4. The expression parser uses one function per
//! precedence level (§4.4). Errors carry spans and short fix-oriented
//! messages; the parser does not currently attempt synchronisation, which
//! means at most one parse error is reported per compilation.

use crate::ast::*;
use crate::error::CompileError;
use crate::lexer::{Token, TokenKind, comment_body, doc_block_content, has_blank_line_between};
use crate::span::Span;

/// Side-channel store for line-comment trivia (v1.1 LSP spec §3.5).
///
/// Built once up-front by [`split_trivia`] from the raw lexer token stream.
/// Comments are removed from the token stream the parser walks; their text
/// is filed into `leading` (comments on lines preceding a content token)
/// and `trailing` (a single comment on the same line as a content token).
/// The parser consumes entries through [`TriviaTable::take_leading`] and
/// [`TriviaTable::take_trailing`] as it recognises declarations.
#[derive(Debug, Default)]
struct TriviaTable {
    /// `leading[i]` holds the comment-body texts that appear immediately
    /// before content token `i` (zero or more `--` lines, in source order,
    /// not separated from the token by another content token).
    leading: Vec<Vec<String>>,
    /// `trailing[i]` holds an optional comment on the same source line as
    /// content token `i`. Only one trailing comment is recorded per token
    /// because a single `--` consumes the rest of the line.
    trailing: Vec<Option<String>>,
    /// Any pending leading comments at end-of-file (no content token
    /// followed). Used to preserve file-trailing comments.
    epilogue: Vec<String>,
}

impl TriviaTable {
    fn take_leading(&mut self, index: usize) -> Vec<String> {
        match self.leading.get_mut(index) {
            Some(v) => std::mem::take(v),
            None => Vec::new(),
        }
    }

    fn take_trailing(&mut self, index: usize) -> Option<String> {
        self.trailing.get_mut(index).and_then(|s| s.take())
    }

    fn take_epilogue(&mut self) -> Vec<String> {
        std::mem::take(&mut self.epilogue)
    }
}

/// Remove `Comment` trivia tokens from `tokens` and bin them into a
/// [`TriviaTable`] keyed against the surviving content tokens. A comment
/// on the same source line as the preceding content token is recorded as
/// that token's *trailing* trivia; everything else is *leading* for the
/// next content token.
fn split_trivia(tokens: &[Token], source: &str) -> (Vec<Token>, TriviaTable) {
    let mut filtered: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut table = TriviaTable::default();
    let mut pending_leading: Vec<String> = Vec::new();
    let mut last_content_end: Option<usize> = None;
    for tok in tokens {
        if tok.kind == TokenKind::Comment {
            let body = comment_body(source, tok.span).to_string();
            // If nothing has been buffered as leading for the next token and
            // there is no newline between the previous content token and
            // this comment, it trails that token.
            if pending_leading.is_empty()
                && let Some(prev_end) = last_content_end
                && !source[prev_end..tok.span.start].contains('\n')
            {
                let last_idx = filtered.len() - 1;
                // Only attach if no trailing already recorded (shouldn't
                // happen because `--` consumes through end-of-line).
                if table.trailing[last_idx].is_none() {
                    table.trailing[last_idx] = Some(body);
                    continue;
                }
            }
            pending_leading.push(body);
            continue;
        }
        filtered.push(*tok);
        table.leading.push(std::mem::take(&mut pending_leading));
        table.trailing.push(None);
        last_content_end = Some(tok.span.end);
    }
    table.epilogue = pending_leading;
    (filtered, table)
}

/// Parse a token slice into a [`Commons`] AST.
///
/// Accepts either form of v0.3 commons file:
/// - Brace form: `commons name { items... }` (v0–v0.2 compatible).
/// - Fragment form: `commons name uses... items...` to EOF (v0.3).
pub fn parse(tokens: &[Token], source: &str) -> Result<Commons, Vec<CompileError>> {
    match parse_unit(tokens, source)? {
        SourceUnit::Commons(c) => Ok(c),
        SourceUnit::Context(ctx) => Err(vec![
            CompileError::new(
                "karn.parse.unexpected_context",
                ctx.span,
                "expected a `commons` declaration but found a `context` declaration",
            )
            .with_note(
                "single-file compilation does not support contexts; use `compile_project` instead",
            ),
        ]),
        SourceUnit::Test(t) => Err(vec![
            CompileError::new(
                "karn.parse.unexpected_test",
                t.span,
                "expected a `commons` declaration but found a `test` declaration",
            )
            .with_note(
                "single-file compilation does not support tests; use `compile_project` instead",
            ),
        ]),
    }
}

/// Parse a token slice into a [`SourceUnit`] with error recovery, returning a
/// best-effort partial AST plus the full list of parse errors and warnings.
///
/// Used by the LSP: item-level recovery skips past a malformed declaration to
/// the next top-level item, so multiple errors are reported per compilation
/// rather than just the first. Compared to [`parse_unit`], this never bails;
/// if no SourceUnit could be parsed at all (e.g. the file is empty or the
/// header itself fails) the returned `Option` is `None`.
pub fn parse_unit_with_recovery(
    tokens: &[Token],
    source: &str,
) -> (Option<SourceUnit>, Vec<CompileError>) {
    let (filtered, trivia) = split_trivia(tokens, source);
    let mut warnings = Vec::new();
    let mut p = Parser::new(&filtered, source, trivia, &mut warnings);
    p.recover_mode = true;
    let unit_opt = match p.parse_unit() {
        Ok(u) => {
            if let Some(extra) = p.peek() {
                p.recovered_errors.push(
                    CompileError::new(
                        "karn.parse.extra_tokens",
                        extra.span,
                        "unexpected token after top-level declaration",
                    )
                    .with_note(
                        "a `.karn` file contains exactly one `commons` or `context` declaration",
                    ),
                );
            }
            Some(u)
        }
        Err(e) => {
            p.recovered_errors.push(e);
            None
        }
    };
    let mut all_errors = p.recovered_errors;
    all_errors.append(&mut warnings);
    (unit_opt, all_errors)
}

/// Parse a token slice into a [`SourceUnit`] — either a commons or a context.
///
/// Each `.karn` file is exactly one declaration of one kind.
pub fn parse_unit(tokens: &[Token], source: &str) -> Result<SourceUnit, Vec<CompileError>> {
    let (filtered, trivia) = split_trivia(tokens, source);
    let mut warnings = Vec::new();
    let mut p = Parser::new(&filtered, source, trivia, &mut warnings);
    let result = match p.parse_unit() {
        Ok(u) => {
            if let Some(extra) = p.peek() {
                Err(vec![
                    CompileError::new(
                        "karn.parse.extra_tokens",
                        extra.span,
                        "unexpected token after top-level declaration",
                    )
                    .with_note(
                        "a `.karn` file contains exactly one `commons` or `context` declaration",
                    ),
                ])
            } else {
                Ok(u)
            }
        }
        Err(e) => Err(vec![e]),
    };
    // Warnings (e.g. orphan doc blocks) are returned as errors in v0.3 — there
    // is no separate warning channel yet; the test harness matches on category.
    if !warnings.is_empty() {
        match result {
            Ok(_) => return Err(warnings),
            Err(mut errs) => {
                errs.append(&mut warnings);
                return Err(errs);
            }
        }
    }
    result
}

struct Parser<'a> {
    tokens: &'a [Token],
    source: &'a str,
    pos: usize,
    /// Accumulated non-fatal diagnostics. v0.3 uses this for orphan-doc
    /// warnings, which are emitted as errors with a distinguishable category.
    warnings: &'a mut Vec<CompileError>,
    /// When true, the item-level loops catch errors from individual item
    /// parses, push them into `recovered_errors`, and skip forward to the
    /// next top-level item boundary instead of bailing. Used by the LSP via
    /// [`parse_unit_with_recovery`]; disabled in the normal `parse` path so
    /// existing single-error behaviour is preserved.
    recover_mode: bool,
    /// Errors collected during recovery-mode parsing. Only populated when
    /// `recover_mode` is true.
    recovered_errors: Vec<CompileError>,
    /// Line-comment trivia separated from the token stream. See
    /// [`TriviaTable`].
    trivia: TriviaTable,
}

impl<'a> Parser<'a> {
    fn new(
        tokens: &'a [Token],
        source: &'a str,
        trivia: TriviaTable,
        warnings: &'a mut Vec<CompileError>,
    ) -> Self {
        Self {
            tokens,
            source,
            pos: 0,
            warnings,
            recover_mode: false,
            recovered_errors: Vec::new(),
            trivia,
        }
    }

    /// Comments immediately preceding the current peek position. Consumed
    /// (the table entry is cleared) so the same comments are not attached
    /// to two nodes.
    fn take_leading_trivia(&mut self) -> Vec<String> {
        self.trivia.take_leading(self.pos)
    }

    /// Trailing comment, if any, on the same source line as the most
    /// recently consumed content token. Call AFTER finishing a declaration
    /// or statement, while `self.pos` points one past its last token.
    fn take_trailing_trivia(&mut self) -> Option<String> {
        if self.pos == 0 {
            return None;
        }
        self.trivia.take_trailing(self.pos - 1)
    }

    /// Handle a per-item parse error. In recovery mode, record the error and
    /// advance to the next sync point so the item loop can continue; otherwise
    /// propagate as a hard failure.
    fn handle_item_err(&mut self, e: CompileError) -> Result<(), CompileError> {
        if self.recover_mode {
            self.recovered_errors.push(e);
            self.recover_to_top_item();
            Ok(())
        } else {
            Err(e)
        }
    }

    /// Skip forward to the next top-level item boundary: either a top-level
    /// declaration keyword (`type`, `fn`, `uses`, `consumes`, `exports`,
    /// `capability`, `provides`, `service`, `agent`), a closing brace, or
    /// end-of-input. Used only in recovery mode.
    fn recover_to_top_item(&mut self) {
        while let Some(t) = self.peek() {
            match t.kind {
                TokenKind::Type
                | TokenKind::Fn
                | TokenKind::Uses
                | TokenKind::Consumes
                | TokenKind::Exports
                | TokenKind::Capability
                | TokenKind::Provides
                | TokenKind::Service
                | TokenKind::Agent
                | TokenKind::Mocks
                | TokenKind::Test
                | TokenKind::RBrace
                | TokenKind::Commons
                | TokenKind::Context => return,
                _ => {
                    self.bump();
                }
            }
        }
    }

    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.pos).copied()
    }

    fn peek_kind(&self) -> Option<TokenKind> {
        self.peek().map(|t| t.kind)
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.peek();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, kind: TokenKind) -> Option<Token> {
        if self.peek_kind() == Some(kind) {
            self.bump()
        } else {
            None
        }
    }

    fn slice(&self, span: Span) -> &'a str {
        &self.source[span.range()]
    }

    /// Span pointing at the end of input — used for "unexpected EOF" reports.
    fn eof_span(&self) -> Span {
        let end = self.source.len();
        Span::new(end.saturating_sub(1), end)
    }

    fn expect(&mut self, kind: TokenKind, ctx: &str) -> Result<Token, CompileError> {
        match self.peek() {
            Some(t) if t.kind == kind => {
                self.bump();
                Ok(t)
            }
            Some(t) => Err(CompileError::new(
                "karn.parse.expected_token",
                t.span,
                format!(
                    "expected {} {ctx}, found {}",
                    kind.describe(),
                    t.kind.describe()
                ),
            )),
            None => Err(CompileError::new(
                "karn.parse.unexpected_eof",
                self.eof_span(),
                format!("expected {} {ctx}, found end of file", kind.describe()),
            )),
        }
    }

    fn expect_ident(&mut self, ctx: &str) -> Result<Ident, CompileError> {
        match self.peek() {
            Some(t) if t.kind == TokenKind::Ident => {
                self.bump();
                Ok(Ident {
                    name: self.slice(t.span).to_string(),
                    span: t.span,
                })
            }
            // v0.5 contextual keywords (`state`, `on`) double as identifiers
            // in expression / field-access positions so users can name fields
            // and parameters using them. They retain their keyword meaning
            // only at agent-decl-level (`state { ... }`) and handler-decl-level
            // (`on call(...)`).
            //
            // v0.7: `test` is contextual too — it introduces the test
            // declaration kind at the file top level, but is a perfectly
            // valid commons or context name otherwise.
            Some(t) if matches!(t.kind, TokenKind::State | TokenKind::On | TokenKind::Test) => {
                self.bump();
                Ok(Ident {
                    name: self.slice(t.span).to_string(),
                    span: t.span,
                })
            }
            Some(t) if is_reserved_keyword(t.kind) => Err(CompileError::new(
                "karn.parse.reserved_keyword",
                t.span,
                format!(
                    "expected identifier {ctx}, but `{}` is a reserved keyword",
                    self.slice(t.span)
                ),
            )
            .with_note("rename the identifier to something that is not a keyword")),
            Some(t) => Err(CompileError::new(
                "karn.parse.expected_token",
                t.span,
                format!("expected identifier {ctx}, found {}", t.kind.describe()),
            )),
            None => Err(CompileError::new(
                "karn.parse.unexpected_eof",
                self.eof_span(),
                format!("expected identifier {ctx}, found end of file"),
            )),
        }
    }

    // -- top level --

    /// Consume an optional doc block at the current position, returning the
    /// (content, end-of-doc span) pair. Returns None if the next token is not
    /// a doc block.
    fn take_doc_block(&mut self) -> Option<(String, Span)> {
        if self.peek_kind() == Some(TokenKind::DocBlock) {
            let t = self.bump().unwrap();
            let body = doc_block_content(self.source, t.span);
            return Some((body, t.span));
        }
        None
    }

    /// Collect all line-comment trivia leading the next declaration plus
    /// the optional doc block. Comments may appear both *before* and
    /// *between* the doc and the declaration; the spec canonicalises both
    /// groups above the doc, so we concatenate them.
    fn collect_item_lead(&mut self) -> (Vec<String>, Option<(String, Span)>) {
        let mut leading = self.take_leading_trivia();
        let doc = self.take_doc_block();
        if doc.is_some() {
            leading.extend(self.take_leading_trivia());
        }
        (leading, doc)
    }

    /// Attach a parsed doc block to a following declaration unless a blank
    /// line separates them, in which case the doc is orphaned (warning).
    fn finalize_doc(&mut self, doc: Option<(String, Span)>, next_span: Span) -> Option<String> {
        let (content, doc_span) = doc?;
        // A blank line between the doc and the next decl orphans the doc.
        if has_blank_line_between(self.source, doc_span.end, next_span.start) {
            self.warnings.push(
                CompileError::new(
                    "karn.parse.orphan_doc_block",
                    doc_span,
                    "documentation block is separated from the following declaration by a blank line; it will not be attached",
                )
                .with_note(
                    "remove the blank line to attach the doc to the next declaration, \
                     or remove the doc block if it is not meant to document anything",
                ),
            );
            return None;
        }
        Some(content)
    }

    fn parse_unit(&mut self) -> Result<SourceUnit, CompileError> {
        // Optional doc block describing the declaration itself, plus any
        // line comments that lead the file.
        let (header_leading, leading_doc) = self.collect_item_lead();
        let header_trivia = Trivia {
            leading: header_leading,
            trailing: None,
        };
        match self.peek_kind() {
            Some(TokenKind::Commons) => {
                let start = self.expect(TokenKind::Commons, "to start the commons declaration")?;
                let doc = self.finalize_doc(leading_doc, start.span);
                let name = self.parse_qualified_name()?;
                let mut c = match self.peek_kind() {
                    Some(TokenKind::LBrace) => self.parse_commons_brace(start.span, name, doc)?,
                    _ => self.parse_commons_fragment(start.span, name, doc)?,
                };
                c.trivia = header_trivia;
                Ok(SourceUnit::Commons(c))
            }
            Some(TokenKind::Context) => {
                let start = self.expect(TokenKind::Context, "to start the context declaration")?;
                let doc = self.finalize_doc(leading_doc, start.span);
                let name = self.parse_qualified_name()?;
                let mut c = match self.peek_kind() {
                    Some(TokenKind::LBrace) => self.parse_context_brace(start.span, name, doc)?,
                    _ => self.parse_context_fragment(start.span, name, doc)?,
                };
                c.trivia = header_trivia;
                Ok(SourceUnit::Context(c))
            }
            Some(TokenKind::Test) => {
                let start = self.expect(TokenKind::Test, "to start the test declaration")?;
                let doc = self.finalize_doc(leading_doc, start.span);
                let name = self.parse_qualified_name()?;
                let mut t = match self.peek_kind() {
                    Some(TokenKind::LBrace) => self.parse_test_brace(start.span, name, doc)?,
                    _ => self.parse_test_fragment(start.span, name, doc)?,
                };
                t.trivia = header_trivia;
                Ok(SourceUnit::Test(t))
            }
            Some(_) => {
                let t = self.peek().unwrap();
                if let Some((_, doc_span)) = leading_doc {
                    self.warnings.push(CompileError::new(
                        "karn.parse.orphan_doc_block",
                        doc_span,
                        "documentation block has no following declaration to attach to",
                    ));
                }
                Err(CompileError::new(
                    "karn.parse.expected_unit_header",
                    t.span,
                    format!(
                        "expected `commons`, `context`, or `test` to start the file, found {}",
                        t.kind.describe()
                    ),
                )
                .with_note(
                    "every `.karn` file begins with either a `commons`, `context`, or `test` declaration",
                ))
            }
            None => {
                if let Some((_, doc_span)) = leading_doc {
                    self.warnings.push(CompileError::new(
                        "karn.parse.orphan_doc_block",
                        doc_span,
                        "documentation block has no following declaration to attach to",
                    ));
                }
                Err(CompileError::new(
                    "karn.parse.unexpected_eof",
                    self.eof_span(),
                    "expected `commons`, `context`, or `test` to start the file, found end of file",
                ))
            }
        }
    }

    fn parse_commons_brace(
        &mut self,
        start: Span,
        name: QualifiedName,
        documentation: Option<String>,
    ) -> Result<Commons, CompileError> {
        self.expect(TokenKind::LBrace, "after the commons name")?;
        let mut items = Vec::new();
        let mut uses = Vec::new();
        let trailing_comments: Vec<String>;
        loop {
            // Optional doc block and leading line comments before the next item.
            let (mut leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::RBrace) => {
                    // Doc not attachable; treat as orphan if present. Any
                    // leading comments at this position end up as the
                    // body's trailing comments.
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block has no following declaration to attach to",
                        ));
                    }
                    trailing_comments = std::mem::take(&mut leading);
                    break;
                }
                Some(TokenKind::Uses) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(
                            CompileError::new(
                                "karn.parse.orphan_doc_block",
                                doc_span,
                                "documentation block before `uses` is not allowed; only `type` and `fn` declarations carry docs",
                            ),
                        );
                    }
                    match self.parse_uses_decl() {
                        Ok(mut u) => {
                            u.trivia.leading = leading;
                            u.trivia.trailing = self.take_trailing_trivia();
                            uses.push(u);
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Type) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_type_decl() {
                        Ok(mut t) => {
                            t.documentation = doc;
                            t.trivia.leading = leading;
                            t.trivia.trailing = self.take_trailing_trivia();
                            items.push(CommonsItem::Type(t));
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Fn) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_fn_decl() {
                        Ok(mut f) => {
                            f.documentation = doc;
                            f.trivia.leading = leading;
                            f.trivia.trailing = self.take_trailing_trivia();
                            items.push(CommonsItem::Fn(f));
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Capability) => {
                    let err = CompileError::new(
                        "karn.capability.outside_context",
                        self.peek().unwrap().span,
                        "`capability` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(TokenKind::Provides) => {
                    let err = CompileError::new(
                        "karn.provider.outside_context",
                        self.peek().unwrap().span,
                        "`provides` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(TokenKind::Service) => {
                    let err = CompileError::new(
                        "karn.service.outside_context",
                        self.peek().unwrap().span,
                        "`service` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(TokenKind::Agent) => {
                    let err = CompileError::new(
                        "karn.agent.outside_context",
                        self.peek().unwrap().span,
                        "`agent` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(_) => {
                    let t = self.peek().unwrap();
                    let err = CompileError::new(
                        "karn.parse.expected_item",
                        t.span,
                        format!(
                            "expected `type`, `fn`, or `uses` declaration, found {}",
                            t.kind.describe()
                        ),
                    )
                    .with_note(
                        "the body of a commons contains zero or more `type`, `fn`, or `uses` declarations",
                    );
                    if self.recover_mode {
                        self.recovered_errors.push(err);
                        self.bump();
                        self.recover_to_top_item();
                    } else {
                        return Err(err);
                    }
                }
                None => {
                    return Err(CompileError::new(
                        "karn.parse.unexpected_eof",
                        self.eof_span(),
                        "expected `}` to close the commons body, found end of file",
                    ));
                }
            }
        }
        let end = self.expect(TokenKind::RBrace, "to close the commons body")?;
        Ok(Commons {
            name,
            items,
            uses,
            documentation,
            form: CommonsForm::Brace,
            span: start.merge(end.span),
            trivia: Trivia::default(),
            trailing_comments,
        })
    }

    fn parse_commons_fragment(
        &mut self,
        start: Span,
        name: QualifiedName,
        documentation: Option<String>,
    ) -> Result<Commons, CompileError> {
        let mut items = Vec::new();
        let mut uses = Vec::new();
        let mut last_span = start;
        let mut seen_item = false;
        let trailing_comments: Vec<String>;
        loop {
            let (mut leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::Uses) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(
                            CompileError::new(
                                "karn.parse.orphan_doc_block",
                                doc_span,
                                "documentation block before `uses` is not allowed; only `type` and `fn` declarations carry docs",
                            ),
                        );
                    }
                    if seen_item {
                        let t = self.peek().unwrap();
                        return Err(CompileError::new(
                            "karn.parse.uses_after_decls",
                            t.span,
                            "`uses` clauses must appear before any `type` or `fn` declaration in a fragment-form commons",
                        )
                        .with_note(
                            "move all `uses` lines to immediately after the `commons` header",
                        ));
                    }
                    match self.parse_uses_decl() {
                        Ok(mut u) => {
                            u.trivia.leading = leading;
                            u.trivia.trailing = self.take_trailing_trivia();
                            last_span = u.span;
                            uses.push(u);
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Type) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_type_decl() {
                        Ok(mut t) => {
                            t.documentation = doc;
                            t.trivia.leading = leading;
                            t.trivia.trailing = self.take_trailing_trivia();
                            last_span = t.span;
                            items.push(CommonsItem::Type(t));
                            seen_item = true;
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Fn) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_fn_decl() {
                        Ok(mut f) => {
                            f.documentation = doc;
                            f.trivia.leading = leading;
                            f.trivia.trailing = self.take_trailing_trivia();
                            last_span = f.span;
                            items.push(CommonsItem::Fn(f));
                            seen_item = true;
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                None => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block has no following declaration to attach to",
                        ));
                    }
                    // Comments we held as leading for the next item, plus
                    // any held in the trivia table's epilogue, become the
                    // commons body's trailing comments.
                    leading.extend(self.trivia.take_epilogue());
                    trailing_comments = leading;
                    break;
                }
                Some(TokenKind::Capability) => {
                    let err = CompileError::new(
                        "karn.capability.outside_context",
                        self.peek().unwrap().span,
                        "`capability` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(TokenKind::Provides) => {
                    let err = CompileError::new(
                        "karn.provider.outside_context",
                        self.peek().unwrap().span,
                        "`provides` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(TokenKind::Service) => {
                    let err = CompileError::new(
                        "karn.service.outside_context",
                        self.peek().unwrap().span,
                        "`service` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(TokenKind::Agent) => {
                    let err = CompileError::new(
                        "karn.agent.outside_context",
                        self.peek().unwrap().span,
                        "`agent` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(_) => {
                    let t = self.peek().unwrap();
                    let err = CompileError::new(
                        "karn.parse.expected_item",
                        t.span,
                        format!(
                            "expected `type`, `fn`, or `uses` declaration, found {}",
                            t.kind.describe()
                        ),
                    )
                    .with_note(
                        "in fragment-form commons (no braces), the body is a sequence of `type`, `fn`, or `uses` declarations to end of file",
                    );
                    if self.recover_mode {
                        self.recovered_errors.push(err);
                        // Force progress in recovery: bump at least one token, then sync.
                        self.bump();
                        self.recover_to_top_item();
                    } else {
                        return Err(err);
                    }
                }
            }
        }
        Ok(Commons {
            name,
            items,
            uses,
            documentation,
            form: CommonsForm::Fragment,
            span: start.merge(last_span),
            trivia: Trivia::default(),
            trailing_comments,
        })
    }

    fn parse_uses_decl(&mut self) -> Result<UsesDecl, CompileError> {
        let kw = self.expect(TokenKind::Uses, "to start a `uses` declaration")?;
        let target = self.parse_qualified_name()?;
        let span = kw.span.merge(target.span);
        Ok(UsesDecl {
            target,
            span,
            trivia: Trivia::default(),
        })
    }

    fn parse_consumes_decl(&mut self) -> Result<ConsumesDecl, CompileError> {
        let kw = self.expect(TokenKind::Consumes, "to start a `consumes` declaration")?;
        let target = self.parse_qualified_name()?;
        let mut span = kw.span.merge(target.span);
        let alias = if self.peek_kind() == Some(TokenKind::As) {
            self.bump();
            let id = self.expect_ident("as an alias for the consumed context")?;
            span = span.merge(id.span);
            Some(id)
        } else {
            None
        };
        Ok(ConsumesDecl {
            target,
            alias,
            span,
            trivia: Trivia::default(),
        })
    }

    fn parse_exports_decl(&mut self) -> Result<ExportsDecl, CompileError> {
        let kw = self.expect(TokenKind::Exports, "to start an `exports` declaration")?;
        let visibility = match self.peek_kind() {
            Some(TokenKind::Opaque) => {
                self.bump();
                Visibility::Opaque
            }
            Some(TokenKind::Transparent) => {
                self.bump();
                Visibility::Transparent
            }
            Some(_) => {
                let t = self.peek().unwrap();
                return Err(CompileError::new(
                    "karn.parse.expected_visibility",
                    t.span,
                    format!(
                        "expected `opaque` or `transparent` after `exports`, found {}",
                        t.kind.describe()
                    ),
                )
                .with_note(
                    "exports clauses are `exports opaque { ... }` or `exports transparent { ... }`",
                ));
            }
            None => {
                return Err(CompileError::new(
                    "karn.parse.unexpected_eof",
                    self.eof_span(),
                    "expected `opaque` or `transparent` after `exports`, found end of file",
                ));
            }
        };
        self.expect(TokenKind::LBrace, "to open the exports list")?;
        let mut names = Vec::new();
        while self.peek_kind() != Some(TokenKind::RBrace) {
            names.push(self.expect_ident("as an exported type name")?);
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let close = self.expect(TokenKind::RBrace, "to close the exports list")?;
        let span = kw.span.merge(close.span);
        Ok(ExportsDecl {
            visibility,
            names,
            span,
            trivia: Trivia::default(),
        })
    }

    fn parse_test_brace(
        &mut self,
        start: Span,
        target: QualifiedName,
        documentation: Option<String>,
    ) -> Result<TestDecl, CompileError> {
        self.expect(TokenKind::LBrace, "after the test target name")?;
        let mut uses = Vec::new();
        let mut mocks = Vec::new();
        let mut cases = Vec::new();
        let trailing_comments: Vec<String>;
        loop {
            let (mut leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::RBrace) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block has no following declaration to attach to",
                        ));
                    }
                    trailing_comments = std::mem::take(&mut leading);
                    break;
                }
                Some(TokenKind::Uses) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block before `uses` is not allowed",
                        ));
                    }
                    match self.parse_uses_decl() {
                        Ok(mut u) => {
                            u.trivia.leading = leading;
                            u.trivia.trailing = self.take_trailing_trivia();
                            uses.push(u);
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Mocks) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_mock_decl() {
                        Ok(mut m) => {
                            m.documentation = doc;
                            m.trivia.leading = leading;
                            m.trivia.trailing = self.take_trailing_trivia();
                            mocks.push(m);
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Test) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_test_case() {
                        Ok(mut c) => {
                            c.documentation = doc;
                            c.trivia.leading = leading;
                            c.trivia.trailing = self.take_trailing_trivia();
                            cases.push(c);
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(_) => {
                    let t = self.peek().unwrap();
                    let err = CompileError::new(
                        "karn.parse.expected_item",
                        t.span,
                        format!(
                            "expected `uses`, `mocks`, or `test \"name\"` declaration, found {}",
                            t.kind.describe()
                        ),
                    )
                    .with_note(
                        "the body of a test contains zero or more `uses`, `mocks`, or `test \"name\"` declarations",
                    );
                    if self.recover_mode {
                        self.recovered_errors.push(err);
                        self.bump();
                        self.recover_to_top_item();
                    } else {
                        return Err(err);
                    }
                }
                None => {
                    return Err(CompileError::new(
                        "karn.parse.unexpected_eof",
                        self.eof_span(),
                        "expected `}` to close the test body, found end of file",
                    ));
                }
            }
        }
        let end = self.expect(TokenKind::RBrace, "to close the test body")?;
        Ok(TestDecl {
            target,
            uses,
            mocks,
            cases,
            form: CommonsForm::Brace,
            documentation,
            span: start.merge(end.span),
            trivia: Trivia::default(),
            trailing_comments,
        })
    }

    fn parse_test_fragment(
        &mut self,
        start: Span,
        target: QualifiedName,
        documentation: Option<String>,
    ) -> Result<TestDecl, CompileError> {
        let mut uses = Vec::new();
        let mut mocks = Vec::new();
        let mut cases = Vec::new();
        let mut last_span = start;
        let mut seen_non_uses = false;
        let trailing_comments: Vec<String>;
        loop {
            let (mut leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::Uses) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block before `uses` is not allowed",
                        ));
                    }
                    if seen_non_uses {
                        let t = self.peek().unwrap();
                        return Err(CompileError::new(
                            "karn.parse.uses_after_decls",
                            t.span,
                            "`uses` clauses must appear before any `mocks` or `test` declarations in a fragment-form test",
                        ));
                    }
                    match self.parse_uses_decl() {
                        Ok(mut u) => {
                            u.trivia.leading = leading;
                            u.trivia.trailing = self.take_trailing_trivia();
                            last_span = u.span;
                            uses.push(u);
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Mocks) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_mock_decl() {
                        Ok(mut m) => {
                            m.documentation = doc;
                            m.trivia.leading = leading;
                            m.trivia.trailing = self.take_trailing_trivia();
                            last_span = m.span;
                            mocks.push(m);
                            seen_non_uses = true;
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Test) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_test_case() {
                        Ok(mut c) => {
                            c.documentation = doc;
                            c.trivia.leading = leading;
                            c.trivia.trailing = self.take_trailing_trivia();
                            last_span = c.span;
                            cases.push(c);
                            seen_non_uses = true;
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                None => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block has no following declaration to attach to",
                        ));
                    }
                    leading.extend(self.trivia.take_epilogue());
                    trailing_comments = leading;
                    break;
                }
                Some(_) => {
                    let t = self.peek().unwrap();
                    let err = CompileError::new(
                        "karn.parse.expected_item",
                        t.span,
                        format!(
                            "expected `uses`, `mocks`, or `test \"name\"` declaration, found {}",
                            t.kind.describe()
                        ),
                    )
                    .with_note(
                        "in fragment-form tests, the body is a sequence of `uses`, `mocks`, or `test \"name\"` declarations to end of file",
                    );
                    if self.recover_mode {
                        self.recovered_errors.push(err);
                        self.bump();
                        self.recover_to_top_item();
                    } else {
                        return Err(err);
                    }
                }
            }
        }
        Ok(TestDecl {
            target,
            uses,
            mocks,
            cases,
            form: CommonsForm::Fragment,
            documentation,
            span: start.merge(last_span),
            trivia: Trivia::default(),
            trailing_comments,
        })
    }

    fn parse_mock_decl(&mut self) -> Result<MockDecl, CompileError> {
        let kw = self.expect(TokenKind::Mocks, "to start a mocks declaration")?;
        let target_name = self.expect_ident("after `mocks`")?;
        self.expect(TokenKind::Eq, "after the mock target name")?;
        let impl_name = self.expect_ident("after `=` in a mocks declaration")?;
        self.expect(TokenKind::LBrace, "to open the mock body")?;
        let mut ops = Vec::new();
        while self.peek_kind() != Some(TokenKind::RBrace) {
            let (leading, item_doc) = self.collect_item_lead();
            if let Some((_, doc_span)) = item_doc {
                self.warnings.push(CompileError::new(
                    "karn.parse.orphan_doc_block",
                    doc_span,
                    "documentation blocks on mock operations are not supported",
                ));
            }
            if self.peek_kind() == Some(TokenKind::RBrace) {
                // Allow trailing leading comments to be silently dropped here.
                let _ = leading;
                break;
            }
            let mut op = self.parse_mock_op()?;
            op.trivia.leading = leading;
            op.trivia.trailing = self.take_trailing_trivia();
            ops.push(op);
        }
        let end = self.expect(TokenKind::RBrace, "to close the mock body")?;
        if ops.is_empty() {
            return Err(CompileError::new(
                "karn.parse.empty_mock_body",
                kw.span.merge(end.span),
                "mocks declaration must contain at least one `fn` operation",
            ));
        }
        Ok(MockDecl {
            target_name,
            impl_name,
            ops,
            documentation: None,
            span: kw.span.merge(end.span),
            trivia: Trivia::default(),
        })
    }

    fn parse_mock_op(&mut self) -> Result<MockOp, CompileError> {
        let kw = self.expect(TokenKind::Fn, "to start a mock operation")?;
        let name = self.expect_ident("after `fn` in a mock operation")?;
        self.expect(TokenKind::LParen, "after the mock operation name")?;
        let mut params = Vec::new();
        if self.peek_kind() != Some(TokenKind::RParen) {
            params.push(self.parse_param()?);
            while self.eat(TokenKind::Comma).is_some() {
                params.push(self.parse_param()?);
            }
        }
        self.expect(
            TokenKind::RParen,
            "to close the mock operation parameter list",
        )?;
        self.expect(TokenKind::Arrow, "before the mock operation return type")?;
        let return_type = self.parse_type_ref("as the mock operation return type")?;
        let body = self.parse_block("to open the mock operation body")?;
        let span = kw.span.merge(body.span);
        Ok(MockOp {
            name,
            params,
            return_type,
            body,
            span,
            trivia: Trivia::default(),
        })
    }

    fn parse_test_case(&mut self) -> Result<TestCase, CompileError> {
        let kw = self.expect(TokenKind::Test, "to start a test case")?;
        let name_tok = self.expect(TokenKind::StrLit, "as the test case name")?;
        let name = parse_string_literal(self.slice(name_tok.span), name_tok.span)?;
        let body = self.parse_block("to open the test case body")?;
        let span = kw.span.merge(body.span);
        Ok(TestCase {
            name,
            name_span: name_tok.span,
            body,
            documentation: None,
            span,
            trivia: Trivia::default(),
        })
    }

    fn parse_context_brace(
        &mut self,
        start: Span,
        name: QualifiedName,
        documentation: Option<String>,
    ) -> Result<Context, CompileError> {
        self.expect(TokenKind::LBrace, "after the context name")?;
        let mut items = Vec::new();
        let mut uses = Vec::new();
        let mut consumes = Vec::new();
        let mut exports = Vec::new();
        let trailing_comments: Vec<String>;
        loop {
            let (mut leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::RBrace) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block has no following declaration to attach to",
                        ));
                    }
                    trailing_comments = std::mem::take(&mut leading);
                    break;
                }
                Some(TokenKind::Uses) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block before `uses` is not allowed; only `type` and `fn` declarations carry docs",
                        ));
                    }
                    match self.parse_uses_decl() {
                        Ok(mut u) => {
                            u.trivia.leading = leading;
                            u.trivia.trailing = self.take_trailing_trivia();
                            uses.push(u);
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Consumes) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block before `consumes` is not allowed; only `type` and `fn` declarations carry docs",
                        ));
                    }
                    match self.parse_consumes_decl() {
                        Ok(mut c) => {
                            c.trivia.leading = leading;
                            c.trivia.trailing = self.take_trailing_trivia();
                            consumes.push(c);
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Exports) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block before `exports` is not allowed; only `type` and `fn` declarations carry docs",
                        ));
                    }
                    match self.parse_exports_decl() {
                        Ok(mut e) => {
                            e.trivia.leading = leading;
                            e.trivia.trailing = self.take_trailing_trivia();
                            exports.push(e);
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Type) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_type_decl() {
                        Ok(mut t) => {
                            t.documentation = doc;
                            t.trivia.leading = leading;
                            t.trivia.trailing = self.take_trailing_trivia();
                            items.push(CommonsItem::Type(t));
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Fn) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_fn_decl() {
                        Ok(mut f) => {
                            f.documentation = doc;
                            f.trivia.leading = leading;
                            f.trivia.trailing = self.take_trailing_trivia();
                            items.push(CommonsItem::Fn(f));
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Capability) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_capability_decl() {
                        Ok(mut c) => {
                            c.documentation = doc;
                            c.trivia.leading = leading;
                            c.trivia.trailing = self.take_trailing_trivia();
                            items.push(CommonsItem::Capability(c));
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Provides) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_provider_decl() {
                        Ok(mut p) => {
                            p.documentation = doc;
                            p.trivia.leading = leading;
                            p.trivia.trailing = self.take_trailing_trivia();
                            items.push(CommonsItem::Provider(p));
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Service) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_service_decl() {
                        Ok(mut s) => {
                            s.documentation = doc;
                            s.trivia.leading = leading;
                            s.trivia.trailing = self.take_trailing_trivia();
                            items.push(CommonsItem::Service(s));
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Agent) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_agent_decl() {
                        Ok(mut a) => {
                            a.documentation = doc;
                            a.trivia.leading = leading;
                            a.trivia.trailing = self.take_trailing_trivia();
                            items.push(CommonsItem::Agent(a));
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(_) => {
                    let t = self.peek().unwrap();
                    let err = CompileError::new(
                        "karn.parse.expected_item",
                        t.span,
                        format!(
                            "expected a `type`, `fn`, `uses`, `consumes`, `exports`, `capability`, `provides`, `service`, or `agent` declaration, found {}",
                            t.kind.describe()
                        ),
                    );
                    if self.recover_mode {
                        self.recovered_errors.push(err);
                        self.bump();
                        self.recover_to_top_item();
                    } else {
                        return Err(err);
                    }
                }
                None => {
                    return Err(CompileError::new(
                        "karn.parse.unexpected_eof",
                        self.eof_span(),
                        "expected `}` to close the context body, found end of file",
                    ));
                }
            }
        }
        let end = self.expect(TokenKind::RBrace, "to close the context body")?;
        Ok(Context {
            name,
            items,
            uses,
            consumes,
            exports,
            documentation,
            form: CommonsForm::Brace,
            span: start.merge(end.span),
            trivia: Trivia::default(),
            trailing_comments,
        })
    }

    fn parse_context_fragment(
        &mut self,
        start: Span,
        name: QualifiedName,
        documentation: Option<String>,
    ) -> Result<Context, CompileError> {
        let mut items = Vec::new();
        let mut uses = Vec::new();
        let mut consumes = Vec::new();
        let mut exports = Vec::new();
        let mut last_span = start;
        let mut seen_item = false;
        let trailing_comments: Vec<String>;
        loop {
            let (mut leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::Uses) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block before `uses` is not allowed; only `type` and `fn` declarations carry docs",
                        ));
                    }
                    if seen_item {
                        let t = self.peek().unwrap();
                        return Err(CompileError::new(
                            "karn.parse.uses_after_decls",
                            t.span,
                            "`uses` clauses must appear before any `type` or `fn` declaration in a fragment-form context",
                        )
                        .with_note(
                            "move all `uses` lines to immediately after the `context` header",
                        ));
                    }
                    match self.parse_uses_decl() {
                        Ok(mut u) => {
                            u.trivia.leading = leading;
                            u.trivia.trailing = self.take_trailing_trivia();
                            last_span = u.span;
                            uses.push(u);
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Consumes) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block before `consumes` is not allowed; only `type` and `fn` declarations carry docs",
                        ));
                    }
                    if seen_item {
                        let t = self.peek().unwrap();
                        let err = CompileError::new(
                            "karn.parse.consumes_after_decls",
                            t.span,
                            "`consumes` clauses must appear before any `type` or `fn` declaration in a fragment-form context",
                        )
                        .with_note(
                            "move all `consumes` lines to immediately after the `uses` clauses",
                        );
                        if self.recover_mode {
                            self.recovered_errors.push(err);
                            self.bump();
                            self.recover_to_top_item();
                            continue;
                        } else {
                            return Err(err);
                        }
                    }
                    match self.parse_consumes_decl() {
                        Ok(mut c) => {
                            c.trivia.leading = leading;
                            c.trivia.trailing = self.take_trailing_trivia();
                            last_span = c.span;
                            consumes.push(c);
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Exports) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block before `exports` is not allowed; only `type` and `fn` declarations carry docs",
                        ));
                    }
                    if seen_item {
                        let t = self.peek().unwrap();
                        let err = CompileError::new(
                            "karn.parse.exports_after_decls",
                            t.span,
                            "`exports` clauses must appear before any `type` or `fn` declaration in a fragment-form context",
                        )
                        .with_note(
                            "move all `exports` lines to immediately after the `consumes` clauses",
                        );
                        if self.recover_mode {
                            self.recovered_errors.push(err);
                            self.bump();
                            self.recover_to_top_item();
                            continue;
                        } else {
                            return Err(err);
                        }
                    }
                    match self.parse_exports_decl() {
                        Ok(mut e) => {
                            e.trivia.leading = leading;
                            e.trivia.trailing = self.take_trailing_trivia();
                            last_span = e.span;
                            exports.push(e);
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Type) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_type_decl() {
                        Ok(mut t) => {
                            t.documentation = doc;
                            t.trivia.leading = leading;
                            t.trivia.trailing = self.take_trailing_trivia();
                            last_span = t.span;
                            items.push(CommonsItem::Type(t));
                            seen_item = true;
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Fn) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_fn_decl() {
                        Ok(mut f) => {
                            f.documentation = doc;
                            f.trivia.leading = leading;
                            f.trivia.trailing = self.take_trailing_trivia();
                            last_span = f.span;
                            items.push(CommonsItem::Fn(f));
                            seen_item = true;
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Capability) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_capability_decl() {
                        Ok(mut c) => {
                            c.documentation = doc;
                            c.trivia.leading = leading;
                            c.trivia.trailing = self.take_trailing_trivia();
                            last_span = c.span;
                            items.push(CommonsItem::Capability(c));
                            seen_item = true;
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Provides) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_provider_decl() {
                        Ok(mut p) => {
                            p.documentation = doc;
                            p.trivia.leading = leading;
                            p.trivia.trailing = self.take_trailing_trivia();
                            last_span = p.span;
                            items.push(CommonsItem::Provider(p));
                            seen_item = true;
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Service) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_service_decl() {
                        Ok(mut s) => {
                            s.documentation = doc;
                            s.trivia.leading = leading;
                            s.trivia.trailing = self.take_trailing_trivia();
                            last_span = s.span;
                            items.push(CommonsItem::Service(s));
                            seen_item = true;
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Agent) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_agent_decl() {
                        Ok(mut a) => {
                            a.documentation = doc;
                            a.trivia.leading = leading;
                            a.trivia.trailing = self.take_trailing_trivia();
                            last_span = a.span;
                            items.push(CommonsItem::Agent(a));
                            seen_item = true;
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                None => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block has no following declaration to attach to",
                        ));
                    }
                    leading.extend(self.trivia.take_epilogue());
                    trailing_comments = leading;
                    break;
                }
                Some(_) => {
                    let t = self.peek().unwrap();
                    let err = CompileError::new(
                        "karn.parse.expected_item",
                        t.span,
                        format!(
                            "expected a `type`, `fn`, `uses`, `consumes`, `exports`, `capability`, `provides`, `service`, or `agent` declaration, found {}",
                            t.kind.describe()
                        ),
                    );
                    if self.recover_mode {
                        self.recovered_errors.push(err);
                        self.bump();
                        self.recover_to_top_item();
                    } else {
                        return Err(err);
                    }
                }
            }
        }
        Ok(Context {
            name,
            items,
            uses,
            consumes,
            exports,
            documentation,
            form: CommonsForm::Fragment,
            span: start.merge(last_span),
            trivia: Trivia::default(),
            trailing_comments,
        })
    }

    fn parse_qualified_name(&mut self) -> Result<QualifiedName, CompileError> {
        let first = self.expect_ident("for the commons name")?;
        let mut parts = vec![first];
        let mut span = parts[0].span;
        while self.eat(TokenKind::Dot).is_some() {
            let part = self.expect_ident("after `.` in the commons name")?;
            span = span.merge(part.span);
            parts.push(part);
        }
        Ok(QualifiedName { parts, span })
    }

    // -- type declarations --

    fn parse_type_decl(&mut self) -> Result<TypeDecl, CompileError> {
        let kw = self.expect(TokenKind::Type, "to start a type declaration")?;
        let name = self.expect_ident("after `type`")?;
        self.expect(TokenKind::Eq, "after the type name")?;
        // Dispatch on the head token to decide which kind of type body to parse:
        //   `{ ... }`         → record body (v0.2)
        //   `|` ...           → pipe-form sum (v0.2)
        //   `enum { ... }`    → enum-form sum (v0.2)
        //   `opaque ...`      → opaque base type (v0.3)
        //   anything else     → refined base type (v0)
        let (body, end_span) = match self.peek_kind() {
            Some(TokenKind::LBrace) => {
                let r = self.parse_record_body()?;
                let span = r.span;
                (TypeBody::Record(r), span)
            }
            Some(TokenKind::Pipe) => {
                let s = self.parse_sum_body_pipe()?;
                let span = s.span;
                (TypeBody::Sum(s), span)
            }
            Some(TokenKind::Enum) => {
                let s = self.parse_sum_body_enum()?;
                let span = s.span;
                (TypeBody::Sum(s), span)
            }
            Some(TokenKind::Opaque) => {
                self.bump();
                let (base, base_span) = self.parse_base_type()?;
                let mut refinement = None;
                let mut end_span = base_span;
                if self.eat(TokenKind::Where).is_some() {
                    let r = self.parse_refinement()?;
                    end_span = r.span;
                    refinement = Some(r);
                }
                (
                    TypeBody::Opaque {
                        base,
                        base_span,
                        refinement,
                    },
                    end_span,
                )
            }
            _ => {
                let (base, base_span) = self.parse_base_type()?;
                let mut refinement = None;
                let mut end_span = base_span;
                if self.eat(TokenKind::Where).is_some() {
                    let r = self.parse_refinement()?;
                    end_span = r.span;
                    refinement = Some(r);
                }
                (
                    TypeBody::Refined {
                        base,
                        base_span,
                        refinement,
                    },
                    end_span,
                )
            }
        };
        Ok(TypeDecl {
            name,
            body,
            documentation: None,
            span: kw.span.merge(end_span),
            trivia: Trivia::default(),
        })
    }

    /// Parse the body of a record type: `{ field, field, ... }`.
    /// Each field is `name : type-ref (where refinement)?`; trailing
    /// comma after the last field is allowed.
    fn parse_record_body(&mut self) -> Result<RecordBody, CompileError> {
        let open = self.expect(TokenKind::LBrace, "to open the record body")?;
        let mut fields = Vec::new();
        while self.peek_kind() != Some(TokenKind::RBrace) {
            fields.push(self.parse_record_field()?);
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let close = self.expect(TokenKind::RBrace, "to close the record body")?;
        Ok(RecordBody {
            fields,
            span: open.span.merge(close.span),
        })
    }

    fn parse_record_field(&mut self) -> Result<RecordField, CompileError> {
        let name = self.expect_ident("as a record field name")?;
        self.expect(TokenKind::Colon, "after the field name")?;
        let type_ref = self.parse_type_ref("as the field type")?;
        let mut refinement = None;
        let mut end_span = type_ref.span();
        if self.eat(TokenKind::Where).is_some() {
            let r = self.parse_refinement()?;
            end_span = r.span;
            refinement = Some(r);
        }
        Ok(RecordField {
            name: name.clone(),
            type_ref,
            refinement,
            span: name.span.merge(end_span),
        })
    }

    /// Parse a pipe-form sum body: `| Variant | Variant(field, ...)`.
    /// The leading `|` is required (spec v0.2 §3.2).
    fn parse_sum_body_pipe(&mut self) -> Result<SumBody, CompileError> {
        let mut variants = Vec::new();
        let mut span: Option<Span> = None;
        while self.peek_kind() == Some(TokenKind::Pipe) {
            let bar = self.bump().unwrap();
            let name = self.expect_ident("after `|` in a sum variant")?;
            let mut payload = Vec::new();
            let mut end_span = name.span;
            if self.peek_kind() == Some(TokenKind::LParen) {
                self.bump();
                if self.peek_kind() != Some(TokenKind::RParen) {
                    payload.push(self.parse_variant_field()?);
                    while self.eat(TokenKind::Comma).is_some() {
                        if self.peek_kind() == Some(TokenKind::RParen) {
                            break;
                        }
                        payload.push(self.parse_variant_field()?);
                    }
                }
                let close =
                    self.expect(TokenKind::RParen, "to close the variant's payload list")?;
                end_span = close.span;
            }
            let v_span = bar.span.merge(end_span);
            variants.push(Variant {
                name,
                payload,
                span: v_span,
            });
            span = Some(match span {
                Some(s) => s.merge(v_span),
                None => v_span,
            });
        }
        let span = span.expect("parse_sum_body_pipe called without `|`");
        Ok(SumBody { variants, span })
    }

    /// Parse an enum-shorthand sum body: `enum { Tag, Tag, Tag }`.
    fn parse_sum_body_enum(&mut self) -> Result<SumBody, CompileError> {
        let kw = self.expect(TokenKind::Enum, "to start an enum-form sum body")?;
        self.expect(TokenKind::LBrace, "after `enum`")?;
        let mut variants = Vec::new();
        while self.peek_kind() != Some(TokenKind::RBrace) {
            let name = self.expect_ident("as an enum tag name")?;
            let span = name.span;
            variants.push(Variant {
                name,
                payload: Vec::new(),
                span,
            });
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let close = self.expect(TokenKind::RBrace, "to close the enum body")?;
        Ok(SumBody {
            variants,
            span: kw.span.merge(close.span),
        })
    }

    fn parse_variant_field(&mut self) -> Result<VariantField, CompileError> {
        let name = self.expect_ident("as a variant payload field name")?;
        self.expect(TokenKind::Colon, "after the variant payload field name")?;
        let type_ref = self.parse_type_ref("as the variant payload field type")?;
        let span = name.span.merge(type_ref.span());
        Ok(VariantField {
            name,
            type_ref,
            span,
        })
    }

    fn parse_base_type(&mut self) -> Result<(BaseType, Span), CompileError> {
        match self.peek() {
            Some(t) => match t.kind {
                TokenKind::Int => {
                    self.bump();
                    Ok((BaseType::Int, t.span))
                }
                TokenKind::String => {
                    self.bump();
                    Ok((BaseType::String, t.span))
                }
                TokenKind::Bool => {
                    self.bump();
                    Ok((BaseType::Bool, t.span))
                }
                _ => Err(CompileError::new(
                    "karn.parse.expected_base_type",
                    t.span,
                    format!(
                        "expected `Int`, `String`, or `Bool`, found {}",
                        t.kind.describe()
                    ),
                )
                .with_note("type declarations are refined base types in v0")),
            },
            None => Err(CompileError::new(
                "karn.parse.unexpected_eof",
                self.eof_span(),
                "expected `Int`, `String`, or `Bool`, found end of file",
            )),
        }
    }

    fn parse_refinement(&mut self) -> Result<Refinement, CompileError> {
        let mut predicates = vec![self.parse_refinement_pred()?];
        let mut span = predicates[0].span;
        while self.eat(TokenKind::And).is_some() {
            let p = self.parse_refinement_pred()?;
            span = span.merge(p.span);
            predicates.push(p);
        }
        Ok(Refinement { predicates, span })
    }

    fn parse_refinement_pred(&mut self) -> Result<RefinementPred, CompileError> {
        let t = self.peek().ok_or_else(|| {
            CompileError::new(
                "karn.parse.unexpected_eof",
                self.eof_span(),
                "expected a refinement predicate, found end of file",
            )
        })?;
        // Allow `Int` etc. through here would be wrong; predicate names are plain
        // identifiers (and not keywords).
        if t.kind != TokenKind::Ident {
            return Err(CompileError::new(
                "karn.parse.expected_predicate",
                t.span,
                format!(
                    "expected a refinement predicate name, found {}",
                    t.kind.describe()
                ),
            )
            .with_note(
                "valid predicates: Matches, InRange, MinLength, MaxLength, Length, \
                 NonNegative, Positive, NonEmpty",
            ));
        }
        self.bump();
        let name = self.slice(t.span);
        let start = t.span;
        let (kind, end_span) = match name {
            "Matches" => {
                self.expect(TokenKind::LParen, "after `Matches`")?;
                let s_tok = self.expect(TokenKind::StrLit, "as the argument to `Matches`")?;
                let pat = parse_string_literal(self.slice(s_tok.span), s_tok.span)?;
                let close = self.expect(TokenKind::RParen, "after the `Matches` argument")?;
                (PredKind::Matches(pat), close.span)
            }
            "InRange" => {
                self.expect(TokenKind::LParen, "after `InRange`")?;
                let lo = self.parse_signed_int_literal("as the lower bound of `InRange`")?;
                self.expect(TokenKind::Comma, "between `InRange` arguments")?;
                let hi = self.parse_signed_int_literal("as the upper bound of `InRange`")?;
                let close = self.expect(TokenKind::RParen, "after the `InRange` arguments")?;
                (PredKind::InRange(lo, hi), close.span)
            }
            "MinLength" => {
                self.expect(TokenKind::LParen, "after `MinLength`")?;
                let n = self.parse_signed_int_literal("as the argument to `MinLength`")?;
                let close = self.expect(TokenKind::RParen, "after the `MinLength` argument")?;
                (PredKind::MinLength(n), close.span)
            }
            "MaxLength" => {
                self.expect(TokenKind::LParen, "after `MaxLength`")?;
                let n = self.parse_signed_int_literal("as the argument to `MaxLength`")?;
                let close = self.expect(TokenKind::RParen, "after the `MaxLength` argument")?;
                (PredKind::MaxLength(n), close.span)
            }
            "Length" => {
                self.expect(TokenKind::LParen, "after `Length`")?;
                let n = self.parse_signed_int_literal("as the argument to `Length`")?;
                let close = self.expect(TokenKind::RParen, "after the `Length` argument")?;
                (PredKind::Length(n), close.span)
            }
            "NonNegative" => (PredKind::NonNegative, t.span),
            "Positive" => (PredKind::Positive, t.span),
            "NonEmpty" => (PredKind::NonEmpty, t.span),
            other => {
                return Err(CompileError::new(
                    "karn.parse.unknown_predicate",
                    t.span,
                    format!("unknown refinement predicate `{other}`"),
                )
                .with_note(
                    "valid predicates: Matches, InRange, MinLength, MaxLength, Length, \
                     NonNegative, Positive, NonEmpty",
                ));
            }
        };
        Ok(RefinementPred {
            kind,
            span: start.merge(end_span),
        })
    }

    fn parse_signed_int_literal(&mut self, ctx: &str) -> Result<i64, CompileError> {
        let neg = self.eat(TokenKind::Minus).is_some();
        let t = self.expect(TokenKind::IntLit, ctx)?;
        let slice = self.slice(t.span);
        let n: i64 = slice.parse().map_err(|_| {
            CompileError::new(
                "karn.lex.integer_overflow",
                t.span,
                format!("integer literal `{slice}` is out of range for a 64-bit signed integer"),
            )
        })?;
        Ok(if neg { -n } else { n })
    }

    // -- function declarations --

    fn parse_fn_decl(&mut self) -> Result<FnDecl, CompileError> {
        let kw = self.expect(TokenKind::Fn, "to start a function declaration")?;
        let first = self.expect_ident("after `fn`")?;
        // A method declaration uses `TypeName.methodName`; a free function
        // is just an identifier. Disambiguate on the next token.
        let name = if self.eat(TokenKind::Dot).is_some() {
            let method = self.expect_ident("after `.` in a method declaration")?;
            FnName::Method {
                type_name: first,
                method_name: method,
            }
        } else {
            FnName::Free(first)
        };
        self.expect(TokenKind::LParen, "after the function name")?;
        // For methods, the first parameter may be the special `self` keyword.
        let mut params = Vec::new();
        let mut has_self = false;
        if self.peek_kind() == Some(TokenKind::Self_) {
            let self_tok = self.bump().unwrap();
            if !matches!(name, FnName::Method { .. }) {
                return Err(CompileError::new(
                    "karn.parse.self_outside_method",
                    self_tok.span,
                    "`self` can only appear as the first parameter of a method declaration",
                )
                .with_note(
                    "use `fn TypeName.method(self, ...)` to declare a method, \
                     or remove `self` for a free function",
                ));
            }
            has_self = true;
            // Allow a trailing comma after `self` for further params.
            if self.peek_kind() == Some(TokenKind::Comma) {
                self.bump();
                if self.peek_kind() != Some(TokenKind::RParen) {
                    params.push(self.parse_param()?);
                    while self.eat(TokenKind::Comma).is_some() {
                        params.push(self.parse_param()?);
                    }
                }
            }
        } else if self.peek_kind() != Some(TokenKind::RParen) {
            params.push(self.parse_param()?);
            while self.eat(TokenKind::Comma).is_some() {
                params.push(self.parse_param()?);
            }
        }
        self.expect(TokenKind::RParen, "to close the parameter list")?;
        self.expect(TokenKind::Arrow, "before the return type")?;
        let return_type = self.parse_type_ref("as the return type")?;
        let body = self.parse_block("to open the function body")?;
        let span = kw.span.merge(body.span);
        Ok(FnDecl {
            name,
            params,
            return_type,
            body,
            has_self,
            documentation: None,
            span,
            trivia: Trivia::default(),
        })
    }

    /// Parse a brace-delimited block: `{ statement* expr }` (v0.1 §3.1, v0.5).
    fn parse_block(&mut self, ctx: &str) -> Result<Block, CompileError> {
        let open = self.expect(TokenKind::LBrace, ctx)?;
        let mut statements = Vec::new();
        // Loop: parse statements until we hit something that's not a statement.
        // v0.1: `let`. v0.5: `commit` and `let ... <-` are also statements.
        // v0.7: `assert` is a statement form inside test bodies.
        let tail_leading: Vec<String>;
        loop {
            let leading = self.take_leading_trivia();
            match self.peek_kind() {
                Some(TokenKind::Let) | Some(TokenKind::Commit) | Some(TokenKind::Assert) => {
                    let mut stmt = self.parse_statement()?;
                    let trailing = self.take_trailing_trivia();
                    match &mut stmt {
                        Statement::Let(l) | Statement::EffectLet(l) => {
                            l.trivia.leading = leading;
                            l.trivia.trailing = trailing;
                        }
                        Statement::Commit(c) => {
                            c.trivia.leading = leading;
                            c.trivia.trailing = trailing;
                        }
                        Statement::Assert(a) => {
                            a.trivia.leading = leading;
                            a.trivia.trailing = trailing;
                        }
                    }
                    statements.push(stmt);
                }
                _ => {
                    tail_leading = leading;
                    break;
                }
            }
        }
        // v0.7: a block whose last statement is an `assert` may close without
        // an explicit tail expression. The implicit tail is `()` (unit).
        if self.peek_kind() == Some(TokenKind::RBrace)
            && matches!(statements.last(), Some(Statement::Assert(_)))
        {
            let close = self.expect(TokenKind::RBrace, "to close the block")?;
            let tail = Expr {
                kind: ExprKind::UnitLit,
                span: close.span,
            };
            return Ok(Block {
                statements,
                tail: Box::new(tail),
                span: open.span.merge(close.span),
                tail_leading_comments: tail_leading,
            });
        }
        let tail = self.parse_expr()?;
        let close = self.expect(TokenKind::RBrace, "to close the block")?;
        Ok(Block {
            statements,
            tail: Box::new(tail),
            span: open.span.merge(close.span),
            tail_leading_comments: tail_leading,
        })
    }

    fn parse_statement(&mut self) -> Result<Statement, CompileError> {
        if self.peek_kind() == Some(TokenKind::Commit) {
            let kw = self.expect(TokenKind::Commit, "to start a commit statement")?;
            let value = self.parse_expr()?;
            let span = kw.span.merge(value.span);
            return Ok(Statement::Commit(CommitStmt {
                value,
                span,
                trivia: Trivia::default(),
            }));
        }
        if self.peek_kind() == Some(TokenKind::Assert) {
            let kw = self.expect(TokenKind::Assert, "to start an assert statement")?;
            let value = self.parse_expr()?;
            let span = kw.span.merge(value.span);
            return Ok(Statement::Assert(AssertStmt {
                value,
                span,
                trivia: Trivia::default(),
            }));
        }
        let kw = self.expect(TokenKind::Let, "to start a let statement")?;
        // Allow `_` as a discard name in `let _ = ...` and `let _ <- ...`.
        let name = if self.peek_kind() == Some(TokenKind::Underscore) {
            let t = self.bump().unwrap();
            Ident {
                name: "_".to_string(),
                span: t.span,
            }
        } else {
            self.expect_ident("after `let`")?
        };
        let type_annot = if self.eat(TokenKind::Colon).is_some() {
            Some(self.parse_type_ref("as the let-binding's type annotation")?)
        } else {
            None
        };
        match self.peek_kind() {
            Some(TokenKind::Eq) => {
                self.bump();
                let value = self.parse_expr()?;
                let span = kw.span.merge(value.span);
                Ok(Statement::Let(LetStmt {
                    name,
                    type_annot,
                    value,
                    span,
                    trivia: Trivia::default(),
                }))
            }
            Some(TokenKind::LArrow) => {
                self.bump();
                let value = self.parse_expr()?;
                let span = kw.span.merge(value.span);
                Ok(Statement::EffectLet(LetStmt {
                    name,
                    type_annot,
                    value,
                    span,
                    trivia: Trivia::default(),
                }))
            }
            Some(_) => {
                let t = self.peek().unwrap();
                Err(CompileError::new(
                    "karn.parse.expected_token",
                    t.span,
                    format!(
                        "expected `=` or `<-` after the let-binding's name, found {}",
                        t.kind.describe()
                    ),
                ))
            }
            None => Err(CompileError::new(
                "karn.parse.unexpected_eof",
                self.eof_span(),
                "expected `=` or `<-` after the let-binding's name, found end of file",
            )),
        }
    }

    fn parse_param(&mut self) -> Result<Param, CompileError> {
        let name = self.expect_ident("as a parameter name")?;
        self.expect(TokenKind::Colon, "after the parameter name")?;
        let type_ref = self.parse_type_ref("as the parameter type")?;
        let span = name.span.merge(type_ref.span());
        Ok(Param {
            name,
            type_ref,
            span,
        })
    }

    fn parse_type_ref(&mut self, ctx: &str) -> Result<TypeRef, CompileError> {
        match self.peek() {
            Some(t) => match t.kind {
                TokenKind::Int => {
                    self.bump();
                    Ok(TypeRef::Base(BaseType::Int, t.span))
                }
                TokenKind::String => {
                    self.bump();
                    Ok(TypeRef::Base(BaseType::String, t.span))
                }
                TokenKind::Bool => {
                    self.bump();
                    Ok(TypeRef::Base(BaseType::Bool, t.span))
                }
                TokenKind::Result => {
                    self.bump();
                    // Must be followed by `[T, E]`.
                    let lb = self.peek().map(|t| t.kind);
                    if lb != Some(TokenKind::LBracket) {
                        return Err(CompileError::new(
                            "karn.parse.expected_token",
                            t.span,
                            "the built-in `Result` type requires two type arguments: `Result[T, E]`",
                        )
                        .with_note(
                            "`Result` cannot appear without its `[T, E]` parameters in v0.1",
                        ));
                    }
                    self.bump();
                    let arg_t = self.parse_type_ref("as the first `Result` type argument")?;
                    // Check for missing comma — typical user error is `Result[T]`.
                    if self.peek_kind() == Some(TokenKind::RBracket) {
                        let close = self.bump().unwrap();
                        return Err(CompileError::new(
                            "karn.parse.generic_arg_count",
                            t.span.merge(close.span),
                            "the built-in `Result` type requires two type arguments: `Result[T, E]`",
                        )
                        .with_note("v0.1 has no other generic types; `Result` always has two parameters"));
                    }
                    self.expect(TokenKind::Comma, "between the `Result` type arguments")?;
                    let arg_e = self.parse_type_ref("as the second `Result` type argument")?;
                    let close =
                        self.expect(TokenKind::RBracket, "to close the `Result` type arguments")?;
                    Ok(TypeRef::Result(
                        Box::new(arg_t),
                        Box::new(arg_e),
                        t.span.merge(close.span),
                    ))
                }
                TokenKind::ValidationError => {
                    self.bump();
                    Ok(TypeRef::ValidationError(t.span))
                }
                TokenKind::Option => {
                    self.bump();
                    if self.peek_kind() != Some(TokenKind::LBracket) {
                        return Err(CompileError::new(
                            "karn.parse.expected_token",
                            t.span,
                            "the built-in `Option` type requires one type argument: `Option[T]`",
                        ));
                    }
                    self.bump();
                    let arg = self.parse_type_ref("as the `Option` type argument")?;
                    let close =
                        self.expect(TokenKind::RBracket, "to close the `Option` type argument")?;
                    Ok(TypeRef::Option(Box::new(arg), t.span.merge(close.span)))
                }
                TokenKind::Effect => {
                    self.bump();
                    if self.peek_kind() != Some(TokenKind::LBracket) {
                        return Err(CompileError::new(
                            "karn.parse.expected_token",
                            t.span,
                            "the built-in `Effect` type requires one type argument: `Effect[T]`",
                        ));
                    }
                    self.bump();
                    let arg = self.parse_type_ref("as the `Effect` type argument")?;
                    let close =
                        self.expect(TokenKind::RBracket, "to close the `Effect` type argument")?;
                    Ok(TypeRef::Effect(Box::new(arg), t.span.merge(close.span)))
                }
                TokenKind::LParen => {
                    // `()` — unit type (v0.5).
                    self.bump();
                    let close = self.expect(TokenKind::RParen, "to close the unit type `()`")?;
                    Ok(TypeRef::Unit(t.span.merge(close.span)))
                }
                TokenKind::Ident => {
                    self.bump();
                    let name = self.slice(t.span).to_string();
                    // v0.9: `HttpResult` is a predeclared built-in generic.
                    if name == "HttpResult" {
                        if self.peek_kind() != Some(TokenKind::LBracket) {
                            return Err(CompileError::new(
                                "karn.parse.expected_token",
                                t.span,
                                "the built-in `HttpResult` type requires one type argument: `HttpResult[T]`",
                            ));
                        }
                        self.bump();
                        let arg = self.parse_type_ref("as the `HttpResult` type argument")?;
                        let close = self.expect(
                            TokenKind::RBracket,
                            "to close the `HttpResult` type argument",
                        )?;
                        return Ok(TypeRef::HttpResult(Box::new(arg), t.span.merge(close.span)));
                    }
                    Ok(TypeRef::Named(Ident { name, span: t.span }))
                }
                _ => Err(CompileError::new(
                    "karn.parse.expected_type",
                    t.span,
                    format!("expected a type {ctx}, found {}", t.kind.describe()),
                )),
            },
            None => Err(CompileError::new(
                "karn.parse.unexpected_eof",
                self.eof_span(),
                format!("expected a type {ctx}, found end of file"),
            )),
        }
    }

    // -- expressions --

    fn parse_expr(&mut self) -> Result<Expr, CompileError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, CompileError> {
        let mut lhs = self.parse_and()?;
        while self.peek_kind() == Some(TokenKind::PipePipe) {
            self.bump();
            let rhs = self.parse_and()?;
            let span = lhs.span.merge(rhs.span);
            lhs = Expr {
                kind: ExprKind::BinOp(BinOp::Or, Box::new(lhs), Box::new(rhs)),
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, CompileError> {
        let mut lhs = self.parse_eq()?;
        while self.peek_kind() == Some(TokenKind::AmpAmp) {
            self.bump();
            let rhs = self.parse_eq()?;
            let span = lhs.span.merge(rhs.span);
            lhs = Expr {
                kind: ExprKind::BinOp(BinOp::And, Box::new(lhs), Box::new(rhs)),
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_eq(&mut self) -> Result<Expr, CompileError> {
        let lhs = self.parse_cmp()?;
        // v0.2: the `is` operator sits at the same precedence level as
        // equality but produces a Bool from a pattern test.
        if self.peek_kind() == Some(TokenKind::Is) {
            self.bump();
            let pattern = self.parse_pattern()?;
            let span = lhs.span.merge(pattern.span());
            return Ok(Expr {
                kind: ExprKind::Is {
                    value: Box::new(lhs),
                    pattern,
                },
                span,
            });
        }
        let op = match self.peek_kind() {
            Some(TokenKind::EqEq) => Some(BinOp::Eq),
            Some(TokenKind::BangEq) => Some(BinOp::NotEq),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            let rhs = self.parse_cmp()?;
            // Non-associative: reject a second `==` or `!=` at this level.
            if matches!(
                self.peek_kind(),
                Some(TokenKind::EqEq) | Some(TokenKind::BangEq)
            ) {
                let t = self.peek().unwrap();
                return Err(CompileError::new(
                    "karn.parse.non_associative",
                    t.span,
                    format!(
                        "`{}` is non-associative; chained equality is not allowed",
                        t.kind.describe().trim_matches('`')
                    ),
                )
                .with_note("parenthesise to disambiguate, e.g. `(a == b) == c`"));
            }
            let span = lhs.span.merge(rhs.span);
            Ok(Expr {
                kind: ExprKind::BinOp(op, Box::new(lhs), Box::new(rhs)),
                span,
            })
        } else {
            Ok(lhs)
        }
    }

    fn parse_cmp(&mut self) -> Result<Expr, CompileError> {
        let lhs = self.parse_add()?;
        let op = match self.peek_kind() {
            Some(TokenKind::Lt) => Some(BinOp::Lt),
            Some(TokenKind::LtEq) => Some(BinOp::LtEq),
            Some(TokenKind::Gt) => Some(BinOp::Gt),
            Some(TokenKind::GtEq) => Some(BinOp::GtEq),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            let rhs = self.parse_add()?;
            if matches!(
                self.peek_kind(),
                Some(TokenKind::Lt)
                    | Some(TokenKind::LtEq)
                    | Some(TokenKind::Gt)
                    | Some(TokenKind::GtEq)
            ) {
                let t = self.peek().unwrap();
                return Err(CompileError::new(
                    "karn.parse.non_associative",
                    t.span,
                    "comparison operators are non-associative; chained comparison is not allowed",
                )
                .with_note("split the comparison: `a < b && b < c` instead of `a < b < c`"));
            }
            let span = lhs.span.merge(rhs.span);
            Ok(Expr {
                kind: ExprKind::BinOp(op, Box::new(lhs), Box::new(rhs)),
                span,
            })
        } else {
            Ok(lhs)
        }
    }

    fn parse_add(&mut self) -> Result<Expr, CompileError> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek_kind() {
                Some(TokenKind::Plus) => BinOp::Add,
                Some(TokenKind::Minus) => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_mul()?;
            let span = lhs.span.merge(rhs.span);
            lhs = Expr {
                kind: ExprKind::BinOp(op, Box::new(lhs), Box::new(rhs)),
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<Expr, CompileError> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek_kind() {
                Some(TokenKind::Star) => BinOp::Mul,
                Some(TokenKind::Slash) => BinOp::Div,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_unary()?;
            let span = lhs.span.merge(rhs.span);
            lhs = Expr {
                kind: ExprKind::BinOp(op, Box::new(lhs), Box::new(rhs)),
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, CompileError> {
        match self.peek_kind() {
            Some(TokenKind::Minus) => {
                let t = self.bump().unwrap();
                let inner = self.parse_unary()?;
                let span = t.span.merge(inner.span);
                Ok(Expr {
                    kind: ExprKind::UnaryOp(UnaryOp::Neg, Box::new(inner)),
                    span,
                })
            }
            Some(TokenKind::Bang) => {
                let t = self.bump().unwrap();
                let inner = self.parse_unary()?;
                let span = t.span.merge(inner.span);
                Ok(Expr {
                    kind: ExprKind::UnaryOp(UnaryOp::Not, Box::new(inner)),
                    span,
                })
            }
            _ => self.parse_postfix(),
        }
    }

    /// Parse a primary expression and then apply postfix operators (`?`,
    /// `.identifier` field access, `.identifier(args)` method call —
    /// v0.2 §3.7).
    fn parse_postfix(&mut self) -> Result<Expr, CompileError> {
        let mut e = self.parse_primary()?;
        loop {
            match self.peek_kind() {
                Some(TokenKind::Question) => {
                    let q = self.bump().unwrap();
                    let span = e.span.merge(q.span);
                    e = Expr {
                        kind: ExprKind::Question(Box::new(e)),
                        span,
                    };
                }
                Some(TokenKind::Dot) => {
                    self.bump();
                    let member = self.expect_ident("after `.` in field access or method call")?;
                    if self.peek_kind() == Some(TokenKind::LParen) {
                        // Method call: `receiver.method(args)`.
                        self.bump();
                        let mut args = Vec::new();
                        if self.peek_kind() != Some(TokenKind::RParen) {
                            args.push(self.parse_expr()?);
                            while self.eat(TokenKind::Comma).is_some() {
                                args.push(self.parse_expr()?);
                            }
                        }
                        let close = self
                            .expect(TokenKind::RParen, "to close the method-call argument list")?;
                        let span = e.span.merge(close.span);
                        e = Expr {
                            kind: ExprKind::MethodCall {
                                receiver: Box::new(e),
                                method: member,
                                args,
                            },
                            span,
                        };
                    } else {
                        // Field access: `receiver.field`.
                        let span = e.span.merge(member.span);
                        e = Expr {
                            kind: ExprKind::FieldAccess {
                                receiver: Box::new(e),
                                field: member,
                            },
                            span,
                        };
                    }
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn parse_primary(&mut self) -> Result<Expr, CompileError> {
        let t = self.peek().ok_or_else(|| {
            CompileError::new(
                "karn.parse.unexpected_eof",
                self.eof_span(),
                "expected an expression, found end of file",
            )
        })?;
        match t.kind {
            TokenKind::IntLit => {
                self.bump();
                let slice = self.slice(t.span);
                let n: i64 = slice.parse().map_err(|_| {
                    CompileError::new(
                        "karn.lex.integer_overflow",
                        t.span,
                        format!("integer literal `{slice}` out of 64-bit range"),
                    )
                })?;
                Ok(Expr {
                    kind: ExprKind::IntLit(n),
                    span: t.span,
                })
            }
            TokenKind::StrLit => {
                self.bump();
                let s = parse_string_literal(self.slice(t.span), t.span)?;
                Ok(Expr {
                    kind: ExprKind::StrLit(s),
                    span: t.span,
                })
            }
            TokenKind::True => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::BoolLit(true),
                    span: t.span,
                })
            }
            TokenKind::False => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::BoolLit(false),
                    span: t.span,
                })
            }
            TokenKind::LParen => {
                self.bump();
                // `()` — unit literal (v0.5).
                if self.peek_kind() == Some(TokenKind::RParen) {
                    let close = self.bump().unwrap();
                    return Ok(Expr {
                        kind: ExprKind::UnitLit,
                        span: t.span.merge(close.span),
                    });
                }
                let inner = self.parse_expr()?;
                let close =
                    self.expect(TokenKind::RParen, "to close the parenthesised expression")?;
                Ok(Expr {
                    kind: ExprKind::Paren(Box::new(inner)),
                    span: t.span.merge(close.span),
                })
            }
            // `Effect.pure(value)` — wrap a synchronous value as `Effect[T]` (v0.5).
            TokenKind::Effect => {
                let kw = self.bump().unwrap();
                self.expect(TokenKind::Dot, "after `Effect` in `Effect.pure(...)`")?;
                let method = self.expect_ident("after `Effect.`")?;
                if method.name != "pure" {
                    return Err(CompileError::new(
                        "karn.parse.unknown_effect_method",
                        method.span,
                        format!(
                            "the only operation on `Effect` in expression position is `pure`, but got `{}`",
                            method.name
                        ),
                    )
                    .with_note("use `Effect.pure(value)` to lift a synchronous value into `Effect[T]`"));
                }
                self.expect(TokenKind::LParen, "after `Effect.pure`")?;
                let value = self.parse_expr()?;
                let close =
                    self.expect(TokenKind::RParen, "to close the `Effect.pure` argument")?;
                Ok(Expr {
                    kind: ExprKind::EffectPure(Box::new(value)),
                    span: kw.span.merge(close.span),
                })
            }
            TokenKind::Ident => {
                self.bump();
                let ident = Ident {
                    name: self.slice(t.span).to_string(),
                    span: t.span,
                };
                if self.peek_kind() == Some(TokenKind::LParen) {
                    self.bump();
                    let mut args = Vec::new();
                    if self.peek_kind() != Some(TokenKind::RParen) {
                        args.push(self.parse_expr()?);
                        while self.eat(TokenKind::Comma).is_some() {
                            args.push(self.parse_expr()?);
                        }
                    }
                    let close = self.expect(TokenKind::RParen, "to close the argument list")?;
                    Ok(Expr {
                        kind: ExprKind::Call(ident.clone(), args),
                        span: ident.span.merge(close.span),
                    })
                } else if self.peek_kind() == Some(TokenKind::LBrace)
                    && self.looks_like_record_construction()
                {
                    // Record construction: `TypeName { field: value, ... }`.
                    self.parse_record_construction(ident)
                } else {
                    Ok(Expr {
                        kind: ExprKind::Ident(ident.clone()),
                        span: ident.span,
                    })
                }
            }
            // v0.1: `if cond { ... } else { ... }`.
            TokenKind::If => self.parse_if_expr(),
            // v0.1: `Ok(value)` and `Err(value)` result constructors.
            TokenKind::Ok => self.parse_result_expr(true),
            TokenKind::Err => self.parse_result_expr(false),
            // v0.2: `Some(value)` / `None` / `match` / `self`.
            TokenKind::Some => self.parse_some_expr(),
            TokenKind::None => {
                let tok = self.bump().unwrap();
                Ok(Expr {
                    kind: ExprKind::None,
                    span: tok.span,
                })
            }
            TokenKind::Match => self.parse_match_expr(),
            TokenKind::Self_ => {
                // `self` is parsed as a primary identifier with the literal
                // name `self`; the resolver scopes it to method bodies.
                let tok = self.bump().unwrap();
                Ok(Expr {
                    kind: ExprKind::Ident(Ident {
                        name: "self".to_string(),
                        span: tok.span,
                    }),
                    span: tok.span,
                })
            }
            // v0.5: bare record spread `{ ...base, field: value }`. Used by
            // `commit { ... }` when the state type is implied.
            TokenKind::LBrace => {
                if self.tokens.get(self.pos + 1).map(|t| t.kind) == Some(TokenKind::DotDotDot) {
                    self.parse_bare_record_spread()
                } else {
                    Err(CompileError::new(
                        "karn.parse.expected_expression",
                        t.span,
                        "expected an expression, found `{`",
                    )
                    .with_note(
                        "bare record-spread `{ ...base, ... }` is the only `{`-led expression in v0.5; for record construction, use `TypeName { ... }`",
                    ))
                }
            }
            // Reserved future syntax.
            TokenKind::LBracket => Err(CompileError::new(
                "karn.parse.reserved_syntax",
                t.span,
                "`[` is reserved for future generic syntax and is not allowed in expressions",
            )),
            _ => Err(CompileError::new(
                "karn.parse.expected_expression",
                t.span,
                format!("expected an expression, found {}", t.kind.describe()),
            )),
        }
    }
}

impl<'a> Parser<'a> {
    /// Lookahead helper: distinguish record construction `T { ... }` from
    /// a `T` ident followed by an unrelated block (which can happen inside
    /// match-arm bodies or if-branches that take a block).
    ///
    /// A record construction has either `Ident :` or `Ident ,` or `Ident }`
    /// after the opening brace, or `}` immediately for the empty case.
    /// A function body or match body never starts with `Ident :` or `Ident ,`
    /// at this position because a `let` would come first as a statement.
    fn looks_like_record_construction(&self) -> bool {
        debug_assert_eq!(self.peek_kind(), Some(TokenKind::LBrace));
        let a = self.tokens.get(self.pos + 1).map(|t| t.kind);
        let b = self.tokens.get(self.pos + 2).map(|t| t.kind);
        match (a, b) {
            // `T {}` — empty record.
            (Some(TokenKind::RBrace), _) => true,
            // `T { ...base, ... }` — record spread (v0.5).
            (Some(TokenKind::DotDotDot), _) => true,
            // `T { field: ... }` or `T { field, ... }` — record construction.
            (
                Some(TokenKind::Ident),
                Some(TokenKind::Colon) | Some(TokenKind::Comma) | Some(TokenKind::RBrace),
            ) => true,
            _ => false,
        }
    }

    /// Parse `TypeName { field: value, ... }` or `TypeName { ...base, field: value }`
    /// once we've already consumed the type name and the next token is `{`.
    fn parse_record_construction(&mut self, type_name: Ident) -> Result<Expr, CompileError> {
        let open = self.expect(TokenKind::LBrace, "to open the record construction")?;
        // v0.5: spread form `TypeName { ...base, field: value, ... }`.
        if self.peek_kind() == Some(TokenKind::DotDotDot) {
            self.bump();
            let base = self.parse_expr()?;
            let mut overrides = Vec::new();
            while self.eat(TokenKind::Comma).is_some() {
                if self.peek_kind() == Some(TokenKind::RBrace) {
                    break;
                }
                overrides.push(self.parse_field_init()?);
            }
            let close = self.expect(TokenKind::RBrace, "to close the record spread")?;
            let span = type_name.span.merge(close.span);
            return Ok(Expr {
                kind: ExprKind::RecordSpread {
                    type_name: Some(type_name),
                    base: Box::new(base),
                    overrides,
                },
                span,
            });
        }
        let mut fields = Vec::new();
        while self.peek_kind() != Some(TokenKind::RBrace) {
            fields.push(self.parse_field_init()?);
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let close = self.expect(TokenKind::RBrace, "to close the record construction")?;
        let span = type_name.span.merge(close.span);
        let _ = open;
        Ok(Expr {
            kind: ExprKind::RecordConstruction { type_name, fields },
            span,
        })
    }

    /// Parse `{ ...base, field: value, ... }` — the bare record-spread form.
    fn parse_bare_record_spread(&mut self) -> Result<Expr, CompileError> {
        let open = self.expect(TokenKind::LBrace, "to open the record spread")?;
        self.expect(TokenKind::DotDotDot, "after `{` in a record spread")?;
        let base = self.parse_expr()?;
        let mut overrides = Vec::new();
        while self.eat(TokenKind::Comma).is_some() {
            if self.peek_kind() == Some(TokenKind::RBrace) {
                break;
            }
            overrides.push(self.parse_field_init()?);
        }
        let close = self.expect(TokenKind::RBrace, "to close the record spread")?;
        let span = open.span.merge(close.span);
        Ok(Expr {
            kind: ExprKind::RecordSpread {
                type_name: None,
                base: Box::new(base),
                overrides,
            },
            span,
        })
    }

    fn parse_field_init(&mut self) -> Result<FieldInit, CompileError> {
        let name = self.expect_ident("as a record-field initialiser name")?;
        // `name : expr` (full form) or `name ,` / `name }` (shorthand).
        if self.eat(TokenKind::Colon).is_some() {
            let value = self.parse_expr()?;
            let span = name.span.merge(value.span);
            Ok(FieldInit {
                name,
                value: Some(value),
                span,
            })
        } else {
            let span = name.span;
            Ok(FieldInit {
                name,
                value: None,
                span,
            })
        }
    }

    /// Parse a `Some(value)` expression.
    fn parse_some_expr(&mut self) -> Result<Expr, CompileError> {
        let kw = self.expect(TokenKind::Some, "to start a `Some` expression")?;
        self.expect(TokenKind::LParen, "after `Some`")?;
        let value = self.parse_expr()?;
        let close = self.expect(TokenKind::RParen, "to close the `Some` argument")?;
        Ok(Expr {
            kind: ExprKind::Some(Box::new(value)),
            span: kw.span.merge(close.span),
        })
    }

    /// Parse a `match` expression: `match expr { pat => body, ... }`.
    fn parse_match_expr(&mut self) -> Result<Expr, CompileError> {
        let kw = self.expect(TokenKind::Match, "to start a match expression")?;
        let discriminant = self.parse_expr()?;
        self.expect(TokenKind::LBrace, "to open the match-arm list")?;
        let mut arms = Vec::new();
        while self.peek_kind() != Some(TokenKind::RBrace) {
            arms.push(self.parse_match_arm()?);
            // Arms are separated by newlines (significant via the iterator),
            // optionally by a comma. We just keep parsing arms greedily.
            let _ = self.eat(TokenKind::Comma);
        }
        let close = self.expect(TokenKind::RBrace, "to close the match-arm list")?;
        if arms.is_empty() {
            return Err(CompileError::new(
                "karn.parse.empty_match",
                kw.span.merge(close.span),
                "a `match` expression must have at least one arm",
            ));
        }
        Ok(Expr {
            kind: ExprKind::Match {
                discriminant: Box::new(discriminant),
                arms,
            },
            span: kw.span.merge(close.span),
        })
    }

    fn parse_match_arm(&mut self) -> Result<MatchArm, CompileError> {
        let pattern = self.parse_pattern()?;
        self.expect(TokenKind::FatArrow, "after a match-arm pattern")?;
        let body = if self.peek_kind() == Some(TokenKind::LBrace) {
            MatchBody::Block(self.parse_block("to open the match-arm body")?)
        } else {
            MatchBody::Expr(self.parse_expr()?)
        };
        let span = pattern.span().merge(body.span());
        Ok(MatchArm {
            pattern,
            body,
            span,
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern, CompileError> {
        if let Some(t) = self.peek() {
            if t.kind == TokenKind::Underscore {
                self.bump();
                return Ok(Pattern::Wildcard(t.span));
            }
            // Built-in variant patterns: `Ok(...)`, `Err(...)`, `Some(...)`, `None`.
            match t.kind {
                TokenKind::Ok | TokenKind::Err | TokenKind::Some | TokenKind::None => {
                    return self.parse_variant_pattern_builtin();
                }
                _ => {}
            }
        }
        // Otherwise: an ident-led pattern. Possibly qualified as `Type.Variant`.
        let first = self.expect_ident("as a match-arm pattern")?;
        let (type_name, variant) = if self.eat(TokenKind::Dot).is_some() {
            let v = self.expect_ident("after `.` in a qualified pattern")?;
            (Some(first), v)
        } else {
            (None, first)
        };
        let mut bindings = Vec::new();
        let mut end_span = variant.span;
        if self.peek_kind() == Some(TokenKind::LParen) {
            self.bump();
            if self.peek_kind() != Some(TokenKind::RParen) {
                bindings.push(self.parse_pattern_binding()?);
                while self.eat(TokenKind::Comma).is_some() {
                    bindings.push(self.parse_pattern_binding()?);
                }
            }
            let close = self.expect(TokenKind::RParen, "to close the pattern binding list")?;
            end_span = close.span;
        }
        let start_span = type_name.as_ref().map(|t| t.span).unwrap_or(variant.span);
        Ok(Pattern::Variant {
            type_name,
            variant,
            bindings,
            span: start_span.merge(end_span),
        })
    }

    /// Parse a built-in variant pattern (Ok/Err/Some/None) — these are
    /// keyword tokens rather than Idents so they need special handling.
    fn parse_variant_pattern_builtin(&mut self) -> Result<Pattern, CompileError> {
        let t = self.bump().unwrap();
        let variant_name = match t.kind {
            TokenKind::Ok => "Ok",
            TokenKind::Err => "Err",
            TokenKind::Some => "Some",
            TokenKind::None => "None",
            _ => unreachable!(),
        };
        let variant = Ident {
            name: variant_name.to_string(),
            span: t.span,
        };
        let mut bindings = Vec::new();
        let mut end_span = variant.span;
        if self.peek_kind() == Some(TokenKind::LParen) {
            self.bump();
            if self.peek_kind() != Some(TokenKind::RParen) {
                bindings.push(self.parse_pattern_binding()?);
                while self.eat(TokenKind::Comma).is_some() {
                    bindings.push(self.parse_pattern_binding()?);
                }
            }
            let close = self.expect(TokenKind::RParen, "to close the pattern binding list")?;
            end_span = close.span;
        }
        let variant_span = variant.span;
        Ok(Pattern::Variant {
            type_name: None,
            variant,
            bindings,
            span: variant_span.merge(end_span),
        })
    }

    fn parse_pattern_binding(&mut self) -> Result<PatternBinding, CompileError> {
        // Allowed shapes:
        //   `_`              positional wildcard
        //   `name`           positional bind
        //   `field: name`    named bind (where `name` may be `_`)
        if let Some(t) = self.peek()
            && t.kind == TokenKind::Underscore
        {
            self.bump();
            return Ok(PatternBinding {
                kind: PatternBindingKind::Positional {
                    name: Ident {
                        name: "_".to_string(),
                        span: t.span,
                    },
                },
                span: t.span,
            });
        }
        let first = self.expect_ident("as a pattern binding")?;
        if self.eat(TokenKind::Colon).is_some() {
            let name = if self.peek_kind() == Some(TokenKind::Underscore) {
                let t = self.bump().unwrap();
                Ident {
                    name: "_".to_string(),
                    span: t.span,
                }
            } else {
                self.expect_ident("as the local name in a named pattern binding")?
            };
            let span = first.span.merge(name.span);
            Ok(PatternBinding {
                kind: PatternBindingKind::Named { field: first, name },
                span,
            })
        } else {
            let span = first.span;
            Ok(PatternBinding {
                kind: PatternBindingKind::Positional { name: first },
                span,
            })
        }
    }

    /// Parse `if expr block 'else' (if-expr | block)` (v0.1 §3.2).
    /// Both branches are represented as Blocks; an `else if` chain becomes a
    /// Block whose tail is another If expression.
    fn parse_if_expr(&mut self) -> Result<Expr, CompileError> {
        let kw = self.expect(TokenKind::If, "to start an if expression")?;
        let cond = self.parse_expr()?;
        let then_block = self.parse_block("to open the `if` branch")?;
        let else_kw = self.expect(TokenKind::Else, "every `if` requires a matching `else`")?;
        let _ = else_kw;
        let else_block = if self.peek_kind() == Some(TokenKind::If) {
            // `else if ...` desugars to `else { if ... }`.
            let inner = self.parse_if_expr()?;
            let span = inner.span;
            Block {
                statements: Vec::new(),
                tail: Box::new(inner),
                span,
                tail_leading_comments: Vec::new(),
            }
        } else {
            self.parse_block("to open the `else` branch")?
        };
        let span = kw.span.merge(else_block.span);
        Ok(Expr {
            kind: ExprKind::If {
                cond: Box::new(cond),
                then_block: Box::new(then_block),
                else_block: Box::new(else_block),
            },
            span,
        })
    }

    // -- v0.5 declarations --

    fn parse_capability_decl(&mut self) -> Result<CapabilityDecl, CompileError> {
        let kw = self.expect(TokenKind::Capability, "to start a capability declaration")?;
        let name = self.expect_ident("after `capability`")?;
        self.expect(TokenKind::LBrace, "to open the capability body")?;
        let mut ops = Vec::new();
        loop {
            let (leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::RBrace) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block has no following operation to attach to",
                        ));
                    }
                    break;
                }
                Some(TokenKind::Fn) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    let mut op = self.parse_capability_op()?;
                    op.documentation = doc;
                    op.trivia.leading = leading;
                    op.trivia.trailing = self.take_trailing_trivia();
                    ops.push(op);
                }
                Some(_) => {
                    let t = self.peek().unwrap();
                    return Err(CompileError::new(
                        "karn.parse.expected_capability_op",
                        t.span,
                        format!(
                            "expected `fn` to declare a capability operation, found {}",
                            t.kind.describe()
                        ),
                    ));
                }
                None => {
                    return Err(CompileError::new(
                        "karn.parse.unexpected_eof",
                        self.eof_span(),
                        "expected `}` to close the capability body, found end of file",
                    ));
                }
            }
        }
        let close = self.expect(TokenKind::RBrace, "to close the capability body")?;
        if ops.is_empty() {
            return Err(CompileError::new(
                "karn.parse.empty_capability",
                kw.span.merge(close.span),
                "a capability must declare at least one operation",
            ));
        }
        Ok(CapabilityDecl {
            name,
            ops,
            documentation: None,
            span: kw.span.merge(close.span),
            trivia: Trivia::default(),
        })
    }

    fn parse_capability_op(&mut self) -> Result<CapabilityOp, CompileError> {
        let kw = self.expect(TokenKind::Fn, "to start a capability operation")?;
        let name = self.expect_ident("as the capability operation name")?;
        self.expect(TokenKind::LParen, "after the operation name")?;
        let mut params = Vec::new();
        if self.peek_kind() != Some(TokenKind::RParen) {
            params.push(self.parse_param()?);
            while self.eat(TokenKind::Comma).is_some() {
                params.push(self.parse_param()?);
            }
        }
        self.expect(TokenKind::RParen, "to close the operation parameter list")?;
        self.expect(TokenKind::Arrow, "before the operation return type")?;
        let return_type = self.parse_type_ref("as the operation return type")?;
        let end_span = return_type.span();
        Ok(CapabilityOp {
            name,
            params,
            return_type,
            documentation: None,
            span: kw.span.merge(end_span),
            trivia: Trivia::default(),
        })
    }

    fn parse_provider_decl(&mut self) -> Result<ProviderDecl, CompileError> {
        let kw = self.expect(TokenKind::Provides, "to start a provider declaration")?;
        let capability = self.expect_ident("after `provides`")?;
        self.expect(TokenKind::Eq, "after the capability name")?;
        let provider_name = self.expect_ident("as the provider name")?;
        self.expect(TokenKind::LBrace, "to open the provider body")?;
        let mut ops = Vec::new();
        loop {
            let leading = self.take_leading_trivia();
            match self.peek_kind() {
                Some(TokenKind::RBrace) => break,
                Some(TokenKind::Fn) => {
                    let mut op = self.parse_provider_op()?;
                    op.trivia.leading = leading;
                    op.trivia.trailing = self.take_trailing_trivia();
                    ops.push(op);
                }
                Some(_) => {
                    let t = self.peek().unwrap();
                    return Err(CompileError::new(
                        "karn.parse.expected_provider_op",
                        t.span,
                        format!(
                            "expected `fn` to declare a provider operation, found {}",
                            t.kind.describe()
                        ),
                    ));
                }
                None => {
                    return Err(CompileError::new(
                        "karn.parse.unexpected_eof",
                        self.eof_span(),
                        "expected `}` to close the provider body, found end of file",
                    ));
                }
            }
        }
        let close = self.expect(TokenKind::RBrace, "to close the provider body")?;
        Ok(ProviderDecl {
            capability,
            provider_name,
            ops,
            documentation: None,
            span: kw.span.merge(close.span),
            trivia: Trivia::default(),
        })
    }

    fn parse_provider_op(&mut self) -> Result<ProviderOp, CompileError> {
        let kw = self.expect(TokenKind::Fn, "to start a provider operation")?;
        let name = self.expect_ident("as the provider operation name")?;
        self.expect(TokenKind::LParen, "after the operation name")?;
        let mut params = Vec::new();
        if self.peek_kind() != Some(TokenKind::RParen) {
            params.push(self.parse_param()?);
            while self.eat(TokenKind::Comma).is_some() {
                params.push(self.parse_param()?);
            }
        }
        self.expect(TokenKind::RParen, "to close the operation parameter list")?;
        self.expect(TokenKind::Arrow, "before the operation return type")?;
        let return_type = self.parse_type_ref("as the operation return type")?;
        let body = self.parse_block("to open the provider operation body")?;
        let span = kw.span.merge(body.span);
        Ok(ProviderOp {
            name,
            params,
            return_type,
            body,
            span,
            trivia: Trivia::default(),
        })
    }

    fn parse_service_decl(&mut self) -> Result<ServiceDecl, CompileError> {
        let kw = self.expect(TokenKind::Service, "to start a service declaration")?;
        let name = self.expect_ident("after `service`")?;
        self.expect(TokenKind::LBrace, "to open the service body")?;
        let mut handlers = Vec::new();
        loop {
            let (leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::RBrace) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block has no following handler to attach to",
                        ));
                    }
                    break;
                }
                Some(TokenKind::On) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    let mut h = self.parse_handler(false)?;
                    h.documentation = doc;
                    h.trivia.leading = leading;
                    h.trivia.trailing = self.take_trailing_trivia();
                    handlers.push(h);
                }
                Some(_) => {
                    let t = self.peek().unwrap();
                    return Err(CompileError::new(
                        "karn.parse.expected_handler",
                        t.span,
                        format!(
                            "expected `on` to start a handler, found {}",
                            t.kind.describe()
                        ),
                    ));
                }
                None => {
                    return Err(CompileError::new(
                        "karn.parse.unexpected_eof",
                        self.eof_span(),
                        "expected `}` to close the service body, found end of file",
                    ));
                }
            }
        }
        let close = self.expect(TokenKind::RBrace, "to close the service body")?;
        if handlers.is_empty() {
            return Err(CompileError::new(
                "karn.parse.empty_service",
                kw.span.merge(close.span),
                "a service must declare at least one handler",
            ));
        }
        Ok(ServiceDecl {
            name,
            handlers,
            documentation: None,
            span: kw.span.merge(close.span),
            trivia: Trivia::default(),
        })
    }

    fn parse_agent_decl(&mut self) -> Result<AgentDecl, CompileError> {
        let kw = self.expect(TokenKind::Agent, "to start an agent declaration")?;
        let name = self.expect_ident("after `agent`")?;
        self.expect(TokenKind::LBrace, "to open the agent body")?;
        // key id: Type
        // The `key` keyword is recognised as an identifier with the literal
        // name "key" — we don't have a dedicated keyword so it can be a
        // method name elsewhere. v0.5 reserves it only inside an agent body.
        let key_ident =
            self.expect_ident("expected `key id: Type` at the start of the agent body")?;
        if key_ident.name != "key" {
            return Err(CompileError::new(
                "karn.parse.expected_agent_key",
                key_ident.span,
                format!(
                    "expected `key id: Type` at the start of the agent body, found `{}`",
                    key_ident.name
                ),
            ));
        }
        let key_name = self.expect_ident("as the agent key field name")?;
        self.expect(TokenKind::Colon, "after the agent key field name")?;
        let key_type = self.parse_type_ref("as the agent key type")?;
        // state { ... }
        let state_kw = self.expect(
            TokenKind::State,
            "expected `state { ... }` after the agent key",
        )?;
        self.expect(TokenKind::LBrace, "to open the agent state block")?;
        let mut state_fields = Vec::new();
        while self.peek_kind() != Some(TokenKind::RBrace) {
            state_fields.push(self.parse_record_field()?);
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let state_close = self.expect(TokenKind::RBrace, "to close the agent state block")?;
        let state_span = state_kw.span.merge(state_close.span);
        // handlers
        let mut handlers = Vec::new();
        loop {
            let (leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::RBrace) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "karn.parse.orphan_doc_block",
                            doc_span,
                            "documentation block has no following handler to attach to",
                        ));
                    }
                    break;
                }
                Some(TokenKind::On) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    let mut h = self.parse_handler(true)?;
                    h.documentation = doc;
                    h.trivia.leading = leading;
                    h.trivia.trailing = self.take_trailing_trivia();
                    handlers.push(h);
                }
                Some(_) => {
                    let t = self.peek().unwrap();
                    return Err(CompileError::new(
                        "karn.parse.expected_handler",
                        t.span,
                        format!(
                            "expected `on` to start a handler, found {}",
                            t.kind.describe()
                        ),
                    ));
                }
                None => {
                    return Err(CompileError::new(
                        "karn.parse.unexpected_eof",
                        self.eof_span(),
                        "expected `}` to close the agent body, found end of file",
                    ));
                }
            }
        }
        let close = self.expect(TokenKind::RBrace, "to close the agent body")?;
        if handlers.is_empty() {
            return Err(CompileError::new(
                "karn.parse.empty_agent",
                kw.span.merge(close.span),
                "an agent must declare at least one handler",
            ));
        }
        Ok(AgentDecl {
            name,
            key_name,
            key_type,
            state_fields,
            state_span,
            handlers,
            documentation: None,
            span: kw.span.merge(close.span),
            trivia: Trivia::default(),
        })
    }

    /// Parse a handler block.
    ///
    /// Service handlers are `on call(args) -> T given C1, C2 { body }`.
    /// Agent handlers are `on call methodName(args) -> T given C1, C2 { body }`,
    /// where the method name is the agent operation invoked on an instance.
    fn parse_handler(&mut self, is_agent: bool) -> Result<Handler, CompileError> {
        let kw = self.expect(TokenKind::On, "to start a handler")?;
        // v0.9: the handler kind is either `call` (an identifier) or `http`
        // (a reserved keyword followed by method + path).
        let kind = if self.peek_kind() == Some(TokenKind::Http) {
            let http_tok = self.bump().unwrap();
            if is_agent {
                return Err(CompileError::new(
                    "karn.parse.http_in_agent",
                    http_tok.span,
                    "`on http` handlers are only valid inside `service` declarations, not `agent`",
                )
                .with_note(
                    "agents persist state and respond to `on call`; HTTP routes belong on services",
                ));
            }
            let method_ident = self.expect_ident(
                "expected an HTTP method (GET, POST, PUT, PATCH, DELETE) after `on http`",
            )?;
            let Some(method) = HttpMethod::from_ident(&method_ident.name) else {
                return Err(CompileError::new(
                    "karn.parse.unknown_http_method",
                    method_ident.span,
                    format!(
                        "unknown HTTP method `{}` — expected one of GET, POST, PUT, PATCH, DELETE",
                        method_ident.name
                    ),
                ));
            };
            let path_tok = self.expect(
                TokenKind::StrLit,
                "expected a path pattern string literal after the HTTP method",
            )?;
            let path = parse_string_literal(self.slice(path_tok.span), path_tok.span)?;
            HandlerKind::Http { method, path }
        } else {
            let kind_ident = self.expect_ident("expected handler kind (e.g. `call`) after `on`")?;
            match kind_ident.name.as_str() {
                "call" => HandlerKind::Call,
                other => {
                    return Err(CompileError::new(
                        "karn.parse.unknown_handler_kind",
                        kind_ident.span,
                        format!(
                            "unknown handler kind `{other}` — supported kinds are `call` and `http`"
                        ),
                    )
                    .with_note(
                        "queue and cron handlers come in v0.10; for now use `on call(...)` or `on http METHOD \"/path\" (...)`",
                    ));
                }
            }
        };
        // Agent handlers have a method name before the parameter list:
        //   on call addItem(item: CartItem) -> ...
        // Service handlers have just the parameter list:
        //   on call(amount: Money) -> ...
        let method_name = if is_agent && self.peek_kind() == Some(TokenKind::Ident) {
            Some(self.expect_ident("as the agent handler operation name")?)
        } else {
            None
        };
        self.expect(TokenKind::LParen, "before the handler parameter list")?;
        let mut params = Vec::new();
        if self.peek_kind() != Some(TokenKind::RParen) {
            params.push(self.parse_param()?);
            while self.eat(TokenKind::Comma).is_some() {
                params.push(self.parse_param()?);
            }
        }
        self.expect(TokenKind::RParen, "to close the handler parameter list")?;
        self.expect(TokenKind::Arrow, "before the handler return type")?;
        let return_type = self.parse_type_ref("as the handler return type")?;
        let mut given = Vec::new();
        if self.peek_kind() == Some(TokenKind::Given) {
            self.bump();
            given.push(self.expect_ident("as a capability name in the `given` clause")?);
            while self.eat(TokenKind::Comma).is_some() {
                given.push(self.expect_ident("as a capability name in the `given` clause")?);
            }
        }
        let body = self.parse_block("to open the handler body")?;
        let span = kw.span.merge(body.span);
        Ok(Handler {
            kind,
            method_name,
            params,
            return_type,
            given,
            body,
            documentation: None,
            span,
            trivia: Trivia::default(),
        })
    }

    /// Parse `Ok(value)` (when `ok` is true) or `Err(error)` (when `ok` is false).
    fn parse_result_expr(&mut self, ok: bool) -> Result<Expr, CompileError> {
        let kw = if ok {
            self.expect(TokenKind::Ok, "to start an `Ok` expression")?
        } else {
            self.expect(TokenKind::Err, "to start an `Err` expression")?
        };
        self.expect(
            TokenKind::LParen,
            if ok { "after `Ok`" } else { "after `Err`" },
        )?;
        let value = self.parse_expr()?;
        let close = self.expect(
            TokenKind::RParen,
            if ok {
                "to close the `Ok` argument"
            } else {
                "to close the `Err` argument"
            },
        )?;
        let span = kw.span.merge(close.span);
        let kind = if ok {
            ExprKind::Ok(Box::new(value))
        } else {
            ExprKind::Err(Box::new(value))
        };
        Ok(Expr { kind, span })
    }
}

/// Parse the body of a lexed double-quoted string literal (the lexeme,
/// including surrounding quotes), applying the v0 escape rules.
fn parse_string_literal(lexeme: &str, span: Span) -> Result<String, CompileError> {
    let bytes = lexeme.as_bytes();
    debug_assert!(bytes.first() == Some(&b'"') && bytes.last() == Some(&b'"'));
    let inner = &lexeme[1..lexeme.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                other => {
                    return Err(CompileError::new(
                        "karn.lex.bad_escape",
                        span,
                        format!(
                            "invalid escape sequence `\\{}` in string literal",
                            other.map(|c| c.to_string()).unwrap_or_default()
                        ),
                    )
                    .with_note("supported escapes: \\n \\t \\\" \\\\"));
                }
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

fn is_reserved_keyword(kind: TokenKind) -> bool {
    use TokenKind::*;
    matches!(
        kind,
        Commons
            | Type
            | Fn
            | Where
            | And
            | True
            | False
            | Int
            | String
            | Bool
            | Let
            | If
            | Else
            | Ok
            | Err
            | Result
            | ValidationError
            | Enum
            | Match
            | Option
            | Record
            | Self_
            | Some
            | None
            | Is
            | Opaque
            | Uses
            | Context
            | Consumes
            | Exports
            | Transparent
            | Agent
            | As
            | Capability
            | Commit
            | Effect
            | Given
            | On
            | Http
            | Provides
            | Service
            | State
            | Assert
            | Expect
            | Mocks
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn parse_str(src: &str) -> Result<Commons, Vec<CompileError>> {
        let toks = tokenize(src).map_err(|e| vec![e])?;
        parse(&toks, src)
    }

    fn parse_recover_str(src: &str) -> (Option<SourceUnit>, Vec<CompileError>) {
        let toks = match tokenize(src) {
            Ok(t) => t,
            Err(e) => return (None, vec![e]),
        };
        parse_unit_with_recovery(&toks, src)
    }

    #[test]
    fn recovery_skips_garbage_between_decls() {
        // Two `type` declarations separated by garbage. Recovery should
        // accept both and report one error for the garbage between them.
        let src = "commons x {\n\
                   type A = Int where NonNegative\n\
                   ??? !!!\n\
                   type B = String where NonEmpty\n\
                   }";
        let (unit, errors) = parse_recover_str(src);
        let unit = unit.expect("recovery should produce a partial AST");
        let SourceUnit::Commons(c) = unit else {
            panic!("expected commons")
        };
        // Both type decls should have been collected despite the garbage.
        let names: Vec<_> = c
            .items
            .iter()
            .map(|i| match i {
                CommonsItem::Type(t) => t.name.name.clone(),
                _ => panic!("expected only types"),
            })
            .collect();
        assert!(
            names.contains(&"A".to_string()) && names.contains(&"B".to_string()),
            "expected both A and B; got {names:?}",
        );
        assert!(!errors.is_empty(), "expected at least one parse error");
    }

    #[test]
    fn recovery_handles_bad_first_decl_then_good_second() {
        // First decl is malformed (missing `=`); second is well-formed.
        let src = "commons x {\n\
                   type A Int where NonNegative\n\
                   type B = String where NonEmpty\n\
                   }";
        let (unit, errors) = parse_recover_str(src);
        let unit = unit.expect("recovery should produce a partial AST");
        let SourceUnit::Commons(c) = unit else {
            panic!("expected commons")
        };
        let names: Vec<_> = c
            .items
            .iter()
            .filter_map(|i| match i {
                CommonsItem::Type(t) => Some(t.name.name.clone()),
                _ => None,
            })
            .collect();
        assert!(
            names.contains(&"B".to_string()),
            "B should be parsed after A's failure; got {names:?}"
        );
        assert!(!errors.is_empty(), "expected at least one parse error");
    }

    #[test]
    fn doc_block_attaches_to_type() {
        let c =
            parse_str("commons x {\n---\nA descriptive doc.\n---\ntype T = Int where Positive\n}")
                .unwrap();
        let CommonsItem::Type(t) = &c.items[0] else {
            panic!()
        };
        assert!(t.documentation.is_some());
        assert!(
            t.documentation
                .as_ref()
                .unwrap()
                .contains("A descriptive doc.")
        );
    }

    #[test]
    fn fragment_form_parses() {
        let c = parse_str("commons x.y\n\ntype T = Int where NonNegative\n").unwrap();
        assert_eq!(c.form, CommonsForm::Fragment);
        assert_eq!(c.items.len(), 1);
    }

    #[test]
    fn uses_parses() {
        let c = parse_str("commons x\n\nuses other.lib\n").unwrap();
        assert_eq!(c.uses.len(), 1);
        assert_eq!(c.uses[0].target.joined(), "other.lib");
    }

    fn parse_unit_str(src: &str) -> Result<SourceUnit, Vec<CompileError>> {
        let toks = tokenize(src).map_err(|e| vec![e])?;
        parse_unit(&toks, src)
    }

    #[test]
    fn minimal_context_parses() {
        let u = parse_unit_str("context commerce.orders {}").unwrap();
        let SourceUnit::Context(c) = u else {
            panic!("expected context");
        };
        assert_eq!(c.name.joined(), "commerce.orders");
        assert!(c.items.is_empty());
    }

    #[test]
    fn context_consumes_and_exports_parse() {
        let src = "context commerce.orders {\n  uses commerce.money\n  consumes commerce.payment\n  exports opaque { OrderId }\n  exports transparent { OrderError }\n  type OrderId = String where Matches(\"ORD-[0-9]+\")\n  type OrderError = enum { CartEmpty, BadInput }\n}";
        let u = parse_unit_str(src).unwrap();
        let SourceUnit::Context(c) = u else { panic!() };
        assert_eq!(c.uses.len(), 1);
        assert_eq!(c.consumes.len(), 1);
        assert_eq!(c.exports.len(), 2);
        assert_eq!(c.exports[0].visibility, Visibility::Opaque);
        assert_eq!(c.exports[1].visibility, Visibility::Transparent);
    }

    #[test]
    fn context_fragment_form_parses() {
        let src = "context x.y\n\nuses other.lib\nconsumes other.ctx\nexports opaque { T }\n\ntype T = Int where NonNegative\n";
        let u = parse_unit_str(src).unwrap();
        let SourceUnit::Context(c) = u else { panic!() };
        assert_eq!(c.form, CommonsForm::Fragment);
        assert_eq!(c.uses.len(), 1);
        assert_eq!(c.consumes.len(), 1);
        assert_eq!(c.exports.len(), 1);
    }

    #[test]
    fn opaque_type_parses() {
        let c = parse_str("commons x { type T = opaque Int where NonNegative }").unwrap();
        let CommonsItem::Type(t) = &c.items[0] else {
            panic!()
        };
        assert!(matches!(t.body, TypeBody::Opaque { .. }));
    }

    #[test]
    fn empty_commons() {
        let c = parse_str("commons fitness.units {}").unwrap();
        assert_eq!(c.name.joined(), "fitness.units");
        assert!(c.items.is_empty());
    }

    #[test]
    fn one_type_decl() {
        let c = parse_str("commons x { type Metres = Int where NonNegative }").unwrap();
        assert_eq!(c.items.len(), 1);
        let CommonsItem::Type(t) = &c.items[0] else {
            panic!()
        };
        assert_eq!(t.name.name, "Metres");
        match &t.body {
            TypeBody::Refined {
                base, refinement, ..
            } => {
                assert_eq!(*base, BaseType::Int);
                assert!(refinement.is_some());
            }
            _ => panic!("expected refined body"),
        }
    }

    #[test]
    fn function_decl() {
        let c = parse_str("commons x { fn add(a: Int, b: Int) -> Int { a + b } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert_eq!(f.name.ident().name, "add");
        assert_eq!(f.params.len(), 2);
    }

    #[test]
    fn chained_comparison_is_error() {
        let errs = parse_str("commons x { fn f(a: Int, b: Int, c: Int) -> Bool { a < b < c } }")
            .unwrap_err();
        assert_eq!(errs[0].category, "karn.parse.non_associative");
    }

    #[test]
    fn chained_equality_is_error() {
        let errs = parse_str("commons x { fn f(a: Int, b: Int, c: Int) -> Bool { a == b == c } }")
            .unwrap_err();
        assert_eq!(errs[0].category, "karn.parse.non_associative");
    }

    #[test]
    fn let_statement_parses() {
        let c = parse_str("commons x { fn f(n: Int) -> Int { let y = n + 1\n y } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert_eq!(f.body.statements.len(), 1);
        match &f.body.statements[0] {
            Statement::Let(l) => {
                assert_eq!(l.name.name, "y");
                assert!(l.type_annot.is_none());
            }
            _ => panic!("expected a pure `let` statement"),
        }
    }

    #[test]
    fn let_with_annotation() {
        let c = parse_str("commons x { fn f(n: Int) -> Int { let y: Int = n\n y } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        match &f.body.statements[0] {
            Statement::Let(l) => assert!(l.type_annot.is_some()),
            _ => panic!("expected a pure `let` statement"),
        }
    }

    #[test]
    fn if_else_parses_as_expression() {
        let c = parse_str("commons x { fn f(b: Bool) -> Int { if b { 1 } else { 0 } } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert!(matches!(f.body.tail.kind, ExprKind::If { .. }));
    }

    #[test]
    fn else_if_chain_parses() {
        let c = parse_str(
            "commons x { fn f(n: Int) -> Int { if n < 0 { -1 } else if n == 0 { 0 } else { 1 } } }",
        )
        .unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        let ExprKind::If { else_block, .. } = &f.body.tail.kind else {
            panic!()
        };
        // The else-branch is a block whose tail is another `If`.
        assert!(else_block.statements.is_empty());
        assert!(matches!(else_block.tail.kind, ExprKind::If { .. }));
    }

    #[test]
    fn ok_and_err_parse_as_expressions() {
        let c = parse_str("commons x { fn f(n: Int) -> Result[Int, String] { Ok(n) } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert!(matches!(f.body.tail.kind, ExprKind::Ok(_)));

        let c =
            parse_str("commons x { fn f(n: Int) -> Result[Int, String] { Err(\"x\") } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert!(matches!(f.body.tail.kind, ExprKind::Err(_)));
    }

    #[test]
    fn question_postfix_parses() {
        let c = parse_str(
            "commons x { type T = Int where Positive\n fn f(n: Int) -> Result[T, ValidationError] { let x = T.of(n)?\n Ok(x) } }",
        )
        .unwrap();
        let CommonsItem::Fn(f) = &c.items[1] else {
            panic!()
        };
        let Statement::Let(l) = &f.body.statements[0] else {
            panic!("expected a pure `let` statement");
        };
        assert!(matches!(l.value.kind, ExprKind::Question(_)));
    }

    #[test]
    fn constructor_call_parses() {
        let c = parse_str(
            "commons x { type T = Int where Positive\n fn f(n: Int) -> Result[T, ValidationError] { T.of(n) } }",
        )
        .unwrap();
        let CommonsItem::Fn(f) = &c.items[1] else {
            panic!()
        };
        // v0.2: T.of(n) parses as a MethodCall with receiver Ident("T"); the
        // checker reinterprets it as a static call by noticing T is a type.
        let ExprKind::MethodCall {
            receiver, method, ..
        } = &f.body.tail.kind
        else {
            panic!("expected MethodCall, got {:?}", f.body.tail.kind)
        };
        let ExprKind::Ident(id) = &receiver.kind else {
            panic!("expected receiver Ident");
        };
        assert_eq!(id.name, "T");
        assert_eq!(method.name, "of");
    }

    #[test]
    fn result_type_ref_parses() {
        let c = parse_str("commons x { fn f(n: Int) -> Result[Int, String] { Ok(n) } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert!(matches!(f.return_type, TypeRef::Result(_, _, _)));
    }

    #[test]
    fn result_missing_arg_count_errors() {
        let errs = parse_str("commons x { fn f(n: Int) -> Result[Int] { Ok(n) } }").unwrap_err();
        assert_eq!(errs[0].category, "karn.parse.generic_arg_count");
    }

    #[test]
    fn field_access_parses_in_v0_2() {
        // v0.2: field access is supported (the type checker validates the
        // field exists on the receiver's type). Parser-level acceptance:
        let c =
            parse_str("commons x { type R = { foo: Int }\n fn f(r: R) -> Int { r.foo } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[1] else {
            panic!()
        };
        assert!(matches!(f.body.tail.kind, ExprKind::FieldAccess { .. }));
    }

    // -- v1.1 trivia attachment --

    #[test]
    fn leading_line_comment_attaches_to_next_decl() {
        let src = "commons x {\n-- explain the type\ntype T = Int where NonNegative\n}";
        let c = parse_str(src).unwrap();
        let CommonsItem::Type(t) = &c.items[0] else {
            panic!()
        };
        assert_eq!(t.trivia.leading, vec![" explain the type".to_string()]);
        assert!(t.trivia.trailing.is_none());
    }

    #[test]
    fn trailing_line_comment_attaches_to_prev_decl() {
        let src = "commons x {\ntype T = Int where NonNegative  -- trailing note\n}";
        let c = parse_str(src).unwrap();
        let CommonsItem::Type(t) = &c.items[0] else {
            panic!()
        };
        assert!(t.trivia.leading.is_empty());
        assert_eq!(t.trivia.trailing.as_deref(), Some(" trailing note"));
    }

    #[test]
    fn grouped_leading_comments_attach_together() {
        let src = "commons x {\n-- one\n-- two\n-- three\ntype T = Int where Positive\n}";
        let c = parse_str(src).unwrap();
        let CommonsItem::Type(t) = &c.items[0] else {
            panic!()
        };
        assert_eq!(
            t.trivia.leading,
            vec![" one".to_string(), " two".to_string(), " three".to_string()],
        );
    }

    #[test]
    fn comment_with_doc_block_keeps_both() {
        // Both `-- intro` and the doc block should attach to the type decl.
        let src = "commons x {\n-- intro\n---\ndocs\n---\ntype T = Int where Positive\n}";
        let c = parse_str(src).unwrap();
        let CommonsItem::Type(t) = &c.items[0] else {
            panic!()
        };
        assert_eq!(t.trivia.leading, vec![" intro".to_string()]);
        assert_eq!(t.documentation.as_deref(), Some("docs"));
    }

    #[test]
    fn comment_before_let_statement_attaches() {
        let src = "commons x {\nfn f(n: Int) -> Int {\n-- pick a value\nlet y = n + 1\ny\n}\n}";
        let c = parse_str(src).unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        let Statement::Let(l) = &f.body.statements[0] else {
            panic!()
        };
        assert_eq!(l.trivia.leading, vec![" pick a value".to_string()]);
    }

    #[test]
    fn comment_before_tail_attaches_to_block_tail() {
        let src = "commons x {\nfn f(n: Int) -> Int {\nlet y = n + 1\n-- result\ny\n}\n}";
        let c = parse_str(src).unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert_eq!(f.body.tail_leading_comments, vec![" result".to_string()],);
    }

    #[test]
    fn trailing_file_comment_becomes_unit_trailing() {
        // A comment after the last item but before EOF (fragment form)
        // becomes the commons body's trailing comments so the formatter
        // can preserve it.
        let src = "commons x\n\ntype T = Int where Positive\n-- afterword\n";
        let c = parse_str(src).unwrap();
        assert_eq!(c.trailing_comments, vec![" afterword".to_string()]);
    }
}
