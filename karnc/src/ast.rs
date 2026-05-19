//! Abstract syntax tree types for Karn v0 (spec §9.2).

use crate::span::Span;

/// An identifier with its source span.
#[derive(Debug, Clone)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

/// Comment trivia attached to a declaration or statement (v1.1 LSP spec
/// §3.5). The parser collects line comments from the token stream and
/// attaches them to nearby AST nodes so the formatter can re-emit them.
///
/// - `leading` holds comments that appear immediately above the node,
///   ordered top-to-bottom. Each entry is the body of one `--` line
///   (the text after the marker, with its original inline whitespace
///   preserved).
/// - `trailing` holds a single comment that appears on the same source
///   line as the node's final token (e.g. `expr  -- note`).
#[derive(Debug, Clone, Default)]
pub struct Trivia {
    pub leading: Vec<String>,
    pub trailing: Option<String>,
}

impl Trivia {
    pub fn is_empty(&self) -> bool {
        self.leading.is_empty() && self.trailing.is_none()
    }
}

/// A whole parsed commons source file.
///
/// In v0.3 a commons may be split across multiple files in a directory; the
/// resolver merges them into one logical commons. Each parsed AST instance
/// represents the contribution from a single source file.
#[derive(Debug, Clone)]
pub struct Commons {
    pub name: QualifiedName,
    pub items: Vec<CommonsItem>,
    /// `uses` clauses declared in this file.
    pub uses: Vec<UsesDecl>,
    /// Optional documentation block attached to the commons declaration.
    pub documentation: Option<String>,
    /// Surface form of the file: brace-delimited body or headerless fragment.
    pub form: CommonsForm,
    pub span: Span,
    /// Trivia attached to the commons declaration itself — leading comments
    /// before the `commons` keyword and a trailing comment after the header
    /// or closing brace.
    pub trivia: Trivia,
    /// Comments appearing after the last item but before the file ends
    /// (or the closing brace, for brace form). One entry per `--` line.
    pub trailing_comments: Vec<String>,
}

/// The two surface forms in which a commons body may be parsed (v0.3 §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommonsForm {
    /// `commons name { ... }`
    Brace,
    /// `commons name` followed by top-level declarations to EOF.
    Fragment,
}

/// A `uses other.commons` declaration (v0.3 §3.3).
#[derive(Debug, Clone)]
pub struct UsesDecl {
    pub target: QualifiedName,
    pub span: Span,
    pub trivia: Trivia,
}

/// A whole parsed context source file (v0.4 §3.1).
///
/// Contexts are the architectural-layer declaration kind. Like commons, a
/// context may be split across multiple files in a directory.
#[derive(Debug, Clone)]
pub struct Context {
    pub name: QualifiedName,
    pub items: Vec<CommonsItem>,
    /// `uses` clauses declared in this file.
    pub uses: Vec<UsesDecl>,
    /// `consumes` clauses declared in this file.
    pub consumes: Vec<ConsumesDecl>,
    /// `exports` clauses declared in this file.
    pub exports: Vec<ExportsDecl>,
    /// Optional documentation block attached to the context declaration.
    pub documentation: Option<String>,
    /// Surface form of the file: brace-delimited body or headerless fragment.
    pub form: CommonsForm,
    pub span: Span,
    /// Trivia attached to the context declaration itself — leading comments
    /// before the `context` keyword.
    pub trivia: Trivia,
    /// Comments appearing after the last item but before the file ends
    /// (or the closing brace, for brace form). One entry per `--` line.
    pub trailing_comments: Vec<String>,
}

/// A `consumes other.context` declaration (v0.4 §3.2).
#[derive(Debug, Clone)]
pub struct ConsumesDecl {
    pub target: QualifiedName,
    pub span: Span,
    pub trivia: Trivia,
}

/// An `exports visibility { names }` clause (v0.4 §3.3).
#[derive(Debug, Clone)]
pub struct ExportsDecl {
    pub visibility: Visibility,
    pub names: Vec<Ident>,
    pub span: Span,
    pub trivia: Trivia,
}

/// Visibility level for an exports clause (v0.4 §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Token-only outside the context: hold, pass, compare; no inspect, no construct.
    Opaque,
    /// Readable shape outside the context: inspect fields, match variants; no construct.
    Transparent,
}

/// Either a commons or a context — the two declaration kinds at the file
/// level (v0.4 §3.1).
#[derive(Debug, Clone)]
pub enum SourceUnit {
    Commons(Commons),
    Context(Context),
}

impl SourceUnit {
    pub fn name(&self) -> &QualifiedName {
        match self {
            SourceUnit::Commons(c) => &c.name,
            SourceUnit::Context(c) => &c.name,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            SourceUnit::Commons(c) => c.span,
            SourceUnit::Context(c) => c.span,
        }
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            SourceUnit::Commons(_) => "commons",
            SourceUnit::Context(_) => "context",
        }
    }
}

/// A dotted name like `fitness.units`.
#[derive(Debug, Clone)]
pub struct QualifiedName {
    pub parts: Vec<Ident>,
    pub span: Span,
}

impl QualifiedName {
    pub fn joined(&self) -> String {
        self.parts
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(".")
    }
}

#[derive(Debug, Clone)]
pub enum CommonsItem {
    Type(TypeDecl),
    Fn(FnDecl),
    /// `capability Name { fn op(...) -> T ... }` (v0.5; contexts only).
    Capability(CapabilityDecl),
    /// `provides Cap = ProviderName { fn op(...) -> T { ... } ... }` (v0.5).
    Provider(ProviderDecl),
    /// `service Name { on call(...) -> T { ... } ... }` (v0.5).
    Service(ServiceDecl),
    /// `agent Name { key id: T; state { ... }; on call ... }` (v0.5).
    Agent(AgentDecl),
}

impl CommonsItem {
    pub fn name(&self) -> &Ident {
        match self {
            CommonsItem::Type(t) => &t.name,
            CommonsItem::Fn(f) => f.name.ident(),
            CommonsItem::Capability(c) => &c.name,
            CommonsItem::Provider(p) => &p.provider_name,
            CommonsItem::Service(s) => &s.name,
            CommonsItem::Agent(a) => &a.name,
        }
    }
}

/// A capability declaration (v0.5 §3.3). Capabilities are interface-like
/// contracts for external dependencies, used inside contexts. They may only
/// appear inside a `context` declaration.
#[derive(Debug, Clone)]
pub struct CapabilityDecl {
    pub name: Ident,
    pub ops: Vec<CapabilityOp>,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// One operation in a capability (signature only; no body).
#[derive(Debug, Clone)]
pub struct CapabilityOp {
    pub name: Ident,
    pub params: Vec<Param>,
    pub return_type: TypeRef,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// A provider declaration (v0.5 §3.4). Supplies an implementation for a
/// capability.
#[derive(Debug, Clone)]
pub struct ProviderDecl {
    /// The capability being implemented.
    pub capability: Ident,
    /// The provider's identifier (used in tests/config to select impls).
    pub provider_name: Ident,
    pub ops: Vec<ProviderOp>,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// One operation in a provider (signature plus body).
#[derive(Debug, Clone)]
pub struct ProviderOp {
    pub name: Ident,
    pub params: Vec<Param>,
    pub return_type: TypeRef,
    pub body: Block,
    pub span: Span,
    pub trivia: Trivia,
}

/// A service declaration (v0.5 §3.5). Services are the boundary interface
/// of a context.
#[derive(Debug, Clone)]
pub struct ServiceDecl {
    pub name: Ident,
    pub handlers: Vec<Handler>,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// An agent declaration (v0.5 §3.6). Agents are state-bearing entities
/// with their own handlers.
#[derive(Debug, Clone)]
pub struct AgentDecl {
    pub name: Ident,
    /// `key id: Type` — the identifier-typed value identifying instances.
    pub key_name: Ident,
    pub key_type: TypeRef,
    /// State fields — a record-shaped declaration of persistent state.
    pub state_fields: Vec<RecordField>,
    pub state_span: Span,
    pub handlers: Vec<Handler>,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// A handler block — `on call(args) -> T given C1, C2 { body }`.
/// Used by both services and agents.
#[derive(Debug, Clone)]
pub struct Handler {
    pub kind: HandlerKind,
    /// For agent handlers, the method-style handler name (e.g.
    /// `on call addItem(...)`). For service handlers, this is None (just
    /// `on call(...)`).
    pub method_name: Option<Ident>,
    pub params: Vec<Param>,
    pub return_type: TypeRef,
    pub given: Vec<Ident>,
    pub body: Block,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerKind {
    /// `on call(...)` — typed RPC (the only kind in v0.5).
    Call,
}

#[derive(Debug, Clone)]
pub struct TypeDecl {
    pub name: Ident,
    pub body: TypeBody,
    /// Documentation block attached to this declaration (v0.3).
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// The right-hand side of a `type` declaration. In v0/v0.1 only the
/// `Refined` variant existed; v0.2 adds records and sums; v0.3 adds opaque.
#[derive(Debug, Clone)]
pub enum TypeBody {
    /// Refined base type: `BaseType where refinement`.
    Refined {
        base: BaseType,
        base_span: Span,
        refinement: Option<Refinement>,
    },
    /// Record type: `{ field: T where ..., ... }`.
    Record(RecordBody),
    /// Sum type: pipe-form variants or `enum { ... }` shorthand.
    Sum(SumBody),
    /// Opaque base type: `opaque BaseType (where refinement)?` (v0.3 §3.4).
    /// Identity is nominal; the base type is hidden outside the defining commons.
    Opaque {
        base: BaseType,
        base_span: Span,
        refinement: Option<Refinement>,
    },
}

/// Body of a record-type declaration (v0.2 §3.1).
#[derive(Debug, Clone)]
pub struct RecordBody {
    pub fields: Vec<RecordField>,
    pub span: Span,
}

/// One field of a record type declaration. Each field may carry inline
/// refinement, which is enforced at construction time on the field's value.
#[derive(Debug, Clone)]
pub struct RecordField {
    pub name: Ident,
    pub type_ref: TypeRef,
    pub refinement: Option<Refinement>,
    pub span: Span,
}

/// Body of a sum-type declaration (v0.2 §3.2).
#[derive(Debug, Clone)]
pub struct SumBody {
    pub variants: Vec<Variant>,
    pub span: Span,
}

/// One variant of a sum type. Variants may have payload fields; a
/// payload-less variant is a simple tag.
#[derive(Debug, Clone)]
pub struct Variant {
    pub name: Ident,
    pub payload: Vec<VariantField>,
    pub span: Span,
}

/// One payload field of a sum variant. Variant payload fields use named
/// declarations like record fields, but do not carry refinement in v0.2.
#[derive(Debug, Clone)]
pub struct VariantField {
    pub name: Ident,
    pub type_ref: TypeRef,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseType {
    Int,
    String,
    Bool,
}

impl BaseType {
    pub fn name(self) -> &'static str {
        match self {
            BaseType::Int => "Int",
            BaseType::String => "String",
            BaseType::Bool => "Bool",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Refinement {
    pub predicates: Vec<RefinementPred>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct RefinementPred {
    pub kind: PredKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PredKind {
    Matches(String),
    InRange(i64, i64),
    MinLength(i64),
    MaxLength(i64),
    Length(i64),
    NonNegative,
    Positive,
    NonEmpty,
}

impl PredKind {
    pub fn name(&self) -> &'static str {
        match self {
            PredKind::Matches(_) => "Matches",
            PredKind::InRange(..) => "InRange",
            PredKind::MinLength(_) => "MinLength",
            PredKind::MaxLength(_) => "MaxLength",
            PredKind::Length(_) => "Length",
            PredKind::NonNegative => "NonNegative",
            PredKind::Positive => "Positive",
            PredKind::NonEmpty => "NonEmpty",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FnDecl {
    /// Free function or method (`TypeName.methodName`). See [`FnName`].
    pub name: FnName,
    pub params: Vec<Param>,
    pub return_type: TypeRef,
    pub body: Block,
    /// True when the first parameter is the special `self` parameter. Only
    /// valid for method declarations.
    pub has_self: bool,
    /// Documentation block attached to this declaration (v0.3).
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// A function-declaration name: either a free function `f` or a method
/// `T.method` (v0.2 §3.6).
#[derive(Debug, Clone)]
pub enum FnName {
    /// `fn name(...)` — a free function.
    Free(Ident),
    /// `fn TypeName.methodName(...)` — a method attached to a type.
    Method {
        type_name: Ident,
        method_name: Ident,
    },
}

impl FnName {
    /// The function's short name for diagnostics. For methods returns the
    /// method portion only; the type prefix is recovered via `type_name`.
    pub fn ident(&self) -> &Ident {
        match self {
            FnName::Free(id) => id,
            FnName::Method { method_name, .. } => method_name,
        }
    }

    /// For methods, the attached type's identifier; `None` for free fns.
    pub fn type_name(&self) -> Option<&Ident> {
        match self {
            FnName::Free(_) => None,
            FnName::Method { type_name, .. } => Some(type_name),
        }
    }

    /// The displayed full name (e.g., `Money.add` or `parseSku`).
    pub fn display(&self) -> String {
        match self {
            FnName::Free(id) => id.name.clone(),
            FnName::Method {
                type_name,
                method_name,
            } => format!("{}.{}", type_name.name, method_name.name),
        }
    }
}

/// A brace-delimited block of statements ending in a tail expression
/// whose value is the block's value (spec v0.1 §3.1).
#[derive(Debug, Clone)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub tail: Box<Expr>,
    pub span: Span,
    /// Line comments that appear between the last statement (or the
    /// opening brace) and the tail expression. Preserved here because
    /// expressions do not carry trivia in v1.1.
    pub tail_leading_comments: Vec<String>,
}

/// Block-level statement.
#[derive(Debug, Clone)]
pub enum Statement {
    /// `let name (: T)? = expr` — pure binding (v0.1).
    Let(LetStmt),
    /// `let name (: T)? <- expr` — effectful binding (v0.5).
    EffectLet(LetStmt),
    /// `commit expr` — within an agent handler, declares the new persistent
    /// state (v0.5).
    Commit(CommitStmt),
}

impl Statement {
    pub fn span(&self) -> Span {
        match self {
            Statement::Let(l) | Statement::EffectLet(l) => l.span,
            Statement::Commit(c) => c.span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LetStmt {
    pub name: Ident,
    pub type_annot: Option<TypeRef>,
    pub value: Expr,
    pub span: Span,
    pub trivia: Trivia,
}

#[derive(Debug, Clone)]
pub struct CommitStmt {
    pub value: Expr,
    pub span: Span,
    pub trivia: Trivia,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: Ident,
    pub type_ref: TypeRef,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeRef {
    Base(BaseType, Span),
    Named(Ident),
    /// `Result[T, E]` — the built-in generic Result type (v0.1).
    Result(Box<TypeRef>, Box<TypeRef>, Span),
    /// `Option[T]` — the built-in generic Option type (v0.2).
    Option(Box<TypeRef>, Span),
    /// `Effect[T]` — the built-in generic Effect type (v0.5).
    Effect(Box<TypeRef>, Span),
    /// `ValidationError` — the built-in error type used by refined-type
    /// constructors (v0.1).
    ValidationError(Span),
    /// `()` — the unit type (v0.5).
    Unit(Span),
}

impl TypeRef {
    pub fn span(&self) -> Span {
        match self {
            TypeRef::Base(_, s) => *s,
            TypeRef::Named(id) => id.span,
            TypeRef::Result(_, _, s) => *s,
            TypeRef::Option(_, s) => *s,
            TypeRef::Effect(_, s) => *s,
            TypeRef::ValidationError(s) => *s,
            TypeRef::Unit(s) => *s,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    IntLit(i64),
    StrLit(String),
    BoolLit(bool),
    Ident(Ident),
    Call(Ident, Vec<Expr>),
    BinOp(BinOp, Box<Expr>, Box<Expr>),
    UnaryOp(UnaryOp, Box<Expr>),
    Paren(Box<Expr>),
    /// `{ stmts; expr }` — block expression (v0.1).
    Block(Block),
    /// `if cond { then } else { else }` (v0.1).
    If {
        cond: Box<Expr>,
        then_block: Box<Block>,
        else_block: Box<Block>,
    },
    /// `Ok(value)` — Result success constructor (v0.1).
    Ok(Box<Expr>),
    /// `Err(error)` — Result failure constructor (v0.1).
    Err(Box<Expr>),
    /// `expr?` — propagation operator (v0.1).
    Question(Box<Expr>),
    /// `TypeName.method(args)` — qualified static call on a type
    /// (v0.1: only refined-type `of`; v0.2: any static method or variant
    /// constructor for sum types). The resolver decides which.
    ConstructorCall {
        type_name: Ident,
        method: Ident,
        args: Vec<Expr>,
    },
    /// `TypeName { field: value, ... }` — record construction (v0.2).
    RecordConstruction {
        type_name: Ident,
        fields: Vec<FieldInit>,
    },
    /// `receiver.field` — field access on a record value (v0.2). v0.3 adds
    /// `.raw` on opaque types within the defining commons.
    FieldAccess {
        receiver: Box<Expr>,
        field: Ident,
    },
    /// `receiver.method(args)` — instance method call (v0.2). The
    /// resolver determines the receiver's type and looks up the method.
    MethodCall {
        receiver: Box<Expr>,
        method: Ident,
        args: Vec<Expr>,
    },
    /// `match disc { arm+ }` — pattern matching (v0.2).
    Match {
        discriminant: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    /// `expr is pattern` — pattern test, returns Bool (v0.2).
    Is {
        value: Box<Expr>,
        pattern: Pattern,
    },
    /// `Some(value)` — Option Some constructor (v0.2).
    Some(Box<Expr>),
    /// `None` — Option None constructor (v0.2).
    None,
    /// `()` — unit literal (v0.5).
    UnitLit,
    /// `TypeName { ...base, field: value, ... }` or `{ ...base, ... }` —
    /// record spread expression (v0.5).
    RecordSpread {
        /// Optional type prefix (`TypeName { ...base }`). Absent for the
        /// bare form used inside `commit`.
        type_name: Option<Ident>,
        /// The base record being spread.
        base: Box<Expr>,
        /// Field overrides (always full `name: value` form — never shorthand).
        overrides: Vec<FieldInit>,
    },
    /// `Effect.pure(value)` — wrap a synchronous value into `Effect[T]`
    /// (v0.5). Recognised in the parser as a special-form.
    EffectPure(Box<Expr>),
}

/// One field-initialiser inside a record construction expression:
/// either `name: expr` or the shorthand `name` (which requires a binding
/// of the same name in scope and uses its value).
#[derive(Debug, Clone)]
pub struct FieldInit {
    pub name: Ident,
    /// `None` means shorthand — the field's value is the same-named binding.
    pub value: Option<Expr>,
    pub span: Span,
}

/// One arm of a `match` expression: `pattern => body`.
#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: MatchBody,
    pub span: Span,
}

/// The right-hand side of a match arm — either a single expression or
/// a block.
#[derive(Debug, Clone)]
pub enum MatchBody {
    Expr(Expr),
    Block(Block),
}

impl MatchBody {
    pub fn span(&self) -> Span {
        match self {
            MatchBody::Expr(e) => e.span,
            MatchBody::Block(b) => b.span,
        }
    }
}

/// A pattern (v0.2 §3.8). Patterns appear in `match` arms and as the
/// right-hand side of the `is` operator.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// `_` — matches any value, no bindings.
    Wildcard(Span),
    /// `Variant` or `Variant(bindings)` or `TypeName.Variant(bindings)`.
    Variant {
        /// Optional qualifier: `TypeName.Variant`.
        type_name: Option<Ident>,
        /// The variant name.
        variant: Ident,
        /// Payload bindings (empty for nullary variants).
        bindings: Vec<PatternBinding>,
        span: Span,
    },
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Wildcard(s) => *s,
            Pattern::Variant { span, .. } => *span,
        }
    }
}

/// A single binding inside a variant pattern. Two surface forms:
/// `name` (positional — bind the i-th payload field) and
/// `fieldName: bindName` (named — bind the named payload field).
/// Both forms also accept `_` as the bind name to discard.
#[derive(Debug, Clone)]
pub struct PatternBinding {
    /// Source form: positional or named.
    pub kind: PatternBindingKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PatternBindingKind {
    /// `name` (or `_`): bind the payload field at this position to `name`.
    Positional { name: Ident },
    /// `field: name` (or `field: _`): bind the named payload field to `name`.
    Named { field: Ident, name: Ident },
}

impl PatternBinding {
    /// The local name introduced by this binding (used for scope).
    /// `_` is a sentinel for "no binding"; callers should compare against it.
    pub fn local_name(&self) -> &Ident {
        match &self.kind {
            PatternBindingKind::Positional { name } => name,
            PatternBindingKind::Named { name, .. } => name,
        }
    }

    pub fn is_wildcard(&self) -> bool {
        self.local_name().name == "_"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Add,
    Sub,
    Mul,
    Div,
}

impl BinOp {
    pub fn name(self) -> &'static str {
        match self {
            BinOp::Or => "||",
            BinOp::And => "&&",
            BinOp::Eq => "==",
            BinOp::NotEq => "!=",
            BinOp::Lt => "<",
            BinOp::LtEq => "<=",
            BinOp::Gt => ">",
            BinOp::GtEq => ">=",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

impl UnaryOp {
    pub fn name(self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
        }
    }
}
