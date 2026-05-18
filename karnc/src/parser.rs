//! Hand-written recursive-descent parser for Karn v0.
//!
//! Token grammar in spec §4. The expression parser uses one function per
//! precedence level (§4.4). Errors carry spans and short fix-oriented
//! messages; the parser does not currently attempt synchronisation, which
//! means at most one parse error is reported per compilation.

use crate::ast::*;
use crate::error::CompileError;
use crate::lexer::{Token, TokenKind};
use crate::span::Span;

/// Parse a token slice into a [`Commons`] AST.
pub fn parse(tokens: &[Token], source: &str) -> Result<Commons, Vec<CompileError>> {
    let mut p = Parser::new(tokens, source);
    match p.parse_commons() {
        Ok(c) => {
            if let Some(extra) = p.peek() {
                Err(vec![
                    CompileError::new(
                        "karn.parse.extra_tokens",
                        extra.span,
                        "unexpected token after commons declaration",
                    )
                    .with_note("a commons file contains exactly one `commons` declaration"),
                ])
            } else {
                Ok(c)
            }
        }
        Err(e) => Err(vec![e]),
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    source: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token], source: &'a str) -> Self {
        Self {
            tokens,
            source,
            pos: 0,
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

    fn parse_commons(&mut self) -> Result<Commons, CompileError> {
        let start = self.expect(TokenKind::Commons, "to start the commons declaration")?;
        let name = self.parse_qualified_name()?;
        self.expect(TokenKind::LBrace, "after the commons name")?;
        let mut items = Vec::new();
        while let Some(t) = self.peek() {
            match t.kind {
                TokenKind::RBrace => break,
                TokenKind::Type => items.push(CommonsItem::Type(self.parse_type_decl()?)),
                TokenKind::Fn => items.push(CommonsItem::Fn(self.parse_fn_decl()?)),
                _ => {
                    return Err(CompileError::new(
                        "karn.parse.expected_item",
                        t.span,
                        format!(
                            "expected `type` or `fn` declaration, found {}",
                            t.kind.describe()
                        ),
                    )
                    .with_note(
                        "the body of a commons contains zero or more `type` or `fn` declarations",
                    ));
                }
            }
        }
        let end = self.expect(TokenKind::RBrace, "to close the commons body")?;
        Ok(Commons {
            name,
            items,
            span: start.span.merge(end.span),
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
        let (base, base_span) = self.parse_base_type()?;
        let mut refinement = None;
        let mut end_span = base_span;
        if self.eat(TokenKind::Where).is_some() {
            let r = self.parse_refinement()?;
            end_span = r.span;
            refinement = Some(r);
        }
        Ok(TypeDecl {
            name,
            base,
            base_span,
            refinement,
            span: kw.span.merge(end_span),
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
        let name = self.expect_ident("after `fn`")?;
        self.expect(TokenKind::LParen, "after the function name")?;
        let mut params = Vec::new();
        if self.peek_kind() != Some(TokenKind::RParen) {
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
            span,
        })
    }

    /// Parse a brace-delimited block: `{ statement* expr }` (v0.1 §3.1).
    fn parse_block(&mut self, ctx: &str) -> Result<Block, CompileError> {
        let open = self.expect(TokenKind::LBrace, ctx)?;
        let mut statements = Vec::new();
        // Loop: parse statements until we hit something that's not a statement.
        // In v0.1 the only statement is `let`. Anything else marks the tail expression.
        while self.peek_kind() == Some(TokenKind::Let) {
            statements.push(self.parse_statement()?);
        }
        let tail = self.parse_expr()?;
        let close = self.expect(TokenKind::RBrace, "to close the block")?;
        Ok(Block {
            statements,
            tail: Box::new(tail),
            span: open.span.merge(close.span),
        })
    }

    fn parse_statement(&mut self) -> Result<Statement, CompileError> {
        let kw = self.expect(TokenKind::Let, "to start a let statement")?;
        let name = self.expect_ident("after `let`")?;
        let type_annot = if self.eat(TokenKind::Colon).is_some() {
            Some(self.parse_type_ref("as the let-binding's type annotation")?)
        } else {
            None
        };
        self.expect(
            TokenKind::Eq,
            "after the let-binding's name (or type annotation)",
        )?;
        let value = self.parse_expr()?;
        let span = kw.span.merge(value.span);
        Ok(Statement::Let(LetStmt {
            name,
            type_annot,
            value,
            span,
        }))
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
                TokenKind::Ident => {
                    self.bump();
                    Ok(TypeRef::Named(Ident {
                        name: self.slice(t.span).to_string(),
                        span: t.span,
                    }))
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

    /// Parse a primary expression and then apply postfix operators (`?`
    /// in v0.1; `.identifier` is rejected with an error suggesting it
    /// is reserved for v0.2 records).
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
                    // Field access (postfix `.identifier`) is reserved for
                    // future record support; reject it at parse time.
                    let dot = self.bump().unwrap();
                    let _ = self.expect_ident("after `.` in a field access");
                    return Err(CompileError::new(
                        "karn.parse.field_access_unsupported",
                        dot.span,
                        "field access is not supported in v0.1",
                    )
                    .with_note(
                        "records and field access arrive in v0.2; \
                         constructor calls use the form `TypeName.of(value)`",
                    ));
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
                let inner = self.parse_expr()?;
                let close =
                    self.expect(TokenKind::RParen, "to close the parenthesised expression")?;
                Ok(Expr {
                    kind: ExprKind::Paren(Box::new(inner)),
                    span: t.span.merge(close.span),
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
                } else if self.peek_kind() == Some(TokenKind::Dot)
                    && self.looks_like_qualified_call()
                {
                    // Qualified constructor call: `TypeName.method(args)` (v0.1).
                    self.bump();
                    let method = self.expect_ident("after `.` in a constructor call")?;
                    self.expect(TokenKind::LParen, "after the constructor method name")?;
                    let mut args = Vec::new();
                    if self.peek_kind() != Some(TokenKind::RParen) {
                        args.push(self.parse_expr()?);
                        while self.eat(TokenKind::Comma).is_some() {
                            args.push(self.parse_expr()?);
                        }
                    }
                    let close =
                        self.expect(TokenKind::RParen, "to close the constructor argument list")?;
                    Ok(Expr {
                        kind: ExprKind::ConstructorCall {
                            type_name: ident.clone(),
                            method,
                            args,
                        },
                        span: ident.span.merge(close.span),
                    })
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
    /// Lookahead helper: when the current token is `.`, check whether
    /// the next two tokens are `Ident` followed by `(` (the qualified
    /// constructor-call shape `T.method(args)`). Otherwise the `.` is
    /// a postfix field access, which is rejected later.
    fn looks_like_qualified_call(&self) -> bool {
        debug_assert_eq!(self.peek_kind(), Some(TokenKind::Dot));
        let after_dot = self.tokens.get(self.pos + 1);
        let after_ident = self.tokens.get(self.pos + 2);
        matches!(after_dot.map(|t| t.kind), Some(TokenKind::Ident))
            && matches!(after_ident.map(|t| t.kind), Some(TokenKind::LParen))
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
        assert_eq!(t.base, BaseType::Int);
        assert!(t.refinement.is_some());
    }

    #[test]
    fn function_decl() {
        let c = parse_str("commons x { fn add(a: Int, b: Int) -> Int { a + b } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert_eq!(f.name.name, "add");
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
        let Statement::Let(l) = &f.body.statements[0];
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
        let ExprKind::ConstructorCall {
            type_name, method, ..
        } = &f.body.tail.kind
        else {
            panic!("expected ConstructorCall, got {:?}", f.body.tail.kind)
        };
        assert_eq!(type_name.name, "T");
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
    fn field_access_unsupported() {
        let errs = parse_str("commons x { fn f(n: Int) -> Int { n.foo } }").unwrap_err();
        assert_eq!(errs[0].category, "karn.parse.field_access_unsupported");
    }
}
