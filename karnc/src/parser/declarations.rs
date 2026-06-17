//! Declaration parsing — unit/commons/context/test/mock/adapter/binding
//! declarations and their fragment forms. Split out of `parser.rs`
//! (ADR 0060) as a second `impl Parser` block; the scanning core (`expect`,
//! `peek`, `bump`, the trivia/doc helpers) and the other parse concerns stay
//! in the parent module, reached as ancestor privates via `self`.

use super::*;

impl<'a> Parser<'a> {
    pub(crate) fn parse_unit(&mut self) -> Result<SourceUnit, CompileError> {
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
            Some(TokenKind::Adapter) => {
                let start = self.expect(TokenKind::Adapter, "to start the adapter declaration")?;
                let doc = self.finalize_doc(leading_doc, start.span);
                let name = self.parse_qualified_name()?;
                let mut a = match self.peek_kind() {
                    Some(TokenKind::LBrace) => {
                        self.parse_adapter_body(start.span, name, doc, true)?
                    }
                    _ => self.parse_adapter_body(start.span, name, doc, false)?,
                };
                a.trivia = header_trivia;
                Ok(SourceUnit::Adapter(a))
            }
            Some(TokenKind::Test) => {
                // v0.16: `test integration "name" { … }` is the integration-test
                // kind. `integration` is contextual — it's an ordinary identifier
                // everywhere except directly after `test` and before a string
                // literal (the suite name). Anything else is a v0.7 unit test.
                let next = self.tokens.get(self.pos + 1);
                let after = self.tokens.get(self.pos + 2).map(|t| t.kind);
                let is_integration = matches!(next, Some(t)
                    if t.kind == TokenKind::Ident
                        && self.slice(t.span) == "integration")
                    && after == Some(TokenKind::StrLit);
                let start = self.expect(TokenKind::Test, "to start the test declaration")?;
                let doc = self.finalize_doc(leading_doc, start.span);
                if is_integration {
                    let mut i = self.parse_integration(start.span, doc)?;
                    i.trivia = header_trivia;
                    return Ok(SourceUnit::Integration(i));
                }
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
                Some(TokenKind::Actor) => {
                    let err = CompileError::new(
                        "karn.actor.outside_context",
                        self.peek().unwrap().span,
                        "`actor` declarations are only allowed inside a context, not a commons",
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
                Some(TokenKind::Actor) => {
                    let err = CompileError::new(
                        "karn.actor.outside_context",
                        self.peek().unwrap().span,
                        "`actor` declarations are only allowed inside a context, not a commons",
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
        let mut alias = None;
        let mut selected = None;
        match self.peek_kind() {
            // v0.6: `consumes U as Alias`.
            Some(TokenKind::As) => {
                self.bump();
                let id = self.expect_ident("as an alias for the consumed context")?;
                span = span.merge(id.span);
                alias = Some(id);
            }
            // v0.17: `consumes U { Cap, … }` — selected capabilities (§3.3).
            Some(TokenKind::LBrace) => {
                self.bump();
                let mut names = Vec::new();
                while self.peek_kind() != Some(TokenKind::RBrace) {
                    let id = self.expect_ident("a capability name in `consumes U { … }`")?;
                    names.push(id);
                    if self.eat(TokenKind::Comma).is_none() {
                        break;
                    }
                }
                let close =
                    self.expect(TokenKind::RBrace, "to close the consumed-capability list")?;
                span = span.merge(close.span);
                selected = Some(names);
            }
            _ => {}
        }
        Ok(ConsumesDecl {
            target,
            alias,
            selected,
            span,
            trivia: Trivia::default(),
        })
    }

    fn parse_exports_decl(&mut self) -> Result<ExportsDecl, CompileError> {
        let kw = self.expect(TokenKind::Exports, "to start an `exports` declaration")?;
        let kind = match self.peek_kind() {
            Some(TokenKind::Opaque) => {
                self.bump();
                ExportKind::Type(Visibility::Opaque)
            }
            Some(TokenKind::Transparent) => {
                self.bump();
                ExportKind::Type(Visibility::Transparent)
            }
            // v0.15: `exports capability { ... }` offers capabilities to consumers.
            Some(TokenKind::Capability) => {
                self.bump();
                ExportKind::Capability
            }
            Some(_) => {
                let t = self.peek().unwrap();
                return Err(CompileError::new(
                    "karn.parse.expected_visibility",
                    t.span,
                    format!(
                        "expected `opaque`, `transparent`, or `capability` after `exports`, found {}",
                        t.kind.describe()
                    ),
                )
                .with_note(
                    "exports clauses are `exports opaque { ... }`, `exports transparent { ... }`, or `exports capability { ... }`",
                ));
            }
            None => {
                return Err(CompileError::new(
                    "karn.parse.unexpected_eof",
                    self.eof_span(),
                    "expected `opaque`, `transparent`, or `capability` after `exports`, found end of file",
                ));
            }
        };
        self.expect(TokenKind::LBrace, "to open the exports list")?;
        let mut names = Vec::new();
        let name_role = match kind {
            ExportKind::Capability => "as an exported capability name",
            ExportKind::Type(_) => "as an exported type name",
        };
        while self.peek_kind() != Some(TokenKind::RBrace) {
            names.push(self.expect_ident(name_role)?);
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let close = self.expect(TokenKind::RBrace, "to close the exports list")?;
        let span = kw.span.merge(close.span);
        Ok(ExportsDecl {
            kind,
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

    /// Parse a `test integration "name"` declaration (the leading `test` has
    /// already been consumed; `start` is its span). Handles both the brace form
    /// (`{ wires …; cases }`) and the headerless fragment form. The `integration`
    /// contextual keyword and the suite-name literal are consumed here.
    fn parse_integration(
        &mut self,
        start: Span,
        documentation: Option<String>,
    ) -> Result<IntegrationDecl, CompileError> {
        // The contextual `integration` keyword (an Ident, validated by the caller).
        let kw = self.expect(TokenKind::Ident, "the contextual keyword `integration`")?;
        debug_assert_eq!(self.slice(kw.span), "integration");
        let name_tok = self.expect(TokenKind::StrLit, "as the integration suite name")?;
        let suite = parse_string_literal(self.slice(name_tok.span), name_tok.span)?;
        let synth_name = QualifiedName {
            parts: vec![Ident {
                name: format!("integration {suite}"),
                span: name_tok.span,
            }],
            span: name_tok.span,
        };

        let brace = self.peek_kind() == Some(TokenKind::LBrace);
        if brace {
            self.bump();
        }

        // The `wires` clause is required and leads the body.
        let participants = self.parse_wires_clause()?;

        let mut uses = Vec::new();
        let mut cases = Vec::new();
        let mut last_span = name_tok.span;
        let mut seen_non_uses = false;
        let trailing_comments: Vec<String>;
        loop {
            let (mut leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::RBrace) if brace => {
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
                None if !brace => {
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
                            "`uses` clauses must appear before any `test` cases in an integration test",
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
                Some(TokenKind::Mocks) => {
                    let t = self.peek().unwrap();
                    let err = CompileError::new(
                        "karn.integration.mock_in_integration",
                        t.span,
                        "`mocks` is not allowed in an integration test",
                    )
                    .with_note(
                        "integration tests wire participants with their real implementations; use a unit test (`test <context> { mocks … }`) for mocking",
                    );
                    if self.recover_mode {
                        self.recovered_errors.push(err);
                        self.bump();
                        self.recover_to_top_item();
                    } else {
                        return Err(err);
                    }
                }
                _ => {
                    let t = self.peek();
                    let (span, found) = match t {
                        Some(t) => (t.span, t.kind.describe().to_string()),
                        None => (self.eof_span(), "end of file".to_string()),
                    };
                    let err = CompileError::new(
                        "karn.parse.expected_item",
                        span,
                        format!("expected `uses` or `test \"name\"` declaration, found {found}"),
                    )
                    .with_note(
                        "an integration test body is a `wires` clause followed by `uses` and `test \"name\"` declarations",
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
        let end_span = if brace {
            self.expect(TokenKind::RBrace, "to close the integration test body")?
                .span
        } else {
            last_span
        };
        Ok(IntegrationDecl {
            suite,
            suite_span: name_tok.span,
            name: synth_name,
            participants,
            uses,
            cases,
            form: if brace {
                CommonsForm::Brace
            } else {
                CommonsForm::Fragment
            },
            documentation,
            span: start.merge(end_span),
            trivia: Trivia::default(),
            trailing_comments,
        })
    }

    /// Parse the required `wires C1, C2, …` clause that leads an integration
    /// test body. Accepts one-or-more here; the ≥ 2 rule is a checker
    /// diagnostic (`karn.integration.too_few_participants`) for a better message.
    fn parse_wires_clause(&mut self) -> Result<Vec<QualifiedName>, CompileError> {
        self.expect(
            TokenKind::Wires,
            "to begin the integration participant list",
        )?;
        let mut participants = vec![self.parse_qualified_name()?];
        while self.eat(TokenKind::Comma).is_some() {
            // Allow a trailing comma before the next item/`}`.
            if matches!(
                self.peek_kind(),
                Some(TokenKind::RBrace) | Some(TokenKind::Uses) | Some(TokenKind::Test) | None
            ) {
                break;
            }
            participants.push(self.parse_qualified_name()?);
        }
        Ok(participants)
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
                Some(TokenKind::Actor) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_actor_decl() {
                        Ok(mut a) => {
                            a.documentation = doc;
                            a.trivia.leading = leading;
                            a.trivia.trailing = self.take_trailing_trivia();
                            items.push(CommonsItem::Actor(a));
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
                            "expected a `type`, `fn`, `uses`, `consumes`, `exports`, `capability`, `provides`, `service`, `agent`, or `actor` declaration, found {}",
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
                Some(TokenKind::Actor) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_actor_decl() {
                        Ok(mut a) => {
                            a.documentation = doc;
                            a.trivia.leading = leading;
                            a.trivia.trailing = self.take_trailing_trivia();
                            last_span = a.span;
                            items.push(CommonsItem::Actor(a));
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
                            "expected a `type`, `fn`, `uses`, `consumes`, `exports`, `capability`, `provides`, `service`, `agent`, or `actor` declaration, found {}",
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

    /// Parse an `adapter` body in either brace (`brace = true`) or fragment
    /// (`brace = false`) form (v0.17 §3.1). An adapter accepts a `binding`
    /// clause plus the same item set as a context; service, agent and
    /// bodied-provider placement is validated by the checker, not rejected
    /// here, so the diagnostics can be precise. v0.18 admits `consumes`
    /// (braced form, adapter targets — also checked semantically).
    fn parse_adapter_body(
        &mut self,
        start: Span,
        name: QualifiedName,
        documentation: Option<String>,
        brace: bool,
    ) -> Result<AdapterDecl, CompileError> {
        if brace {
            self.expect(TokenKind::LBrace, "after the adapter name")?;
        }
        let mut items = Vec::new();
        let mut uses = Vec::new();
        let mut exports = Vec::new();
        let mut consumes = Vec::new();
        let mut binding: Option<BindingDecl> = None;
        let mut last_span = start;
        let trailing_comments: Vec<String>;
        loop {
            let (mut leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::RBrace) if brace => {
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
                None if !brace => {
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
                Some(TokenKind::Binding) => {
                    let mut b = self.parse_binding_decl()?;
                    b.trivia.leading = leading;
                    b.trivia.trailing = self.take_trailing_trivia();
                    last_span = b.span;
                    if binding.is_some() {
                        let err = CompileError::new(
                            "karn.adapter.duplicate_binding",
                            b.span,
                            "an adapter may declare at most one `binding` clause",
                        );
                        self.handle_item_err(err)?;
                    } else {
                        binding = Some(b);
                    }
                }
                Some(TokenKind::Uses) => match self.parse_uses_decl() {
                    Ok(mut u) => {
                        u.trivia.leading = leading;
                        u.trivia.trailing = self.take_trailing_trivia();
                        last_span = u.span;
                        uses.push(u);
                    }
                    Err(e) => self.handle_item_err(e)?,
                },
                // v0.18: adapter-to-adapter capability dependencies. The braced-form
                // and adapter-target restrictions are checked semantically so the
                // diagnostics can be precise.
                Some(TokenKind::Consumes) => match self.parse_consumes_decl() {
                    Ok(mut c) => {
                        c.trivia.leading = leading;
                        c.trivia.trailing = self.take_trailing_trivia();
                        last_span = c.span;
                        consumes.push(c);
                    }
                    Err(e) => self.handle_item_err(e)?,
                },
                Some(TokenKind::Exports) => match self.parse_exports_decl() {
                    Ok(mut e) => {
                        e.trivia.leading = leading;
                        e.trivia.trailing = self.take_trailing_trivia();
                        last_span = e.span;
                        exports.push(e);
                    }
                    Err(e) => self.handle_item_err(e)?,
                },
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
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                // `service` and `agent` parse into items so the checker can
                // reject them precisely (`karn.adapter.disallowed_item`).
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
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Actor) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_actor_decl() {
                        Ok(mut a) => {
                            a.documentation = doc;
                            a.trivia.leading = leading;
                            a.trivia.trailing = self.take_trailing_trivia();
                            last_span = a.span;
                            items.push(CommonsItem::Actor(a));
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                _ => {
                    let t = match self.peek() {
                        Some(t) => t,
                        None => {
                            return Err(CompileError::new(
                                "karn.parse.unexpected_eof",
                                self.eof_span(),
                                "expected `}` to close the adapter body, found end of file",
                            ));
                        }
                    };
                    let err = CompileError::new(
                        "karn.parse.expected_item",
                        t.span,
                        format!(
                            "expected a `binding`, `type`, `fn`, `uses`, `consumes`, `exports`, `capability`, or `provides` declaration, found {}",
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
        let span = if brace {
            let end = self.expect(TokenKind::RBrace, "to close the adapter body")?;
            start.merge(end.span)
        } else {
            start.merge(last_span)
        };
        Ok(AdapterDecl {
            name,
            items,
            uses,
            exports,
            consumes,
            binding,
            documentation,
            form: if brace {
                CommonsForm::Brace
            } else {
                CommonsForm::Fragment
            },
            span,
            trivia: Trivia::default(),
            trailing_comments,
        })
    }

    /// Parse a `binding "<module>" requires { "pkg": "range", … }` clause
    /// (v0.17 §3.5). The `requires { … }` map is optional.
    fn parse_binding_decl(&mut self) -> Result<BindingDecl, CompileError> {
        let kw = self.expect(TokenKind::Binding, "to start a `binding` declaration")?;
        let mod_tok = self.expect(
            TokenKind::StrLit,
            "the binding module path as a string literal",
        )?;
        let module = parse_string_literal(self.slice(mod_tok.span), mod_tok.span)?;
        let mut span = kw.span.merge(mod_tok.span);
        let mut requires = Vec::new();
        if self.peek_kind() == Some(TokenKind::Ident)
            && self.slice(self.peek().unwrap().span) == "requires"
        {
            self.bump(); // `requires`
            self.expect(TokenKind::LBrace, "to open the `requires` map")?;
            loop {
                match self.peek_kind() {
                    Some(TokenKind::RBrace) => break,
                    Some(TokenKind::StrLit) => {
                        let pkg_tok = self.bump().unwrap();
                        let package = parse_string_literal(self.slice(pkg_tok.span), pkg_tok.span)?;
                        self.expect(TokenKind::Colon, "after the package name")?;
                        let range_tok = self
                            .expect(TokenKind::StrLit, "the version range as a string literal")?;
                        let range =
                            parse_string_literal(self.slice(range_tok.span), range_tok.span)?;
                        requires.push(RequiresDep {
                            package,
                            range,
                            span: pkg_tok.span.merge(range_tok.span),
                        });
                        // optional trailing comma between entries
                        self.eat(TokenKind::Comma);
                    }
                    _ => {
                        let t = self.peek().unwrap();
                        return Err(CompileError::new(
                            "karn.parse.expected_item",
                            t.span,
                            format!(
                                "expected a `\"package\": \"range\"` entry or `}}` in the `requires` map, found {}",
                                t.kind.describe()
                            ),
                        ));
                    }
                }
            }
            let close = self.expect(TokenKind::RBrace, "to close the `requires` map")?;
            span = span.merge(close.span);
        }
        Ok(BindingDecl {
            module,
            module_span: mod_tok.span,
            requires,
            span,
            trivia: Trivia::default(),
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

    /// v0.45: an actor declaration — a nominal boundary contract.
    ///
    /// Normal form: `actor Name { auth = Scheme }` or
    /// `actor Name { auth = Scheme, identity = Type }`.
    ///
    /// Reserved refinement form (parsed, rejected in the checker):
    /// `actor Admin = User where <predicate>`.
    fn parse_actor_decl(&mut self) -> Result<ActorDecl, CompileError> {
        let kw = self.expect(TokenKind::Actor, "to start an actor declaration")?;
        let name = self.expect_ident("after `actor`")?;

        // Refinement form: `actor Name = Base where <predicate>` (Q3). Parsed so
        // the grammar is fixed now; the checker emits
        // `karn.actor.refinement_unsupported`.
        if self.peek_kind() == Some(TokenKind::Eq) {
            self.bump();
            let base = self.expect_ident("as the base actor after `=`")?;
            self.expect(TokenKind::Where, "before the actor refinement predicate")?;
            let predicate = self.parse_expr()?;
            let span = kw.span.merge(predicate.span);
            return Ok(ActorDecl {
                name,
                auth: None,
                auth_config: Vec::new(),
                identity: None,
                refinement: Some(ActorRefinement {
                    base,
                    predicate,
                    span,
                }),
                documentation: None,
                span,
                trivia: Trivia::default(),
            });
        }

        // Normal form: `actor Name { auth = Scheme (, identity = Type)? }`.
        self.expect(TokenKind::LBrace, "to open the actor body")?;
        let auth_kw = self.expect_ident("expected `auth` to start the actor body")?;
        if auth_kw.name != "auth" {
            return Err(CompileError::new(
                "karn.parse.expected_token",
                auth_kw.span,
                format!(
                    "expected `auth` in the actor body, found `{}`",
                    auth_kw.name
                ),
            )
            .with_note("an actor body begins with `auth = <Scheme>`"));
        }
        self.expect(TokenKind::Eq, "after `auth`")?;
        // The scheme name is an identifier, except `None` (which is also the
        // `Option` keyword) — accept that token here as the scheme name.
        let auth = if self.peek_kind() == Some(TokenKind::None) {
            let t = self.expect(TokenKind::None, "as the authentication scheme")?;
            Ident {
                name: "None".to_string(),
                span: t.span,
            }
        } else {
            self.expect_ident("as the authentication scheme after `auth =`")?
        };

        // v0.47/v0.51: a scheme may carry a keyed config —
        // `Scheme(key = value, …)` (e.g. `Bearer(secret = "…")`,
        // `Signature(secret = "…", header = "…", timestamp = "…", tolerance =
        // 300)`). Values are string or integer literals; the checker validates
        // which keys each scheme requires/allows.
        let mut auth_config = Vec::new();
        if self.peek_kind() == Some(TokenKind::LParen) {
            self.bump();
            loop {
                let key = self.expect_ident("as a scheme config key")?;
                self.expect(TokenKind::Eq, "after the scheme config key")?;
                let (value, vspan) = match self.peek_kind() {
                    Some(TokenKind::StrLit) => {
                        let t = self.expect(TokenKind::StrLit, "as a scheme config value")?;
                        (
                            SchemeArgValue::Str(parse_string_literal(self.slice(t.span), t.span)?),
                            t.span,
                        )
                    }
                    Some(TokenKind::IntLit) => {
                        let t = self.expect(TokenKind::IntLit, "as a scheme config value")?;
                        let n: i64 = self.slice(t.span).parse().map_err(|_| {
                            CompileError::new(
                                "karn.parse.expected_token",
                                t.span,
                                "invalid integer in scheme config".to_string(),
                            )
                        })?;
                        (SchemeArgValue::Int(n), t.span)
                    }
                    _ => {
                        let t = self.peek();
                        return Err(CompileError::new(
                            "karn.parse.expected_token",
                            t.map(|t| t.span).unwrap_or_else(|| self.eof_span()),
                            "expected a string or integer scheme config value".to_string(),
                        ));
                    }
                };
                auth_config.push(SchemeArg {
                    key,
                    value,
                    span: vspan,
                });
                if self.eat(TokenKind::Comma).is_none() {
                    break;
                }
                if self.peek_kind() == Some(TokenKind::RParen) {
                    break; // trailing comma
                }
            }
            self.expect(TokenKind::RParen, "to close the scheme config")?;
        }

        let mut identity = None;
        if self.eat(TokenKind::Comma).is_some() {
            let id_kw = self.expect_ident("expected `identity` after `,`")?;
            if id_kw.name != "identity" {
                return Err(CompileError::new(
                    "karn.parse.expected_token",
                    id_kw.span,
                    format!("expected `identity`, found `{}`", id_kw.name),
                )
                .with_note("the only actor field after `auth` is `identity = <Type>`"));
            }
            self.expect(TokenKind::Eq, "after `identity`")?;
            identity = Some(self.parse_type_ref("as the actor identity type")?);
        }

        let close = self.expect(TokenKind::RBrace, "to close the actor body")?;
        let span = kw.span.merge(close.span);
        Ok(ActorDecl {
            name,
            auth: Some(auth),
            auth_config,
            identity,
            refinement: None,
            documentation: None,
            span,
            trivia: Trivia::default(),
        })
    }

    fn parse_service_decl(&mut self) -> Result<ServiceDecl, CompileError> {
        let kw = self.expect(TokenKind::Service, "to start a service declaration")?;
        let name = self.expect_ident("after `service`")?;
        let protocol = self.parse_service_protocol()?;
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
            protocol,
            handlers,
            documentation: None,
            span: kw.span.merge(close.span),
            trivia: Trivia::default(),
        })
    }

    /// Parse the optional `from <protocol>` header clause (v0.44). Absent ⇒
    /// `Call` (the contract-mediated internal-RPC default). `from queue("name")`
    /// carries its bound queue; `from http`/`from cron` carry no binding.
    fn parse_service_protocol(&mut self) -> Result<ServiceProtocol, CompileError> {
        if self.eat(TokenKind::From).is_none() {
            return Ok(ServiceProtocol::Call);
        }
        match self.peek_kind() {
            Some(TokenKind::Http) => {
                self.bump();
                Ok(ServiceProtocol::Http)
            }
            Some(TokenKind::Cron) => {
                self.bump();
                Ok(ServiceProtocol::Cron)
            }
            Some(TokenKind::Queue) => {
                self.bump();
                self.expect(
                    TokenKind::LParen,
                    "expected a queue binding `(\"name\")` after `from queue`",
                )?;
                let name_tok = self.expect(
                    TokenKind::StrLit,
                    "expected the bound queue name as a string literal",
                )?;
                let name = parse_string_literal(self.slice(name_tok.span), name_tok.span)?;
                self.expect(TokenKind::RParen, "to close the queue binding")?;
                Ok(ServiceProtocol::Queue { name })
            }
            _ => {
                let (span, found) = match self.peek() {
                    Some(t) => (t.span, t.kind.describe()),
                    None => (self.eof_span(), "end of file"),
                };
                Err(CompileError::new(
                    "karn.service.unknown_protocol",
                    span,
                    format!(
                        "unknown protocol after `from` — found {found}, expected `http`, `cron`, or `queue`"
                    ),
                )
                .with_note(
                    "protocols are a closed set; Kafka and MQTT are transports, not protocols — \
                     use `from queue` and bind the broker at the platform layer",
                ))
            }
        }
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
        // v0.44: the handler form is a single ident after `on` — `call`, an
        // HTTP method-builder (`GET("/route")`), `schedule("expr")`, or
        // `message(...)`. The protocol lives on the service header; the checker
        // verifies the form matches it.
        let kind_ident = self.expect_ident(
            "expected a handler form (e.g. `call`, `GET`, `schedule`, `message`) after `on`",
        )?;
        let kind = match kind_ident.name.as_str() {
            "call" => HandlerKind::Call,
            "message" => HandlerKind::Message,
            "schedule" => {
                self.expect(TokenKind::LParen, "before the cron schedule expression")?;
                let expr_tok = self.expect(
                    TokenKind::StrLit,
                    "expected a cron expression string literal in `schedule(\"…\")`",
                )?;
                let expr = parse_string_literal(self.slice(expr_tok.span), expr_tok.span)?;
                self.expect(TokenKind::RParen, "to close the schedule expression")?;
                HandlerKind::Cron { expr }
            }
            method if HttpMethod::from_ident(method).is_some() => {
                let method = HttpMethod::from_ident(method).unwrap();
                self.expect(TokenKind::LParen, "before the route pattern")?;
                let path_tok = self.expect(
                    TokenKind::StrLit,
                    "expected a route pattern string literal in `GET(\"…\")`",
                )?;
                let path = parse_string_literal(self.slice(path_tok.span), path_tok.span)?;
                self.expect(TokenKind::RParen, "to close the route pattern")?;
                HandlerKind::Http { method, path }
            }
            other => {
                return Err(CompileError::new(
                    "karn.parse.unknown_handler_kind",
                    kind_ident.span,
                    format!(
                        "unknown handler form `{other}` — expected `call`, an HTTP method (`GET`/`POST`/`PUT`/`PATCH`/`DELETE`), `schedule`, or `message`"
                    ),
                )
                .with_note(
                    "use `on call(...)`, `on GET(\"/path\") (...)`, `on schedule(\"expr\") (...)`, or `on message(m: T)`",
                ));
            }
        };
        // Only `on call` handlers are valid inside an agent.
        if is_agent && !matches!(kind, HandlerKind::Call) {
            return Err(CompileError::new(
                "karn.parse.handler_in_agent",
                kind_ident.span,
                "only `on call` handlers are valid inside an `agent`; protocol handlers belong on a `service`",
            )
            .with_note(
                "agents persist state and respond to `on call`; HTTP routes, schedules, and queue messages belong on services",
            ));
        }
        // Agent handlers have a method name before the parameter list:
        //   on call addItem(item: CartItem) -> ...
        // Service handlers have just the parameter list:
        //   on call(amount: Money) -> ...
        let method_name = if is_agent && self.peek_kind() == Some(TokenKind::Ident) {
            Some(self.expect_ident("as the agent handler operation name")?)
        } else {
            None
        };
        // v0.45/v0.50: the optional `by (<binder>:)? <Actor>` clause sits after
        // the protocol config and before the parameters. `by <name>: <Actor>`
        // captures the verified identity (read as `name.identity`);
        // `by <Actor>` declares-and-verifies the contract without capturing it
        // (anonymous / verify-and-discard). One-token lookahead on `:`
        // disambiguates.
        let by_clause = if self.peek_kind() == Some(TokenKind::By) {
            let by_kw = self.expect(TokenKind::By, "to start the handler actor clause")?;
            // `_` is not a valid binder — guide to the binder-less form.
            if self.peek_kind() == Some(TokenKind::Underscore) {
                let t = self.peek().unwrap();
                return Err(CompileError::new(
                    "karn.parse.expected_token",
                    t.span,
                    "`_` is not a valid actor binder".to_string(),
                )
                .with_note("omit the binder for an anonymous handler — `by <Actor>`"));
            }
            let first = self.expect_ident("as the actor (or its binder) after `by`")?;
            // A `:` after the first name means it was the binder; otherwise the
            // first name *is* the actor (binder-less form).
            let (binder, mut actors) = if self.peek_kind() == Some(TokenKind::Colon) {
                self.bump();
                (
                    Some(first),
                    vec![self.expect_ident("as the actor contract name after `:`")?],
                )
            } else {
                (None, vec![first])
            };
            // v0.52: a `|`-separated list names an ordered sum of peer actors
            // (`by who: A | B`), resolved first-wins. One name is the ordinary
            // single-actor handler. The binder requirement for a sum is a
            // semantic rule (`karn.actor.sum_requires_binder`), not a parse one.
            while self.eat(TokenKind::Pipe).is_some() {
                actors.push(self.expect_ident("as a peer actor after `|`")?);
            }
            let last = actors.last().unwrap();
            let span = by_kw.span.merge(last.span);
            Some(ByClause {
                binder,
                actors,
                span,
            })
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
            by_clause,
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
