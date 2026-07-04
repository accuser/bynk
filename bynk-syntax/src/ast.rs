//! Abstract syntax tree types for Bynk v0 (spec §9.2).

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
/// co-locates a capability contract with a non-Bynk binding: it may declare
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
    /// dependencies (spec §4.5, \[N\]). Braced form only; adapter targets only
    /// (both enforced semantically, not in the parser).
    pub consumes: Vec<ConsumesDecl>,
    /// The `binding "<module>" requires { … }` clause, if present. Required
    /// when the adapter declares any external provider (`bynk.adapter.no_binding`).
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
    Suite(SuiteDecl),
    /// v0.17: an `adapter` unit — the host boundary (capability contract +
    /// external binding).
    Adapter(AdapterDecl),
}

impl SourceUnit {
    pub fn name(&self) -> &QualifiedName {
        match self {
            SourceUnit::Commons(c) => &c.name,
            SourceUnit::Context(c) => &c.name,
            SourceUnit::Suite(t) => &t.target,
            SourceUnit::Adapter(a) => &a.name,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            SourceUnit::Commons(c) => c.span,
            SourceUnit::Context(c) => c.span,
            SourceUnit::Suite(t) => t.span,
            SourceUnit::Adapter(a) => a.span,
        }
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            SourceUnit::Commons(_) => "commons",
            SourceUnit::Context(_) => "context",
            SourceUnit::Suite(_) => "suite",
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
pub struct SuiteDecl {
    /// The targeted commons or context.
    pub target: QualifiedName,
    /// `uses` clauses brought in by this test fragment.
    pub uses: Vec<UsesDecl>,
    /// v0.118: suite-scoped `provides` clauses — per-seam provider overrides
    /// applied to every case (a case-scoped `provides` takes precedence).
    pub provides: Vec<ProvidesClause>,
    /// The individual test cases.
    pub cases: Vec<Case>,
    /// v0.114: generative `property` blocks (testing track slice 2).
    pub properties: Vec<PropertyDecl>,
    /// v0.118: the suite-level tier default (`suite … as integration`). `None`
    /// means the `unit` default; a `case`'s own tier overrides it. A `property`
    /// ignores a suite tier (tiers are a `case`-only affordance).
    pub tier: Option<TestTier>,
    /// Surface form: brace-delimited body or headerless fragment.
    pub form: CommonsForm,
    /// Optional documentation block attached to the test declaration.
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
    pub trailing_comments: Vec<String>,
}

/// v0.118: the tier a `case` runs at (testing track slice 6, ADR 0153). One
/// body promoted across the testing pyramid; `unit` is the default and elided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestTier {
    /// Collaborators stubbed (the default).
    Unit,
    /// Real collaborators within one context, no serialisation wire.
    Integration,
    /// Contexts wired across the real serialise → JSON → deserialise boundary.
    System,
}

impl TestTier {
    pub fn as_str(self) -> &'static str {
        match self {
            TestTier::Unit => "unit",
            TestTier::Integration => "integration",
            TestTier::System => "system",
        }
    }
}

/// v0.118: a per-seam provider override `provides Cap.method(<args>) returns <v>
/// | fails` (testing track slice 6, ADR 0154). Substitutes one capability
/// method's provision under test; the right-hand side is a value or a fault,
/// never a computed body.
#[derive(Debug, Clone)]
pub struct ProvidesClause {
    /// The capability being overridden (a consumed seam of the unit).
    pub capability: Ident,
    /// The overridden method.
    pub method: Ident,
    /// One argument pattern per parameter (`_` or a value the arg must equal).
    pub args: Vec<ArgPattern>,
    /// The provision: a value, a fault, or a per-call sequence.
    pub rhs: ProvidesRhs,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// v0.118: one argument pattern in a `provides` call pattern. Patterns for the
/// same method are tried top-to-bottom, first match wins.
#[derive(Debug, Clone)]
pub enum ArgPattern {
    /// `_` — matches any argument.
    Any(Span),
    /// A value the recorded argument must equal (a literal or pure value expr).
    Value(Expr),
}

/// v0.118: the right-hand side of a `provides` clause.
#[derive(Debug, Clone)]
pub enum ProvidesRhs {
    /// `returns <value>` — a single success value, repeated for every call.
    Returns(Expr),
    /// `fails` — inject a capability fault (Principle 3).
    Fails(Span),
    /// `returns each [<outcome>, …]` — one outcome per call, in order; the last
    /// outcome repeats once the sequence is exhausted (DECISION V).
    ReturnsEach(Vec<SeqOutcome>, Span),
}

impl ProvidesRhs {
    pub fn span(&self) -> Span {
        match self {
            ProvidesRhs::Returns(e) => e.span,
            ProvidesRhs::Fails(s) => *s,
            ProvidesRhs::ReturnsEach(_, s) => *s,
        }
    }
}

/// v0.118: one outcome in a sequenced (`returns each`) `provides`.
#[derive(Debug, Clone)]
pub enum SeqOutcome {
    /// A success value.
    Value(Expr),
    /// A fault.
    Fails(Span),
}

/// A `case "name" [as <tier>] { [provides …] body }` block inside a suite
/// (v0.7 §3.3; v0.118 adds the tier clause and case-scoped `provides`).
#[derive(Debug, Clone)]
pub struct Case {
    /// The test name, taken from the string literal.
    pub name: String,
    /// The span of the string literal — used for diagnostics and runtime
    /// failure reports.
    pub name_span: Span,
    /// v0.118: the case's own tier, if written (`as integration` / `as system`).
    /// `None` means inherit the suite default (itself `unit` when unset).
    pub tier: Option<TestTier>,
    /// v0.118: case-scoped `provides` clauses (override the suite's, and the
    /// tier default).
    pub provides: Vec<ProvidesClause>,
    pub body: Block,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// A `property "name" { for all <bindings> [where <pred>] { body } }` block
/// inside a suite (v0.114, testing track slice 2, ADR 0149). The generative
/// sibling of [`Case`]: the runner draws inhabitants of each binding's type from
/// its refinement domain and evaluates the body's `expect`s over them.
#[derive(Debug, Clone)]
pub struct PropertyDecl {
    /// The property name, taken from the string literal.
    pub name: String,
    /// The span of the string literal — used for diagnostics and reports.
    pub name_span: Span,
    /// The `for all` binder: the generated bindings, an optional `where` filter,
    /// and the predicate body.
    pub forall: ForAll,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// The `for all x: T, … [where <pred>] { … }` binder inside a [`PropertyDecl`].
#[derive(Debug, Clone)]
pub struct ForAll {
    /// The generated bindings, `x: T` (one or more).
    pub bindings: Vec<ForAllBinding>,
    /// An optional `where <pred>` filter (a pure `Bool`) applied to generated
    /// tuples before the body runs.
    pub where_pred: Option<Expr>,
    /// The body — one or more statements, typically `expect`s.
    pub body: Block,
    pub span: Span,
}

/// One `for all` binding: `name: T`, where the runner generates inhabitants of
/// `T` from its refinements.
#[derive(Debug, Clone)]
pub struct ForAllBinding {
    pub name: Ident,
    pub type_ref: TypeRef,
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
    /// `actor Name { auth = Scheme, identity = T }` (v0.45). A nominal boundary
    /// contract consumed by a handler's `by` clause; not a runnable entity.
    Actor(ActorDecl),
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
            CommonsItem::Actor(a) => &a.name,
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
    /// a Bynk body. When `true`, `ops` is empty and the emitter produces no
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
    /// The protocol the service conforms to, from the `from <protocol>` header
    /// clause (v0.44). `Call` when there is no clause.
    pub protocol: ServiceProtocol,
    /// The optional cross-origin (CORS) policy (v0.131, ADR 0159) — a `cors { }`
    /// section in the service body, only meaningful on a `from http` service.
    /// `None` when absent (same-origin default, byte-for-byte unchanged output).
    pub cors: Option<CorsPolicy>,
    pub handlers: Vec<Handler>,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// A cross-origin resource-sharing policy on a `from http` service (v0.131,
/// ADR 0159): the `cors { }` section in the service body. Parsed leniently as a
/// list of `name: value` fields (the grammar accepts any field name — an unknown
/// one is a checker diagnostic, per the `@`-annotation precedent, ADR 0111), and
/// interpreted through the typed accessors below.
///
/// `Access-Control-Allow-Methods` is deliberately **not** a field — it is derived
/// from the service's routes at emit time (the routes already enumerate the
/// methods; a restated list would drift). Likewise `Allow-Headers` defaults to
/// `content-type` (+ `Authorization` when a Bearer route exists) and is only
/// stored here when the author overrides it.
#[derive(Debug, Clone)]
pub struct CorsPolicy {
    /// The `cors { }` fields as written, in source order. Field names are
    /// validated against the closed set (`origins`/`headers`/`credentials`/
    /// `maxAge`) by the checker, not the parser.
    pub fields: Vec<CorsField>,
    pub span: Span,
    pub trivia: Trivia,
}

/// One `name: value` field inside a `cors { }` policy (v0.131).
#[derive(Debug, Clone)]
pub struct CorsField {
    pub name: Ident,
    pub value: Expr,
    pub span: Span,
}

impl CorsPolicy {
    /// The raw value expression for a field, by name (the last one wins if a
    /// field is repeated — the checker flags the duplicate separately).
    pub fn field(&self, name: &str) -> Option<&Expr> {
        self.fields
            .iter()
            .rev()
            .find(|f| f.name.name == name)
            .map(|f| &f.value)
    }

    /// The allowed origins — the string literals of the `origins:` list. An
    /// absent or malformed field yields an empty list (the checker has already
    /// reported the shape error; the emitter fails closed on an empty list).
    pub fn origins(&self) -> Vec<String> {
        Self::str_list(self.field("origins")).unwrap_or_default()
    }

    /// `true` iff `origins` is exactly the wildcard `["*"]`.
    pub fn is_wildcard(&self) -> bool {
        let os = self.origins();
        os.len() == 1 && os[0] == "*"
    }

    /// Whether credentialed requests are allowed (`credentials: true`); defaults
    /// to `false` when the field is absent.
    pub fn credentials(&self) -> bool {
        matches!(
            self.field("credentials").map(|e| &e.kind),
            Some(ExprKind::BoolLit(true))
        )
    }

    /// The explicit `Access-Control-Allow-Headers` override, if the author gave
    /// a `headers:` list; `None` leaves the emitter to apply its smart default.
    pub fn allow_headers(&self) -> Option<Vec<String>> {
        self.field("headers").and_then(Self::str_list_of)
    }

    /// The `Access-Control-Max-Age` in whole seconds, if a `maxAge:` duration was
    /// given; `None` leaves the header off (the browser default).
    pub fn max_age_secs(&self) -> Option<i64> {
        match self.field("maxAge").map(|e| &e.kind) {
            Some(ExprKind::DurationLit { millis, .. }) => Some(millis / 1_000),
            _ => None,
        }
    }

    /// Interpret an expression as a list of string literals, if it is one.
    fn str_list(expr: Option<&Expr>) -> Option<Vec<String>> {
        expr.and_then(Self::str_list_of)
    }

    fn str_list_of(expr: &Expr) -> Option<Vec<String>> {
        match &expr.kind {
            ExprKind::ListLit(items) => items
                .iter()
                .map(|e| match &e.kind {
                    ExprKind::StrLit(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            _ => None,
        }
    }
}

/// The protocol a service conforms to — declared on the header via
/// `from <protocol>` (v0.44). `Call` is the default (no `from` clause): a
/// contract-mediated internal-RPC surface, not a wire protocol. Multi-endpoint
/// protocols (`Http`, `Cron`) carry no binding — the endpoint lives on each
/// handler; single-binding `Queue` carries its queue name.
#[derive(Debug, Clone)]
pub enum ServiceProtocol {
    /// No `from` clause: the service holds `on call` handlers only.
    Call,
    /// `from http` — many routes; each handler is `on <Method>("route")`.
    Http,
    /// `from cron` — many schedules; each handler is `on schedule("expr")`.
    Cron,
    /// `from queue("name")` — one bound queue; handlers are `on message(...)`.
    Queue { name: String },
    /// `from WebSocket(in: ClientFrame, out: ServerFrame)` — a held WebSocket
    /// connection (v0.103, real-time track slice 3). `in_type` is the inbound
    /// frame type (client→server, decoded and routed as typed agent messages);
    /// `out_type` is the server→client frame type the held `Connection[out_type]`
    /// carries. The service holds exactly one `on open` handler (edge auth via
    /// `by`, then transfer of the connection to an agent).
    WebSocket { in_type: TypeRef, out_type: TypeRef },
}

/// An agent declaration (v0.5 §3.6). Agents are state-bearing entities
/// with their own handlers.
#[derive(Debug, Clone)]
pub struct AgentDecl {
    pub name: Ident,
    /// `key id: Type` — the identifier-typed value identifying instances.
    pub key_name: Ident,
    pub key_type: TypeRef,
    /// `store` fields (v0.81, storage track) — each an access-pattern slot of a
    /// declared storage kind (`Cell`/`Map`/…). The successor to the removed
    /// `state { }` record (ADR 0108); every agent declares its state this way.
    pub store_fields: Vec<StoreField>,
    /// Invariants (v0.80 §14) — universally-quantified predicates over the
    /// agent's `store` fields. The phase sits between the fields and the
    /// handlers; each is checked against the state staged by a handler's writes
    /// before it commits.
    pub invariants: Vec<Invariant>,
    /// Step invariants (v0.116 §, testing track slice 4) — named predicates over
    /// the pre-/post-commit state *pair* (`old`/`new`), checked at the commit
    /// boundary beside [`invariants`], from the second commit onward. Widen the
    /// invariant subject from a snapshot to a step (ADR 0144 — one predicate
    /// surface).
    ///
    /// [`invariants`]: AgentDecl::invariants
    pub transitions: Vec<Transition>,
    pub handlers: Vec<Handler>,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// A `store` field (v0.81, storage track). Each is an access-pattern slot of a
/// declared storage kind: `store <name>: <Kind>[…] [@annotations] [= <init>]`.
/// The kind and its element type are carried as an ordinary [`TypeRef`]
/// (`Cell[Int]`, `Map[K, V]`); the checker restricts which heads are storage
/// kinds. Access-pattern annotations (`@indexed`, …) parse into [`annotations`]
/// (v0.85, ADR 0111); the checker validates them against the closed registry.
///
/// [`annotations`]: StoreField::annotations
#[derive(Debug, Clone)]
pub struct StoreField {
    pub name: Ident,
    /// The storage kind and its element type(s): `Cell[Int]`, `Map[K, V]`. A
    /// dedicated [`StoreKind`] rather than a [`TypeRef`] — storage kinds are not
    /// value types, and the checker dispatches kind-aware operations on the head.
    pub kind: StoreKind,
    /// Storage annotations on the field (v0.85, ADR 0111): `@ttl(5.minutes)`,
    /// `@indexed(by: orderId)`. Parsed in declaration order (after the kind,
    /// before the initialiser); the checker validates names against the closed
    /// registry and gates each to the slice that implements it.
    pub annotations: Vec<Annotation>,
    /// The fresh-key initial value (`= expr`), if given — same disposition as a
    /// `state` field's initialiser (ADRs 0003/0004 carry forward).
    pub init: Option<Expr>,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// A storage annotation on a `store` field (v0.85, storage track; ADR 0111):
/// `@<name>(<args>)`. The `name` is matched against the closed registry
/// (`@indexed`/`@ttl`/`@retain`/`@bounded`) by the checker; the grammar accepts
/// any identifier so an unknown name is a checker diagnostic, not a parse error.
/// Arguments are compile-time metadata, restricted to literals (and the `by:`
/// field-name labels of `@indexed`) by the checker per ADR 0111 D4.
#[derive(Debug, Clone)]
pub struct Annotation {
    pub name: Ident,
    pub args: Vec<AnnotationArg>,
    pub span: Span,
}

/// A single annotation argument (v0.85; ADR 0111): an optional `label:` followed
/// by a value expression — `by: orderId` (labelled) or `5.minutes` (positional).
/// The value is parsed as an ordinary [`Expr`] so the duration-literal form
/// (`5.minutes`, landing with the `Duration` slice) needs no special grammar;
/// the checker restricts it to a literal where the annotation is functional.
#[derive(Debug, Clone)]
pub struct AnnotationArg {
    pub label: Option<Ident>,
    pub value: Expr,
    pub span: Span,
}

/// A storage kind applied to its element type(s) (v0.81): `Cell[Int]`,
/// `Map[ReservationId, Reservation]`. The `head` is the kind name (`Cell`,
/// `Map`, `Set`, `Log`, `Queue`, `Cache`); the checker validates it against the
/// closed catalogue. Element types are ordinary [`TypeRef`]s. Refined element
/// types (`Cell[Int where NonNegative]`) ride a later slice (parse_type_ref does
/// not yet accept an inline refinement in type-argument position).
#[derive(Debug, Clone)]
pub struct StoreKind {
    pub head: Ident,
    pub args: Vec<TypeRef>,
    pub span: Span,
}

/// An agent invariant (v0.80 §14). A named predicate over the agent's state
/// fields that must hold of every committed state; a commit that would violate
/// it faults (`InvariantViolation`) before the state is persisted. The
/// predicate references state fields by bare name, mirroring the design-notes
/// worked examples (`status == Paid implies paymentRef.isSome()`).
#[derive(Debug, Clone)]
pub struct Invariant {
    pub name: Ident,
    /// The predicate expression — an ordinary `Bool`-typed expression over the
    /// state fields, plus `implies` and `is`. The parsed-predicate-on-a-
    /// declaration shape mirrors [`ActorRefinement::predicate`].
    pub predicate: Expr,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// An agent step invariant (v0.116 §, testing track slice 4). A named predicate
/// over the *pair* of committed states — the pre-commit `old` and the proposed
/// `new`, each the agent's state record — that must hold of every state move; a
/// commit that would violate it faults (`InvariantViolation`) before the state is
/// persisted, exactly as a snapshot [`Invariant`] does. Widens the invariant
/// subject from a snapshot to a step (ADR 0144 — one predicate surface); the
/// predicate reuses the invariant surface (`implies`/`is`/pure methods) with
/// `old`/`new` bound contextually (`old.status is Paid implies new.status is
/// Paid`).
#[derive(Debug, Clone)]
pub struct Transition {
    pub name: Ident,
    /// The predicate expression — an ordinary `Bool`-typed expression over the
    /// `old` and `new` state records, with `implies`/`is` and pure methods,
    /// mirroring [`Invariant`].
    pub predicate: Expr,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

/// A function contract clause (v0.115 §, testing track slice 3). A named
/// predicate on a `fn` signature — a `requires` (precondition) or `ensures`
/// (postcondition). A contract is the invariant predicate attached to a
/// function (ADR 0144 — one predicate surface): the predicate is a pure `Bool`
/// expression over the parameters (`requires`) or the parameters plus `result`
/// (`ensures`), with `implies`/`is` and pure methods, mirroring [`Invariant`].
/// The name rides the failure report and the redundant-test dedup.
#[derive(Debug, Clone)]
pub struct Contract {
    pub name: Ident,
    /// The predicate expression — an ordinary `Bool`-typed expression over the
    /// parameters (and, for an `ensures`, the contextual `result` binding).
    pub predicate: Expr,
    pub span: Span,
}

/// An actor declaration (v0.45 §3.7). An actor is a nominal *contract type*
/// describing an external party at a boundary — not a runnable entity. A
/// handler consumes an actor on its `by` clause; the boundary verifies the
/// declared `auth` scheme and mints a sealed identity (`name.identity`).
#[derive(Debug, Clone)]
pub struct ActorDecl {
    pub name: Ident,
    /// The authentication scheme from `auth = <Scheme>`, stored as the raw
    /// identifier. The checker classifies it: `None`/`Internal`/`Bearer` are
    /// admitted; `Signature` is reserved-and-rejected
    /// (`bynk.actor.scheme_unsupported`); anything else is
    /// `bynk.actor.unknown_scheme`. `None` for the refinement form.
    pub auth: Option<Ident>,
    /// The scheme's keyed config from `auth = Scheme(key = value, …)` (v0.47
    /// `Bearer(secret = "…")`; v0.51 generalised for `Signature(secret, header,
    /// timestamp?, tolerance?)`). Empty for schemes/forms with no config. The
    /// checker validates which keys each scheme requires/allows.
    pub auth_config: Vec<SchemeArg>,
    /// The optional identity type from `, identity = <T>`. Absent ⇒ the
    /// scheme default (`()` for `None`; a sealed `CallerId` for the `Internal`
    /// `on call` channel, `()` for other `Internal` channels).
    pub identity: Option<TypeRef>,
    /// The reserved-and-rejected refinement form `actor Admin = Base where p`
    /// (Q3). Parsed so the grammar is fixed now; the checker emits
    /// `bynk.actor.refinement_unsupported`.
    pub refinement: Option<ActorRefinement>,
    pub documentation: Option<String>,
    pub span: Span,
    pub trivia: Trivia,
}

impl ActorDecl {
    /// The value of a scheme config arg by key, if present (e.g. `secret`,
    /// `header`).
    pub fn scheme_arg(&self, key: &str) -> Option<&SchemeArg> {
        self.auth_config.iter().find(|a| a.key.name == key)
    }
}

/// One `key = value` argument in a scheme config (`Scheme(key = value, …)`).
#[derive(Debug, Clone)]
pub struct SchemeArg {
    pub key: Ident,
    pub value: SchemeArgValue,
    /// Span of the value, for diagnostics.
    pub span: Span,
}

/// A scheme config arg value — a string literal or an integer.
#[derive(Debug, Clone)]
pub enum SchemeArgValue {
    Str(String),
    Int(i64),
}

impl SchemeArgValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            SchemeArgValue::Str(s) => Some(s),
            SchemeArgValue::Int(_) => None,
        }
    }
    pub fn as_int(&self) -> Option<i64> {
        match self {
            SchemeArgValue::Int(n) => Some(*n),
            SchemeArgValue::Str(_) => None,
        }
    }
}

/// The reserved refinement form `actor Admin = User where <predicate>` (Q3).
/// Parsed in Foundations so the grammar is fixed; admission is a later slice.
#[derive(Debug, Clone)]
pub struct ActorRefinement {
    /// The base actor being refined.
    pub base: Ident,
    /// The `where` predicate. Parsed but not yet checked.
    pub predicate: Expr,
    pub span: Span,
}

/// The `by (<binder>:)? <Actor>` clause on a handler (v0.45; binder optional in
/// v0.50). Names the actor contract the handler consumes; when a `binder` is
/// given, the verified identity binds to it and is read as `binder.identity`.
/// Omitting the binder (`by <Actor>`) declares-and-verifies the contract without
/// capturing the identity — for anonymous or verify-and-discard handlers. Sits
/// after the protocol config and before the parameters.
#[derive(Debug, Clone)]
pub struct ByClause {
    /// The identity binder, if the handler consumes the identity. `None` for the
    /// binder-less `by <Actor>` form. Required when `actors` names more than one
    /// (a sum is resolved by matching on the bound actor).
    pub binder: Option<Ident>,
    /// The actor contract(s) referenced — each a local actor decl or a prelude
    /// actor. A single name is the ordinary single-actor handler; more than one
    /// (`by who: A | B`, v0.52) is an **ordered sum of peer actors** resolved
    /// first-wins, the body matching on the resolved actor. Always non-empty.
    pub actors: Vec<Ident>,
    pub span: Span,
}

impl ByClause {
    /// The first (and, for a single-actor handler, only) actor contract named.
    pub fn primary(&self) -> &Ident {
        &self.actors[0]
    }
    /// Whether this `by` clause names an ordered sum of peer actors (`A | B`).
    pub fn is_sum(&self) -> bool {
        self.actors.len() > 1
    }
}

/// A handler block — `on call(args) -> T given C1, C2 { body }`.
/// Used by both services and agents.
#[derive(Debug, Clone)]
pub struct Handler {
    pub kind: HandlerKind,
    /// Handler-position annotations (v0.140, ADR 0163): `@cache(maxAge: 5.minutes)`
    /// written immediately before `on <METHOD>(…)`. Reuses the [`Annotation`] AST
    /// shared with `store` fields (ADR 0111); the grammar accepts any `@name(args)`
    /// so an unknown name is a project-validation diagnostic, not a parse error. The
    /// first handler-position annotation surface — empty for every handler that
    /// carries none.
    pub annotations: Vec<Annotation>,
    /// For agent handlers, the method-style handler name (e.g.
    /// `on call addItem(...)`). For service handlers, this is None (just
    /// `on call(...)`).
    pub method_name: Option<Ident>,
    /// The `by <binder>: <Actor>` clause (v0.45), if present. Service handlers
    /// only; an absent clause inherits the protocol's default actor.
    pub by_clause: Option<ByClause>,
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
    /// `on message(m: T)` — a message off the service's bound queue. The queue
    /// binding lives on the service's `ServiceProtocol::Queue` (v0.44).
    Message,
    /// `on open ...` — the WebSocket upgrade handler (v0.103, real-time track
    /// slice 3). Exactly one per `from WebSocket` service; carries a mandatory
    /// `by` clause (edge auth) and receives a fresh owned `Connection[out]`.
    Open,
    /// `on close ...` — the WebSocket close handler (v0.106, real-time track slice
    /// 3b-iii). Optional, ≤1 per `from WebSocket` service; runs when the socket
    /// closes. Like `on open`, edge-authenticated (`by`), with the identity/params
    /// recovered from the socket attachment (set at `on open`). (A `from WebSocket`
    /// `on message` reuses [`HandlerKind::Message`], disambiguated by the protocol.)
    Close,
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
    /// Carries a `String` target URL, emitted as a `Location` header — the
    /// redirect variants (`Found`, `SeeOther`, `PermanentRedirect`, …).
    Location,
    /// Carries a `Stream[String]`, emitted as an SSE (`text/event-stream`)
    /// streaming body — the `Streaming` (200) variant (v0.101, real-time track
    /// slice 1).
    Streamed,
    /// Carries `(body: Bytes, contentType: String)` — the author-owned raw body
    /// written straight into the response with the declared `content-type` and
    /// **no codec** (the typed-wire guarantee is deliberately off). The `Raw`
    /// (200) variant (v0.111); the first two-argument payload shape.
    Raw,
}

/// One variant of the built-in `HttpResult[T]` sum (v0.9 §3.3).
#[derive(Debug, Clone, Copy)]
pub struct HttpVariant {
    pub name: &'static str,
    pub payload: HttpVariantPayload,
    pub status: u16,
}

/// All `HttpResult[T]` variants, in declaration order (ascending status). The
/// vocabulary tracks the common, modern HTTP status codes (RFC 9110): success
/// and created/accepted (`Value`), redirects carrying a `Location` URL, and
/// the client/server failures that handlers routinely return (`Message` when
/// an explanation helps the caller, `None` for self-describing statuses).
pub const HTTP_VARIANTS: &[HttpVariant] = &[
    // ── 2xx success ──────────────────────────────────────────────────────
    HttpVariant {
        name: "Ok",
        payload: HttpVariantPayload::Value,
        status: 200,
    },
    // v0.101 (real-time track slice 1): a 200 whose body is a streamed
    // `Stream[String]`, SSE-framed. Status precedes the body, so streaming is
    // 200-only — pre-stream failures are ordinary variants returned instead.
    HttpVariant {
        name: "Streaming",
        payload: HttpVariantPayload::Streamed,
        status: 200,
    },
    // v0.111: a 200 whose body is an author-owned `Bytes` written straight into
    // the response with the declared `content-type` — no codec runs. 200-only,
    // like `Streaming`: it serves service-tier raw bodies (`robots.txt`,
    // `sitemap.xml`, feeds, a QR PNG), not custom-status error pages.
    HttpVariant {
        name: "Raw",
        payload: HttpVariantPayload::Raw,
        status: 200,
    },
    HttpVariant {
        name: "Created",
        payload: HttpVariantPayload::Value,
        status: 201,
    },
    HttpVariant {
        name: "Accepted",
        payload: HttpVariantPayload::Value,
        status: 202,
    },
    HttpVariant {
        name: "NoContent",
        payload: HttpVariantPayload::None,
        status: 204,
    },
    // ── 3xx redirection (carry a `Location` URL) ─────────────────────────
    HttpVariant {
        name: "MovedPermanently",
        payload: HttpVariantPayload::Location,
        status: 301,
    },
    HttpVariant {
        name: "Found",
        payload: HttpVariantPayload::Location,
        status: 302,
    },
    HttpVariant {
        name: "SeeOther",
        payload: HttpVariantPayload::Location,
        status: 303,
    },
    HttpVariant {
        name: "TemporaryRedirect",
        payload: HttpVariantPayload::Location,
        status: 307,
    },
    HttpVariant {
        name: "PermanentRedirect",
        payload: HttpVariantPayload::Location,
        status: 308,
    },
    // ── 4xx client error ─────────────────────────────────────────────────
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
        name: "MethodNotAllowed",
        payload: HttpVariantPayload::None,
        status: 405,
    },
    HttpVariant {
        name: "NotAcceptable",
        payload: HttpVariantPayload::None,
        status: 406,
    },
    HttpVariant {
        name: "RequestTimeout",
        payload: HttpVariantPayload::None,
        status: 408,
    },
    HttpVariant {
        name: "Conflict",
        payload: HttpVariantPayload::Message,
        status: 409,
    },
    HttpVariant {
        name: "Gone",
        payload: HttpVariantPayload::None,
        status: 410,
    },
    HttpVariant {
        name: "LengthRequired",
        payload: HttpVariantPayload::None,
        status: 411,
    },
    HttpVariant {
        name: "PayloadTooLarge",
        payload: HttpVariantPayload::Message,
        status: 413,
    },
    HttpVariant {
        name: "UnsupportedMediaType",
        payload: HttpVariantPayload::Message,
        status: 415,
    },
    HttpVariant {
        name: "UnprocessableEntity",
        payload: HttpVariantPayload::Message,
        status: 422,
    },
    HttpVariant {
        name: "TooManyRequests",
        payload: HttpVariantPayload::Message,
        status: 429,
    },
    HttpVariant {
        name: "UnavailableForLegalReasons",
        payload: HttpVariantPayload::Message,
        status: 451,
    },
    // ── 5xx server error ─────────────────────────────────────────────────
    HttpVariant {
        name: "ServerError",
        payload: HttpVariantPayload::Message,
        status: 500,
    },
    HttpVariant {
        name: "NotImplemented",
        payload: HttpVariantPayload::Message,
        status: 501,
    },
    HttpVariant {
        name: "BadGateway",
        payload: HttpVariantPayload::Message,
        status: 502,
    },
    HttpVariant {
        name: "ServiceUnavailable",
        payload: HttpVariantPayload::Message,
        status: 503,
    },
    HttpVariant {
        name: "GatewayTimeout",
        payload: HttpVariantPayload::Message,
        status: 504,
    },
];

/// Find an `HttpResult[T]` variant by name. Returns the variant info or
/// `None` if the name doesn't match.
pub fn http_variant(name: &str) -> Option<HttpVariant> {
    HTTP_VARIANTS.iter().copied().find(|v| v.name == name)
}

/// Payload shape of a `QueueResult` variant (v0.44). Non-generic — a verdict
/// carries no value; `Retry` carries a `String` reason for the log path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueVariantPayload {
    /// No payload (`Ack`).
    None,
    /// Carries a `String` reason (`Retry`).
    Message,
}

/// One variant of the built-in `QueueResult` sum (v0.44).
#[derive(Debug, Clone, Copy)]
pub struct QueueVariant {
    pub name: &'static str,
    pub payload: QueueVariantPayload,
}

/// All `QueueResult` variants, in declaration order. `Ack` confirms the
/// message; `Retry` redelivers it, carrying a reason for observability.
pub const QUEUE_VARIANTS: &[QueueVariant] = &[
    QueueVariant {
        name: "Ack",
        payload: QueueVariantPayload::None,
    },
    QueueVariant {
        name: "Retry",
        payload: QueueVariantPayload::Message,
    },
];

/// Find a `QueueResult` variant by name.
pub fn queue_variant(name: &str) -> Option<QueueVariant> {
    QUEUE_VARIANTS.iter().copied().find(|v| v.name == name)
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
    Float,
    /// `Duration` (v0.86, ADR 0112) — a span of time, a distinct base type
    /// erased to TS `number` carrying milliseconds (the `Clock` unit). Modelled
    /// on `Float`: Bynk-side-only, no implicit `Int` coercion (save the one
    /// sanctioned clock-math mix).
    Duration,
    /// `Instant` (v0.90, ADR 0114) — an absolute point in time, a distinct base
    /// type erased to TS `number` carrying Unix epoch milliseconds (the
    /// `Clock` unit). No literal (minted by `Clock.now()`); arithmetic composes
    /// with `Duration` (`Instant ± Duration -> Instant`, `Instant − Instant ->
    /// Duration`). Supersedes ADR 0112 D4's `Int`↔`Duration` clock-math mix.
    Instant,
    /// `Bytes` (v0.110, ADR 0142) — an immutable finite octet sequence, the
    /// seventh base type. Unlike its neighbours it does **not** erase to TS
    /// `number`: a `Bytes` lowers to a `Uint8Array`. No source literal
    /// (constructed via `Bytes.fromUtf8`/`fromBase64`/`empty`); `==` compares
    /// by content (real emitter codegen, not host `===`); wires as a base64
    /// JSON string; not `Map`-keyable and not orderable.
    Bytes,
}

impl BaseType {
    pub fn name(self) -> &'static str {
        match self {
            BaseType::Int => "Int",
            BaseType::String => "String",
            BaseType::Bool => "Bool",
            BaseType::Float => "Float",
            BaseType::Duration => "Duration",
            BaseType::Instant => "Instant",
            BaseType::Bytes => "Bytes",
        }
    }
}

/// A `Duration` literal unit (v0.86, ADR 0112) — the closed set of suffixes in a
/// `<int>.<unit>` literal. Each maps to a fixed millisecond factor (`Duration`
/// erases to `Int` milliseconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationUnit {
    Milliseconds,
    Seconds,
    Minutes,
    Hours,
    Days,
}

impl DurationUnit {
    /// Resolve a unit name (`minutes`) to its variant, or `None` if it is not one
    /// of the closed set. Used by the parser to recognise an `<int>.<unit>`
    /// literal; an unrecognised name leaves the expression a field access.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "milliseconds" => DurationUnit::Milliseconds,
            "seconds" => DurationUnit::Seconds,
            "minutes" => DurationUnit::Minutes,
            "hours" => DurationUnit::Hours,
            "days" => DurationUnit::Days,
            _ => return None,
        })
    }

    /// The unit name as written.
    pub fn name(self) -> &'static str {
        match self {
            DurationUnit::Milliseconds => "milliseconds",
            DurationUnit::Seconds => "seconds",
            DurationUnit::Minutes => "minutes",
            DurationUnit::Hours => "hours",
            DurationUnit::Days => "days",
        }
    }

    /// The unit's value in milliseconds.
    pub fn millis(self) -> i64 {
        match self {
            DurationUnit::Milliseconds => 1,
            DurationUnit::Seconds => 1_000,
            DurationUnit::Minutes => 60_000,
            DurationUnit::Hours => 3_600_000,
            DurationUnit::Days => 86_400_000,
        }
    }
}

/// An integer refinement bound (v0.40, ADR 0073): the parsed value plus the
/// bound's source span (covering a leading `-`). Value-only beyond the span —
/// ints have one canonical printed form, so the formatter stays idempotent
/// without a stored lexeme. The span backs the `InRange`-swap quick-fix.
#[derive(Debug, Clone)]
pub struct IntBound {
    pub value: i64,
    pub span: Span,
}

/// A float refinement bound (v0.21): the parsed value plus the signed source
/// lexeme (for byte-stable emission). v0.40 (ADR 0073): also the source span,
/// for the `InRange`-swap quick-fix.
#[derive(Debug, Clone)]
pub struct FloatBound {
    pub value: f64,
    pub lexeme: String,
    pub span: Span,
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
    InRange(IntBound, IntBound),
    /// `InRange` with float bounds (v0.21) — a separate variant so every
    /// `Int` refinement path stays untouched. Bounds keep their source
    /// lexemes (including any sign) so emitted runtime checks are
    /// byte-stable.
    InRangeF(FloatBound, FloatBound),
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
            PredKind::InRange(..) | PredKind::InRangeF(..) => "InRange",
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
    /// v0.115: preconditions (`requires <name>: <pred>`), parsed between the
    /// return type and the body. A contract clause is the invariant predicate
    /// attached to a function (ADR 0144 — one predicate surface); `requires`
    /// scopes over the parameters only.
    pub requires: Vec<Contract>,
    /// v0.115: postconditions (`ensures <name>: <pred>`). Scopes over the
    /// parameters *and* `result`, the contextual binding for the return value.
    pub ensures: Vec<Contract>,
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
    /// `expect expr` — verify a Bool predicate at test runtime (v0.7; renamed
    /// from `assert` in v0.112). Only valid inside test case bodies.
    Expect(ExpectStmt),
    /// `~> expr` — an asynchronous fire-and-forget send (v0.79). The caller does
    /// not await the reply; legal only when the reply is `Effect[()]`. No binder.
    Send(SendStmt),
    /// `name := expr` — a `Cell` store write (v0.81, storage track). The
    /// unconditional write form; `.update(fn)` (a method call) is the
    /// read-modify-write form. ADR 0108.
    Assign(AssignStmt),
}

impl Statement {
    pub fn span(&self) -> Span {
        match self {
            Statement::Let(l) | Statement::EffectLet(l) => l.span,
            Statement::Expect(a) => a.span,
            Statement::Send(s) => s.span,
            Statement::Assign(a) => a.span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExpectStmt {
    pub value: Expr,
    pub span: Span,
    pub trivia: Trivia,
}

/// `name := expr` — a `Cell` store write (v0.81, storage track). `target` is the
/// `Cell` field being written (a bare name for now; the checker resolves it to a
/// `store` field). `value` is the new value.
#[derive(Debug, Clone)]
pub struct AssignStmt {
    pub target: Ident,
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
pub struct SendStmt {
    /// The send target — a recipient call, e.g. `Logger.info(msg)`.
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
    /// `QueueResult` — the built-in queue verdict sum (`Ack | Retry`),
    /// non-generic; the required return of a queue handler (v0.44).
    QueueResult(Span),
    /// `List[T]` — the built-in generic immutable list type (v0.20b).
    List(Box<TypeRef>, Span),
    /// `Map[K, V]` — the built-in generic immutable map type (v0.20b).
    /// Keys are confined to value-keyable types
    /// (`bynk.types.unkeyable_map_key`).
    Map(Box<TypeRef>, Box<TypeRef>, Span),
    /// `Query[T]` — the built-in lazy storage-read description (v0.91, ADR 0115).
    /// Nameable in a pure helper's return type; non-storable and non-boundary
    /// (like `Effect`/`Fn`).
    Query(Box<TypeRef>, Span),
    /// `Stream[T]` — the value-over-time primitive (v0.100, real-time track
    /// slice 0). A lazy, pull-shaped sequence produced over time; non-storable
    /// and non-boundary (like `Query`/`Effect`/`Fn`).
    Stream(Box<TypeRef>, Span),
    /// `Connection[F]` — a held WebSocket connection (v0.102, real-time track
    /// slice 2). `F` is the server→client frame type. A `Held` resource:
    /// non-serialisable, non-boundary, and governed by the linearity discipline
    /// (§2.9); storable only in `Cell[Option[Connection]]` / `Map[K, Connection]`.
    Connection(Box<TypeRef>, Span),
    /// `History[Agent]` — a generated, driven call-history of an agent (v0.119,
    /// testing track slice 7, ADR 0155). A test-only generator, legal only in
    /// `for all` binding position inside a `property`; it is not a value type,
    /// so it never resolves in a field/param/return position. The bound subject
    /// behaves as an ordinary `List[Step]`.
    History(Box<TypeRef>, Span),
    /// `ValidationError` — the built-in error type used by refined-type
    /// constructors (v0.1).
    ValidationError(Span),
    /// `JsonError` — the built-in JSON-decode error type (v0.22b). A
    /// uniform record (`kind`/`path`/`message`, all `String`) the codec
    /// maps `BoundaryError` variants and parse failures into.
    JsonError(Span),
    /// `()` — the unit type (v0.5).
    Unit(Span),
    /// `A -> B` / `(A, B) -> C` / `() -> B` — a function type (v0.20a).
    /// Right-associative; effectful iff the return type is `Effect[_]`
    /// (the structural rule). Confined to non-boundary positions
    /// (`bynk.types.function_at_boundary`).
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
            TypeRef::QueueResult(s) => *s,
            TypeRef::List(_, s) => *s,
            TypeRef::Map(_, _, s) => *s,
            TypeRef::Query(_, s) => *s,
            TypeRef::Stream(_, s) => *s,
            TypeRef::Connection(_, s) => *s,
            TypeRef::History(_, s) => *s,
            TypeRef::ValidationError(s) => *s,
            TypeRef::JsonError(s) => *s,
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
    /// A float literal (v0.21). The lexeme is kept alongside the parsed
    /// value so emission and formatting are byte-stable (`1e10` must not
    /// normalise to `10000000000`).
    FloatLit {
        value: f64,
        lexeme: String,
    },
    /// A duration literal `<int>.<unit>` (v0.86, ADR 0112): `5.minutes`,
    /// `30.days`. The parser recognises the `IntLit . <unit>` shape and records
    /// the magnitude, the unit, and the resolved milliseconds (the value the
    /// emitter lowers to). Typed `Duration`.
    DurationLit {
        /// The integer magnitude as written (`5` in `5.minutes`).
        value: i64,
        /// The unit name (`minutes`), one of the closed set.
        unit: DurationUnit,
        /// The value in milliseconds — `value * unit factor`.
        millis: i64,
    },
    StrLit(String),
    /// An interpolated string `"… \(expr) …"` (v0.43, ADR 0075). Chunks and
    /// holes alternate. A plain `"…"` with no holes stays [`ExprKind::StrLit`],
    /// so existing code and the emitter/formatter fast-path are untouched.
    InterpStr(Vec<InterpPart>),
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
        /// v0.22b: explicit type arguments on a qualified static
        /// (`Json.decode[T](…)`); empty when absent. The same-line-`[`
        /// rule applies as for `Call` type application (0039).
        type_args: Vec<TypeRef>,
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
    /// `expect expr` — expectation as an expression of type `()` (v0.9.1;
    /// renamed from `assert` in v0.112). Valid only inside test bodies. Evaluates
    /// `expr` (must be Bool); if false, the surrounding test case fails.
    Expect(Box<Expr>),
    /// `Val[T]`, `Val[T](args)` — test-context value construction (v0.9.4).
    /// `args` is empty for the bare form and holds the pin arguments for
    /// `Val[T](...)`. The record-override form `Val[T] { ... }` is not yet
    /// parsed. Valid only inside test bodies; has type `T`.
    Val {
        type_ref: TypeRef,
        args: Vec<Expr>,
    },
    /// `[a, b, c]` — list literal (v0.20b). An empty `[]` requires an
    /// expected type (`bynk.types.uninferable_element_type`).
    ListLit(Vec<Expr>),
    /// An observation over a consumed capability's recorded calls (v0.117,
    /// testing track slice 5). The direct subject of an `expect` in a `case`
    /// body — `expect Cap.op called once with <pred>`, `expect Cap.op never
    /// called`, `expect A.op before B.op`. Types as `Bool` (the claim about the
    /// recorded trace), lowered to a boolean over the recorded log.
    Observation(ObservationExpr),
    /// `trace(Cap.op)` — the bound-trace escape hatch (v0.117, testing track
    /// slice 5). Yields the recorded calls of `Cap.op` as a `List[<CallRecord>]`
    /// (a synthetic record of the operation's parameters), asserted over with the
    /// ordinary value surface. Test-body-only, like [`ExprKind::Val`].
    Trace {
        cap: Ident,
        op: Ident,
    },
}

/// An observation of a capability operation's recorded calls (v0.117, testing
/// track slice 5). `cap`/`op` name the seam (`Logger.log`); `matcher` is the
/// claim about the recorded calls.
#[derive(Debug, Clone)]
pub struct ObservationExpr {
    pub cap: Ident,
    pub op: Ident,
    pub matcher: ObservationMatcher,
}

/// The claim an [`ObservationExpr`] makes about a seam's recorded calls (v0.117).
#[derive(Debug, Clone)]
pub enum ObservationMatcher {
    /// `called` [`once` | `<n> times`]? [`with` `<pred>`]?. `count` is `None`
    /// for a bare `called` (at least one); `Some(expr)` is the exact-count claim
    /// (a literal; `once` desugars to `1`). `with_pred` matches a call whose
    /// arguments (in scope by the operation's parameter names) satisfy it.
    Called {
        count: Option<Box<Expr>>,
        with_pred: Option<Box<Expr>>,
    },
    /// `never called` — zero calls.
    NeverCalled,
    /// `before Cap.op` — the first call of the subject precedes the first call
    /// of the named operation (both must have occurred).
    Before { cap: Ident, op: Ident },
}

/// One part of an interpolated string (v0.43, ADR 0075). An
/// [`ExprKind::InterpStr`] holds an alternating run of these.
#[derive(Debug, Clone)]
pub enum InterpPart {
    /// Literal text between holes, with escapes already resolved.
    Chunk(String),
    /// An interpolated expression `\(expr)`. Type-checked by the hole rule
    /// (base scalars only; see the checker) and lowered into a template-
    /// literal `${…}` slot.
    Hole(Box<Expr>),
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
    /// A literal pattern — `31`, `"english"`, `true` (v0.130 §2.3.4). Matches a
    /// primitive scrutinee (`Int`/`String`/`Bool`) by value equality. The
    /// admitted set mirrors ADR 0001's closed literal set (integers — including
    /// a leading unary minus — strings, and booleans); `Float`/`()` are not
    /// admitted as patterns.
    Literal { value: LiteralValue, span: Span },
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

/// The value carried by a [`Pattern::Literal`]. A closed set (ADR 0001):
/// integer, string, and boolean. Kept distinct from [`ExprKind`] so patterns
/// carry only what they can actually match, and so it is `Eq`/`Hash` for the
/// duplicate-arm check.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LiteralValue {
    Int(i64),
    Str(String),
    Bool(bool),
}

impl LiteralValue {
    /// A human-readable rendering for diagnostics (`31`, `"english"`, `true`).
    pub fn describe(&self) -> String {
        match self {
            LiteralValue::Int(n) => n.to_string(),
            LiteralValue::Str(s) => format!("{s:?}"),
            LiteralValue::Bool(b) => b.to_string(),
        }
    }
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Wildcard(s) => *s,
            Pattern::Literal { span, .. } => *span,
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
    /// `P implies Q` — logical implication (v0.80). Desugars to `!P || Q`; sits
    /// at the lowest precedence (below `||`). Reads directionally (P → Q).
    Implies,
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
            BinOp::Implies => "implies",
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
