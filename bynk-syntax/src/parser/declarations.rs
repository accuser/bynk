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
            Some(TokenKind::Suite) => {
                let start = self.expect(TokenKind::Suite, "to start the suite declaration")?;
                let doc = self.finalize_doc(leading_doc, start.span);
                let name = self.parse_qualified_name()?;
                // v0.118: an optional `as <tier>` sets the suite's default tier,
                // which its `case` members inherit and override (a `property`
                // ignores it). `as` is unambiguous here — no `consumes` is in
                // scope in a suite header (DECISION N).
                let tier = if self.peek_kind() == Some(TokenKind::As) {
                    self.bump();
                    Some(self.parse_tier()?)
                } else {
                    None
                };
                let mut t = match self.peek_kind() {
                    Some(TokenKind::LBrace) => {
                        self.parse_test_brace(start.span, name, doc, tier)?
                    }
                    _ => self.parse_test_fragment(start.span, name, doc, tier)?,
                };
                t.trivia = header_trivia;
                Ok(SourceUnit::Suite(t))
            }
            Some(_) => {
                let t = self.peek().unwrap();
                if let Some((_, doc_span)) = leading_doc {
                    self.warnings.push(CompileError::new(
                        "bynk.parse.orphan_doc_block",
                        doc_span,
                        "documentation block has no following declaration to attach to",
                    ));
                }
                Err(CompileError::new(
                    "bynk.parse.expected_unit_header",
                    t.span,
                    format!(
                        "expected `commons`, `context`, or `suite` to start the file, found {}",
                        t.kind.describe()
                    ),
                )
                .with_note(
                    "every `.bynk` file begins with either a `commons`, `context`, or `suite` declaration",
                ))
            }
            None => {
                if let Some((_, doc_span)) = leading_doc {
                    self.warnings.push(CompileError::new(
                        "bynk.parse.orphan_doc_block",
                        doc_span,
                        "documentation block has no following declaration to attach to",
                    ));
                }
                Err(CompileError::new(
                    "bynk.parse.unexpected_eof",
                    self.eof_span(),
                    "expected `commons`, `context`, or `suite` to start the file, found end of file",
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
                            "bynk.parse.orphan_doc_block",
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
                                "bynk.parse.orphan_doc_block",
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
                        "bynk.capability.outside_context",
                        self.peek().unwrap().span,
                        "`capability` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(TokenKind::Provides) => {
                    let err = CompileError::new(
                        "bynk.provider.outside_context",
                        self.peek().unwrap().span,
                        "`provides` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(TokenKind::Service) => {
                    let err = CompileError::new(
                        "bynk.service.outside_context",
                        self.peek().unwrap().span,
                        "`service` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(TokenKind::Agent) => {
                    let err = CompileError::new(
                        "bynk.agent.outside_context",
                        self.peek().unwrap().span,
                        "`agent` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(TokenKind::Actor) => {
                    let err = CompileError::new(
                        "bynk.actor.outside_context",
                        self.peek().unwrap().span,
                        "`actor` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(_) => {
                    let t = self.peek().unwrap();
                    let err = CompileError::new(
                        "bynk.parse.expected_item",
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
                        "bynk.parse.unexpected_eof",
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
        // Cover the header (`commons <name>`) so the unit span stays valid even
        // when every item is dropped by error recovery — the document-symbol
        // selection range (the name span) must remain contained in this span.
        let mut last_span = start.merge(name.span);
        let mut seen_item = false;
        let trailing_comments: Vec<String>;
        loop {
            let (mut leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::Uses) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(
                            CompileError::new(
                                "bynk.parse.orphan_doc_block",
                                doc_span,
                                "documentation block before `uses` is not allowed; only `type` and `fn` declarations carry docs",
                            ),
                        );
                    }
                    if seen_item {
                        let t = self.peek().unwrap();
                        return Err(CompileError::new(
                            "bynk.parse.uses_after_decls",
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
                            "bynk.parse.orphan_doc_block",
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
                        "bynk.capability.outside_context",
                        self.peek().unwrap().span,
                        "`capability` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(TokenKind::Provides) => {
                    let err = CompileError::new(
                        "bynk.provider.outside_context",
                        self.peek().unwrap().span,
                        "`provides` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(TokenKind::Service) => {
                    let err = CompileError::new(
                        "bynk.service.outside_context",
                        self.peek().unwrap().span,
                        "`service` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(TokenKind::Agent) => {
                    let err = CompileError::new(
                        "bynk.agent.outside_context",
                        self.peek().unwrap().span,
                        "`agent` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(TokenKind::Actor) => {
                    let err = CompileError::new(
                        "bynk.actor.outside_context",
                        self.peek().unwrap().span,
                        "`actor` declarations are only allowed inside a context, not a commons",
                    );
                    self.handle_item_err(err)?;
                }
                Some(_) => {
                    let t = self.peek().unwrap();
                    let err = CompileError::new(
                        "bynk.parse.expected_item",
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
                    "bynk.parse.expected_visibility",
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
                    "bynk.parse.unexpected_eof",
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
        tier: Option<TestTier>,
    ) -> Result<SuiteDecl, CompileError> {
        self.expect(TokenKind::LBrace, "after the test target name")?;
        let mut uses = Vec::new();
        let mut provides = Vec::new();
        let mut cases = Vec::new();
        let mut properties = Vec::new();
        let trailing_comments: Vec<String>;
        loop {
            let (mut leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::RBrace) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "bynk.parse.orphan_doc_block",
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
                            "bynk.parse.orphan_doc_block",
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
                Some(TokenKind::Provides) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_provides_clause() {
                        Ok(mut p) => {
                            p.documentation = doc;
                            p.trivia.leading = leading;
                            p.trivia.trailing = self.take_trailing_trivia();
                            provides.push(p);
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Case) => {
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
                Some(TokenKind::Property) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_property() {
                        Ok(mut p) => {
                            p.documentation = doc;
                            p.trivia.leading = leading;
                            p.trivia.trailing = self.take_trailing_trivia();
                            properties.push(p);
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(_) => {
                    let t = self.peek().unwrap();
                    let err = CompileError::new(
                        "bynk.parse.expected_item",
                        t.span,
                        format!(
                            "expected `uses`, `provides`, `case \"name\"`, or `property \"name\"` declaration, found {}",
                            t.kind.describe()
                        ),
                    )
                    .with_note(
                        "the body of a suite contains zero or more `uses`, `provides`, `case`, or `property` declarations",
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
                        "bynk.parse.unexpected_eof",
                        self.eof_span(),
                        "expected `}` to close the test body, found end of file",
                    ));
                }
            }
        }
        let end = self.expect(TokenKind::RBrace, "to close the test body")?;
        Ok(SuiteDecl {
            target,
            uses,
            provides,
            cases,
            properties,
            tier,
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
        tier: Option<TestTier>,
    ) -> Result<SuiteDecl, CompileError> {
        let mut uses = Vec::new();
        let mut provides = Vec::new();
        let mut cases = Vec::new();
        let mut properties = Vec::new();
        // Cover the header (`test <target>`) so the unit span stays valid even
        // when every item is dropped by error recovery (see commons fragment).
        let mut last_span = start.merge(target.span);
        let mut seen_non_uses = false;
        let trailing_comments: Vec<String>;
        loop {
            let (mut leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::Uses) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "bynk.parse.orphan_doc_block",
                            doc_span,
                            "documentation block before `uses` is not allowed",
                        ));
                    }
                    if seen_non_uses {
                        let t = self.peek().unwrap();
                        return Err(CompileError::new(
                            "bynk.parse.uses_after_decls",
                            t.span,
                            "`uses` clauses must appear before any `provides`, `case`, or `property` declarations in a fragment-form test",
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
                Some(TokenKind::Provides) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_provides_clause() {
                        Ok(mut p) => {
                            p.documentation = doc;
                            p.trivia.leading = leading;
                            p.trivia.trailing = self.take_trailing_trivia();
                            last_span = p.span;
                            provides.push(p);
                            seen_non_uses = true;
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                Some(TokenKind::Case) => {
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
                Some(TokenKind::Property) => {
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    match self.parse_property() {
                        Ok(mut p) => {
                            p.documentation = doc;
                            p.trivia.leading = leading;
                            p.trivia.trailing = self.take_trailing_trivia();
                            last_span = p.span;
                            properties.push(p);
                            seen_non_uses = true;
                        }
                        Err(e) => self.handle_item_err(e)?,
                    }
                }
                None => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "bynk.parse.orphan_doc_block",
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
                        "bynk.parse.expected_item",
                        t.span,
                        format!(
                            "expected `uses`, `provides`, `case \"name\"`, or `property \"name\"` declaration, found {}",
                            t.kind.describe()
                        ),
                    )
                    .with_note(
                        "in fragment-form suites, the body is a sequence of `uses`, `provides`, `case`, or `property` declarations to end of file",
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
        Ok(SuiteDecl {
            target,
            uses,
            provides,
            cases,
            properties,
            tier,
            form: CommonsForm::Fragment,
            documentation,
            span: start.merge(last_span),
            trivia: Trivia::default(),
            trailing_comments,
        })
    }

    /// v0.118: parse a tier name (`unit` / `integration` / `system`) after `as`.
    /// The names are contextual identifiers, not keywords, so they stay usable
    /// as ordinary identifiers everywhere else.
    fn parse_tier(&mut self) -> Result<TestTier, CompileError> {
        let tok = self.expect(TokenKind::Ident, "a tier name after `as`")?;
        match self.slice(tok.span) {
            "unit" => Ok(TestTier::Unit),
            "integration" => Ok(TestTier::Integration),
            "system" => Ok(TestTier::System),
            other => Err(CompileError::new(
                "bynk.parse.unknown_tier",
                tok.span,
                format!(
                    "`{other}` is not a test tier; expected `unit`, `integration`, or `system`"
                ),
            )
            .with_note(
                "a `case`/`suite` tier clause is `as unit`, `as integration`, or `as system`",
            )),
        }
    }

    /// v0.118: parse a `provides Cap.method(<args>) returns <v> | fails` clause
    /// (the leading `provides` has not yet been consumed).
    fn parse_provides_clause(&mut self) -> Result<ProvidesClause, CompileError> {
        let kw = self.expect(TokenKind::Provides, "to start a `provides` clause")?;
        let capability = self.expect_ident("the capability after `provides`")?;
        self.expect(
            TokenKind::Dot,
            "after the capability in a `provides` clause",
        )?;
        let method = self.expect_ident("the method name after `.`")?;
        self.expect(
            TokenKind::LParen,
            "after the method name in a `provides` clause",
        )?;
        let mut args = Vec::new();
        if self.peek_kind() != Some(TokenKind::RParen) {
            args.push(self.parse_arg_pattern()?);
            while self.eat(TokenKind::Comma).is_some() {
                if self.peek_kind() == Some(TokenKind::RParen) {
                    break;
                }
                args.push(self.parse_arg_pattern()?);
            }
        }
        self.expect(TokenKind::RParen, "to close the `provides` argument list")?;
        let rhs = self.parse_provides_rhs()?;
        let span = kw.span.merge(rhs.span());
        Ok(ProvidesClause {
            capability,
            method,
            args,
            rhs,
            documentation: None,
            span,
            trivia: Trivia::default(),
        })
    }

    /// One argument pattern in a `provides` call pattern: `_` or a value.
    fn parse_arg_pattern(&mut self) -> Result<ArgPattern, CompileError> {
        if self.peek_kind() == Some(TokenKind::Underscore) {
            let span = self.peek().unwrap().span;
            self.bump();
            Ok(ArgPattern::Any(span))
        } else {
            Ok(ArgPattern::Value(self.parse_expr()?))
        }
    }

    /// The right-hand side of a `provides` clause: `fails`, `returns <value>`,
    /// or `returns each [<outcome>, …]`. `returns`, `fails`, and `each` are
    /// contextual identifiers here.
    fn parse_provides_rhs(&mut self) -> Result<ProvidesRhs, CompileError> {
        let tok = self.expect(
            TokenKind::Ident,
            "`returns` or `fails` after the `provides` call pattern",
        )?;
        match self.slice(tok.span) {
            "fails" => Ok(ProvidesRhs::Fails(tok.span)),
            "returns" => {
                let is_each = self.peek_kind() == Some(TokenKind::Ident)
                    && self.slice(self.peek().unwrap().span) == "each";
                if is_each {
                    self.bump();
                    let open = self.expect(TokenKind::LBracket, "after `returns each`")?;
                    let mut outcomes = Vec::new();
                    if self.peek_kind() != Some(TokenKind::RBracket) {
                        outcomes.push(self.parse_seq_outcome()?);
                        while self.eat(TokenKind::Comma).is_some() {
                            if self.peek_kind() == Some(TokenKind::RBracket) {
                                break;
                            }
                            outcomes.push(self.parse_seq_outcome()?);
                        }
                    }
                    let close =
                        self.expect(TokenKind::RBracket, "to close the `returns each` sequence")?;
                    Ok(ProvidesRhs::ReturnsEach(outcomes, open.span.merge(close.span)))
                } else {
                    Ok(ProvidesRhs::Returns(self.parse_expr()?))
                }
            }
            other => Err(CompileError::new(
                "bynk.parse.expected_token",
                tok.span,
                format!("expected `returns` or `fails` in a `provides` clause, found `{other}`"),
            )
            .with_note(
                "a `provides` clause ends `returns <value>`, `returns each [<outcome>, …]`, or `fails`",
            )),
        }
    }

    /// One outcome in a `returns each` sequence: `fails` or a value.
    fn parse_seq_outcome(&mut self) -> Result<SeqOutcome, CompileError> {
        if self.peek_kind() == Some(TokenKind::Ident)
            && self.slice(self.peek().unwrap().span) == "fails"
        {
            let span = self.peek().unwrap().span;
            self.bump();
            Ok(SeqOutcome::Fails(span))
        } else {
            Ok(SeqOutcome::Value(self.parse_expr()?))
        }
    }

    fn parse_test_case(&mut self) -> Result<Case, CompileError> {
        let kw = self.expect(TokenKind::Case, "to start a test case")?;
        let name_tok = self.expect(TokenKind::StrLit, "as the test case name")?;
        let name = parse_string_literal(self.slice(name_tok.span), name_tok.span)?;
        // v0.118: an optional `as <tier>` after the case name.
        let tier = if self.peek_kind() == Some(TokenKind::As) {
            self.bump();
            Some(self.parse_tier()?)
        } else {
            None
        };
        // v0.118: case-scoped `provides` clauses lead the case body, before any
        // statements. The case opens its own brace so it can peel them off.
        let open = self.expect(TokenKind::LBrace, "to open the test case body")?;
        let mut provides = Vec::new();
        while self.peek_kind() == Some(TokenKind::Provides) {
            let (leading, item_doc) = self.collect_item_lead();
            let next_span = self.peek().unwrap().span;
            let doc = self.finalize_doc(item_doc, next_span);
            let mut p = self.parse_provides_clause()?;
            p.documentation = doc;
            p.trivia.leading = leading;
            p.trivia.trailing = self.take_trailing_trivia();
            provides.push(p);
        }
        let body = self.parse_block_rest(open.span)?;
        let span = kw.span.merge(body.span);
        Ok(Case {
            name,
            name_span: name_tok.span,
            tier,
            provides,
            body,
            documentation: None,
            span,
            trivia: Trivia::default(),
        })
    }

    /// v0.114 (testing track slice 2): a generative `property "name" { for all
    /// … }` block inside a suite.
    fn parse_property(&mut self) -> Result<PropertyDecl, CompileError> {
        let kw = self.expect(TokenKind::Property, "to start a property")?;
        let name_tok = self.expect(TokenKind::StrLit, "as the property name")?;
        let name = parse_string_literal(self.slice(name_tok.span), name_tok.span)?;
        self.expect(TokenKind::LBrace, "to open the property body")?;
        let forall = self.parse_for_all()?;
        let end = self.expect(TokenKind::RBrace, "to close the property body")?;
        Ok(PropertyDecl {
            name,
            name_span: name_tok.span,
            forall,
            documentation: None,
            span: kw.span.merge(end.span),
            trivia: Trivia::default(),
        })
    }

    /// `for all x: T, y: U [where <pred>] { … }` — the generative binder. Each
    /// binding's type supplies the runner's inhabitant space; `where` filters
    /// generated tuples before the body runs.
    fn parse_for_all(&mut self) -> Result<ForAll, CompileError> {
        // `for` and `all` are contextual identifiers, not keywords, so `all`
        // stays usable as a list combinator (`all(xs, p)`). Here they lead the
        // binder — validated by text.
        let start = self.expect(TokenKind::Ident, "to start a `for all` binder")?;
        if self.slice(start.span) != "for" {
            return Err(CompileError::new(
                "bynk.parse.expected_token",
                start.span,
                format!(
                    "expected `for` to start a `for all` binder, found `{}`",
                    self.slice(start.span)
                ),
            ));
        }
        let all_tok = self.expect(TokenKind::Ident, "after `for` in a `for all` binder")?;
        if self.slice(all_tok.span) != "all" {
            return Err(CompileError::new(
                "bynk.parse.expected_token",
                all_tok.span,
                format!(
                    "expected `all` after `for` in a `for all` binder, found `{}`",
                    self.slice(all_tok.span)
                ),
            ));
        }
        let mut bindings = Vec::new();
        loop {
            let name = self.expect_ident("as a `for all` binding name")?;
            self.expect(TokenKind::Colon, "after a `for all` binding name")?;
            let type_ref = self.parse_type_ref("as the type of a `for all` binding")?;
            bindings.push(ForAllBinding { name, type_ref });
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let where_pred = if self.eat(TokenKind::Where).is_some() {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let body = self.parse_block("to open the `for all` body")?;
        let span = start.span.merge(body.span);
        Ok(ForAll {
            bindings,
            where_pred,
            body,
            span,
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
                            "bynk.parse.orphan_doc_block",
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
                            "bynk.parse.orphan_doc_block",
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
                            "bynk.parse.orphan_doc_block",
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
                            "bynk.parse.orphan_doc_block",
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
                        "bynk.parse.expected_item",
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
                        "bynk.parse.unexpected_eof",
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
        // Cover the header (`context <name>`) so the unit span stays valid even
        // when every item is dropped by error recovery (see commons fragment).
        let mut last_span = start.merge(name.span);
        let mut seen_item = false;
        let trailing_comments: Vec<String>;
        loop {
            let (mut leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::Uses) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "bynk.parse.orphan_doc_block",
                            doc_span,
                            "documentation block before `uses` is not allowed; only `type` and `fn` declarations carry docs",
                        ));
                    }
                    if seen_item {
                        let t = self.peek().unwrap();
                        return Err(CompileError::new(
                            "bynk.parse.uses_after_decls",
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
                            "bynk.parse.orphan_doc_block",
                            doc_span,
                            "documentation block before `consumes` is not allowed; only `type` and `fn` declarations carry docs",
                        ));
                    }
                    if seen_item {
                        let t = self.peek().unwrap();
                        let err = CompileError::new(
                            "bynk.parse.consumes_after_decls",
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
                            "bynk.parse.orphan_doc_block",
                            doc_span,
                            "documentation block before `exports` is not allowed; only `type` and `fn` declarations carry docs",
                        ));
                    }
                    if seen_item {
                        let t = self.peek().unwrap();
                        let err = CompileError::new(
                            "bynk.parse.exports_after_decls",
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
                            "bynk.parse.orphan_doc_block",
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
                        "bynk.parse.expected_item",
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
        // Cover the header (`adapter <name>`) so the unit span stays valid even
        // when every item is dropped by error recovery (see commons fragment).
        let mut last_span = start.merge(name.span);
        let trailing_comments: Vec<String>;
        loop {
            let (mut leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::RBrace) if brace => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "bynk.parse.orphan_doc_block",
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
                            "bynk.parse.orphan_doc_block",
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
                            "bynk.adapter.duplicate_binding",
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
                // reject them precisely (`bynk.adapter.disallowed_item`).
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
                                "bynk.parse.unexpected_eof",
                                self.eof_span(),
                                "expected `}` to close the adapter body, found end of file",
                            ));
                        }
                    };
                    let err = CompileError::new(
                        "bynk.parse.expected_item",
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
        // v0.115: `requires` is a keyword (contract clauses). In a `binding`
        // it heads the optional dependency map (`requires { "pkg": "range" }`);
        // the two uses never overlap syntactically.
        if self.peek_kind() == Some(TokenKind::Requires) {
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
                            "bynk.parse.expected_item",
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
                            "bynk.parse.orphan_doc_block",
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
                        "bynk.parse.expected_capability_op",
                        t.span,
                        format!(
                            "expected `fn` to declare a capability operation, found {}",
                            t.kind.describe()
                        ),
                    ));
                }
                None => {
                    return Err(CompileError::new(
                        "bynk.parse.unexpected_eof",
                        self.eof_span(),
                        "expected `}` to close the capability body, found end of file",
                    ));
                }
            }
        }
        let close = self.expect(TokenKind::RBrace, "to close the capability body")?;
        if ops.is_empty() {
            return Err(CompileError::new(
                "bynk.parse.empty_capability",
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
                        "bynk.parse.expected_provider_op",
                        t.span,
                        format!(
                            "expected `fn` to declare a provider operation, found {}",
                            t.kind.describe()
                        ),
                    ));
                }
                None => {
                    return Err(CompileError::new(
                        "bynk.parse.unexpected_eof",
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
        // `bynk.actor.refinement_unsupported`.
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
                "bynk.parse.expected_token",
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
                                "bynk.parse.expected_token",
                                t.span,
                                "invalid integer in scheme config".to_string(),
                            )
                        })?;
                        (SchemeArgValue::Int(n), t.span)
                    }
                    _ => {
                        let t = self.peek();
                        return Err(CompileError::new(
                            "bynk.parse.expected_token",
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
                    "bynk.parse.expected_token",
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
        let mut cors: Option<CorsPolicy> = None;
        loop {
            let (leading, item_doc) = self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::RBrace) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "bynk.parse.orphan_doc_block",
                            doc_span,
                            "documentation block has no following handler to attach to",
                        ));
                    }
                    break;
                }
                // `cors { … }` is a contextual keyword (like `store`/`key`): the
                // identifier `cors` in service-body item position introduces the
                // CORS policy, so it stays usable as an ordinary identifier
                // elsewhere. At most one per service.
                Some(TokenKind::Ident) if self.peek_is_cors_kw() => {
                    let policy = self.parse_cors_policy(leading)?;
                    if cors.is_some() {
                        return Err(CompileError::new(
                            "bynk.parse.duplicate_cors",
                            policy.span,
                            "a service declares at most one `cors { }` policy",
                        ));
                    }
                    cors = Some(policy);
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
                        "bynk.parse.expected_handler",
                        t.span,
                        format!(
                            "expected `on` to start a handler, found {}",
                            t.kind.describe()
                        ),
                    ));
                }
                None => {
                    return Err(CompileError::new(
                        "bynk.parse.unexpected_eof",
                        self.eof_span(),
                        "expected `}` to close the service body, found end of file",
                    ));
                }
            }
        }
        let close = self.expect(TokenKind::RBrace, "to close the service body")?;
        if handlers.is_empty() {
            return Err(CompileError::new(
                "bynk.parse.empty_service",
                kw.span.merge(close.span),
                "a service must declare at least one handler",
            ));
        }
        Ok(ServiceDecl {
            name,
            protocol,
            cors,
            handlers,
            documentation: None,
            span: kw.span.merge(close.span),
            trivia: Trivia::default(),
        })
    }

    /// True when the next token is the contextual keyword `cors` (an identifier
    /// literally spelled `cors`), introducing the CORS policy section.
    fn peek_is_cors_kw(&self) -> bool {
        matches!(self.peek(), Some(t) if t.kind == TokenKind::Ident && self.slice(t.span) == "cors")
    }

    /// Parse a `cors { name: value, … }` policy (v0.131, ADR 0158). Fields are
    /// parsed leniently as `name: expr` pairs; the checker validates the field
    /// names (closed set) and the value shapes. A trailing comma is allowed and
    /// newlines separate fields, mirroring a record construction.
    fn parse_cors_policy(&mut self, leading: Vec<String>) -> Result<CorsPolicy, CompileError> {
        let kw = self.expect_ident("to start a `cors` policy")?;
        self.expect(TokenKind::LBrace, "to open the `cors` policy body")?;
        let mut fields: Vec<CorsField> = Vec::new();
        loop {
            self.collect_item_lead();
            match self.peek_kind() {
                Some(TokenKind::RBrace) => break,
                Some(_) => {
                    let name = self.expect_ident("as a `cors` policy field name")?;
                    self.expect(TokenKind::Colon, "after the `cors` field name")?;
                    let value = self.parse_expr()?;
                    let span = name.span.merge(value.span);
                    fields.push(CorsField { name, value, span });
                    let _ = self.eat(TokenKind::Comma);
                }
                None => {
                    return Err(CompileError::new(
                        "bynk.parse.unexpected_eof",
                        self.eof_span(),
                        "expected `}` to close the `cors` policy, found end of file",
                    ));
                }
            }
        }
        let close = self.expect(TokenKind::RBrace, "to close the `cors` policy body")?;
        Ok(CorsPolicy {
            fields,
            span: kw.span.merge(close.span),
            trivia: Trivia {
                leading,
                trailing: self.take_trailing_trivia(),
            },
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
            // v0.103 (real-time track slice 3): `from WebSocket(in: T, out: T)`.
            // `WebSocket` is a contextual identifier (not a keyword), like the
            // built-in type names.
            Some(TokenKind::Ident)
                if self
                    .peek()
                    .is_some_and(|t| self.slice(t.span) == "WebSocket") =>
            {
                self.bump();
                self.expect(
                    TokenKind::LParen,
                    "expected `(in: ClientFrame, out: ServerFrame)` after `from WebSocket`",
                )?;
                let in_label = self.expect_ident("expected `in:` in the WebSocket header")?;
                if in_label.name != "in" {
                    return Err(CompileError::new(
                        "bynk.service.websocket_header",
                        in_label.span,
                        "the WebSocket header binds frame types as `in: <type>, out: <type>`",
                    ));
                }
                self.expect(TokenKind::Colon, "after `in`")?;
                let in_type = self.parse_type_ref("as the WebSocket `in:` frame type")?;
                self.expect(TokenKind::Comma, "between the `in:` and `out:` frame types")?;
                let out_label = self.expect_ident("expected `out:` in the WebSocket header")?;
                if out_label.name != "out" {
                    return Err(CompileError::new(
                        "bynk.service.websocket_header",
                        out_label.span,
                        "the WebSocket header binds frame types as `in: <type>, out: <type>`",
                    ));
                }
                self.expect(TokenKind::Colon, "after `out`")?;
                let out_type = self.parse_type_ref("as the WebSocket `out:` frame type")?;
                self.expect(TokenKind::RParen, "to close the WebSocket header")?;
                Ok(ServiceProtocol::WebSocket { in_type, out_type })
            }
            _ => {
                let (span, found) = match self.peek() {
                    Some(t) => (t.span, t.kind.describe()),
                    None => (self.eof_span(), "end of file"),
                };
                Err(CompileError::new(
                    "bynk.service.unknown_protocol",
                    span,
                    format!(
                        "unknown protocol after `from` — found {found}, expected `http`, `cron`, `queue`, or `WebSocket`"
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
                "bynk.parse.expected_agent_key",
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
        // Agent body — a pinned four-phase parse (identity → storage → contracts
        // → behaviour). v0.81 (storage track): the storage phase is the legacy
        // `state { }` block and/or the successor `store` fields, which coexist
        // during the track (ADR 0108 D3). All body items are doc-prefixed, so a
        // single loop collects the lead once and dispatches; ordering guards keep
        // the phases pinned (storage before contracts/behaviour; invariants before
        // handlers).
        let mut store_fields: Vec<StoreField> = Vec::new();
        let mut invariants = Vec::new();
        let mut transitions = Vec::new();
        let mut handlers = Vec::new();
        loop {
            let (leading, item_doc) = self.collect_item_lead();
            let storage_closed =
                !invariants.is_empty() || !transitions.is_empty() || !handlers.is_empty();
            match self.peek_kind() {
                Some(TokenKind::RBrace) => {
                    if let Some((_, doc_span)) = item_doc {
                        self.warnings.push(CompileError::new(
                            "bynk.parse.orphan_doc_block",
                            doc_span,
                            "documentation block has no following declaration to attach to",
                        ));
                    }
                    break;
                }
                // `store` is a contextual keyword (like `key`): the literal
                // identifier `store` in agent-body item position introduces a
                // store field, so it stays usable as an ordinary identifier
                // elsewhere (e.g. a `cache.store` context).
                Some(TokenKind::Ident) if self.peek_is_store_kw() => {
                    if storage_closed {
                        return Err(self.storage_after_phase_err());
                    }
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    let mut sf = self.parse_store_field()?;
                    sf.documentation = doc;
                    sf.trivia.leading = leading;
                    sf.trivia.trailing = self.take_trailing_trivia();
                    store_fields.push(sf);
                }
                Some(TokenKind::Invariant) => {
                    if !handlers.is_empty() {
                        let t = self.peek().unwrap();
                        return Err(CompileError::new(
                            "bynk.parse.invariant_after_handler",
                            t.span,
                            "an `invariant` must be declared before the agent's handlers",
                        )
                        .with_note(
                            "invariants form a phase between the storage fields and the \
                             `on` handlers; move this invariant above the first handler",
                        ));
                    }
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    let mut inv = self.parse_invariant()?;
                    inv.documentation = doc;
                    inv.trivia.leading = leading;
                    inv.trivia.trailing = self.take_trailing_trivia();
                    invariants.push(inv);
                }
                Some(TokenKind::Transition) => {
                    if !handlers.is_empty() {
                        let t = self.peek().unwrap();
                        return Err(CompileError::new(
                            "bynk.parse.transition_after_handler",
                            t.span,
                            "a `transition` must be declared before the agent's handlers",
                        )
                        .with_note(
                            "step invariants form a phase between the storage fields and the \
                             `on` handlers (beside `invariant`); move this transition above the \
                             first handler",
                        ));
                    }
                    let next_span = self.peek().unwrap().span;
                    let doc = self.finalize_doc(item_doc, next_span);
                    let mut tr = self.parse_transition()?;
                    tr.documentation = doc;
                    tr.trivia.leading = leading;
                    tr.trivia.trailing = self.take_trailing_trivia();
                    transitions.push(tr);
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
                        "bynk.parse.expected_handler",
                        t.span,
                        format!(
                            "expected `on` to start a handler, found {}",
                            t.kind.describe()
                        ),
                    ));
                }
                None => {
                    return Err(CompileError::new(
                        "bynk.parse.unexpected_eof",
                        self.eof_span(),
                        "expected `}` to close the agent body, found end of file",
                    ));
                }
            }
        }
        let close = self.expect(TokenKind::RBrace, "to close the agent body")?;
        if store_fields.is_empty() {
            return Err(CompileError::new(
                "bynk.parse.expected_agent_storage",
                kw.span.merge(close.span),
                "an agent must declare its storage — it has no `store` fields",
            ));
        }
        if handlers.is_empty() {
            return Err(CompileError::new(
                "bynk.parse.empty_agent",
                kw.span.merge(close.span),
                "an agent must declare at least one handler",
            ));
        }
        Ok(AgentDecl {
            name,
            key_name,
            key_type,
            store_fields,
            invariants,
            transitions,
            handlers,
            documentation: None,
            span: kw.span.merge(close.span),
            trivia: Trivia::default(),
        })
    }

    /// The error for a `state`/`store` declaration appearing after the agent's
    /// invariants or handlers (the storage phase must come first).
    fn storage_after_phase_err(&self) -> CompileError {
        let t = self.peek().unwrap();
        CompileError::new(
            "bynk.parse.storage_after_phase",
            t.span,
            "agent storage (`state` / `store`) must be declared before the invariants and handlers",
        )
        .with_note("the agent body is ordered: key → storage → invariants → handlers")
    }

    /// Parse a single `store` field (v0.81): `store <name>: <Kind>[…] [= init]`.
    /// The kind is an ordinary type reference (`Cell[Int]`, `Map[K, V]`); the
    /// checker restricts which heads are storage kinds. Doc/trivia are attached
    /// by the caller. Access-pattern annotations are deferred (storage track Q3).
    /// True when the next token is the contextual keyword `store` (an identifier
    /// literally spelled `store`), introducing a storage-kind field.
    fn peek_is_store_kw(&self) -> bool {
        matches!(self.peek(), Some(t) if t.kind == TokenKind::Ident && self.slice(t.span) == "store")
    }

    fn parse_store_field(&mut self) -> Result<StoreField, CompileError> {
        let kw = self.expect_ident("to start a `store` field")?;
        let name = self.expect_ident("expected the store field name after `store`")?;
        self.expect(TokenKind::Colon, "after the store field name")?;
        let kind = self.parse_store_kind()?;
        let mut end = kind.span;
        // v0.85 (ADR 0111): zero or more `@name(args)` annotations sit between the
        // storage kind and the `=` initialiser. The name is any identifier (the
        // checker matches it against the closed registry), so an unknown
        // annotation is a checker diagnostic rather than a parse error.
        let mut annotations = Vec::new();
        while self.peek_kind() == Some(TokenKind::At) {
            let ann = self.parse_annotation()?;
            end = ann.span;
            annotations.push(ann);
        }
        let init = if self.eat(TokenKind::Eq).is_some() {
            let e = self.parse_expr()?;
            end = e.span;
            Some(e)
        } else {
            None
        };
        Ok(StoreField {
            name,
            kind,
            annotations,
            init,
            documentation: None,
            span: kw.span.merge(end),
            trivia: Trivia::default(),
        })
    }

    /// Parse one storage annotation (v0.85; ADR 0111): `@<name>` or
    /// `@<name>(<arg>, …)`. Each argument is an optional `label:` then a value
    /// expression (`by: orderId`, `5.minutes`). The empty `@name()` form is
    /// accepted and yields no arguments.
    fn parse_annotation(&mut self) -> Result<Annotation, CompileError> {
        let at = self.expect(TokenKind::At, "to start a storage annotation")?;
        let name = self.expect_ident("expected an annotation name after `@`")?;
        let mut end = name.span;
        let mut args = Vec::new();
        if self.eat(TokenKind::LParen).is_some() {
            if self.peek_kind() != Some(TokenKind::RParen) {
                loop {
                    args.push(self.parse_annotation_arg()?);
                    if self.eat(TokenKind::Comma).is_none() {
                        break;
                    }
                }
            }
            let close =
                self.expect(TokenKind::RParen, "to close the annotation's argument list")?;
            end = close.span;
        }
        Ok(Annotation {
            name,
            args,
            span: at.span.merge(end),
        })
    }

    /// Parse one annotation argument: an optional `label:` prefix then a value
    /// expression. A word directly followed by `:` is a label (`by: orderId`);
    /// anything else is a positional value (`5.minutes`). The label word may be a
    /// reserved keyword (`by` is one), so it is taken by lexeme rather than via
    /// [`expect_ident`] — annotation labels are a closed metadata vocabulary, not
    /// expression identifiers.
    fn parse_annotation_arg(&mut self) -> Result<AnnotationArg, CompileError> {
        let label = if self.peek_annotation_label() {
            let tok = self.bump().unwrap();
            let label = Ident {
                name: self.slice(tok.span).to_string(),
                span: tok.span,
            };
            self.expect(TokenKind::Colon, "after an annotation argument label")?;
            Some(label)
        } else {
            None
        };
        let value = self.parse_expr()?;
        let span = label
            .as_ref()
            .map(|l| l.span.merge(value.span))
            .unwrap_or(value.span);
        Ok(AnnotationArg { label, value, span })
    }

    /// True when the cursor is on an alphabetic word immediately followed by `:`
    /// — the `label:` form of an annotation argument. Accepts keyword tokens
    /// (e.g. `by`) since labels are not expression identifiers.
    fn peek_annotation_label(&self) -> bool {
        let Some(tok) = self.peek() else { return false };
        if self.tokens.get(self.pos + 1).map(|t| t.kind) != Some(TokenKind::Colon) {
            return false;
        }
        self.slice(tok.span)
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
    }

    /// Parse a storage kind applied to its element type(s): `Cell[Int]`,
    /// `Map[K, V]`, or a bare head. The head is any identifier; the checker
    /// restricts it to the closed kind catalogue.
    fn parse_store_kind(&mut self) -> Result<StoreKind, CompileError> {
        let head = self.expect_ident("as the storage kind (e.g. `Cell`, `Map`, `Log`)")?;
        let head_span = head.span;
        let mut end = head_span;
        let mut args = Vec::new();
        if self.eat(TokenKind::LBracket).is_some() {
            loop {
                args.push(self.parse_type_ref("as a storage-kind type argument")?);
                if self.eat(TokenKind::Comma).is_none() {
                    break;
                }
            }
            let close = self.expect(
                TokenKind::RBracket,
                "to close the storage-kind type arguments",
            )?;
            end = close.span;
        }
        Ok(StoreKind {
            head,
            args,
            span: head_span.merge(end),
        })
    }

    /// Parse a single invariant declaration (v0.80): `invariant <name>: <expr>`.
    /// The predicate is an ordinary expression (with `implies`/`is`) over the
    /// agent's state fields; well-formedness is the checker's job. Doc and
    /// trivia are attached by the caller (the inline-declaration doc convention).
    fn parse_invariant(&mut self) -> Result<Invariant, CompileError> {
        let kw = self.expect(TokenKind::Invariant, "to start an invariant declaration")?;
        let name = self.expect_ident("expected the invariant name after `invariant`")?;
        self.expect(TokenKind::Colon, "after the invariant name")?;
        let predicate = self.parse_expr()?;
        let span = kw.span.merge(predicate.span);
        Ok(Invariant {
            name,
            predicate,
            documentation: None,
            span,
            trivia: Trivia::default(),
        })
    }

    /// Parse a step invariant: `transition <name>: <predicate>` (v0.116, testing
    /// track slice 4). Mirrors [`parse_invariant`], but the predicate ranges over
    /// the contextual `old`/`new` state pair rather than the bare state fields.
    fn parse_transition(&mut self) -> Result<Transition, CompileError> {
        let kw = self.expect(TokenKind::Transition, "to start a transition declaration")?;
        let name = self.expect_ident("expected the transition name after `transition`")?;
        self.expect(TokenKind::Colon, "after the transition name")?;
        let predicate = self.parse_expr()?;
        let span = kw.span.merge(predicate.span);
        Ok(Transition {
            name,
            predicate,
            documentation: None,
            span,
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
            // v0.103: the WebSocket upgrade handler — `on open by … (params) …`.
            "open" => HandlerKind::Open,
            // v0.106 (slice 3b-iii): the WebSocket close handler. (`on message` on a
            // `from WebSocket` service reuses `HandlerKind::Message`; the checker
            // disambiguates the queue and WebSocket forms by the service protocol.)
            "close" => HandlerKind::Close,
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
                    "bynk.parse.unknown_handler_kind",
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
                "bynk.parse.handler_in_agent",
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
                    "bynk.parse.expected_token",
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
            // semantic rule (`bynk.actor.sum_requires_binder`), not a parse one.
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
