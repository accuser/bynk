//! Expression and pattern parsing — the precedence ladder (`parse_expr`
//! down through `parse_unary`/`parse_postfix`/`parse_primary`), record
//! construction, `match`/pattern parsing, `if`, and the `Ok`/`Err`
//! expression forms. Split out of `parser.rs` (ADR 0060) as a further
//! `impl Parser` block; the scanning core and the other concerns stay in
//! the parent module, reached as ancestor privates via `self`.

use super::*;

impl<'a> Parser<'a> {
    // -- expressions --

    pub(crate) fn parse_expr(&mut self) -> Result<Expr, CompileError> {
        // v0.9.1: `assert e` is an expression of type `()`. Parsed at the
        // topmost precedence so `assert x == 1` binds as `assert (x == 1)`.
        // In statement position the block parser still consumes `assert` as
        // a Statement::Expect (preserving the v0.7+ form), so this production
        // only fires when `assert` appears in true expression position
        // (e.g., a match-arm body).
        if self.peek_kind() == Some(TokenKind::Expect) {
            let kw = self.expect(TokenKind::Expect, "to start an expect expression")?;
            let value = self.parse_expect_body()?;
            let span = kw.span.merge(value.span);
            return Ok(Expr {
                kind: ExprKind::Expect(Box::new(value)),
                span,
            });
        }
        self.parse_implies()
    }

    /// The subject of an `expect` (v0.117): either an observation over a
    /// consumed capability's recorded calls (`Cap.op called …`, `Cap.op never
    /// called`, `A.op before B.op`) or an ordinary `Bool` predicate. The
    /// observation shape is detected by a `Cap . op` prefix followed by one of
    /// the contextual words `called` / `never` / `before` — which stay ordinary
    /// identifiers everywhere else.
    pub(crate) fn parse_expect_body(&mut self) -> Result<Expr, CompileError> {
        if self.peek_kind() == Some(TokenKind::Ident)
            && self.nth_kind(1) == Some(TokenKind::Dot)
            && self.nth_kind(2) == Some(TokenKind::Ident)
            && self.nth_kind(3) == Some(TokenKind::Ident)
            && matches!(self.nth_text(3), "called" | "never" | "before")
        {
            return self.parse_observation();
        }
        self.parse_expr()
    }

    /// Parse an observation clause (v0.117): `Cap.op` followed by a matcher —
    /// `called [once | <n> times] [with <pred>]`, `never called`, or
    /// `before Cap.op`. The contextual words are matched by text, not token kind.
    fn parse_observation(&mut self) -> Result<Expr, CompileError> {
        let cap = self.expect_ident("as the observed capability name")?;
        self.expect(
            TokenKind::Dot,
            "after the capability name in an observation",
        )?;
        let op = self.expect_ident("as the observed operation name")?;
        let start = cap.span;
        let word = self.nth_text(0);
        let matcher = match word {
            "never" => {
                let never_tok = self.bump().unwrap(); // `never`
                // `never called`
                if self.peek_kind() == Some(TokenKind::Ident) && self.nth_text(0) == "called" {
                    self.bump();
                } else {
                    return Err(CompileError::new(
                        "bynk.parse.expected_expression",
                        never_tok.span,
                        "expected `called` after `never` in an observation",
                    ));
                }
                ObservationMatcher::NeverCalled
            }
            "called" => {
                self.bump(); // `called`
                // Optional count: `once` or `<int> times`.
                let count = if self.peek_kind() == Some(TokenKind::Ident)
                    && self.nth_text(0) == "once"
                {
                    let once = self.bump().unwrap();
                    Some(Box::new(Expr {
                        kind: ExprKind::int_lit(1),
                        span: once.span,
                    }))
                } else if self.peek_kind() == Some(TokenKind::IntLit) {
                    let n = self.parse_primary()?;
                    // `<int> times`
                    if self.peek_kind() == Some(TokenKind::Ident) && self.nth_text(0) == "times" {
                        self.bump();
                    } else {
                        return Err(CompileError::new(
                            "bynk.parse.expected_expression",
                            n.span,
                            "expected `times` after the call count in an observation",
                        ));
                    }
                    Some(Box::new(n))
                } else {
                    None
                };
                // Optional `with <pred>`.
                let with_pred =
                    if self.peek_kind() == Some(TokenKind::Ident) && self.nth_text(0) == "with" {
                        self.bump(); // `with`
                        Some(Box::new(self.parse_expr()?))
                    } else {
                        None
                    };
                ObservationMatcher::Called { count, with_pred }
            }
            "before" => {
                self.bump(); // `before`
                let cap2 = self.expect_ident("as the capability name after `before`")?;
                self.expect(TokenKind::Dot, "after the capability name in `before`")?;
                let op2 = self.expect_ident("as the operation name after `before`")?;
                ObservationMatcher::Before { cap: cap2, op: op2 }
            }
            other => {
                return Err(CompileError::new(
                    "bynk.parse.expected_expression",
                    op.span,
                    format!(
                        "expected an observation matcher (`called`/`never`/`before`), found `{other}`"
                    ),
                ));
            }
        };
        let end = self.prev_span();
        Ok(Expr {
            kind: ExprKind::Observation(ObservationExpr { cap, op, matcher }),
            span: start.merge(end),
        })
    }

    /// v0.80: `P implies Q` — logical implication, the lowest-precedence binary
    /// operator (below `||`). Right-associative, so `A implies B implies C`
    /// parses as `A implies (B implies C)` (matching `A → (B → C)`).
    fn parse_implies(&mut self) -> Result<Expr, CompileError> {
        let lhs = self.parse_or()?;
        if self.peek_kind() == Some(TokenKind::Implies) {
            self.bump();
            let rhs = self.parse_implies()?;
            let span = lhs.span.merge(rhs.span);
            return Ok(Expr {
                kind: ExprKind::BinOp(BinOp::Implies, Box::new(lhs), Box::new(rhs)),
                span,
            });
        }
        Ok(lhs)
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
                    "bynk.parse.non_associative",
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
                    "bynk.parse.non_associative",
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
            // v0.130: a `+`/`-` that begins a new line does not continue the
            // expression — it starts a new construct. Without this, a negative
            // literal pattern in the next match arm (`10` ⏎ `-2 => …`) would be
            // mis-parsed as `10 - 2`. No existing program continues a binary
            // expression with a leading operator on the next line.
            if self.next_token_on_new_line(lhs.span) {
                break;
            }
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
                    let dot = self.bump().unwrap();
                    // `1.` — a numeric literal followed by `.` and no method
                    // name is a malformed float literal, not a member access
                    // (v0.21 §3). `1.toFloat()` stays a method call.
                    if matches!(e.kind, ExprKind::IntLit { .. } | ExprKind::FloatLit { .. })
                        && self.peek_kind() != Some(TokenKind::Ident)
                    {
                        return Err(CompileError::new(
                            "bynk.parse.malformed_float_literal",
                            e.span.merge(dot.span),
                            "a float literal needs a digit on both sides of the `.`",
                        )
                        .with_note(format!(
                            "write `{lit}.0` (or call a method: `{lit}.round()`)",
                            lit = &self.source[e.span.range()]
                        )));
                    }
                    let member = self.expect_ident("after `.` in field access or method call")?;
                    // v0.22b: explicit type arguments on a method/static —
                    // `Json.decode[T](…)`. The v0.20b same-line-`[` rule
                    // applies: a `[` opening a new line is a list literal.
                    let type_args = if self.peek_kind() == Some(TokenKind::LBracket)
                        && !self.next_token_on_new_line(member.span)
                    {
                        self.bump();
                        let mut type_args = Vec::new();
                        loop {
                            type_args.push(self.parse_type_ref("as a type argument")?);
                            if self.eat(TokenKind::Comma).is_none() {
                                break;
                            }
                        }
                        let close =
                            self.expect(TokenKind::RBracket, "to close the type-argument list")?;
                        if self.peek_kind() != Some(TokenKind::LParen) {
                            return Err(CompileError::new(
                                "bynk.parse.expected_token",
                                close.span,
                                "type arguments must be followed by an argument list — `name[T](…)`",
                            )
                            .with_note("a bare `name[T]` value form is reserved"));
                        }
                        type_args
                    } else {
                        Vec::new()
                    };
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
                                type_args,
                                args,
                            },
                            span,
                        };
                    } else if let (ExprKind::IntLit { value: v, .. }, Some(unit)) =
                        (&e.kind, DurationUnit::from_name(&member.name))
                    {
                        // v0.86 (ADR 0112): `<int-literal>.<unit>` is a `Duration`
                        // literal, not a field access — an integer-literal
                        // receiver has no fields, so this is unambiguous.
                        let value = *v;
                        let span = e.span.merge(member.span);
                        let millis = value.checked_mul(unit.millis()).ok_or_else(|| {
                            CompileError::new(
                                "bynk.duration.literal_overflow",
                                span,
                                format!(
                                    "the duration `{value}.{}` overflows the millisecond range",
                                    unit.name()
                                ),
                            )
                        })?;
                        e = Expr {
                            kind: ExprKind::DurationLit {
                                value,
                                unit,
                                millis,
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

    /// `Mock '[' type ']' ( '(' args? ')' )?` — v0.9.4 test-context value
    /// construction. The leading `Val` identifier and the `[` lookahead have
    /// already been confirmed by the caller; `kw_span` is the `Val` span.
    fn parse_val_expr(&mut self, kw_span: Span) -> Result<Expr, CompileError> {
        self.expect(TokenKind::LBracket, "after `Val`")?;
        let type_ref = self.parse_type_ref("as the type argument of `Val[T]`")?;
        let close_b = self.expect(TokenKind::RBracket, "to close `Val[T]`")?;
        let mut args = Vec::new();
        let mut end = close_b.span;
        if self.peek_kind() == Some(TokenKind::LParen) {
            self.bump();
            if self.peek_kind() != Some(TokenKind::RParen) {
                args.push(self.parse_expr()?);
                while self.eat(TokenKind::Comma).is_some() {
                    args.push(self.parse_expr()?);
                }
            }
            end = self
                .expect(TokenKind::RParen, "to close the `Val` arguments")?
                .span;
        }
        Ok(Expr {
            kind: ExprKind::Val { type_ref, args },
            span: kw_span.merge(end),
        })
    }

    fn parse_primary(&mut self) -> Result<Expr, CompileError> {
        let t = self.peek().ok_or_else(|| {
            CompileError::new(
                "bynk.parse.unexpected_eof",
                self.eof_span(),
                "expected an expression, found end of file",
            )
        })?;
        match t.kind {
            TokenKind::IntLit => {
                self.bump();
                let slice = self.slice(t.span);
                // v0.142 (ADR 0166): parse the value from the separator-free form,
                // but keep the as-written lexeme so `fmt` preserves the author's
                // `_` grouping.
                let n: i64 = crate::lexer::strip_digit_separators(slice)
                    .parse()
                    .map_err(|_| {
                        CompileError::new(
                            "bynk.lex.integer_overflow",
                            t.span,
                            format!("integer literal `{slice}` out of 64-bit range"),
                        )
                    })?;
                Ok(Expr {
                    kind: ExprKind::IntLit {
                        value: n,
                        lexeme: slice.to_string(),
                    },
                    span: t.span,
                })
            }
            TokenKind::FloatLit => {
                self.bump();
                let slice = self.slice(t.span);
                // tokenize() already validated the separator-free form, so this
                // parse cannot fail — but a silent NaN would corrupt the value
                // if the two ever desync, so fail loudly instead (v0.142).
                let value: f64 = crate::lexer::strip_digit_separators(slice)
                    .parse()
                    .map_err(|_| {
                        CompileError::new(
                            "bynk.lex.float_literal_overflow",
                            t.span,
                            format!("float literal `{slice}` does not parse"),
                        )
                    })?;
                Ok(Expr {
                    kind: ExprKind::FloatLit {
                        value,
                        lexeme: slice.to_string(),
                    },
                    span: t.span,
                })
            }
            // `.5` — a float literal missing its leading digit (v0.21 §3).
            TokenKind::Dot
                if matches!(
                    self.tokens.get(self.pos + 1).map(|t| t.kind),
                    Some(TokenKind::IntLit | TokenKind::FloatLit)
                ) =>
            {
                let lit = self.tokens[self.pos + 1];
                Err(CompileError::new(
                    "bynk.parse.malformed_float_literal",
                    t.span.merge(lit.span),
                    "a float literal needs a digit on both sides of the `.`",
                )
                .with_note(format!("write `0.{}`", &self.source[lit.span.range()])))
            }
            TokenKind::StrLit => {
                self.bump();
                let s = parse_string_literal(self.slice(t.span), t.span)?;
                Ok(Expr {
                    kind: ExprKind::StrLit(s),
                    span: t.span,
                })
            }
            // An interpolated string `"… \(expr) …"` (v0.43). The lexer has
            // already delimited the token and balanced its holes; here we
            // split the chunks from the holes and parse each hole as a full
            // expression (against the original source, so spans stay absolute).
            TokenKind::InterpStr => {
                self.bump();
                let parts = self.parse_interp_parts(t.span)?;
                Ok(Expr {
                    kind: ExprKind::InterpStr(parts),
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
                // v0.20a: `(params) => …` — a lambda. Token-level scan to the
                // matching `)` then a one-token peek for `=>`; only paren
                // depth matters (strings are single tokens).
                if self.lambda_ahead() {
                    return self.parse_lambda();
                }
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
            // v0.22a: `Int.parse(…)` / `Float.parse(…)` — a numeric base-type
            // keyword in static-receiver position. Only recognised when the
            // next token is `.`, so a bare `Int` in expression position keeps
            // the ordinary "expected an expression" error. Lowered to an
            // Ident-shaped receiver; postfix builds the MethodCall and the
            // resolver/checker own the static dispatch (like `List.empty()`).
            // v0.86 (ADR 0112): `Duration.millis(…)` is the same static-receiver
            // shape, so the `Duration` keyword joins `Int`/`Float` here. v0.90
            // (ADR 0114): `Instant.fromEpochMillis(…)` joins them.
            TokenKind::Int
            | TokenKind::Float
            | TokenKind::Duration
            | TokenKind::Instant
            | TokenKind::Bytes
                if self.tokens.get(self.pos + 1).map(|t| t.kind) == Some(TokenKind::Dot) =>
            {
                self.bump();
                let name = self.slice(t.span).to_string();
                Ok(Expr {
                    kind: ExprKind::Ident(Ident { name, span: t.span }),
                    span: t.span,
                })
            }
            // `Effect.pure(value)` — wrap a synchronous value as `Effect[T]` (v0.5).
            TokenKind::Effect => {
                let kw = self.bump().unwrap();
                self.expect(TokenKind::Dot, "after `Effect` in `Effect.pure(...)`")?;
                let method = self.expect_ident("after `Effect.`")?;
                if method.name != "pure" {
                    return Err(CompileError::new(
                        "bynk.parse.unknown_effect_method",
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
                // v0.9.4: `Val[T]` / `Val[T](args)` — test-context construction.
                if ident.name == "Val" && self.peek_kind() == Some(TokenKind::LBracket) {
                    return self.parse_val_expr(ident.span);
                }
                // v0.117: `trace(Cap.op)` — the observation escape hatch, a
                // test-only builtin. Recognised by the `trace ( Ident . Ident )`
                // shape so an ordinary `trace(value)` call is left untouched.
                if ident.name == "trace"
                    && self.peek_kind() == Some(TokenKind::LParen)
                    && self.nth_kind(1) == Some(TokenKind::Ident)
                    && self.nth_kind(2) == Some(TokenKind::Dot)
                    && self.nth_kind(3) == Some(TokenKind::Ident)
                    && self.nth_kind(4) == Some(TokenKind::RParen)
                {
                    self.bump(); // `(`
                    let cap = self.expect_ident("as the capability name in `trace(Cap.op)`")?;
                    self.expect(TokenKind::Dot, "in `trace(Cap.op)`")?;
                    let op = self.expect_ident("as the operation name in `trace(Cap.op)`")?;
                    let close = self.expect(TokenKind::RParen, "to close `trace(Cap.op)`")?;
                    return Ok(Expr {
                        kind: ExprKind::Trace { cap, op },
                        span: ident.span.merge(close.span),
                    });
                }
                // v0.20a: explicit type arguments — `name[T, U](…)`.
                // Bare `name[T]` without an argument list is reserved.
                // v0.20b: the `[` must sit on the same line — a `[` opening
                // a new line starts a list literal, not type application.
                if self.peek_kind() == Some(TokenKind::LBracket)
                    && !self.next_token_on_new_line(ident.span)
                {
                    self.bump();
                    let mut type_args = Vec::new();
                    loop {
                        type_args.push(self.parse_type_ref("as a type argument")?);
                        if self.eat(TokenKind::Comma).is_none() {
                            break;
                        }
                    }
                    let close =
                        self.expect(TokenKind::RBracket, "to close the type-argument list")?;
                    if self.peek_kind() != Some(TokenKind::LParen) {
                        return Err(CompileError::new(
                            "bynk.parse.expected_token",
                            close.span,
                            "type arguments must be followed by an argument list — `name[T](…)`",
                        )
                        .with_note("a bare `name[T]` value form is reserved"));
                    }
                    self.bump();
                    let mut args = Vec::new();
                    if self.peek_kind() != Some(TokenKind::RParen) {
                        args.push(self.parse_expr()?);
                        while self.eat(TokenKind::Comma).is_some() {
                            args.push(self.parse_expr()?);
                        }
                    }
                    let close_paren =
                        self.expect(TokenKind::RParen, "to close the argument list")?;
                    return Ok(Expr {
                        kind: ExprKind::Call {
                            name: ident.clone(),
                            type_args,
                            args,
                        },
                        span: ident.span.merge(close_paren.span),
                    });
                }
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
                        kind: ExprKind::Call {
                            name: ident.clone(),
                            type_args: Vec::new(),
                            args,
                        },
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
                        "bynk.parse.expected_expression",
                        t.span,
                        "expected an expression, found `{`",
                    )
                    .with_note(
                        "bare record-spread `{ ...base, ... }` is the only `{`-led expression in v0.5; for record construction, use `TypeName { ... }`",
                    ))
                }
            }
            // v0.20b: `[a, b, c]` — list literal. A leading `[` is
            // unambiguous: type application (`name[T](…)`) is parsed as a
            // postfix form on the callee identifier and never reaches here.
            TokenKind::LBracket => {
                let open = self.bump().unwrap();
                let mut elems = Vec::new();
                if self.peek_kind() != Some(TokenKind::RBracket) {
                    loop {
                        elems.push(self.parse_expr()?);
                        if self.eat(TokenKind::Comma).is_none() {
                            break;
                        }
                        // Trailing comma before the closing bracket.
                        if self.peek_kind() == Some(TokenKind::RBracket) {
                            break;
                        }
                    }
                }
                let close = self.expect(TokenKind::RBracket, "to close the list literal")?;
                Ok(Expr {
                    kind: ExprKind::ListLit(elems),
                    span: open.span.merge(close.span),
                })
            }
            _ => Err(CompileError::new(
                "bynk.parse.expected_expression",
                t.span,
                format!("expected an expression, found {}", t.kind.describe()),
            )),
        }
    }
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
                "bynk.parse.empty_match",
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
        // Optional `if <Bool-expr>` guard between the pattern and `=>` (ADR 0169).
        // The guard sees the arm's bindings; it is unambiguous here because an
        // arm-position `if` can only be a guard.
        let guard = if self.peek_kind() == Some(TokenKind::If) {
            self.bump();
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(TokenKind::FatArrow, "after a match-arm pattern")?;
        let body = if self.peek_kind() == Some(TokenKind::LBrace) {
            MatchBody::Block(self.parse_block("to open the match-arm body")?)
        } else {
            MatchBody::Expr(self.parse_expr()?)
        };
        let span = pattern.span().merge(body.span());
        Ok(MatchArm {
            pattern,
            guard,
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
            // Literal patterns (v0.130 §2.3.4): `31`, `"english"`, `true`, and a
            // leading unary minus on an integer literal (`-1`). A closed set
            // (ADR 0001) — no `Float`, no `()`.
            match t.kind {
                TokenKind::IntLit | TokenKind::StrLit | TokenKind::True | TokenKind::False => {
                    return self.parse_literal_pattern();
                }
                TokenKind::Minus
                    if self.tokens.get(self.pos + 1).map(|t| t.kind) == Some(TokenKind::IntLit) =>
                {
                    return self.parse_literal_pattern();
                }
                _ => {}
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
        // A bare lowercase identifier with no qualifier and no payload list is a
        // binding (ADR 0169) — `Some(user)`, or a top-level `n if n > 0`. An
        // uppercase-led identifier (or one carrying a payload list) is a variant
        // constructor. Built-in variants are keyword tokens, handled above.
        let has_payload = self.peek_kind() == Some(TokenKind::LParen);
        if type_name.is_none()
            && !has_payload
            && variant
                .name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase())
        {
            return Ok(Pattern::Binding(variant));
        }
        let mut bindings = Vec::new();
        let mut end_span = variant.span;
        if has_payload {
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

    /// Parse a literal pattern (v0.130 §2.3.4): `31`, `-1`, `"english"`,
    /// `true`/`false`. The caller has already confirmed the leading token is one
    /// of these forms.
    fn parse_literal_pattern(&mut self) -> Result<Pattern, CompileError> {
        // Optional leading `-` on an integer literal (ADR 0001).
        let neg = self.eat(TokenKind::Minus);
        let t = self.bump().expect("literal-pattern lead token");
        let (value, end_span) = match t.kind {
            TokenKind::IntLit => {
                let slice = self.slice(t.span);
                // v0.142 (ADR 0166): parse from the separator-free form.
                let mut n: i64 = crate::lexer::strip_digit_separators(slice)
                    .parse()
                    .map_err(|_| {
                        CompileError::new(
                            "bynk.lex.integer_overflow",
                            t.span,
                            format!("integer literal `{slice}` out of 64-bit range"),
                        )
                    })?;
                if neg.is_some() {
                    n = -n;
                }
                (LiteralValue::Int(n), t.span)
            }
            TokenKind::StrLit => {
                let s = parse_string_literal(self.slice(t.span), t.span)?;
                (LiteralValue::Str(s), t.span)
            }
            TokenKind::True => (LiteralValue::Bool(true), t.span),
            TokenKind::False => (LiteralValue::Bool(false), t.span),
            _ => unreachable!("parse_literal_pattern called on a non-literal token"),
        };
        let span = neg.map(|m| m.span).unwrap_or(t.span).merge(end_span);
        Ok(Pattern::Literal { value, span })
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
        // Allowed shapes (ADR 0169 — each field position is a full sub-pattern):
        //   `_`                 positional wildcard
        //   `name`              positional binding
        //   `Ok(x)` / `Pending` positional nested pattern
        //   `field: <pattern>`  named (where <pattern> is any of the above)
        // A named field is a lowercase-style ident immediately followed by `:`;
        // everything else is a positional sub-pattern.
        if let Some(t) = self.peek()
            && t.kind == TokenKind::Ident
            && self.tokens.get(self.pos + 1).map(|t| t.kind) == Some(TokenKind::Colon)
        {
            let field = self.expect_ident("as a named pattern field")?;
            self.expect(TokenKind::Colon, "after a named pattern field")?;
            let pattern = self.parse_pattern()?;
            let span = field.span.merge(pattern.span());
            return Ok(PatternBinding {
                kind: PatternBindingKind::Named { field, pattern },
                span,
            });
        }
        let pattern = self.parse_pattern()?;
        let span = pattern.span();
        Ok(PatternBinding {
            kind: PatternBindingKind::Positional { pattern },
            span,
        })
    }

    /// Parse `if expr block 'else' (if-expr | block)` (v0.1 §3.2).
    /// Both branches are represented as Blocks; an `else if` chain becomes a
    /// Block whose tail is another If expression.
    fn parse_if_expr(&mut self) -> Result<Expr, CompileError> {
        let kw = self.expect(TokenKind::If, "to start an if expression")?;
        let cond = self.parse_expr()?;
        let then_block = self.parse_block("to open the `if` branch")?;
        // v0.146 (ADR 0170): `else` is optional. A missing `else` defaults to a
        // synthesised `{ () }` (unit) else-branch — legal only when the
        // then-branch is itself unit (`()` / `Effect[()]`), which the checker
        // enforces (`bynk.types.if_without_else_requires_unit`). The synthetic
        // block is marked `implicit_tail` so the formatter round-trips the
        // else-less form. A valued `if` still requires an explicit `else`.
        let else_block = if self.eat(TokenKind::Else).is_some() {
            if self.peek_kind() == Some(TokenKind::If) {
                // `else if ...` desugars to `else { if ... }`.
                let inner = self.parse_if_expr()?;
                let span = inner.span;
                Block {
                    statements: Vec::new(),
                    tail: Box::new(inner),
                    span,
                    tail_leading_comments: Vec::new(),
                    implicit_tail: false,
                }
            } else {
                self.parse_block("to open the `else` branch")?
            }
        } else {
            Block {
                statements: Vec::new(),
                tail: Box::new(Expr {
                    kind: ExprKind::UnitLit,
                    span: then_block.span,
                }),
                span: then_block.span,
                tail_leading_comments: Vec::new(),
                implicit_tail: true,
            }
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

    /// Split an `InterpStr` token (covering the whole `"…"`) into its
    /// alternating chunks and holes, parsing each hole as a full expression.
    /// (v0.43.)
    fn parse_interp_parts(&mut self, span: Span) -> Result<Vec<InterpPart>, CompileError> {
        let segments = crate::lexer::split_interp(self.source, span)?;
        let mut parts = Vec::with_capacity(segments.len());
        for segment in segments {
            match segment {
                crate::lexer::InterpSegment::Chunk(text) => parts.push(InterpPart::Chunk(text)),
                crate::lexer::InterpSegment::Hole(hole) => {
                    parts.push(InterpPart::Hole(Box::new(self.parse_hole_expr(hole)?)));
                }
            }
        }
        Ok(parts)
    }

    /// Parse the body of one interpolation hole (`\(expr)`) — the bytes spanned
    /// by `hole` — as a single expression. The hole source is re-lexed and its
    /// token spans are rebased to absolute positions in the full source, so
    /// diagnostics and the (later) LSP point at the real location. (v0.43.)
    fn parse_hole_expr(&mut self, hole: Span) -> Result<Expr, CompileError> {
        let src = &self.source[hole.range()];
        if src.trim().is_empty() {
            return Err(CompileError::new(
                "bynk.parse.empty_interpolation",
                hole,
                "empty interpolation hole",
            )
            .with_note("`\\(…)` must contain an expression"));
        }
        let mut tokens = crate::lexer::tokenize(src)?;
        for token in &mut tokens {
            token.span = Span::new(token.span.start + hole.start, token.span.end + hole.start);
        }
        let (content, trivia) = split_trivia(&tokens, self.source);
        let mut warnings = Vec::new();
        let mut sub = Parser::new(&content, self.source, trivia, &mut warnings);
        let expr = sub.parse_expr()?;
        if let Some(extra) = sub.peek() {
            return Err(CompileError::new(
                "bynk.parse.extra_tokens",
                extra.span,
                format!(
                    "unexpected {} after the interpolation expression",
                    extra.kind.describe()
                ),
            )
            .with_note("an interpolation hole `\\(…)` holds a single expression"));
        }
        Ok(expr)
    }
}
