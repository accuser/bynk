//! Function-declaration, block, statement, parameter, and lambda parsing.
//! Split out of `parser.rs` (ADR 0060) as a further `impl Parser` block; the
//! scanning core and the other concerns stay in the parent module, reached
//! as ancestor privates via `self`.

use super::*;

impl<'a> Parser<'a> {
    // -- function declarations --

    pub(crate) fn parse_fn_decl(&mut self) -> Result<FnDecl, CompileError> {
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
        // with `bynk.generics.no_bounds`).
        let type_params = self.parse_optional_type_params()?;
        self.expect(TokenKind::LParen, "after the function name")?;
        // For methods, the first parameter may be the special `self` keyword.
        let mut params = Vec::new();
        let mut has_self = false;
        if self.peek_kind() == Some(TokenKind::Self_) {
            let self_tok = self.bump().unwrap();
            if !matches!(name, FnName::Method { .. }) {
                return Err(CompileError::new(
                    "bynk.parse.self_outside_method",
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
        // v0.115: contract clauses ride between the return type and the body —
        // any number of `requires <name>: <pred>` then `ensures <name>: <pred>`.
        // The two-list split is by keyword, not order; the checker enforces the
        // scoping difference (`result` bound only in `ensures`).
        let mut requires = Vec::new();
        let mut ensures = Vec::new();
        loop {
            match self.peek_kind() {
                Some(TokenKind::Requires) => requires.push(self.parse_contract_clause(true)?),
                Some(TokenKind::Ensures) => ensures.push(self.parse_contract_clause(false)?),
                _ => break,
            }
        }
        let body = self.parse_block("to open the function body")?;
        let span = kw.span.merge(body.span);
        Ok(FnDecl {
            type_params,
            name,
            params,
            return_type,
            requires,
            ensures,
            body,
            has_self,
            documentation: None,
            span,
            trivia: Trivia::default(),
        })
    }

    /// Parse a single contract clause (v0.115): `requires <name>: <pred>` or
    /// `ensures <name>: <pred>`. The predicate is an ordinary expression (with
    /// `implies`/`is`) over the parameters — and, for an `ensures`, the
    /// contextual `result` binding; well-formedness (purity, `Bool`, `result`
    /// scope) is the checker's job, mirroring [`parse_invariant`].
    fn parse_contract_clause(&mut self, is_requires: bool) -> Result<Contract, CompileError> {
        let kw = if is_requires {
            self.expect(TokenKind::Requires, "to start a precondition clause")?
        } else {
            self.expect(TokenKind::Ensures, "to start a postcondition clause")?
        };
        let word = if is_requires { "requires" } else { "ensures" };
        let name = self.expect_ident(&format!("as the clause name after `{word}`"))?;
        self.expect(TokenKind::Colon, "after the contract clause name")?;
        let predicate = self.parse_expr()?;
        let span = kw.span.merge(predicate.span);
        Ok(Contract {
            name,
            predicate,
            span,
        })
    }

    /// Parse a brace-delimited block: `{ statement* expr }` (v0.1 §3.1, v0.5).
    pub(crate) fn parse_block(&mut self, ctx: &str) -> Result<Block, CompileError> {
        let open = self.expect(TokenKind::LBrace, ctx)?;
        self.parse_block_rest(open.span)
    }

    /// v0.118: the body of a block after the opening `{` has been consumed. A
    /// `case` body opens the brace itself, peels its leading `provides` clauses,
    /// then calls this to parse the statements and tail.
    pub(crate) fn parse_block_rest(&mut self, open: Span) -> Result<Block, CompileError> {
        let mut statements = Vec::new();
        // Loop: parse statements until we hit something that's not a statement.
        // v0.1: `let`. v0.5: `let ... <-` is also a statement.
        // v0.7: `assert` is a statement form inside test bodies.
        let tail_leading: Vec<String>;
        loop {
            let leading = self.take_leading_trivia();
            // v0.81: `name := expr` is a statement led by an identifier, so it is
            // detected by lookahead rather than a leading keyword.
            let is_statement = matches!(
                self.peek_kind(),
                Some(TokenKind::Let)
                    | Some(TokenKind::Expect)
                    | Some(TokenKind::TildeArrow)
                    | Some(TokenKind::Do)
            ) || self.assign_ahead();
            if is_statement {
                let mut stmt = self.parse_statement()?;
                let trailing = self.take_trailing_trivia();
                match &mut stmt {
                    Statement::Let(l) | Statement::EffectLet(l) => {
                        l.trivia.leading = leading;
                        l.trivia.trailing = trailing;
                    }
                    Statement::Expect(a) => {
                        a.trivia.leading = leading;
                        a.trivia.trailing = trailing;
                    }
                    Statement::Send(s) => {
                        s.trivia.leading = leading;
                        s.trivia.trailing = trailing;
                    }
                    Statement::Do(d) => {
                        d.trivia.leading = leading;
                        d.trivia.trailing = trailing;
                    }
                    Statement::Assign(a) => {
                        a.trivia.leading = leading;
                        a.trivia.trailing = trailing;
                    }
                }
                statements.push(stmt);
            } else {
                tail_leading = leading;
                break;
            }
        }
        // v0.7: a block whose last statement is an `assert` may close without an
        // explicit tail expression. v0.146 (ADR 0170): *any* block may — a block
        // that closes right after its statements (or an empty `{}`) carries a
        // synthesised `()` (unit) tail. Against an `Effect[()]` context the
        // tail-position auto-lift wraps it; against a valued context it is an
        // ordinary type mismatch. `implicit_tail` records the synthesis so the
        // formatter omits the `()` (Bynk has no statement terminator, so a
        // printed `()` would re-attach to the last statement on re-parse).
        if self.peek_kind() == Some(TokenKind::RBrace) {
            let close = self.expect(TokenKind::RBrace, "to close the block")?;
            let tail = Expr {
                kind: ExprKind::UnitLit,
                span: close.span,
            };
            return Ok(Block {
                statements,
                tail: Box::new(tail),
                span: open.merge(close.span),
                tail_leading_comments: tail_leading,
                implicit_tail: true,
            });
        }
        let tail = self.parse_expr()?;
        let close = self.expect(TokenKind::RBrace, "to close the block")?;
        Ok(Block {
            statements,
            tail: Box::new(tail),
            span: open.merge(close.span),
            tail_leading_comments: tail_leading,
            implicit_tail: false,
        })
    }

    /// v0.81: true when the next two tokens are `<ident> :=` — a `Cell` write
    /// statement, which (unlike `let`/`commit`/`~>`) is led by an identifier.
    fn assign_ahead(&self) -> bool {
        self.peek_kind() == Some(TokenKind::Ident)
            && self.tokens.get(self.pos + 1).map(|t| t.kind) == Some(TokenKind::ColonEq)
    }

    /// v0.182 (#664): parse an optional call-site actor clause after an
    /// effect-let value — `by <Actor>` (unit identity) or `by <Actor>(<expr>)`.
    /// Distinct from the handler `by` clause (`parse_by_clause`), which admits a
    /// binder and an actor *sum* but no identity argument.
    fn parse_call_site_actor(&mut self) -> Result<Option<CallSiteActor>, CompileError> {
        if self.peek_kind() != Some(TokenKind::By) {
            return Ok(None);
        }
        let by_kw = self.expect(TokenKind::By, "to start the call-site actor clause")?;
        let actor = self.expect_ident("as the actor after `by` at a call site")?;
        let (identity, end) = if self.peek_kind() == Some(TokenKind::LParen) {
            self.expect(TokenKind::LParen, "after the actor name")?;
            let id = self.parse_expr()?;
            let rp = self.expect(TokenKind::RParen, "after the identity value")?;
            (Some(Box::new(id)), rp.span)
        } else {
            (None, actor.span)
        };
        let span = by_kw.span.merge(end);
        Ok(Some(CallSiteActor {
            actor,
            identity,
            span,
        }))
    }

    fn parse_statement(&mut self) -> Result<Statement, CompileError> {
        if self.assign_ahead() {
            let target = self.expect_ident("as the assignment target")?;
            self.expect(TokenKind::ColonEq, "after the assignment target")?;
            let value = self.parse_expr()?;
            let span = target.span.merge(value.span);
            return Ok(Statement::Assign(AssignStmt {
                target,
                value,
                span,
                trivia: Trivia::default(),
            }));
        }
        if self.peek_kind() == Some(TokenKind::Expect) {
            let kw = self.expect(TokenKind::Expect, "to start an expect statement")?;
            let value = self.parse_expect_body()?;
            let span = kw.span.merge(value.span);
            return Ok(Statement::Expect(ExpectStmt {
                value,
                span,
                trivia: Trivia::default(),
            }));
        }
        if self.peek_kind() == Some(TokenKind::TildeArrow) {
            let kw = self.expect(TokenKind::TildeArrow, "to start an async send statement")?;
            let value = self.parse_expr()?;
            let span = kw.span.merge(value.span);
            return Ok(Statement::Send(SendStmt {
                value,
                span,
                trivia: Trivia::default(),
            }));
        }
        if self.peek_kind() == Some(TokenKind::Do) {
            // v0.146 (ADR 0170): `do e` — perform a unit effect as a statement.
            let kw = self.expect(TokenKind::Do, "to start a `do` statement")?;
            let value = self.parse_expr()?;
            let span = kw.span.merge(value.span);
            return Ok(Statement::Do(DoStmt {
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
                    principal: None,
                    span,
                    trivia: Trivia::default(),
                }))
            }
            Some(TokenKind::LArrow) => {
                self.bump();
                let value = self.parse_expr()?;
                // v0.182 (#664): an optional call-site `by <Actor>(<identity>)`
                // clause — the test-body form that names the actor a case acts as
                // when it drives a service handler. Distinct from the handler
                // `by` (no argument); parsed only here, in the effect-let arm.
                let principal = self.parse_call_site_actor()?;
                let end = principal.as_ref().map(|p| p.span).unwrap_or(value.span);
                let span = kw.span.merge(end);
                Ok(Statement::EffectLet(LetStmt {
                    name,
                    type_annot,
                    value,
                    principal,
                    span,
                    trivia: Trivia::default(),
                }))
            }
            Some(_) => {
                let t = self.peek().unwrap();
                Err(CompileError::new(
                    "bynk.parse.expected_token",
                    t.span,
                    format!(
                        "expected `=` or `<-` after the let-binding's name, found {}",
                        t.kind.describe()
                    ),
                ))
            }
            None => Err(CompileError::new(
                "bynk.parse.unexpected_eof",
                self.eof_span(),
                "expected `=` or `<-` after the let-binding's name, found end of file",
            )),
        }
    }

    pub(crate) fn parse_param(&mut self) -> Result<Param, CompileError> {
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
    pub(crate) fn lambda_ahead(&self) -> bool {
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
    pub(crate) fn parse_lambda(&mut self) -> Result<Expr, CompileError> {
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
}
