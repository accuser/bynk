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
mod declarations;
mod expressions;

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
                "contexts must be compiled as part of a project — pass the source directory, e.g. `karnc compile --target bundle --output out src`",
            ),
        ]),
        SourceUnit::Test(t) => Err(vec![
            CompileError::new(
                "karn.parse.unexpected_test",
                t.span,
                "expected a `commons` declaration but found a `test` declaration",
            )
            .with_note(
                "tests must be compiled as part of a project — pass the source directory, e.g. `karnc compile --target bundle --output out src`",
            ),
        ]),
        SourceUnit::Integration(i) => Err(vec![
            CompileError::new(
                "karn.parse.unexpected_test",
                i.span,
                "expected a `commons` declaration but found an integration test",
            )
            .with_note(
                "tests must be compiled as part of a project — pass the source directory, e.g. `karnc compile --target bundle --output out src`",
            ),
        ]),
        SourceUnit::Adapter(a) => Err(vec![
            CompileError::new(
                "karn.parse.unexpected_adapter",
                a.span,
                "expected a `commons` declaration but found an `adapter` declaration",
            )
            .with_note(
                "adapters must be compiled as part of a project — pass the source directory, e.g. `karnc compile --target bundle --output out src`",
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

/// A signed numeric literal in refinement-bound position (v0.21): `InRange`
/// bounds are either both `Int` or both `Float`.
enum SignedNumLit {
    Int(i64),
    Float(FloatBound),
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

    /// True when the next token sits on a later line than `prev`. Used to
    /// keep a `[` that opens a new line out of the postfix type-application
    /// form: `f` followed by `[1, 2]` on the next line is an identifier and
    /// a list literal, not `f[…]` (v0.20b).
    fn next_token_on_new_line(&self, prev: Span) -> bool {
        match self.peek() {
            Some(t) if prev.end <= t.span.start => {
                self.source[prev.end..t.span.start].contains('\n')
            }
            _ => false,
        }
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

    // -- type declarations --

    fn parse_type_decl(&mut self) -> Result<TypeDecl, CompileError> {
        let kw = self.expect(TokenKind::Type, "to start a type declaration")?;
        let name = self.expect_ident("after `type`")?;
        // v0.20a (Open-narrow): generic *type* declarations stay rejected —
        // `List`/`Map` (built-in) remain the only generic types.
        if self.peek_kind() == Some(TokenKind::LBracket) {
            let open = self.bump().unwrap();
            return Err(CompileError::new(
                "karn.generics.no_generic_types",
                open.span,
                format!(
                    "type `{}` declares type parameters — generic type declarations are not in v0.20a (type parameters belong to functions)",
                    name.name
                ),
            )
            .with_note("only functions take type parameters (`fn name[A, B](…)`); the built-in generic types are fixed"));
        }
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
        // v0.11: an optional `= <expr>` initial-value, used by agent state
        // fields. Parsed for every record field; the checker restricts where it
        // is meaningful.
        let mut init = None;
        if self.eat(TokenKind::Eq).is_some() {
            let e = self.parse_expr()?;
            end_span = e.span;
            init = Some(e);
        }
        Ok(RecordField {
            name: name.clone(),
            type_ref,
            refinement,
            init,
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
                TokenKind::Float => {
                    self.bump();
                    Ok((BaseType::Float, t.span))
                }
                _ => Err(CompileError::new(
                    "karn.parse.expected_base_type",
                    t.span,
                    format!(
                        "expected `Int`, `String`, `Bool`, or `Float`, found {}",
                        t.kind.describe()
                    ),
                )
                .with_note("type declarations are refined base types in v0")),
            },
            None => Err(CompileError::new(
                "karn.parse.unexpected_eof",
                self.eof_span(),
                "expected `Int`, `String`, `Bool`, or `Float`, found end of file",
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
                let lo = self.parse_signed_num_literal("as the lower bound of `InRange`")?;
                self.expect(TokenKind::Comma, "between `InRange` arguments")?;
                let hi = self.parse_signed_num_literal("as the upper bound of `InRange`")?;
                let close = self.expect(TokenKind::RParen, "after the `InRange` arguments")?;
                let kind = match (lo, hi) {
                    (SignedNumLit::Int(a), SignedNumLit::Int(b)) => PredKind::InRange(a, b),
                    (SignedNumLit::Float(a), SignedNumLit::Float(b)) => PredKind::InRangeF(a, b),
                    _ => {
                        return Err(CompileError::new(
                            "karn.types.no_numeric_coercion",
                            start.merge(close.span),
                            "`InRange` bounds mix an `Int` literal and a `Float` literal",
                        )
                        .with_note(
                            "both bounds must be the same numeric type — \
                             write `InRange(0, 1)` or `InRange(0.0, 1.0)`",
                        ));
                    }
                };
                (kind, close.span)
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

    /// A signed numeric literal in refinement-bound position (v0.21):
    /// either an `Int` or a `Float` literal, optionally negated. The float
    /// form keeps its (signed) lexeme for byte-stable emission.
    fn parse_signed_num_literal(&mut self, ctx: &str) -> Result<SignedNumLit, CompileError> {
        let neg = self.eat(TokenKind::Minus).is_some();
        match self.peek() {
            Some(t) if t.kind == TokenKind::IntLit => {
                self.bump();
                let slice = self.slice(t.span);
                let n: i64 = slice.parse().map_err(|_| {
                    CompileError::new(
                        "karn.lex.integer_overflow",
                        t.span,
                        format!(
                            "integer literal `{slice}` is out of range for a 64-bit signed integer"
                        ),
                    )
                })?;
                Ok(SignedNumLit::Int(if neg { -n } else { n }))
            }
            Some(t) if t.kind == TokenKind::FloatLit => {
                self.bump();
                let slice = self.slice(t.span);
                // tokenize() already rejected non-finite literals.
                let v: f64 = slice.parse().unwrap_or(f64::NAN);
                let (value, lexeme) = if neg {
                    (-v, format!("-{slice}"))
                } else {
                    (v, slice.to_string())
                };
                Ok(SignedNumLit::Float(FloatBound { value, lexeme }))
            }
            Some(t) => Err(CompileError::new(
                "karn.parse.expected_token",
                t.span,
                format!(
                    "expected a numeric literal {ctx}, found {}",
                    t.kind.describe()
                ),
            )),
            None => Err(CompileError::new(
                "karn.parse.unexpected_eof",
                self.eof_span(),
                format!("expected a numeric literal {ctx}, found end of file"),
            )),
        }
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
        // v0.20a: optional `[A, B]` type parameters (free functions only —
        // generic methods are checked semantically; bounds are rejected here
        // with `karn.generics.no_bounds`).
        let mut type_params = Vec::new();
        if self.peek_kind() == Some(TokenKind::LBracket) {
            self.bump();
            loop {
                let p = self.expect_ident("as a type parameter name")?;
                if self.peek_kind() == Some(TokenKind::Colon) {
                    let colon = self.bump().unwrap();
                    return Err(CompileError::new(
                        "karn.generics.no_bounds",
                        colon.span,
                        format!(
                            "type parameter `{}` carries a bound — bounded generics are not in v0.20a",
                            p.name
                        ),
                    )
                    .with_note("type parameters are unconstrained; remove the `: …` bound"));
                }
                type_params.push(TypeParam {
                    span: p.span,
                    name: p,
                });
                if self.eat(TokenKind::Comma).is_none() {
                    break;
                }
            }
            self.expect(TokenKind::RBracket, "to close the type-parameter list")?;
        }
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
            type_params,
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

    /// v0.20a: at an `LParen` in primary-expression position, decide whether
    /// a lambda follows: scan to the matching `)` counting paren depth, then
    /// peek one token for `=>`. Terminates at EOF; cost is the distance to
    /// the matching paren (the same class as the record-construction
    /// lookahead).
    fn lambda_ahead(&self) -> bool {
        let mut n = 1;
        let mut depth = 1u32;
        loop {
            match self.tokens.get(self.pos + n).map(|t| t.kind) {
                Some(TokenKind::LParen) => depth += 1,
                Some(TokenKind::RParen) => {
                    depth -= 1;
                    if depth == 0 {
                        return self.tokens.get(self.pos + n + 1).map(|t| t.kind)
                            == Some(TokenKind::FatArrow);
                    }
                }
                None => return false,
                _ => {}
            }
            n += 1;
        }
    }

    /// v0.20a: parse `(params) => expr | { block }`. Param annotations are
    /// optional (`(o: Order) => …` / `(o) => …`); the unannotated form relies
    /// on an expected function type at the use site (checked semantically).
    fn parse_lambda(&mut self) -> Result<Expr, CompileError> {
        let open = self.bump().unwrap(); // `(`
        let mut params = Vec::new();
        if self.peek_kind() != Some(TokenKind::RParen) {
            loop {
                let name = self.expect_ident("as a lambda parameter name")?;
                let mut p_span = name.span;
                let type_ref = if self.eat(TokenKind::Colon).is_some() {
                    let t = self.parse_type_ref("as the lambda parameter type")?;
                    p_span = p_span.merge(t.span());
                    Some(t)
                } else {
                    None
                };
                params.push(LambdaParam {
                    name,
                    type_ref,
                    span: p_span,
                });
                if self.eat(TokenKind::Comma).is_none() {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen, "to close the lambda parameter list")?;
        self.expect(TokenKind::FatArrow, "after the lambda parameter list")?;
        let body = if self.peek_kind() == Some(TokenKind::LBrace) {
            let block = self.parse_block("as the lambda body")?;
            let span = block.span;
            Expr {
                kind: ExprKind::Block(block),
                span,
            }
        } else {
            self.parse_expr()?
        };
        let span = open.span.merge(body.span);
        Ok(Expr {
            kind: ExprKind::Lambda(LambdaExpr {
                params,
                body: Box::new(body),
                span,
            }),
            span,
        })
    }

    /// v0.20a: a type reference — an atom, a parenthesised group, or a
    /// function type. `->` is **right-associative** (`A -> B -> C` is
    /// `A -> (B -> C)`). A parenthesised list is a function-type parameter
    /// list when followed by `->`; a grouping when it holds exactly one type
    /// (`(A -> B)` — unwrapped, so the formatter canonicalises); and the unit
    /// type when empty and arrow-free. The disambiguation is deferred to the
    /// arrow peek, so no extra lookahead is needed.
    fn parse_type_ref(&mut self, ctx: &str) -> Result<TypeRef, CompileError> {
        enum Group {
            Unit(Span),
            Single(TypeRef, Span),
            Multi(Vec<TypeRef>, Span),
        }
        let group = if self.peek_kind() == Some(TokenKind::LParen) {
            let open = self.bump().unwrap();
            if self.peek_kind() == Some(TokenKind::RParen) {
                let close = self.bump().unwrap();
                Group::Unit(open.span.merge(close.span))
            } else {
                let mut items = vec![self.parse_type_ref(ctx)?];
                while self.eat(TokenKind::Comma).is_some() {
                    items.push(self.parse_type_ref(ctx)?);
                }
                let close = self.expect(TokenKind::RParen, "to close the parenthesised type")?;
                let span = open.span.merge(close.span);
                if items.len() == 1 {
                    Group::Single(items.pop().unwrap(), span)
                } else {
                    Group::Multi(items, span)
                }
            }
        } else {
            let t = self.parse_type_atom(ctx)?;
            let span = t.span();
            Group::Single(t, span)
        };
        if self.peek_kind() == Some(TokenKind::Arrow) {
            self.bump();
            // Recursing through parse_type_ref makes `->` right-associative.
            let ret = self.parse_type_ref(ctx)?;
            let (params, start) = match group {
                Group::Unit(s) => (Vec::new(), s),
                Group::Single(t, s) => (vec![t], s),
                Group::Multi(ts, s) => (ts, s),
            };
            let span = start.merge(ret.span());
            return Ok(TypeRef::Fn(params, Box::new(ret), span));
        }
        match group {
            Group::Unit(s) => Ok(TypeRef::Unit(s)),
            Group::Single(t, _) => Ok(t),
            Group::Multi(_, s) => Err(CompileError::new(
                "karn.parse.expected_token",
                s,
                "expected `->` after a parenthesised parameter list — a bare `(A, B)` is not a type",
            )),
        }
    }

    fn parse_type_atom(&mut self, ctx: &str) -> Result<TypeRef, CompileError> {
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
                TokenKind::Float => {
                    self.bump();
                    Ok(TypeRef::Base(BaseType::Float, t.span))
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
                TokenKind::JsonError => {
                    self.bump();
                    Ok(TypeRef::JsonError(t.span))
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
                    // v0.20b: `List` and `Map` are predeclared built-in generics.
                    if name == "List" {
                        if self.peek_kind() != Some(TokenKind::LBracket) {
                            return Err(CompileError::new(
                                "karn.parse.expected_token",
                                t.span,
                                "the built-in `List` type requires one type argument: `List[T]`",
                            ));
                        }
                        self.bump();
                        let arg = self.parse_type_ref("as the `List` type argument")?;
                        let close =
                            self.expect(TokenKind::RBracket, "to close the `List` type argument")?;
                        return Ok(TypeRef::List(Box::new(arg), t.span.merge(close.span)));
                    }
                    if name == "Map" {
                        if self.peek_kind() != Some(TokenKind::LBracket) {
                            return Err(CompileError::new(
                                "karn.parse.expected_token",
                                t.span,
                                "the built-in `Map` type requires two type arguments: `Map[K, V]`",
                            ));
                        }
                        self.bump();
                        let arg_k = self.parse_type_ref("as the first `Map` type argument")?;
                        if self.peek_kind() == Some(TokenKind::RBracket) {
                            let close = self.bump().unwrap();
                            return Err(CompileError::new(
                                "karn.parse.generic_arg_count",
                                t.span.merge(close.span),
                                "the built-in `Map` type requires two type arguments: `Map[K, V]`",
                            ));
                        }
                        self.expect(TokenKind::Comma, "between the `Map` type arguments")?;
                        let arg_v = self.parse_type_ref("as the second `Map` type argument")?;
                        let close =
                            self.expect(TokenKind::RBracket, "to close the `Map` type arguments")?;
                        return Ok(TypeRef::Map(
                            Box::new(arg_k),
                            Box::new(arg_v),
                            t.span.merge(close.span),
                        ));
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
}

impl<'a> Parser<'a> {
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

    /// Parse one capability reference in a `given` clause (v0.15 §3.2). A bare
    /// name (`Cap`) is a local capability; a dotted name (`B.Cap` /
    /// `platform.time.Clock`) refers to a capability provided by a consumed
    /// context — every segment but the last forms the context prefix.
    fn parse_cap_ref(&mut self) -> Result<CapRef, CompileError> {
        let role = "as a capability name in the `given` clause";
        let mut parts = vec![self.expect_ident(role)?];
        while self.peek_kind() == Some(TokenKind::Dot) {
            self.bump();
            parts.push(self.expect_ident(role)?);
        }
        let name = parts.pop().unwrap();
        let context = if parts.is_empty() {
            None
        } else {
            let qspan = parts
                .first()
                .unwrap()
                .span
                .merge(parts.last().unwrap().span);
            Some(QualifiedName { parts, span: qspan })
        };
        let span = context
            .as_ref()
            .map(|q| q.span.merge(name.span))
            .unwrap_or(name.span);
        Ok(CapRef {
            context,
            name,
            span,
        })
    }

    fn parse_provider_decl(&mut self) -> Result<ProviderDecl, CompileError> {
        let kw = self.expect(TokenKind::Provides, "to start a provider declaration")?;
        let capability = self.expect_ident("after `provides`")?;
        self.expect(TokenKind::Eq, "after the capability name")?;
        let provider_name = self.expect_ident("as the provider name")?;
        // v0.12: optional `given C1, C2` — capabilities the provider depends on.
        // v0.15: a dependency may be a cross-context capability (`given B.Cap`).
        let mut given = Vec::new();
        if self.peek_kind() == Some(TokenKind::Given) {
            self.bump();
            given.push(self.parse_cap_ref()?);
            while self.eat(TokenKind::Comma).is_some() {
                given.push(self.parse_cap_ref()?);
            }
        }
        // v0.17: a provider with **no** brace block is an *external* provider —
        // its implementation is supplied by an adapter's binding. The absence of
        // the brace block (not an empty one) is the signal. Whether this form is
        // legal here (adapter) or not (context) is decided by the checker, so the
        // parser accepts both shapes structurally.
        if self.peek_kind() != Some(TokenKind::LBrace) {
            let end = given.last().map(|g| g.span).unwrap_or(provider_name.span);
            return Ok(ProviderDecl {
                capability,
                provider_name,
                given,
                ops: Vec::new(),
                external: true,
                documentation: None,
                span: kw.span.merge(end),
                trivia: Trivia::default(),
            });
        }
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
            given,
            ops,
            external: false,
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
        } else if self.peek_kind() == Some(TokenKind::Cron) {
            let cron_tok = self.bump().unwrap();
            if is_agent {
                return Err(CompileError::new(
                    "karn.parse.cron_in_agent",
                    cron_tok.span,
                    "`on cron` handlers are only valid inside `service` declarations, not `agent`",
                )
                .with_note(
                    "agents persist state and respond to `on call`; scheduled tasks belong on services",
                ));
            }
            let expr_tok = self.expect(
                TokenKind::StrLit,
                "expected a cron expression string literal after `on cron`",
            )?;
            let expr = parse_string_literal(self.slice(expr_tok.span), expr_tok.span)?;
            HandlerKind::Cron { expr }
        } else if self.peek_kind() == Some(TokenKind::Queue) {
            let queue_tok = self.bump().unwrap();
            if is_agent {
                return Err(CompileError::new(
                    "karn.parse.queue_in_agent",
                    queue_tok.span,
                    "`on queue` handlers are only valid inside `service` declarations, not `agent`",
                )
                .with_note(
                    "agents persist state and respond to `on call`; queue consumers belong on services",
                ));
            }
            let name_tok = self.expect(
                TokenKind::StrLit,
                "expected a queue name string literal after `on queue`",
            )?;
            let name = parse_string_literal(self.slice(name_tok.span), name_tok.span)?;
            HandlerKind::Queue { name }
        } else {
            let kind_ident = self.expect_ident("expected handler kind (e.g. `call`) after `on`")?;
            match kind_ident.name.as_str() {
                "call" => HandlerKind::Call,
                other => {
                    return Err(CompileError::new(
                        "karn.parse.unknown_handler_kind",
                        kind_ident.span,
                        format!(
                            "unknown handler kind `{other}` — supported kinds are `call`, `http`, `cron`, and `queue`"
                        ),
                    )
                    .with_note(
                        "use `on call(...)`, `on http METHOD \"/path\" (...)`, `on cron \"expr\" (...)`, or `on queue \"name\" (message: T)`",
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
            given.push(self.parse_cap_ref()?);
            while self.eat(TokenKind::Comma).is_some() {
                given.push(self.parse_cap_ref()?);
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
        assert_eq!(c.exports[0].kind, ExportKind::Type(Visibility::Opaque));
        assert_eq!(c.exports[1].kind, ExportKind::Type(Visibility::Transparent));
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
