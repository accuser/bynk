//! Type parsing — type declarations (refined/opaque/record/sum), signed
//! numeric literals, and type references (`parse_type_ref`/`parse_type_atom`
//! incl. function-type arrows). Split out of `parser.rs` (ADR 0060) as a
//! further `impl Parser` block; the scanning core and the other concerns
//! stay in the parent module, reached as ancestor privates via `self`.

use super::*;

impl<'a> Parser<'a> {
    // -- type declarations --

    pub(crate) fn parse_type_decl(&mut self) -> Result<TypeDecl, CompileError> {
        let kw = self.expect(TokenKind::Type, "to start a type declaration")?;
        let name = self.expect_ident("after `type`")?;
        // v0.20a (Open-narrow): generic *type* declarations stay rejected —
        // `List`/`Map` (built-in) remain the only generic types.
        if self.peek_kind() == Some(TokenKind::LBracket) {
            let open = self.bump().unwrap();
            return Err(CompileError::new(
                "bynk.generics.no_generic_types",
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

    pub(crate) fn parse_record_field(&mut self) -> Result<RecordField, CompileError> {
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
                TokenKind::Duration => {
                    self.bump();
                    Ok((BaseType::Duration, t.span))
                }
                _ => Err(CompileError::new(
                    "bynk.parse.expected_base_type",
                    t.span,
                    format!(
                        "expected `Int`, `String`, `Bool`, `Float`, or `Duration`, found {}",
                        t.kind.describe()
                    ),
                )
                .with_note("type declarations are refined base types in v0")),
            },
            None => Err(CompileError::new(
                "bynk.parse.unexpected_eof",
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
                "bynk.parse.unexpected_eof",
                self.eof_span(),
                "expected a refinement predicate, found end of file",
            )
        })?;
        // Allow `Int` etc. through here would be wrong; predicate names are plain
        // identifiers (and not keywords).
        if t.kind != TokenKind::Ident {
            return Err(CompileError::new(
                "bynk.parse.expected_predicate",
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
                            "bynk.types.no_numeric_coercion",
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
                    "bynk.parse.unknown_predicate",
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
                "bynk.lex.integer_overflow",
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
        // v0.40 (ADR 0073): the bound span covers a leading `-` so the swap
        // quick-fix replaces `-10` as a unit.
        let minus = self.eat(TokenKind::Minus);
        let neg = minus.is_some();
        let span_of = |lit: Span| match &minus {
            Some(m) => m.span.merge(lit),
            None => lit,
        };
        match self.peek() {
            Some(t) if t.kind == TokenKind::IntLit => {
                self.bump();
                let slice = self.slice(t.span);
                let n: i64 = slice.parse().map_err(|_| {
                    CompileError::new(
                        "bynk.lex.integer_overflow",
                        t.span,
                        format!(
                            "integer literal `{slice}` is out of range for a 64-bit signed integer"
                        ),
                    )
                })?;
                Ok(SignedNumLit::Int(IntBound {
                    value: if neg { -n } else { n },
                    span: span_of(t.span),
                }))
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
                Ok(SignedNumLit::Float(FloatBound {
                    value,
                    lexeme,
                    span: span_of(t.span),
                }))
            }
            Some(t) => Err(CompileError::new(
                "bynk.parse.expected_token",
                t.span,
                format!(
                    "expected a numeric literal {ctx}, found {}",
                    t.kind.describe()
                ),
            )),
            None => Err(CompileError::new(
                "bynk.parse.unexpected_eof",
                self.eof_span(),
                format!("expected a numeric literal {ctx}, found end of file"),
            )),
        }
    }

    /// v0.20a: a type reference — an atom, a parenthesised group, or a
    /// function type. `->` is **right-associative** (`A -> B -> C` is
    /// `A -> (B -> C)`). A parenthesised list is a function-type parameter
    /// list when followed by `->`; a grouping when it holds exactly one type
    /// (`(A -> B)` — unwrapped, so the formatter canonicalises); and the unit
    /// type when empty and arrow-free. The disambiguation is deferred to the
    /// arrow peek, so no extra lookahead is needed.
    pub(crate) fn parse_type_ref(&mut self, ctx: &str) -> Result<TypeRef, CompileError> {
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
                "bynk.parse.expected_token",
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
                TokenKind::Duration => {
                    self.bump();
                    Ok(TypeRef::Base(BaseType::Duration, t.span))
                }
                TokenKind::Result => {
                    self.bump();
                    // Must be followed by `[T, E]`.
                    let lb = self.peek().map(|t| t.kind);
                    if lb != Some(TokenKind::LBracket) {
                        return Err(CompileError::new(
                            "bynk.parse.expected_token",
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
                            "bynk.parse.generic_arg_count",
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
                            "bynk.parse.expected_token",
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
                            "bynk.parse.expected_token",
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
                                "bynk.parse.expected_token",
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
                    // v0.44: `QueueResult` is a predeclared non-generic built-in.
                    if name == "QueueResult" {
                        return Ok(TypeRef::QueueResult(t.span));
                    }
                    // v0.20b: `List` and `Map` are predeclared built-in generics.
                    if name == "List" {
                        if self.peek_kind() != Some(TokenKind::LBracket) {
                            return Err(CompileError::new(
                                "bynk.parse.expected_token",
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
                                "bynk.parse.expected_token",
                                t.span,
                                "the built-in `Map` type requires two type arguments: `Map[K, V]`",
                            ));
                        }
                        self.bump();
                        let arg_k = self.parse_type_ref("as the first `Map` type argument")?;
                        if self.peek_kind() == Some(TokenKind::RBracket) {
                            let close = self.bump().unwrap();
                            return Err(CompileError::new(
                                "bynk.parse.generic_arg_count",
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
                    "bynk.parse.expected_type",
                    t.span,
                    format!("expected a type {ctx}, found {}", t.kind.describe()),
                )),
            },
            None => Err(CompileError::new(
                "bynk.parse.unexpected_eof",
                self.eof_span(),
                format!("expected a type {ctx}, found end of file"),
            )),
        }
    }
}
