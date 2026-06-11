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

/// A `consumes other.context` declaration (v0.4 §3.2). May optionally carry
/// an alias introduced by `consumes other.context as Alias` (v0.6 §3.1).
#[derive(Debug, Clone)]
pub struct ConsumesDecl {
    pub target: QualifiedName,
    pub alias: Option<Ident>,
    /// v0.17: `consumes U { Cap, … }` — selected capabilities flattened into
    /// the consumer's local capability namespace under their bare names (§3.3).
    /// `None` for the whole-unit forms; `Some` (possibly empty) for the braced
    /// form. Mutually exclusive with `alias`.
    pub selected: Option<Vec<Ident>>,
    pub span: Span,
    pub trivia: Trivia,
}

/// An `exports visibility { names }` clause (v0.4 §3.3) or, v0.15, an
/// `exports capability { names }` clause.
#[derive(Debug, Clone)]
pub struct ExportsDecl {
    pub kind: ExportKind,
    pub names: Vec<Ident>,
    pub span: Span,
    pub trivia: Trivia,
}

/// What an `exports` clause exposes: types (with a visibility) or, v0.15,
/// capabilities offered for cross-context consumption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    /// `exports opaque { ... }` / `exports transparent { ... }` — type exports.
    Type(Visibility),
    /// `exports capability { ... }` — capabilities offered to consumers (v0.15).
    Capability,
}

/// Visibility level for an exports clause (v0.4 §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Token-only outside the context: hold, pass, compare; no inspect, no construct.
    Opaque,
    /// Readable shape outside the context: inspect fields, match variants; no construct.
    Transparent,
}

/// An `adapter qualified.name { … }` declaration (v0.17 §3.1). An adapter
/// co-locates a capability contract with a non-Karn binding: it may declare
/// capabilities, the boundary types they reference, inline pure helper
/// `type`/`fn` (and `uses`), external (bodiless) providers, `exports
/// capability`, and exactly one `binding` clause. It may *not* declare
/// services, agents, or bodied providers. Like commons/contexts it may be
/// split across files in a directory.
#[derive(Debug, Clone)]
pub struct AdapterDecl {
    pub name: QualifiedName,
    pub items: Vec<CommonsItem>,
    /// `uses` clauses declared in this file (pure-vocabulary mixin; allowed
    /// because helpers cannot pierce containment — spec [DECISION B]).
    pub uses: Vec<UsesDecl>,
    /// `exports capability { … }` clauses (adapters export capabilities and
    /// boundary types, never services).
    pub exports: Vec<ExportsDecl>,
    /// v0.18: `consumes U { Cap, … }` clauses — adapter-to-adapter capability
    /// dependencies (spec §4.5, [N]). Braced form only; adapter targets only
    /// (both enforced semantically, not in the parser).
    pub consumes: Vec<ConsumesDecl>,
    /// The `binding "<module>" requires { … }` clause, if present. Required
    /// when the adapter declares any external provider (`karn.adapter.no_binding`).
    pub binding: Option<BindingDecl>,
    pub documentation: Option<String>,
    pub form: CommonsForm,
    pub span: Span,
    pub trivia: Trivia,
    pub trailing_comments: Vec<String>,
}

/// A `binding "<module>" requires { "pkg": "range", … }` clause inside an
/// adapter (v0.17 §3.5). `module` is the TypeScript module supplying the
/// adapter's external provider symbols, resolved relative to the adapter's
/// source file. `requires` declares npm dependencies folded into the
/// generated `package.json`.
#[derive(Debug, Clone)]
pub struct BindingDecl {
    /// The module path as written (the string-literal contents, no quotes).
    pub module: String,
    pub module_span: Span,
    pub requires: Vec<RequiresDep>,
    pub span: Span,
    pub trivia: Trivia,
}

/// One `"pkg": "range"` entry in a binding's `requires { … }` map.
#[derive(Debug, Clone)]
pub struct RequiresDep {
    pub package: String,
    pub range: String,
    pub span: Span,
}

/// Either a commons or a context — the two declaration kinds at the file
/// level (v0.4 §3.1). v0.7 adds the test declaration kind; v0.17 the adapter.
#[derive(Debug, Clone)]
pub enum SourceUnit {
    Commons(Commons),
    Context(Context),
    Test(TestDecl),
    /// v0.16: a `test integration "name" { wires … }` multi-Worker integration
    /// test. Its `name()` is synthesised from the suite name.
    Integration(IntegrationDecl),
    /// v0.17: an `adapter` unit — the host boundary (capability contract +
    /// external binding).
    Adapter(AdapterDecl),
}

impl SourceUnit {
    pub fn name(&self) -> &QualifiedName {
        match self {
            SourceUnit::Commons(c) => &c.name,
            SourceUnit::Context(c) => &c.name,
            SourceUnit::Test(t) => &t.target,
            SourceUnit::Integration(i) => &i.name,
            SourceUnit::Adapter(a) => &a.name,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            SourceUnit::Commons(c) => c.span,
            SourceUnit::Context(c) => c.span,
            SourceUnit::Test(t) => t.span,
            SourceUnit::Integration(i) => i.span,
            SourceUnit::Adapter(a) => a.span,
        }
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            SourceUnit::Commons(_) => "commons",
            SourceUnit::Context(_) => "context",
            SourceUnit::Test(_) => "test",
            SourceUnit::Integration(_) => "integration test",
            SourceUnit::Adapter(_) => "adapter",
        }
    }
}

/// A `test <qualified-name> { ... }` declaration (v0.7 §3.1).
///
/// A test targets a commons or context by qualified name and bundles a set of
/// test cases plus optional mock declarations. As with commons and contexts, a
/// test may be split across multiple files (fragment form).
#[derive(Debug, Clone)]
pub struct TestDecl {
    /// The targeted commons or context.
    pub target: QualifiedName,
    /// `uses` clauses brought in by this test fragment.
    pub uses: Vec<UsesDecl>,
    /// Provider or consumed-context mocks declared for the test.
    pub mocks: Vec<MockDecl>,
    /// The individual test cases.
    pub cases: Vec<TestCase>,
    /// Surface form: brace-delimited body or headerless fragment.
    pub form: CommonsForm,
    /// Optional documentation block attached to the test declaration.
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
    pub trailing_comments: Vec<String>,
}

/// A `mocks Name = Impl { ops }` declaration inside a test body (v0.7 §3.2).
#[derive(Debug, Clone)]
pub struct MockDecl {
    /// The capability or consumed-context alias being mocked.
    pub target_name: Ident,
    /// The implementation identifier (used as the TypeScript class name).
    pub impl_name: Ident,
    /// One operation per mock body entry.
    pub ops: Vec<MockOp>,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// One operation inside a mock declaration: `fn name(params) -> T { body }`.
#[derive(Debug, Clone)]
pub struct MockOp {
    pub name: Ident,
    pub params: Vec<Param>,
    pub return_type: TypeRef,
    pub body: Block,
    pub span: Span,
    pub trivia: Trivia,
}

/// A `test "name" { body }` block inside a test declaration (v0.7 §3.3).
#[derive(Debug, Clone)]
pub struct TestCase {
    /// The test name, taken from the string literal.
    pub name: String,
    /// The span of the string literal — used for diagnostics and runtime
    /// failure reports.
    pub name_span: Span,
    pub body: Block,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// A `test integration "name" { wires C1, C2, … ; cases }` declaration
/// (v0.16 §3.1). Unlike a unit test, an integration test names a *set* of
/// participating contexts (`wires`), stands each up as its own Worker, and
/// exercises a flow across the real Worker boundary. It carries no `mocks`.
#[derive(Debug, Clone)]
pub struct IntegrationDecl {
    /// The suite name, taken from the string literal after `integration`.
    pub suite: String,
    /// The span of the suite-name literal — used in diagnostics and reports.
    pub suite_span: Span,
    /// A synthesised qualified name (`integration <suite>`), so the unit shares
    /// the `SourceUnit::name()` shape. Not user-written.
    pub name: QualifiedName,
    /// The participating contexts, in declaration order (≥ 2, validated later).
    pub participants: Vec<QualifiedName>,
    /// `uses` clauses bringing commons into the case bodies.
    pub uses: Vec<UsesDecl>,
    /// The individual test cases.
    pub cases: Vec<TestCase>,
    /// Surface form: brace-delimited body or headerless fragment.
    pub form: CommonsForm,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
    pub trailing_comments: Vec<String>,
}

/// A capability reference in a `given` clause (v0.15 §3.2). A bare name is a
/// local capability (`given Cap`); a dotted name refers to a capability a
/// consumed context provides (`given B.Cap` / `given Alias.Cap`).
#[derive(Debug, Clone)]
pub struct CapRef {
    /// `None` for a local capability; `Some(prefix)` for a cross-context
    /// reference where `prefix` is a consumed-context qualified name or alias.
    pub context: Option<QualifiedName>,
    /// The capability's simple name (also the local deps key).
    pub name: Ident,
    pub span: Span,
}

impl CapRef {
    /// The local deps key / capability simple name (e.g. `Clock`).
    pub fn key(&self) -> &str {
        &self.name.name
    }

    /// True when this references a capability provided by a consumed context.
    pub fn is_cross_context(&self) -> bool {
        self.context.is_some()
    }

    /// The cross-context prefix (consumed-context qualified name or alias) as
    /// a dotted string, if any.
    pub fn prefix(&self) -> Option<String> {
        self.context.as_ref().map(|q| q.joined())
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
    /// v0.12: capabilities this provider depends on (`provides X = Impl given
    /// Y, Z { … }`). The provider's operation bodies may use these. v0.15:
    /// a dependency may be a cross-context capability (`given B.Cap`).
    pub given: Vec<CapRef>,
    pub ops: Vec<ProviderOp>,
    /// v0.17: an *external* provider — `provides Cap = Name` with **no** brace
    /// block — inside an adapter, supplied by the adapter's binding rather than
    /// a Karn body. When `true`, `ops` is empty and the emitter produces no
    /// class. The absence of the brace block (not an empty one) is the signal.
    pub external: bool,
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
    pub given: Vec<CapRef>,
    pub body: Block,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerKind {
    /// `on call(...)` — typed RPC (the only kind in v0.5).
    Call,
    /// `on http METHOD "path"` — external-facing HTTP route (v0.9).
    Http { method: HttpMethod, path: String },
    /// `on cron "expr"` — scheduled task; `expr` is a 5-field cron
    /// expression (v0.10a).
    Cron { expr: String },
    /// `on queue "name"` — queue consumer bound to the named queue (v0.10b).
    Queue { name: String },
}

/// HTTP methods supported by `on http` handlers (v0.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
        }
    }

    pub fn from_ident(s: &str) -> Option<HttpMethod> {
        match s {
            "GET" => Some(HttpMethod::Get),
            "POST" => Some(HttpMethod::Post),
            "PUT" => Some(HttpMethod::Put),
            "PATCH" => Some(HttpMethod::Patch),
            "DELETE" => Some(HttpMethod::Delete),
            _ => None,
        }
    }

    /// True if this method conventionally has no request body.
    pub fn forbids_body(self) -> bool {
        matches!(self, HttpMethod::Get | HttpMethod::Delete)
    }
}

/// Payload shape of an `HttpResult[T]` variant (v0.9 §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpVariantPayload {
    /// No payload (e.g. `NoContent`, `Unauthorized`).
    None,
    /// Carries a value of the `HttpResult` type parameter `T`.
    Value,
    /// Carries a `String` message (e.g. `BadRequest`, `Conflict`).
    Message,
}

/// One variant of the built-in `HttpResult[T]` sum (v0.9 §3.3).
#[derive(Debug, Clone, Copy)]
pub struct HttpVariant {
    pub name: &'static str,
    pub payload: HttpVariantPayload,
    pub status: u16,
}

/// All `HttpResult[T]` variants, in declaration order.
pub const HTTP_VARIANTS: &[HttpVariant] = &[
    HttpVariant {
        name: "Ok",
        payload: HttpVariantPayload::Value,
        status: 200,
    },
    HttpVariant {
        name: "Created",
        payload: HttpVariantPayload::Value,
        status: 201,
    },
    HttpVariant {
        name: "NoContent",
        payload: HttpVariantPayload::None,
        status: 204,
    },
    HttpVariant {
        name: "BadRequest",
        payload: HttpVariantPayload::Message,
        status: 400,
    },
    HttpVariant {
        name: "Unauthorized",
        payload: HttpVariantPayload::None,
        status: 401,
    },
    HttpVariant {
        name: "Forbidden",
        payload: HttpVariantPayload::None,
        status: 403,
    },
    HttpVariant {
        name: "NotFound",
        payload: HttpVariantPayload::None,
        status: 404,
    },
    HttpVariant {
        name: "Conflict",
        payload: HttpVariantPayload::Message,
        status: 409,
    },
    HttpVariant {
        name: "UnprocessableEntity",
        payload: HttpVariantPayload::Message,
        status: 422,
    },
    HttpVariant {
        name: "ServerError",
        payload: HttpVariantPayload::Message,
        status: 500,
    },
];

/// Find an `HttpResult[T]` variant by name. Returns the variant info or
/// `None` if the name doesn't match.
pub fn http_variant(name: &str) -> Option<HttpVariant> {
    HTTP_VARIANTS.iter().copied().find(|v| v.name == name)
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
    /// v0.11: an optional initial-value expression. Only meaningful on agent
    /// `state` fields (the field's fresh-key value); ignored / rejected on
    /// record-type fields by the checker.
    pub init: Option<Expr>,
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

/// A function type parameter (v0.20a, `fn name[A, B](…)`). A struct rather
/// than a bare Ident so the ADR-0028 "bound-capable" promise is a later field
/// addition, not a representation change.
#[derive(Debug, Clone)]
pub struct TypeParam {
    pub name: Ident,
    pub span: Span,
}

/// A lambda expression (v0.20a): `(params) => expr` or `(params) => { … }`.
/// `=>` is the value arrow (shared with `match`); param annotations are
/// optional where an expected function type supplies them.
#[derive(Debug, Clone)]
pub struct LambdaExpr {
    pub params: Vec<LambdaParam>,
    pub body: Box<Expr>,
    pub span: Span,
}

/// A lambda parameter. A separate type from [`Param`] because its annotation
/// is optional — `Param.type_ref` stays mandatory at every signature site.
#[derive(Debug, Clone)]
pub struct LambdaParam {
    pub name: Ident,
    pub type_ref: Option<TypeRef>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FnDecl {
    /// v0.20a: `[A, B]` type parameters; empty for non-generic functions.
    pub type_params: Vec<TypeParam>,
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
    /// `assert expr` — verify a Bool expression at test runtime (v0.7).
    /// Only valid inside test case bodies.
    Assert(AssertStmt),
}

impl Statement {
    pub fn span(&self) -> Span {
        match self {
            Statement::Let(l) | Statement::EffectLet(l) => l.span,
            Statement::Commit(c) => c.span,
            Statement::Assert(a) => a.span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AssertStmt {
    pub value: Expr,
    pub span: Span,
    pub trivia: Trivia,
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
    /// `HttpResult[T]` — the built-in HTTP-result sum (v0.9).
    HttpResult(Box<TypeRef>, Span),
    /// `List[T]` — the built-in generic immutable list type (v0.20b).
    List(Box<TypeRef>, Span),
    /// `Map[K, V]` — the built-in generic immutable map type (v0.20b).
    /// Keys are confined to value-keyable types
    /// (`karn.types.unkeyable_map_key`).
    Map(Box<TypeRef>, Box<TypeRef>, Span),
    /// `ValidationError` — the built-in error type used by refined-type
    /// constructors (v0.1).
    ValidationError(Span),
    /// `()` — the unit type (v0.5).
    Unit(Span),
    /// `A -> B` / `(A, B) -> C` / `() -> B` — a function type (v0.20a).
    /// Right-associative; effectful iff the return type is `Effect[_]`
    /// (the structural rule). Confined to non-boundary positions
    /// (`karn.types.function_at_boundary`).
    Fn(Vec<TypeRef>, Box<TypeRef>, Span),
}

impl TypeRef {
    pub fn span(&self) -> Span {
        match self {
            TypeRef::Base(_, s) => *s,
            TypeRef::Named(id) => id.span,
            TypeRef::Result(_, _, s) => *s,
            TypeRef::Option(_, s) => *s,
            TypeRef::Effect(_, s) => *s,
            TypeRef::HttpResult(_, s) => *s,
            TypeRef::List(_, s) => *s,
            TypeRef::Map(_, _, s) => *s,
            TypeRef::ValidationError(s) => *s,
            TypeRef::Unit(s) => *s,
            TypeRef::Fn(_, _, s) => *s,
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
    Call {
        name: Ident,
        /// v0.20a: explicit type arguments (`name[T](…)`); empty when absent.
        type_args: Vec<TypeRef>,
        args: Vec<Expr>,
    },
    /// A lambda (v0.20a). See [`LambdaExpr`].
    Lambda(LambdaExpr),
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
    /// `assert expr` — assertion as an expression of type `()` (v0.9.1).
    /// Valid only inside test bodies. Evaluates `expr` (must be Bool); if
    /// false, the surrounding test case fails.
    Assert(Box<Expr>),
    /// `Mock[T]`, `Mock[T](args)` — test-context value construction (v0.9.4).
    /// `args` is empty for the bare form and holds the pin arguments for
    /// `Mock[T](...)`. The record-override form `Mock[T] { ... }` is not yet
    /// parsed. Valid only inside test bodies; has type `T`.
    Mock {
        type_ref: TypeRef,
        args: Vec<Expr>,
    },
    /// `[a, b, c]` — list literal (v0.20b). An empty `[]` requires an
    /// expected type (`karn.types.uninferable_element_type`).
    ListLit(Vec<Expr>),
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
