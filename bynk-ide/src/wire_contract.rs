//! #855: the wire-contract peek — the model behind "hover a service handler"
//! and "Show Wire Contract". Renders `bynk_check::wire::WireModel` (the
//! same IR `bynk-emit`'s codec generation consumes) narrowed to *one*
//! handler, plus the facts the IR does not carry: the request envelope
//! shape, the cross-context contract form + hash (`on call` only), and the
//! HTTP response set a route can actually answer with.
//!
//! **Handler resolution** mirrors `bynk-lsp/src/sequence_request.rs`'s
//! `sequence_model_at`: re-parse the committed snapshot with
//! `parse_unit_with_recovery`, find the `Handler` whose span contains the
//! cursor. The re-parsed `Handler` supplies spans/body; the **retained**
//! [`ContextBoundaryInfo`] supplies the type table — a single-file re-parse
//! cannot see a `uses` target's types, which is the whole reason that table
//! is retained project-wide (Phase 3).
//!
//! **Provenance.** [`bynk_check::wire::Provenance`] distinguishes a type this
//! module owns from one reached through another unit. `ContextBoundaryInfo`'s
//! `types` table is `combined_types_for(unit, …)` — *this* unit's own type
//! namespace, own declarations plus its `uses` targets' — the exact table
//! `own_contract_hashes` (`bynk-emit/src/project.rs`) hashes a service's
//! contract through. Every name resolvable through it is therefore this
//! unit's *own* view of its boundary: `Provenance::Owned` throughout. A
//! genuinely `Consumed` type (reached through a `consumes <other context>`,
//! not a `uses <commons>`) never appears here — that is a *caller's* view of
//! another unit's boundary, not this handler's own, and is out of scope for
//! a peek that answers "what does this handler send and receive".
//!
//! **Scope.** Service handlers only. An agent's `on call` handler crosses a
//! Durable-Object RPC boundary too, but the issue's worked examples (an HTTP
//! route, a cross-context `on call`) are both service handlers, and
//! `ContextBoundaryInfo` does not retain agents' own handler bodies in a form
//! this module needs. Left for a later slice if an agent peek is wanted.

use std::collections::HashMap;
use std::path::PathBuf;

use bynk_check::checker::{Ty, TyId, Types};
use bynk_check::contract;
use bynk_check::resolver::CrossContextService;
use bynk_check::wire::{self, WireModel, WireRef};
use bynk_emit::project::ContextBoundaryInfo;
use bynk_syntax::ast::*;
use bynk_syntax::span::Span;

const HTTP_RESULT: &str = bynk_check::builtin_names::types::HTTP_RESULT;

/// Which protocol hosts the hovered handler — the header facts
/// `discriminator` (`bynk-ide/src/sequence.rs`) also renders, kept here as
/// their own IDE-shaped type rather than re-exporting [`HandlerKind`]
/// directly, so a future field this peek needs (that `HandlerKind` has no
/// room for) is a local addition, not an upstream one.
#[derive(Debug, Clone, PartialEq)]
pub enum BoundaryKind {
    Http { method: HttpMethod, path: String },
    Call,
    Cron { expr: String },
    Message,
    Open,
    Close,
    Event,
}

impl BoundaryKind {
    fn from_handler(h: &Handler) -> Self {
        match &h.kind {
            HandlerKind::Http { method, path } => BoundaryKind::Http {
                method: *method,
                path: path.clone(),
            },
            HandlerKind::Call => BoundaryKind::Call,
            HandlerKind::Cron { expr } => BoundaryKind::Cron { expr: expr.clone() },
            HandlerKind::Message => BoundaryKind::Message,
            HandlerKind::Open => BoundaryKind::Open,
            HandlerKind::Close => BoundaryKind::Close,
            HandlerKind::Event => BoundaryKind::Event,
        }
    }
}

/// The request shape a handler's caller sends. **Three cases, not two** —
/// `bynk-emit/src/emitter/workers_entry.rs`'s `h.params.len() == 1` branch is
/// the *only* bare-value case; everything else, including **zero** params,
/// takes the object branch. Rendering only `Bare`/`Keyed` would misstate a
/// zero-arg service as accepting an (empty) object body it never inspects,
/// when what actually happens is the body is not read at all.
#[derive(Debug, Clone)]
pub enum Envelope {
    /// No params: the request body is not read.
    Empty,
    /// Exactly one param: the request body **is** the value of `param`,
    /// with no wrapping object/key.
    Bare { param: String, shape: WireRef },
    /// Two or more params: an object keyed by parameter name, in
    /// declaration order.
    Keyed { params: Vec<(String, WireRef)> },
}

/// The canonical form + hash of an `on call` handler's contract — the same
/// projection `bynk-emit/src/project.rs`'s `own_contract_hashes` builds, so
/// the hash this peek shows is provably the hash the emitted
/// `X-Bynk-Contract` constant stamps.
#[derive(Debug, Clone)]
pub struct ContractForm {
    pub normal_form: String,
    pub hash: String,
}

/// Why a response was reachable — so a renderer can tell an author-declared
/// outcome from one the boundary injects on their behalf without their
/// having written a line for it.
#[derive(Debug, Clone, PartialEq)]
pub enum ResponseOrigin {
    /// The handler's declared return type, `Effect[HttpResult[T]]` stripped
    /// to its `Ok`/200 case.
    DeclaredSuccess,
    /// A variant literally constructed somewhere in the handler body, at
    /// this span.
    Constructed { span: Span },
    /// A response the body never names — injected by the boundary itself.
    BoundaryImplicit { why: &'static str },
}

/// One reachable HTTP outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    /// The `HttpResult` variant name (`"Ok"`, `"TooManyRequests"`), or a
    /// representative `BoundaryError` `kind` for a boundary-implicit
    /// response with no variant of its own (`"StructuralMismatch"`).
    pub variant: String,
    pub origin: ResponseOrigin,
}

/// Why there is no cross-context contract to show for this handler.
/// Rendered on the panel only — hover stays quiet about an absence (Part 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoCrossContextReason {
    /// Not an `on call` handler — HTTP/cron/message/websocket/event
    /// handlers are never reached by another context's call.
    NotACallHandler,
    /// The project has only this one context/adapter — there is no *other*
    /// context that could call in, so the question does not arise.
    SingleContext,
}

/// The wire contract of one handler: its request envelope, its reachable
/// response set (HTTP handlers only), its cross-context contract form + hash
/// (`on call` only), and the boundary type shapes both reference.
#[derive(Debug, Clone)]
pub struct WireContractModel {
    pub unit: String,
    pub service: String,
    pub kind: BoundaryKind,
    pub handler_span: Span,
    /// 1-indexed line of `handler_span`'s start, against the text this
    /// model was built from.
    pub handler_line: usize,
    pub envelope: Envelope,
    /// `on call` only; `None` whenever [`Self::no_cross_context`] is `Some`.
    pub contract: Option<ContractForm>,
    /// The boundary types this handler's envelope + responses reference,
    /// resolved through the retained [`ContextBoundaryInfo`] — the same IR
    /// `bynk-emit`'s codec generation renders.
    pub boundary: WireModel,
    /// Declaration span for every named type in `boundary.types`, keyed by
    /// name — click-to-code for the panel's per-type blocks.
    pub type_sites: HashMap<String, Span>,
    /// Empty for a non-HTTP handler.
    pub responses: Vec<HttpResponse>,
    pub no_cross_context: Option<NoCrossContextReason>,
}

/// How many **real** contexts/adapters a project has — the input
/// [`wire_contract_at`]/[`wire_contract_for_service`]'s `context_count`
/// wants, deciding [`NoCrossContextReason::SingleContext`].
///
/// **Must** be computed this way, not as a bare `boundary_info.len()`:
/// `boundary_info` (mirroring `sequence_info`, #846) retains an entry for
/// every `UnitKind::Context | UnitKind::Adapter`, which includes the
/// synthetic toolchain-injected `bynk` capability-surface unit — so a
/// project with nothing but `consumes bynk { Clock }` would count *two*
/// "contexts" and `SingleContext` would almost never fire. `unit_sources`
/// (ADR 0095) already excludes synthetic units for exactly this reason
/// (document links, consumed-context navigation); intersecting against it
/// reuses that existing filter rather than a bespoke `name == "bynk"` check.
///
/// A free function (not buried in a caller's own helper) precisely so
/// `bynk-lsp`'s `bynk/wireContract` handler calls this — the one place the
/// filter is encoded — instead of re-deriving `boundary_info.len()` from
/// the plan's literal parameter name and reintroducing the bug.
pub fn real_context_count(
    boundary_info: &HashMap<String, ContextBoundaryInfo>,
    unit_sources: &HashMap<String, Vec<PathBuf>>,
) -> usize {
    boundary_info
        .keys()
        .filter(|k| unit_sources.contains_key(k.as_str()))
        .count()
}

/// Locate the `Handler` enclosing `offset` in `text` and build its wire
/// contract. Mirrors `sequence_request::sequence_model_at`'s re-parse
/// convention. `info` is the owning unit's retained boundary table (Phase
/// 3); `expr_types` is that same file's checked expression types (empty for
/// a file with errors — the `Ok`-overload disambiguation then falls back to
/// the declared return type, see `ResponseWalk::is_http_result_expr` below).
///
/// `context_count` is how many **real** contexts/adapters the project has —
/// build it with [`real_context_count`], not a bare `boundary_info.len()`
/// (see that function's doc for why the difference matters).
///
/// Scoped to service handlers (see the module doc); an agent handler at
/// `offset` answers `None`.
pub fn wire_contract_at(
    unit: &str,
    text: &str,
    offset: usize,
    info: &ContextBoundaryInfo,
    expr_types: &[(Span, TyId)],
    tys: &Types,
    context_count: usize,
) -> Option<WireContractModel> {
    let tokens = bynk_syntax::lexer::tokenize(text).ok()?;
    let (parsed, _errs) = bynk_syntax::parser::parse_unit_with_recovery(&tokens, text);
    let items: &[CommonsItem] = match parsed.as_ref()? {
        SourceUnit::Context(c) => &c.items,
        SourceUnit::Adapter(a) => &a.items,
        SourceUnit::Commons(_) | SourceUnit::Suite(_) => return None,
    };
    for item in items {
        if let CommonsItem::Service(s) = item
            && let Some(h) = handler_at(&s.handlers, offset)
        {
            return wire_contract_for_service(
                unit,
                text,
                &s.name.name,
                h,
                info,
                expr_types,
                tys,
                context_count,
            );
        }
    }
    None
}

fn handler_at(handlers: &[Handler], offset: usize) -> Option<&Handler> {
    handlers
        .iter()
        .find(|h| h.span.start <= offset && offset < h.span.end)
}

/// Build the wire contract for one already-located handler. The pure half of
/// [`wire_contract_at`] — the part that never touches the offset/re-parse
/// machinery — split out the same way `sequence_model_at` delegates to
/// `sequence::sequence_model`.
///
/// `service_name` must name an entry in `info.services` (the owning unit's
/// retained service table) — the handler is spliced into a clone of that
/// declaration (a synthetic single-handler service) so
/// `bynk_check::wire::collect_boundary_types` narrows its walk to exactly
/// this handler's params/return, not the whole service's. `None` if the
/// service is not (yet) in the retained table — a live-buffer/committed-round
/// mismatch (the same class of staleness `sequence_model_at` accepts for
/// `sequence_info`).
#[allow(clippy::too_many_arguments)]
pub fn wire_contract_for_service(
    unit: &str,
    text: &str,
    service_name: &str,
    handler: &Handler,
    info: &ContextBoundaryInfo,
    expr_types: &[(Span, TyId)],
    tys: &Types,
    context_count: usize,
) -> Option<WireContractModel> {
    let real = info.services.get(service_name)?;
    let mut synthetic = real.clone();
    synthetic.handlers = vec![handler.clone()];
    let services: HashMap<String, ServiceDecl> =
        HashMap::from([(service_name.to_string(), synthetic)]);
    let agents: HashMap<String, AgentDecl> = HashMap::new();

    let boundary_names = wire::collect_boundary_types(&info.types, &services, &agents);
    let insts =
        wire::collect_generic_instantiations(&services, &agents, &boundary_names, &info.types);
    // Every name reachable through `info.types` is this unit's own view of
    // its boundary — see the module doc's Provenance note.
    let boundary = wire::boundary_model(&boundary_names, &info.types, insts, |_| {
        wire::Provenance::Owned
    });

    let type_sites: HashMap<String, Span> = boundary_names
        .iter()
        .filter_map(|n| info.types.get(n).map(|d| (n.clone(), d.span)))
        .collect();

    let envelope = envelope_for(handler, &info.types);
    let kind = BoundaryKind::from_handler(handler);

    let no_cross_context = if context_count <= 1 {
        Some(NoCrossContextReason::SingleContext)
    } else if handler.kind != HandlerKind::Call {
        Some(NoCrossContextReason::NotACallHandler)
    } else {
        None
    };
    let contract = if no_cross_context.is_none() {
        Some(contract_form(service_name, handler, &info.types))
    } else {
        None
    };

    let responses = if matches!(kind, BoundaryKind::Http { .. }) {
        http_responses(handler, expr_types, tys)
    } else {
        Vec::new()
    };

    Some(WireContractModel {
        unit: unit.to_string(),
        service: service_name.to_string(),
        kind,
        handler_span: handler.span,
        handler_line: line_of(text, handler.span.start),
        envelope,
        contract,
        boundary,
        type_sites,
        responses,
        no_cross_context,
    })
}

/// 1-indexed line number of `offset` in `text`.
fn line_of(text: &str, offset: usize) -> usize {
    text.get(..offset).unwrap_or(text).matches('\n').count() + 1
}

fn envelope_for(handler: &Handler, types: &HashMap<String, std::sync::Arc<TypeDecl>>) -> Envelope {
    match handler.params.as_slice() {
        [] => Envelope::Empty,
        [p] => Envelope::Bare {
            param: p.name.name.clone(),
            shape: wire::wire_ref(&p.type_ref, types),
        },
        params => Envelope::Keyed {
            params: params
                .iter()
                .map(|p| (p.name.name.clone(), wire::wire_ref(&p.type_ref, types)))
                .collect(),
        },
    }
}

/// The same `CrossContextService` projection `own_contract_hashes`
/// (`bynk-emit/src/project.rs`) builds, canonicalised through the same
/// `info.types` table — so the hash this peek shows is provably the hash
/// the emitted `X-Bynk-Contract` constant stamps.
fn contract_form(
    service_name: &str,
    handler: &Handler,
    types: &HashMap<String, std::sync::Arc<TypeDecl>>,
) -> ContractForm {
    let svc = CrossContextService {
        name: service_name.to_string(),
        params: handler
            .params
            .iter()
            .map(|p| (p.name.name.clone(), p.type_ref.clone()))
            .collect(),
        return_type: handler.return_type.clone(),
        span: handler.span,
    };
    let normal_form = contract::service_normal_form(&svc, types);
    let hash = contract::contract_hash(&normal_form);
    ContractForm { normal_form, hash }
}

/// The reachable HTTP response set: the declared success case, every
/// variant literally constructed in the body, and the boundary-implicit
/// cases the body never names (400 on any param, 404 on an `Option?`
/// short-circuit — ADR 0177). 401 on a caller binding is deferred (plan
/// risk 8; needs actor/`by` resolution this phase does not do).
fn http_responses(
    handler: &Handler,
    expr_types: &[(Span, TyId)],
    tys: &Types,
) -> Vec<HttpResponse> {
    let declared_is_http_result =
        matches!(strip_effect(&handler.return_type), TypeRef::HttpResult(..));

    let mut out = Vec::new();
    if declared_is_http_result {
        out.push(HttpResponse {
            status: 200,
            variant: "Ok".to_string(),
            origin: ResponseOrigin::DeclaredSuccess,
        });
    }

    let mut walk = ResponseWalk {
        expr_types,
        tys,
        declared_is_http_result,
        seen: out.iter().map(|r| r.variant.clone()).collect(),
        saw_option_question: false,
        out: Vec::new(),
    };
    walk.walk_block(&handler.body);
    out.extend(walk.out);

    if !handler.params.is_empty() {
        out.push(HttpResponse {
            status: 400,
            variant: "StructuralMismatch".to_string(),
            origin: ResponseOrigin::BoundaryImplicit {
                why: "every param is structurally re-validated on the way in; a malformed or \
                      refinement-violating value fails closed with a 400 the handler body \
                      never names",
            },
        });
    }
    if walk.saw_option_question {
        out.push(HttpResponse {
            status: 404,
            variant: "NotFound".to_string(),
            origin: ResponseOrigin::BoundaryImplicit {
                why: "an `Option?` short-circuits to 404 on `None` (ADR 0177)",
            },
        });
    }
    out
}

/// Strip `Effect[_]` to expose the inner type — mirrors
/// `bynk-emit/src/emitter/workers_entry.rs`'s `http_result_inner`, one level
/// short of unwrapping `HttpResult` itself (the caller checks that).
fn strip_effect(t: &TypeRef) -> &TypeRef {
    match t {
        TypeRef::Effect(inner, _) => inner.as_ref(),
        other => other,
    }
}

/// The handler-body expression walk finding every literally-constructed
/// `HttpResult` variant, plus whether an `Option?` short-circuit is present
/// (the boundary-implicit 404). Structured after `bynk-ide/src/sequence.rs`'s
/// `Builder` — a small piece of walk state threaded through recursive
/// `walk_*` methods — but this walk is a *full* expression descent (via
/// `bynk_syntax::ast::expr_children`), not `sequence.rs`'s statement-level
/// control-flow-only walk: a response can be constructed anywhere in an
/// expression tree (a call argument, a record field), not only in tail
/// position.
struct ResponseWalk<'a> {
    expr_types: &'a [(Span, TyId)],
    /// T3.6b (R4.1): the table `expr_types`' ids resolve against.
    tys: &'a Types,
    /// #855 risk 7 ("the `Ok` overload"): the declared-return-type fallback
    /// used whenever `expr_types` has no entry for a span (a file with
    /// errors, ADR 0063's clean-file ceiling) — mirrors
    /// `bynk-emit/src/emitter/lower.rs`'s own `Ok`/`HttpResult` overload
    /// disambiguation, which has the identical ambiguity and the identical
    /// (checker-backed, so never actually degraded there) resolution.
    /// Applied to `Ok` and bare `Call` (see `is_http_result_expr` below) but
    /// **not** a bare `Ident` (see `is_http_result_ident` below), which has
    /// no concrete-shaped signal to fall back onto and would misreport an
    /// ordinary local whose name collides with a variant name.
    declared_is_http_result: bool,
    seen: std::collections::HashSet<String>,
    saw_option_question: bool,
    out: Vec<HttpResponse>,
}

impl<'a> ResponseWalk<'a> {
    fn expr_ty(&self, span: Span) -> Option<std::sync::Arc<Ty>> {
        self.expr_types
            .iter()
            .find(|(s, _)| *s == span)
            .map(|(_, t)| self.tys.get(*t))
    }

    /// Whether `span`'s expression checked as `HttpResult[_]` — the same
    /// disambiguation `lower.rs:720-731` performs for the `Ok` overload,
    /// degrading to the declared-return heuristic when the checker recorded
    /// nothing (see `declared_is_http_result`'s doc). Used for `Ok` and
    /// `Call`: a bare `Call { name, .. }` already carries a concrete
    /// variant-shaped name (`TooManyRequests(...)`), so the same fallback
    /// is low-risk there. **Not** used for a bare `Ident` — see
    /// `is_http_result_ident` below.
    fn is_http_result_expr(&self, span: Span) -> bool {
        match self.expr_ty(span).as_deref() {
            Some(Ty::HttpResult(_)) => true,
            Some(_) => false,
            None => self.declared_is_http_result,
        }
    }

    /// The stricter check for a bare `Ident` — never falls back to the
    /// declared-return heuristic. `HTTP_VARIANTS` includes names an author
    /// can plausibly bind as an ordinary local (`Found`, `Gone`, `Conflict`,
    /// `Accepted`, `Created`, `Raw`, `Streaming`): with the same fallback as
    /// `is_http_result_expr`, a file with errors and a `let found = …;
    /// found` tail on an `HttpResult`-returning handler would misreport a
    /// 302 that was never constructed. `Ok`/`Call` need the fallback because
    /// degrading them means answering nothing at all for the *one*
    /// expression the plan's risk 7 is about; a bare identifier read has no
    /// such asymmetry — unknown stays unknown.
    fn is_http_result_ident(&self, span: Span) -> bool {
        matches!(self.expr_ty(span).as_deref(), Some(Ty::HttpResult(_)))
    }

    fn push(&mut self, variant: HttpVariant, span: Span) {
        if self.seen.insert(variant.name.to_string()) {
            self.out.push(HttpResponse {
                status: variant.status,
                variant: variant.name.to_string(),
                origin: ResponseOrigin::Constructed { span },
            });
        }
    }

    fn walk_block(&mut self, b: &Block) {
        for s in &b.statements {
            let mut exprs = Vec::new();
            statement_exprs(s, &mut exprs);
            for e in exprs {
                self.walk_expr(e);
            }
        }
        self.walk_expr(&b.tail);
    }

    fn walk_expr(&mut self, e: &Expr) {
        self.classify(e);
        for child in expr_children(e) {
            self.walk_expr(child);
        }
    }

    /// The four recognition shapes `lower.rs` renders through
    /// (`HttpResult.Variant(args)`/`HttpResult.Variant` qualified forms at
    /// `:1417`/`:4149`, the bare `Ident`/`Call` forms disambiguated by
    /// checker type at `:3781`/`:3834`), plus the `Ok` overload
    /// (`:720-731`) and the `?`-on-`Option` boundary-implicit 404
    /// (`:764-768`, ADR 0177).
    fn classify(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::MethodCall {
                receiver, method, ..
            } => {
                if let ExprKind::Ident(id) = &receiver.kind
                    && id.name == HTTP_RESULT
                    && let Some(v) = http_variant(&method.name)
                {
                    self.push(v, e.span);
                }
            }
            ExprKind::FieldAccess { receiver, field } => {
                if let ExprKind::Ident(id) = &receiver.kind
                    && id.name == HTTP_RESULT
                    && let Some(v) = http_variant(&field.name)
                {
                    self.push(v, e.span);
                }
            }
            ExprKind::Ident(id) => {
                if self.is_http_result_ident(e.span)
                    && let Some(v) = http_variant(&id.name)
                {
                    self.push(v, e.span);
                }
            }
            ExprKind::Call { name, .. } => {
                if self.is_http_result_expr(e.span)
                    && let Some(v) = http_variant(&name.name)
                {
                    self.push(v, e.span);
                }
            }
            ExprKind::Ok(_) => {
                if self.is_http_result_expr(e.span)
                    && let Some(v) = http_variant("Ok")
                {
                    self.push(v, e.span);
                }
            }
            ExprKind::Question(inner) => {
                if matches!(self.expr_ty(inner.span).as_deref(), Some(Ty::Option(_))) {
                    self.saw_option_question = true;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Same convention as `sequence.rs`/`architecture.rs`'s `setup_project`:
    /// a temp dir unique to the test name, self-contained fixtures only
    /// (never `examples/` — `bynk-ide` is published standalone).
    fn setup_project(test_name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "bynk-ide-wire-contract-test-{test_name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create test root");
        for (rel, contents) in files {
            let p = root.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).expect("create parent");
            }
            std::fs::write(&p, contents).expect("write file");
        }
        root
    }

    // -- Fixture: examples/rate-limiter's `GET /check/:client`, reproduced
    // -- self-contained (not read from `examples/` — see `setup_project`)
    // -- and simplified to a single file (no `uses window` — irrelevant to
    // -- what this module resolves).
    const RATELIMIT_SRC: &str = r#"context ratelimit

consumes bynk { Clock }

type ClientId = String where NonEmpty

type RateView = {
  allowed:   Bool,
  remaining: Int,
  resetAt:   Int,
}

agent Limiter {
  key client: ClientId

  store count: Cell[Int]

  on call hit(now: Int) -> Effect[RateView] {
    let _ <- count.update((c) => c + 1)
    RateView { allowed: count < 10, remaining: 10 - count, resetAt: now }
  }
}

service api from http {
  on GET("/check/:client") (client: ClientId) -> Effect[HttpResult[RateView]] by Visitor given Clock {
    let now  <- Clock.now()
    let view <- Limiter(client).hit(now.toEpochMillis())
    if view.allowed {
      Ok(view)
    } else {
      TooManyRequests("rate limit exceeded")
    }
  }
}
"#;

    fn find_offset(text: &str, needle: &str) -> usize {
        text.find(needle)
            .unwrap_or_else(|| panic!("`{needle}` not found in fixture"))
    }

    /// Thin wrapper over the module's own [`real_context_count`] — exercises
    /// the exact function `bynk-lsp`'s `bynk/wireContract` handler must call
    /// (Phase 5), rather than a test-local re-derivation that could drift
    /// from it.
    fn real_context_count(diag: &crate::ProjectDiagnostics) -> usize {
        super::real_context_count(&diag.boundary_info, &diag.unit_sources)
    }

    #[test]
    fn rate_limiter_get_check_client_is_a_bare_envelope_with_a_revalidated_client_id() {
        let root = setup_project("ratelimit", &[("ratelimit.bynk", RATELIMIT_SRC)]);
        let diag = crate::diagnose_project(&root, &HashMap::new());
        let info = diag
            .boundary_info
            .get("ratelimit")
            .expect("boundary_info entry for ratelimit");

        let offset = find_offset(RATELIMIT_SRC, "GET(\"/check/:client\")");
        let model = wire_contract_at(
            "ratelimit",
            RATELIMIT_SRC,
            offset,
            info,
            &[],
            &diag.ty_intern,
            real_context_count(&diag),
        )
        .expect("a wire contract at the GET handler's header");

        assert_eq!(model.unit, "ratelimit");
        assert_eq!(model.service, "api");
        assert_eq!(
            model.kind,
            BoundaryKind::Http {
                method: HttpMethod::Get,
                path: "/check/:client".to_string(),
            }
        );

        // One param (`client`) sends the bare value — not wrapped in an
        // object, and not the two-case envelope the issue text describes.
        let (param, shape) = match &model.envelope {
            Envelope::Bare { param, shape } => (param, shape),
            other => panic!("expected Envelope::Bare, got {other:?}"),
        };
        assert_eq!(param, "client");
        assert!(
            matches!(shape, WireRef::Named { name } if name == "ClientId"),
            "the bare param's shape should resolve to the named ClientId type: {shape:?}"
        );

        // `ClientId` is in the boundary type set, owned (declared in this
        // same context), refined `NonEmpty`, revalidated via its own
        // constructor.
        let client_id = model
            .boundary
            .types
            .iter()
            .find(|t| t.name == "ClientId")
            .expect("ClientId is a boundary type");
        assert_eq!(client_id.provenance, bynk_check::wire::Provenance::Owned);
        let bynk_check::wire::WireBody::Scalar(scalar) = &client_id.body else {
            panic!("ClientId should be a scalar, got {:?}", client_id.body);
        };
        assert!(
            scalar
                .predicates
                .iter()
                .any(|p| matches!(p, PredKind::NonEmpty)),
            "ClientId's predicates should include NonEmpty: {:?}",
            scalar.predicates
        );
        assert_eq!(
            scalar.revalidation,
            bynk_check::wire::Revalidation::ViaConstructor
        );
        assert!(model.type_sites.contains_key("ClientId"));

        // A single-context project: even though `api`'s handler is `Http`
        // (never `NotACallHandler` would even get a chance to fire), the
        // more fundamental "there is no other context" reason wins.
        assert_eq!(
            model.no_cross_context,
            Some(NoCrossContextReason::SingleContext)
        );
        assert!(model.contract.is_none());
    }

    #[test]
    fn rate_limiter_response_set_has_declared_constructed_and_boundary_implicit() {
        let root = setup_project("ratelimit-responses", &[("ratelimit.bynk", RATELIMIT_SRC)]);
        let diag = crate::diagnose_project(&root, &HashMap::new());
        let info = diag.boundary_info.get("ratelimit").expect("entry");

        // Drive with the round's own retained `expr_types` for this file —
        // the primary path (not the degraded fallback).
        let rel = diag
            .files
            .iter()
            .find(|f| {
                f.source_path
                    .file_name()
                    .is_some_and(|n| n == "ratelimit.bynk")
            })
            .map(|f| f.source_path.clone())
            .expect("ratelimit.bynk in the round's files");
        let expr_types: &[(Span, TyId)] = diag
            .expr_types
            .get(&rel)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        let offset = find_offset(RATELIMIT_SRC, "GET(\"/check/:client\")");
        let model = wire_contract_at(
            "ratelimit",
            RATELIMIT_SRC,
            offset,
            info,
            expr_types,
            &diag.ty_intern,
            real_context_count(&diag),
        )
        .expect("a wire contract at the GET handler's header");

        let statuses: Vec<(u16, &str)> = model
            .responses
            .iter()
            .map(|r| (r.status, r.variant.as_str()))
            .collect();
        assert!(
            statuses.contains(&(200, "Ok")),
            "declared success missing: {statuses:?}"
        );
        assert!(
            statuses.contains(&(429, "TooManyRequests")),
            "constructed TooManyRequests missing: {statuses:?}"
        );
        assert!(
            statuses.iter().any(|&(s, _)| s == 400),
            "boundary-implicit 400 (the handler has a param) missing: {statuses:?}"
        );
        assert!(
            model
                .responses
                .iter()
                .find(|r| r.status == 200)
                .is_some_and(|r| r.origin == ResponseOrigin::DeclaredSuccess)
        );
        assert!(
            model.responses.iter().any(|r| matches!(
                r.origin,
                ResponseOrigin::BoundaryImplicit { .. }
            ) && r.status == 400)
        );
    }

    // -- Fixture 2: a two-context project with a real `on call` boundary —
    // -- the contract form + hash + Envelope::Keyed path (multi-param).
    const PROVIDER_SRC: &str = r#"context billing

type Quote = { amount: Int, currency: String }

service Pricing {
  on call(sku: String, qty: Int) -> Effect[Quote] {
    Quote { amount: qty * 100, currency: "USD" }
  }
}
"#;
    const CONSUMER_SRC: &str = r#"context storefront

consumes billing

service checkout {
  on call(sku: String, qty: Int) -> Effect[Int] {
    let q <- billing.Pricing(sku, qty)
    q.amount
  }
}
"#;

    #[test]
    fn two_context_call_handler_has_a_keyed_envelope_and_a_contract_hash() {
        let root = setup_project(
            "two-context",
            &[
                ("billing.bynk", PROVIDER_SRC),
                ("storefront.bynk", CONSUMER_SRC),
            ],
        );
        let diag = crate::diagnose_project(&root, &HashMap::new());
        let info = diag
            .boundary_info
            .get("billing")
            .expect("boundary_info entry for billing");

        let offset = find_offset(PROVIDER_SRC, "on call(");
        let model = wire_contract_at(
            "billing",
            PROVIDER_SRC,
            offset,
            info,
            &[],
            &diag.ty_intern,
            real_context_count(&diag),
        )
        .expect("a wire contract at the `price` handler");

        assert_eq!(model.kind, BoundaryKind::Call);
        let params = match &model.envelope {
            Envelope::Keyed { params } => params,
            other => panic!("expected Envelope::Keyed for a two-param call, got {other:?}"),
        };
        assert_eq!(
            params.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            vec!["sku", "qty"],
            "keyed params stay in declaration order"
        );

        assert!(model.no_cross_context.is_none(), "two contexts + on call");
        let contract = model.contract.expect("on call gets a contract form");
        assert_eq!(
            contract.hash,
            bynk_check::contract::contract_hash(&contract.normal_form)
        );

        // Independent re-derivation via the exact projection
        // `own_contract_hashes` uses, against the same retained table — the
        // hovered hash must equal what the emitter would stamp.
        let svc = CrossContextService {
            name: "Pricing".to_string(),
            params: vec![
                (
                    "sku".to_string(),
                    TypeRef::Base(BaseType::String, Span::new(0, 0)),
                ),
                (
                    "qty".to_string(),
                    TypeRef::Base(BaseType::Int, Span::new(0, 0)),
                ),
            ],
            return_type: TypeRef::Named(Ident {
                name: "Quote".to_string(),
                span: Span::new(0, 0),
            }),
            span: Span::new(0, 0),
        };
        let independent_form = contract::service_normal_form(&svc, &info.types);
        assert_eq!(contract.normal_form, independent_form);
        assert_eq!(contract.hash, contract::contract_hash(&independent_form));
    }

    #[test]
    fn zero_param_call_handler_is_the_empty_envelope() {
        let src = r#"context solo

service Ping {
  on call() -> Effect[Int] {
    1
  }
}
"#;
        let root = setup_project("zero-param", &[("solo.bynk", src)]);
        let diag = crate::diagnose_project(&root, &HashMap::new());
        let info = diag.boundary_info.get("solo").expect("entry");

        let offset = find_offset(src, "on call()");
        let model = wire_contract_at(
            "solo",
            src,
            offset,
            info,
            &[],
            &diag.ty_intern,
            real_context_count(&diag),
        )
        .expect("a wire contract at the `Ping` handler");

        assert!(
            matches!(model.envelope, Envelope::Empty),
            "zero params: the request body is not read, not an empty keyed object"
        );
        // Single-context project: `on call` still answers SingleContext, not
        // a real contract.
        assert_eq!(
            model.no_cross_context,
            Some(NoCrossContextReason::SingleContext)
        );
    }

    // -- Regression: the `Ok`-overload `expr_types` fallback (plan risk 7)
    // -- must not extend to a bare `Ident`. `HTTP_VARIANTS` includes names
    // -- an author can plausibly bind as an ordinary local (`Found`, `Gone`,
    // -- `Conflict`, …) — without a recorded expr type, a bare `Found` read
    // -- must stay unknown, not get misreported as a constructed 302.
    #[test]
    fn bare_ident_collision_with_a_variant_name_is_not_misreported_without_expr_types() {
        const SRC: &str = r#"context oddnames

service api from http {
  on GET("/x") () -> Effect[HttpResult[Int]] by Visitor {
    let Found = 1
    Found
  }
}
"#;
        let root = setup_project("ident-collision", &[("oddnames.bynk", SRC)]);
        let diag = crate::diagnose_project(&root, &HashMap::new());
        let info = diag
            .boundary_info
            .get("oddnames")
            .expect("boundary_info entry for oddnames");

        let offset = find_offset(SRC, "GET(\"/x\")");
        // Deliberately pass an empty `expr_types` — the degraded path (a
        // file with errors, ADR 0063's clean-file ceiling) this fixture is
        // standing in for, even though it type-checks cleanly on its own.
        let model = wire_contract_at(
            "oddnames",
            SRC,
            offset,
            info,
            &[],
            &diag.ty_intern,
            real_context_count(&diag),
        )
        .expect("a wire contract at the GET handler");

        assert!(
            model.responses.iter().all(|r| r.variant != "Found"),
            "a bare `Found` local must not be misreported as HttpResult.Found \
             without a recorded expr type: {:?}",
            model.responses
        );
    }
}
