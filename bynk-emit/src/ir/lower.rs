//! P6.1 (#1141): the `&CheckedProgram → Ir` lowering pass — real construction
//! for the node kinds `design/tracks/the-ir.md`'s own P6.1 row names (`Const`,
//! `Local`, `Global`, `Record`, `Field`, `List`, `Block`, `If`, `And`, `Or`,
//! `Not`, `Return`, `Await`, `Send`, `Pure`); every other [`IrExprKind`]/
//! [`IrStmt`] arm is a `todo!()` naming the slice that completes it
//! (Decision D). Nothing in this module is called from anywhere in
//! `bynk-emit`'s existing emission path (`emitter.rs`/`emitter/lower.rs`) —
//! it has no consumer yet, so it cannot change any emitted output.
//!
//! **Totality discipline (ADR 0334, Q2):** every entry point here takes a
//! `&CheckedProgram`, not a bare `&TypedCommons` — a certified program only,
//! so [`LowerIrCtx::expr_ty`]'s `.expect()` on a miss is the checker and this
//! pass disagreeing about which expressions a unit contains, a compiler bug,
//! not a recoverable state. This scoping is the same discipline
//! `bynk-emit/src/emitter/emit.rs`'s `lower_workers_cross_context_call`
//! already applies to its own `bynk.emit.unresolved_cross_context_signature`
//! panic.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bynk_check::checker::{self, Callee, CheckedProgram, Ty, TyId, TypedCommons};
use bynk_syntax::ast::{
    ActorDecl, AgentDecl, BinOp, Block, CapRef, CapabilityDecl, CapabilityOp, EventPattern,
    EventPatternValue, Expr, ExprId, ExprKind, FieldInit, FnDecl, FnName, Handler, HandlerKind,
    HttpMethod, InterpPart, Invariant, LambdaExpr, LiteralValue, MatchArm, MatchBody, Pattern,
    PatternBindingKind, ProviderDecl, ProviderOp, QualifiedName, ServiceDecl, ServiceProtocol,
    Statement, StoreField, Transition, TypeBody, TypeDecl, UnaryOp, expr_children,
};
use bynk_syntax::span::Span;

use crate::emitter::{
    MUTATING_CELL_OPS, MUTATING_LOG_OPS, MUTATING_MAP_CACHE_OPS, MUTATING_SET_OPS, block_uses_emit,
    match_needs_if_chain,
};
use crate::ir::{
    ActorBinder, ActorSeamIr, BindingMode, CacheIr, CapRefIr, CommitShape, ConnectionBinder,
    ConstVal, CorsIr, EventPatternIr, EventPatternValueIr, Exhaustive, GlobalRef, IndexIr, IrArm,
    IrBinOp, IrExpr, IrExprKind, IrHandler, IrInterpPart, IrItem, IrPat, IrPredicate, IrStmt,
    MatchForm, OpSig, PolicyIr, ProtocolIr, ProviderBody, ProviderOpIr, SecurityIr, StoreFieldIr,
    StoreKindIr, TypeShape,
};

/// The lowering pass's own working state: the certified program's typed
/// output (for `.ty`/`.types` lookups), a lexical scope stack this pass
/// tracks itself — `TypedCommons` has no persisted "what type does this bound
/// name have" table (that lived only in the checker's own transient `Ctx`),
/// so the one case that needs it (a record shorthand field, `{ x }`, which
/// has no `ExprId` of its own to key `expr_types` by) re-derives it from the
/// same param/`let` binding sites the checker itself walked — and the
/// enclosing fn/method's own rigid type variables (`fn identity[T](x: T)`,
/// and a generic type's own params on one of its methods), needed by
/// `resolve_type_ref_in` the same way `Ctx::type_vars` is
/// (`bynk-check/src/checker.rs:2816`); `resolve_type_ref` (no `vars` set)
/// would otherwise resolve a rigid `T` as an unknown declared type and
/// silently fail.
pub(crate) struct LowerIrCtx<'a> {
    program: &'a TypedCommons,
    scopes: Vec<HashMap<String, TyId>>,
    type_vars: HashSet<String>,
    /// Bumped once per [`lower_record_spread_ir`] call — a fixed temp name
    /// is unique against every *source-level* name (`__` is unlexable) but
    /// not against another spread on the same nesting chain (a spread
    /// inside an override value, e.g. `Point { ...p, x: (Point { ...q, x:
    /// 0 }).x }`); a monotonic per-function suffix removes that hazard.
    spread_tmp_counter: usize,
}

impl<'a> LowerIrCtx<'a> {
    fn new(program: &'a CheckedProgram, type_vars: HashSet<String>) -> Self {
        Self::from_commons(program.program(), type_vars)
    }

    /// #1187's own closing scoping pass: a `TypedCommons`-only constructor,
    /// for the one real call path (`lower_op_sig_ir_from_commons`, below)
    /// that never has a `&CheckedProgram` to unwrap. See that function's
    /// own doc comment for why.
    fn from_commons(commons: &'a TypedCommons, type_vars: HashSet<String>) -> Self {
        Self {
            program: commons,
            scopes: vec![HashMap::new()],
            type_vars,
            spread_tmp_counter: 0,
        }
    }

    fn fresh_spread_tmp(&mut self) -> String {
        let n = self.spread_tmp_counter;
        self.spread_tmp_counter += 1;
        format!("__spread_base_{n}")
    }

    /// A type reference resolved in this pass's own rigid-variable scope —
    /// the `resolve_type_ref_in` counterpart to `Ctx::resolve_type_ref_in`
    /// call sites (e.g. `checker.rs:2816,2897`), not the bare
    /// `resolve_type_ref` (which has no `vars` set and cannot resolve a
    /// generic fn/method's own type parameters).
    fn resolve_type_ref(&self, r: &bynk_syntax::ast::TypeRef) -> Option<TyId> {
        checker::resolve_type_ref_in(
            r,
            &self.program.types,
            &self.type_vars,
            &self.program.ty_intern,
        )
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn bind(&mut self, name: String, ty: TyId) {
        self.scopes
            .last_mut()
            .expect("bynk internal error: LowerIrCtx's scope stack is never empty")
            .insert(name, ty);
    }

    fn lookup(&self, name: &str) -> Option<TyId> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }

    /// R6.1 / ADR 0334: the checker already resolved every real expression's
    /// type before this pass runs — a miss here means this pass and the
    /// checker disagree about which expressions the unit contains, which is
    /// a compiler bug on the certified-program path this pass is scoped to,
    /// not a fallback-shaped state.
    fn expr_ty(&self, id: ExprId) -> TyId {
        self.program
            .expr_types
            .get(&id)
            .unwrap_or_else(|| {
                panic!(
                    "bynk internal error (ADR 0334): no recorded type for {id:?} — \
                     bynk-emit::ir::lower and bynk-check disagree about which \
                     expressions this certified unit contains"
                )
            })
            .ty
    }

    /// `Effect[T] -> T` — the type an `Await`/`EffectLet`/`Do` binds, not the
    /// effect value's own recorded type. Falls back to `ty` unchanged on a
    /// non-`Effect` input rather than panicking: every real call site here
    /// only peels a type the checker already required to be `Effect[_]`
    /// (`<-`/`do`/`~>`'s own gate, `bynk-check/src/checker.rs`'s
    /// `Ctx::effectful` sites), so this is defensive, not load-bearing.
    fn peel_effect(&self, ty: TyId) -> TyId {
        match &*self.program.ty_intern.get(ty) {
            Ty::Effect(inner) => *inner,
            _ => ty,
        }
    }

    fn unit_ty(&self) -> TyId {
        self.program.ty_intern.intern(Ty::Unit)
    }

    /// P6.2 (#1143): the `Callee` `bynk-check` recorded for a call-shaped
    /// expression (P6.0), if any. Deliberately `Option`, not `.expect()`-ed
    /// like `expr_ty` — a certified program is only guaranteed a `Callee`
    /// for shapes `calls.rs`'s six functions or the store-field ladder
    /// dispatch (P6.0/P6.2's own "Done when" scope); a handful of shapes
    /// this slice's own Decision C left out on purpose (`HttpResult`/
    /// `QueueResult` bare-variant construction, `Events.emit`, the
    /// production `is_system_http_service` address) reach here with none —
    /// an expected miss, not a bug, so the caller decides what to do with
    /// `None` rather than this accessor panicking on a state that is
    /// sometimes legitimate.
    fn callee(&self, id: ExprId) -> Option<&Callee> {
        self.program.callees.get(&id)
    }
}

/// A fn/method's own rigid type variables — its own `[T, ...]` type
/// parameters, plus a generic receiver's, for a method — the `vars` set
/// `resolve_type_ref_in` needs to resolve a rigid `T` as `Ty::Var` rather
/// than an unknown declared type. Shared by [`lower_fn_body_ir`] and
/// [`lower_fn_item_ir`] so the two can never independently drift on which
/// names are rigid for the same `f`: both functions' own `ADR 0334` panic
/// text warns specifically against `bynk-emit::ir::lower`'s `type_vars`
/// disagreeing with `bynk-check`'s `Ctx::type_vars`, and a hand-duplicated
/// copy of this exact computation would be one more place that guarantee
/// could quietly stop holding.
fn fn_rigid_type_vars(f: &FnDecl, program: &TypedCommons) -> HashSet<String> {
    let mut type_vars: HashSet<String> = f
        .type_params
        .iter()
        .map(|tp| tp.name.name.clone())
        .collect();
    if let FnName::Method { type_name, .. } = &f.name
        && let Some(decl) = program.types.get(&type_name.name)
    {
        type_vars.extend(decl.type_params.iter().map(|tp| tp.name.name.clone()));
    }
    type_vars
}

/// A method's own generic receiver type — `Box[A]`'s `self` is
/// `Ty::Named { name: "Box", args: [Ty::Var("A")], .. }`, not a fixed
/// instantiation like `Box[Int]` — or `None` for a free function or a
/// method declared with no `self` parameter. Shared by
/// [`lower_fn_body_ir`] (which binds the result into scope under
/// `"self"`) and [`lower_fn_item_ir`] (which records it verbatim as
/// `IrItem::Fn::receiver`) — the same value, computed once.
fn fn_receiver_ty(f: &FnDecl, program: &TypedCommons) -> Option<TyId> {
    let FnName::Method { type_name, .. } = &f.name else {
        return None;
    };
    if !f.has_self {
        return None;
    }
    let decl = program.types.get(&type_name.name)?;
    let self_args = decl
        .type_params
        .iter()
        .map(|tp| program.ty_intern.intern(Ty::Var(tp.name.name.clone())))
        .collect();
    Some(checker::named_ty_with_args(
        decl,
        self_args,
        &program.ty_intern,
    ))
}

/// Wrap a lowered body block's own tail in [`IrExprKind::Return`] — the one
/// place this pass builds a `Return` node (Bynk has no `return` keyword; see
/// that variant's own doc comment). Shared by [`lower_fn_body_ir`] and
/// [`lower_handler_body_ir`] (P6.9, #1167) — both are body-lowering entry
/// points that produce a `Block` and need the exact same tail-wrapping, and
/// hand-duplicating it a second time is exactly the kind of re-derivation
/// this module's own header doc (`Callee`, P6.0) already avoids elsewhere.
fn wrap_body_return(block: IrExpr) -> IrExpr {
    let IrExpr {
        kind: IrExprKind::Block { stmts, tail },
        ty,
        span,
    } = block
    else {
        unreachable!("lower_block_ir always returns IrExprKind::Block");
    };
    let tail_ty = tail.ty;
    let tail_span = tail.span;
    IrExpr {
        kind: IrExprKind::Block {
            stmts,
            tail: Box::new(IrExpr {
                kind: IrExprKind::Return { value: tail },
                ty: tail_ty,
                span: tail_span,
            }),
        },
        ty,
        span,
    }
}

/// Lower a function/method body: seeds scope from `f`'s params (and its own
/// rigid type variables — its own `[T, ...]` type parameters, plus a
/// generic receiver's, for a method), lowers the body as an ordinary value
/// block, then wraps the tail via [`wrap_body_return`]. Distinct from
/// [`lower_block_ir`], which lowers a *nested* block as a bare value with no
/// such wrapping.
///
/// Handler bodies are out of scope for this entry point: a handler's own
/// non-local bare-ident forms (store-field/cell reads, agent `self`, the
/// actor binder, transition `old`/`new`) need resolved-identity plumbing
/// (`store_fields`, `agent_state_ty`, `actor_binding` — the `Ctx` fields
/// `check_handler_body` seeds, `checker.rs:930-960`) this entry point has no
/// parameter for and does not commission (P6.1's own Decision C) — calling
/// this on a handler body would silently misclassify any bare ident
/// matching one of those forms as a `Local`/`Global` miss (`todo!()`)
/// rather than the specific kind it actually is. [`lower_handler_body_ir`]
/// (P6.9, #1167) is that dedicated entry point, finally closing this gap —
/// it is *not* built by widening this function, since a free fn/method and
/// an agent handler seed genuinely different scopes (rigid type variables
/// here, agent `self`/store cells there) that would otherwise have to
/// coexist behind one signature for no shared benefit.
/// [`lower_service_handler_body_ir`] (P6.11, #1171) is the third sibling on
/// the same rule, not a second widening — a service handler's own scope
/// (`params`/`binder` only) is disjoint from both of the other two.
pub(crate) fn lower_fn_body_ir(f: &FnDecl, program: &CheckedProgram) -> IrExpr {
    let type_vars = fn_rigid_type_vars(f, program.program());
    let mut cx = LowerIrCtx::new(program, type_vars);
    // `self` is never in `f.params` (`FnDecl::has_self` gates it) — mirrors
    // `check_fn`'s own binding (`checker.rs`, the `f.has_self` arm): a
    // generic receiver's `self` carries the receiver applied to its own
    // rigid type variables (`Box[A]`'s `self`, not a fixed `Box[Int]`).
    if let Some(self_ty) = fn_receiver_ty(f, program.program()) {
        cx.bind("self".to_string(), self_ty);
    }
    for p in &f.params {
        let ty = cx.resolve_type_ref(&p.type_ref).unwrap_or_else(|| {
            panic!(
                "bynk internal error (ADR 0334): parameter `{}`'s type does not resolve in this \
                 pass's own rigid-variable scope, but the checker already accepted this fn body — \
                 bynk-emit::ir::lower's type_vars disagrees with bynk-check's Ctx::type_vars",
                p.name.name
            )
        });
        cx.bind(p.name.name.clone(), ty);
    }
    let block = lower_block_ir(&f.body, &mut cx);
    wrap_body_return(block)
}

/// Lower an agent `on call` handler's own body ([DECISION E], P6.9, #1167) —
/// [`lower_fn_body_ir`]'s own doc comment names exactly why that entry point
/// cannot be reused here: seeds a fresh scope from `self` (bound to
/// `state_ty`, mirroring `check_handler_body`'s own `agent_self_scope`
/// construction, `context_checks.rs:2900-2915`), every `store_cells` entry
/// by bare name (the v0.81 "implicit deref in read position" rule the
/// checker's own `self_scope` loop applies, `context_checks.rs:2910-2915`),
/// and `h`'s own `params` — then lowers the body as an ordinary value block
/// and wraps the tail via [`wrap_body_return`], same as
/// [`lower_fn_body_ir`]. No rigid type variables: `AgentDecl` carries no
/// `type_params` of its own, the same fact
/// [`resolve_store_field_ty`]'s own doc comment already grounds for
/// [`lower_store_field_ir`]. `binder` is not bound into scope: this entry
/// point is only ever reached from [`lower_handler_ir`]'s own agent-only
/// path ([DECISION D]), where `binder` is `None` unconditionally
/// ([`crate::ir::IrHandler`]'s own doc comment). As of P6.11 (#1171) a real
/// service-handler caller does exist — [`lower_service_handler_body_ir`] —
/// and it is a sibling of this function, not a widening of it; that
/// function's own doc comment names the scope-seeding differences in full.
///
/// Once scope is seeded, every store-field method call and every bare `:=`
/// in `h.body` reaches the *ordinary* [`lower_block_ir`]/[`lower_expr_ir`]/
/// [`lower_stmt_ir`] path and lowers correctly with no new call-lowering
/// logic ([DECISION F]): [`lower_call_ir`]'s own existing, already-shipped
/// generic `Callee`-wrapping (P6.2) already lowers a `Callee::Store`- or
/// `Callee::Capability`-classified call the moment it's reached, and
/// [`lower_stmt_ir`]'s own `Statement::Assign` arm ([DECISION B]) is real as
/// of this same slice — this function's only job is making both reachable
/// on a handler body at all, by seeding the scope [`lower_ident_ir`] needs
/// to classify `self`/a store cell as `Local` rather than falling through to
/// its own unresolved-ident `todo!()`.
fn lower_handler_body_ir(
    h: &Handler,
    store_cells: &HashMap<String, TyId>,
    state_ty: TyId,
    program: &CheckedProgram,
) -> IrExpr {
    let mut cx = LowerIrCtx::new(program, HashSet::new());
    cx.bind("self".to_string(), state_ty);
    for (name, ty) in store_cells {
        cx.bind(name.clone(), *ty);
    }
    for p in &h.params {
        let ty = cx.resolve_type_ref(&p.type_ref).unwrap_or_else(|| {
            panic!(
                "bynk internal error (ADR 0334): handler parameter `{}`'s type does not resolve \
                 in this pass's own scope, but the checker already accepted this handler",
                p.name.name
            )
        });
        cx.bind(p.name.name.clone(), ty);
    }
    let block = lower_block_ir(&h.body, &mut cx);
    wrap_body_return(block)
}

/// P6.9's real `IrHandler` constructor ([DECISION C]/[DECISION D]/
/// [DECISION E], #1167) — lowers an agent `on call` handler `h` into a real
/// [`IrHandler`]. `store_cells`/`state_ty`/`invariants`/`transitions` are
/// parameters, not re-derived ([DECISION E], mirroring
/// [`lower_invariant_ir`]'s/[`lower_transition_ir`]'s own precedent, P6.8):
/// no persisted "this agent's store cells / state type" table survives
/// `check_agent_decls`'s own transient scope, and `invariants`/`transitions`
/// are themselves already-lowered [`IrPredicate`]s a caller must have
/// produced via those same two functions — this function only threads them
/// into [`lower_commit_shape_ir`], never lowers or re-derives them itself. A
/// future `IrItem::Agent` builder (not commissioned by this slice — see
/// [`crate::ir::IrItem`]'s own doc comment) computes all four once per agent
/// and calls this function once per handler, not re-deriving any of them per
/// call.
///
/// Threading real `invariants`/`transitions` through (review of #1167,
/// replacing this function's own original empty-slices posture) matters
/// beyond completeness: an empty pair is structurally indistinguishable from
/// a correct lowering of an agent that genuinely declares neither, so an
/// empty-by-construction `commit` would have been a silent-wrong-value trap
/// for whatever future caller forgot to populate them for real.
///
/// Stays agent-only as of P6.11 (#1171) — [`lower_service_handler_ir`] is
/// this function's own sibling for a service handler, not a widening of
/// it; see that function's own doc comment for the three reasons the split
/// is deliberate, and this function's own `by_clause.is_none()` assert
/// below for the guard widening would have deleted.
pub(crate) fn lower_handler_ir(
    h: &Handler,
    store_cells: &HashMap<String, TyId>,
    state_ty: TyId,
    invariants: &[IrPredicate],
    transitions: &[IrPredicate],
    program: &CheckedProgram,
) -> IrHandler {
    // [DECISION D] is agent-only by contract, not merely by convention — an
    // agent handler is checker-guaranteed to carry no `by` clause
    // (`bynk.actor.by_on_agent`, `context_checks.rs:2986-2996`), so a
    // `Handler` that has one has reached this entry point in error (a future
    // service-handler caller silently dropping the binder), the same
    // "assert the checker's invariant loudly" idiom this module's own ADR
    // 0334 panics already use throughout.
    assert!(
        h.by_clause.is_none(),
        "bynk internal error (ADR 0334): lower_handler_ir is agent-only (DECISION D) — an agent \
         handler cannot carry a `by` clause (bynk.actor.by_on_agent), so a handler that does is a \
         service handler reaching the wrong entry point"
    );
    let cx = LowerIrCtx::new(program, HashSet::new());
    let (params, given, ret, effectful) = lower_handler_signature_ir(h, &cx);
    let emits = block_uses_emit(&h.body, &program.program().callees);
    let commit = lower_commit_shape_ir(&h.body, invariants, transitions, emits, program);
    let body = lower_handler_body_ir(h, store_cells, state_ty, program);
    IrHandler {
        kind: h.kind.clone(),
        params,
        given,
        // `h.by_clause.is_none()` was just asserted above — always empty,
        // not merely usually so.
        actors: Vec::new(),
        binder: None,
        // Agent-only by contract (asserted above): the synthetic
        // `connection` binding is a `from websocket` service-handler
        // concept exclusively — see [`ConnectionBinder`]'s own doc
        // comment.
        connection: None,
        body,
        commit,
        ret,
        effectful,
        method_name: h.method_name.as_ref().map(|i| i.name.clone()),
    }
}

/// `(params, given, ret, effectful)` — [`lower_handler_signature_ir`]'s own
/// return shape, and [`lower_service_handler_signature_ir`]'s (#1187's slice
/// 5), reused as a named alias rather than a bare tuple at both call sites
/// once one of them (`emit_service`, `bynk-emit/src/emitter/emit.rs`) had to
/// spell it out in a function signature.
pub(crate) type HandlerSignatureIr = (Vec<(String, TyId)>, Vec<String>, TyId, bool);

/// A handler's own `params`/`given`/`effectful` — the one part of
/// [`IrHandler`] construction genuinely identical between an agent handler
/// ([`lower_handler_ir`]) and a service handler
/// ([`lower_service_handler_ir`], P6.11, #1171), extracted so the two don't
/// hand-duplicate it. Everything else about the two — scope seeding,
/// `binder`, the WebSocket deferral — is genuinely different and stays
/// unshared; see [`lower_handler_ir`]'s own doc comment for why that split
/// is deliberate, not an oversight.
fn lower_handler_signature_ir(h: &Handler, cx: &LowerIrCtx) -> HandlerSignatureIr {
    let params: Vec<(String, TyId)> = h
        .params
        .iter()
        .map(|p| {
            let ty = cx.resolve_type_ref(&p.type_ref).unwrap_or_else(|| {
                panic!(
                    "bynk internal error (ADR 0334): handler parameter `{}`'s type does not \
                     resolve in this pass's own scope, but the checker already accepted this \
                     handler",
                    p.name.name
                )
            });
            (p.name.name.clone(), ty)
        })
        .collect();
    let given: Vec<String> = h.given.iter().map(|c| c.key().to_string()).collect();
    let ret = cx.resolve_type_ref(&h.return_type).unwrap_or_else(|| {
        panic!(
            "bynk internal error (ADR 0334): a handler's return type does not resolve in this \
             pass's own scope, but the checker already accepted this handler"
        )
    });
    let effectful = matches!(&*cx.program.ty_intern.get(ret), Ty::Effect(_));
    (params, given, ret, effectful)
}

/// #1187's slice 5 (the `Service` emitter cutover): `emit_service`'s own
/// standalone entry point for a handler's resolved *signature* only —
/// `params`/`ret`/`effectful`, never the body. Deliberately does not build
/// a real [`IrHandler`]/[`crate::ir::IrItem::Service`]: both
/// [`lower_handler_ir`]/[`lower_service_handler_ir`] unconditionally lower
/// the handler's own body into a real `IrExpr` (`IrHandler::body` is not
/// `Option`), and an ordinary `from http` handler's body routinely uses `?`
/// propagation (`ExprKind::Question`, still `todo!()` — its own three-way
/// desugar fork over `Option[T]?`/`Result[T,E]?`/an `embeds` conversion is
/// its own separate design question, #1225) or an `is`-expression
/// (`ExprKind::Is`, also still `todo!()`) — either still panics `lower_expr_ir`
/// today. (The sibling `Ok`/`Err`/`Some`/`None` gap this comment used to cite
/// alongside them is resolved as of #1225's own ADR — `Ok`/`Err`/`Some`/
/// `None` construction alone no longer blocks a real `IrHandler`; `Question`/
/// `Is` still do.) Building a real `IrHandler` at `emit_service`'s own call
/// site would panic on exactly the ordinary Http services this slice needs
/// to keep working. Mirrors
/// [`body_writes_state`]'s own precedent (#1196): a narrow, standalone
/// reader of already-resolved data, not the full `IrItem`/`IrHandler`
/// assembly — the same posture, applied to signature data instead of a
/// single boolean.
/// Deliberately **not** `lower_handler_signature_ir(h, &cx)` (review of
/// #1198) — that helper's own ADR 0334 `.unwrap_or_else(|| panic!(..))` on a
/// resolution miss is correct for an *agent* handler (the checker
/// guarantees resolution there) but not for a *service* one:
/// `resolver.rs` skips `CommonsItem::Service` in every type-ref-resolution
/// pass, `check_handler_body` silently skips a param whose type doesn't
/// resolve and silently returns on an unresolvable return type (no
/// diagnostic either way), and `check_http_handler` only constrains a
/// param's *name* (path segment or `body`), never validates a `body:`
/// param's own declared type. A service handler naming an undeclared type
/// certifies today (and previously just emitted that bad name verbatim, a
/// `tsc`-only failure) — reusing the strict helper here would turn that
/// pre-existing, real-but-harmless-to-the-compiler gap into an ICE on the
/// production emit path for every service in every project (confirmed live:
/// `on POST("/x") (body: Nope) -> Effect[HttpResult[String]] by v: Visitor
/// { ... }` panics `bynkc` before this fix). Mirrors [`lower_protocol_ir`]'s
/// own `Ty::Unit`-on-miss posture, for the identical underlying reason (that
/// function's own doc comment already documents the checker's Service-wide
/// resolution gap).
///
/// `effectful` is computed from `h.return_type`'s own AST shape
/// (`TypeRef::Effect(..)`, matching [`crate::emitter::is_effectful_return`]
/// exactly), not from the *resolved* `ret`'s `Ty::Effect(_)` shape — a
/// resolution miss on `ret` (falling back to `Ty::Unit` below) must not
/// silently flip an `Effect[Nope]`-returning handler to non-effectful; the
/// top-level `Effect[...]` wrapper is always syntactically determinable
/// regardless of whether its own inner type resolves.
pub(crate) fn lower_service_handler_signature_ir(
    h: &Handler,
    program: &CheckedProgram,
) -> HandlerSignatureIr {
    let cx = LowerIrCtx::new(program, HashSet::new());
    let params: Vec<(String, TyId)> = h
        .params
        .iter()
        .map(|p| {
            let ty = cx
                .resolve_type_ref(&p.type_ref)
                .unwrap_or_else(|| cx.unit_ty());
            (p.name.name.clone(), ty)
        })
        .collect();
    let given: Vec<String> = h.given.iter().map(|c| c.key().to_string()).collect();
    let ret = cx
        .resolve_type_ref(&h.return_type)
        .unwrap_or_else(|| cx.unit_ty());
    let effectful = crate::emitter::is_effectful_return(&h.return_type);
    (params, given, ret, effectful)
}

/// P6.11 ([DECISION E], #1171): lower a service handler's own body — the
/// sibling of [`lower_handler_body_ir`] the reasoning in
/// [`lower_service_handler_ir`]'s own doc comment argues for, not a
/// widening of it. Seeds scope from `connection` (P6.13, #1179) if this
/// handler carries one, then `h.params`, then `binder` if one was resolved
/// — matching `check_handler_body`'s own `param_scope` construction order
/// for a service (`checker.rs`: `params_for_check` — synthetic `connection`
/// prepended first when present, `open_connection_param`,
/// `context_checks.rs:1944-1954` — then `actor_binding`, then
/// `agent_self_scope` only when present). Binder last has no observable
/// effect today only because `handler_actor_binding` already suppresses a
/// param-shadowing binder before this ever runs (`context_checks.rs:2050-
/// 2055`) — recorded here so the ordering isn't silently "fixed" by a later
/// reader who doesn't know it's already load-bearing-adjacent. No rigid
/// type variables: `ServiceDecl` carries no `type_params` of its own, the
/// same fact [`resolve_store_field_ty`]'s own doc comment already grounds
/// for [`lower_store_field_ir`].
fn lower_service_handler_body_ir(
    h: &Handler,
    binder: Option<&ActorBinder>,
    connection: Option<&ConnectionBinder>,
    program: &CheckedProgram,
) -> IrExpr {
    let mut cx = LowerIrCtx::new(program, HashSet::new());
    if let Some(connection) = connection {
        cx.bind("connection".to_string(), connection.ty);
    }
    for p in &h.params {
        let ty = cx.resolve_type_ref(&p.type_ref).unwrap_or_else(|| {
            panic!(
                "bynk internal error (ADR 0334): handler parameter `{}`'s type does not resolve \
                 in this pass's own scope, but the checker already accepted this handler",
                p.name.name
            )
        });
        cx.bind(p.name.name.clone(), ty);
    }
    if let Some(binder) = binder {
        cx.bind(binder.binder.clone(), binder.ty);
    }
    let block = lower_block_ir(&h.body, &mut cx);
    wrap_body_return(block)
}

/// P6.11's real service-handler `IrHandler` constructor ([DECISION E],
/// #1171) — the sibling to [`lower_handler_ir`], not a widening of it.
/// [`lower_fn_body_ir`]'s own doc comment already states the governing
/// rule ("a free fn/method and an agent handler seed genuinely different
/// scopes … that would otherwise have to coexist behind one signature for
/// no shared benefit"); the same reasoning gives the same answer here,
/// more strongly:
/// - The two scopes are disjoint, not overlapping — an agent handler body
///   seeds `self`/`store_cells`; a service handler body seeds `params`/
///   `binder` only. Not one of `store_cells`/`state_ty`/`invariants`/
///   `transitions` means anything for a service.
/// - [`lower_handler_ir`] asserts `h.by_clause.is_none()` — an explicit
///   "agent-only by contract" guard. Widening would delete the one thing
///   that today catches a service handler reaching the wrong entry point.
///
/// `protocol` is a parameter solely to disambiguate `HandlerKind::Message`
/// — the same literal AST variant is a *queue* consumer under
/// `ServiceProtocol::Queue` and a *WebSocket inbound frame* under
/// `ServiceProtocol::WebSocket` (`HandlerKind::Close`'s own doc comment
/// says so), and the checker itself dispatches on exactly this `(kind,
/// protocol)` pair (`context_checks.rs:1938-1945`) — without `protocol`
/// here, the `ConnectionBinder` derivation below would wrongly capture
/// every queue consumer in the language too.
///
/// **`binder` is read back, not threaded in as a parameter** — the
/// deliberate inverse of [`lower_handler_ir`]'s own [DECISION E]
/// (`store_cells`/`state_ty`/`invariants`/`transitions` are parameters
/// *because* no persisted table survives `check_agent_decls`'s own
/// transient scope). Here the opposite premise holds: #1170 persisted
/// exactly this pair into `TypedCommons::actor_bindings`, so the opposite
/// choice follows. `None` is a legitimate outcome, not an error — a
/// binder-less `by <Actor>`, no `by` clause at all, or a binder that
/// shadowed a param and was suppressed (`context_checks.rs:2050-2055`) all
/// resolve to `None` here exactly as they do in `handler_actor_binding`
/// itself; this function does not, and must not, assert `binder.is_some()`
/// from `h.by_clause.is_some()`.
///
/// `commit` passes empty `invariants`/`transitions` slices to
/// [`lower_commit_shape_ir`] — [DECISION F]'s own predicted service call
/// site, real for the first time. `CommitShape::Transactional` is
/// structurally unreachable here, not merely unproduced by convention: a
/// service declares no `store` fields for `body_writes_state` to find a
/// write against. No `is_service` flag is added anywhere — Decision F's
/// whole point is that none is needed.
///
/// **The websocket lifecycle case, closed (P6.13, [DECISION G], #1179).**
/// A `from websocket` service's `on open`/`on message`/`on close` handler
/// receives the synthetic leading `connection: Connection[out]` binding
/// the checker injects into its own `params_for_check` only
/// (`open_connection_param`, `context_checks.rs:2020-2032`, injected at
/// `:1944-1954`) — never into `h.params`, so `params`/`given` above stay
/// derived from `h.params` alone, exactly mirroring the checker's own
/// asymmetry (`handler.params` itself is never mutated either). This
/// function re-derives the same `Connection[out]` type via
/// `cx.resolve_type_ref` over a freshly-built `TypeRef::Connection`, the
/// same construction `open_connection_param` performs, since that function
/// is private to `bynk-check` and there is no persisted `TypedCommons`
/// table to read it back from (contrast `binder`, below, which #1170 did
/// persist). `borrowed` mirrors the checker's own `borrowed_held`
/// distinction (`context_checks.rs:1955-1963`): `false` for `on open` (a
/// fresh owned socket, disposed via transfer to an agent), `true` for `on
/// message`/`on close` (the borrowed firing socket). The resulting
/// [`ConnectionBinder`] is threaded into
/// [`lower_service_handler_body_ir`] so `connection.…` reads resolve via
/// `cx.lookup` like any other bound name, and carried on [`IrHandler`]
/// itself for any consumer that needs the owned/borrowed distinction
/// without re-deriving it from `kind`/`protocol`.
pub(crate) fn lower_service_handler_ir(
    h: &Handler,
    protocol: &ServiceProtocol,
    program: &CheckedProgram,
) -> IrHandler {
    let cx = LowerIrCtx::new(program, HashSet::new());
    let (params, given, ret, effectful) = lower_handler_signature_ir(h, &cx);
    // Read straight off `h.by_clause`, not derived from `binder` below —
    // review of #1180: `binder` alone loses the gate itself for a
    // binder-less `by <Actor>` (`ActorBinder`'s own doc comment already
    // names `None` as legitimate there) and loses the actor's own *name*
    // even in the single-actor happy path (`ActorBinder::ty` carries only
    // the sealed identity type). See `IrHandler::actors`'s own doc comment.
    let actors: Vec<String> = h
        .by_clause
        .as_ref()
        .map(|by| by.actors.iter().map(|a| a.name.clone()).collect())
        .unwrap_or_default();
    let binder = program
        .program()
        .actor_binding(h.span)
        .map(|(name, ty)| ActorBinder {
            binder: name.clone(),
            ty: *ty,
        });
    let connection = match (&h.kind, protocol) {
        (
            HandlerKind::Open | HandlerKind::Message | HandlerKind::Close,
            ServiceProtocol::WebSocket { out_type, .. },
        ) => {
            let conn_ref =
                bynk_syntax::ast::TypeRef::Connection(Box::new(out_type.clone()), h.span);
            let ty = cx.resolve_type_ref(&conn_ref).unwrap_or_else(|| {
                panic!(
                    "bynk internal error (ADR 0334): a `from websocket` lifecycle handler's \
                     synthetic `connection: Connection[out]` type does not resolve in this \
                     pass's own scope, but the checker already accepted this handler"
                )
            });
            Some(ConnectionBinder {
                ty,
                borrowed: matches!(h.kind, HandlerKind::Message | HandlerKind::Close),
            })
        }
        _ => None,
    };
    let emits = block_uses_emit(&h.body, &program.program().callees);
    let commit = lower_commit_shape_ir(&h.body, &[], &[], emits, program);
    let body = lower_service_handler_body_ir(h, binder.as_ref(), connection.as_ref(), program);
    IrHandler {
        kind: h.kind.clone(),
        params,
        given,
        actors,
        binder,
        connection,
        body,
        commit,
        ret,
        effectful,
        method_name: h.method_name.as_ref().map(|i| i.name.clone()),
    }
}

/// P6.6 (#1161): lower a `type` declaration into a real [`IrItem::Type`] —
/// [`IrItem`]'s own doc comment names which of its seven design-sketch
/// variants are real as of this slice (`Type`/`Fn` only, Decision D). Takes
/// a certified `&CheckedProgram`, matching this module's own categorical
/// discipline (this file's own header doc: "every entry point here takes a
/// `&CheckedProgram`"), even though only `TypedCommons::types`/`ty_intern`
/// are read — no per-expression `expr_types` lookup is involved (Q2,
/// `design/tracks/the-ir.md` §3.2), but *which fields are read* isn't the
/// discipline; *which failures are allowed to `panic!`* is. Every panic
/// below asserts "the checker already accepted this declaration" — true
/// only once `certify` has run: a bare `TypedCommons` is not certified by
/// construction (`checker.rs`'s own `CheckedProgram` doc notes the
/// project/batch path holds per-unit `TypedCommons` values *before* that
/// unit's build-wide gate is decided), so accepting one here would make
/// `resolve_type_ref_in` returning `None` a reachable, not just a buggy,
/// outcome.
pub(crate) fn lower_type_item_ir(decl: &Arc<TypeDecl>, program: &CheckedProgram) -> IrItem {
    let program = program.program();
    let type_vars: HashSet<String> = decl
        .type_params
        .iter()
        .map(|tp| tp.name.name.clone())
        .collect();
    let resolve = |r: &bynk_syntax::ast::TypeRef| {
        checker::resolve_type_ref_in(r, &program.types, &type_vars, &program.ty_intern)
    };
    let shape = match &decl.body {
        TypeBody::Record(r) => TypeShape::Record {
            fields: r
                .fields
                .iter()
                .map(|f| {
                    let ty = resolve(&f.type_ref).unwrap_or_else(|| {
                        panic!(
                            "bynk internal error (ADR 0334): field `{}` of type `{}` does not \
                             resolve, but the checker already accepted this declaration",
                            f.name.name, decl.name.name
                        )
                    });
                    (f.name.name.clone(), ty)
                })
                .collect(),
        },
        TypeBody::Sum(s) => TypeShape::Sum {
            variants: s
                .variants
                .iter()
                .map(|v| {
                    let payload = v
                        .payload
                        .iter()
                        .map(|vf| {
                            let ty = resolve(&vf.type_ref).unwrap_or_else(|| {
                                panic!(
                                    "bynk internal error (ADR 0334): field `{}` of variant `{}` \
                                     of type `{}` does not resolve, but the checker already \
                                     accepted this declaration",
                                    vf.name.name, v.name.name, decl.name.name
                                )
                            });
                            (vf.name.name.clone(), ty)
                        })
                        .collect();
                    (v.name.name.clone(), payload)
                })
                .collect(),
            embeds: s
                .embeds
                .iter()
                .map(|e| {
                    let source = resolve(&e.source_type).unwrap_or_else(|| {
                        panic!(
                            "bynk internal error (ADR 0334): `embeds` clause source type on \
                             variant `{}` of type `{}` does not resolve, but the checker \
                             already accepted this declaration",
                            e.variant.name, decl.name.name
                        )
                    });
                    (source, e.variant.name.clone())
                })
                .collect(),
        },
        TypeBody::Refined {
            base, refinement, ..
        } => TypeShape::Refined {
            base: *base,
            refinement: refinement.clone(),
            opaque: false,
        },
        TypeBody::Opaque {
            base, refinement, ..
        } => TypeShape::Refined {
            base: *base,
            refinement: refinement.clone(),
            opaque: true,
        },
    };
    IrItem::Type {
        def: Arc::clone(decl),
        shape,
    }
}

/// P6.6 (#1161): lower a `fn` declaration into a real [`IrItem::Fn`] —
/// wraps [`lower_fn_body_ir`] (#1141, unchanged) rather than re-deriving its
/// own rigid-variable seeding or body lowering; adds only `def`/`receiver`/
/// `params`/`ret`/`effectful` around its existing return value. Covers both
/// free functions and methods alike (`FnName::Free`/`FnName::Method`) —
/// which `IrItem::Fn`s a future printer re-attaches under which
/// `IrItem::Type`'s own namespace (R8.1) is phase 7's own concern, not
/// decided here.
///
/// Takes `f: &Arc<FnDecl>`, not a bare `&FnDecl` — `def` reuses this exact
/// `Arc`, the identity substitution [`IrItem`]'s own doc comment claims
/// (`Record`/`GlobalRef`/[`lower_type_item_ir`]'s own `Arc::clone(decl)`
/// all reuse the program's own handle the same way): a fresh `Arc::new`
/// would both lose pointer identity against `TypedCommons.fns`/
/// `Callee::Fn`/`Callee::Method` (no way back from an `IrItem::Fn` to the
/// call sites that reference the same declaration except by re-comparing
/// names) and deep-clone the whole body AST for every fn in a unit.
pub(crate) fn lower_fn_item_ir(f: &Arc<FnDecl>, program: &CheckedProgram) -> IrItem {
    let type_vars = fn_rigid_type_vars(f, program.program());
    let cx = LowerIrCtx::new(program, type_vars);
    let receiver = fn_receiver_ty(f, program.program());
    let params: Vec<(String, TyId)> = f
        .params
        .iter()
        .map(|p| {
            let ty = cx.resolve_type_ref(&p.type_ref).unwrap_or_else(|| {
                panic!(
                    "bynk internal error (ADR 0334): parameter `{}`'s type does not resolve in \
                     this pass's own rigid-variable scope, but the checker already accepted \
                     this fn — bynk-emit::ir::lower's type_vars disagrees with bynk-check's \
                     Ctx::type_vars",
                    p.name.name
                )
            });
            (p.name.name.clone(), ty)
        })
        .collect();
    let ret = cx.resolve_type_ref(&f.return_type).unwrap_or_else(|| {
        panic!(
            "bynk internal error (ADR 0334): the return type of `{}` does not resolve in this \
             pass's own rigid-variable scope, but the checker already accepted this fn",
            f.name.display()
        )
    });
    let effectful = matches!(&*program.program().ty_intern.get(ret), Ty::Effect(_));
    IrItem::Fn {
        def: Arc::clone(f),
        receiver,
        params,
        ret,
        body: lower_fn_body_ir(f, program),
        effectful,
    }
}

/// A store field's own element/key/value type, resolved with no rigid type
/// variables in scope — `AgentDecl` carries no `type_params` of its own
/// (its own struct shape, `bynk-syntax/src/ast.rs:908-934`), so
/// [`lower_store_field_ir`] never needs the `fn_rigid_type_vars`-shaped
/// seeding every fn/method-level constructor here does. Shared by every
/// arm of that function's own kind dispatch, and by [`lower_store_field_shape_ir`].
///
/// **Not an ADR 0334 `.expect()`-style panic on a resolve miss, deliberately**
/// — the same posture [`lower_op_sig_ir`]'s own doc comment already argues
/// for a capability op's `params`/`return_type`, confirmed empirically for
/// store fields specifically (#1187's Agent state-field slice, step 0):
/// `store x: Cell[Bogus] = "hello"` certifies today (exit 0, no diagnostic),
/// so a `Bogus` store-field type — undeclared, and of the wrong kind for the
/// `= "hello"` initializer besides — reaches this pass with no `expr_types`/
/// `types` entry at all. Nothing in `context_checks.rs`'s store-field
/// checking validates the *type reference* itself (only its shape, e.g.
/// `Cell`/`Map`/`Set`/`Cache`/`Log`, and `@ttl`/`@indexed` legality). Mirror
/// the checker's own silent-fallback posture instead of asserting a
/// guarantee that does not hold.
fn resolve_store_field_ty(cx: &LowerIrCtx, r: &bynk_syntax::ast::TypeRef) -> TyId {
    cx.resolve_type_ref(r).unwrap_or_else(|| cx.unit_ty())
}

/// A `@name(<duration literal>)` annotation's own millisecond value — the
/// shared "find the annotation, read its first argument's `DurationLit`"
/// step both `@ttl` (`Cache`) and `@retain` (`Log`) need, factored out so
/// the two [`lower_store_field_ir`] arms are one call each rather than two
/// near-identical inline `find`/`and_then` chains. Mirrors, but does not
/// call, `cache_ttl_millis` (`bynk-check/src/context_checks.rs`) and the
/// shipped emitter's own equivalent extraction (`emit.rs`'s
/// `store_cache_fields`/`store_log_fields`) — those thread a
/// `&mut Vec<CompileError>` for a missing-`@ttl` diagnostic and are private
/// to their own module, so reusing them directly here is not architecturally
/// available; see [`lower_store_field_ir`]'s own doc comment for why this is
/// an accepted, named duplication rather than a gap this slice closes.
fn duration_millis_annotation(
    annotations: &[bynk_syntax::ast::Annotation],
    name: &str,
) -> Option<i64> {
    annotations
        .iter()
        .find(|a| a.name.name == name)
        .and_then(|a| match a.args.first().map(|arg| &arg.value.kind) {
            Some(ExprKind::DurationLit { millis, .. }) => Some(*millis),
            _ => None,
        })
}

/// P6.7 (#1163): lower an agent `store` field declaration into a real
/// [`StoreFieldIr`] — dispatches on `f.kind.head.name` into
/// [`StoreKindIr`]'s five real variants; `Queue` cannot reach a certified
/// program (`bynk.store.kind_unsupported` gates it before `certify`, R3.10),
/// so this match is total over what one can actually contain, not a gap
/// needing its own extension later.
///
/// `@ttl`/`@retain` millis and `@indexed(by: …)` field names are read
/// directly off `f.annotations`, independently of `cache_ttl_millis`/
/// `store_log_fields`/`store_map_indexes` (`bynk-check`/the shipped
/// emitter) — the *pattern* is the same one those already established
/// ([DECISION B]/[DECISION C], #1163), not a call into shared code: this
/// track's own P6.0 precedent (`design/tracks/the-ir.md` §3.4, Q4) chose to
/// extend the checker's typed output once (`Callee`) rather than let a
/// lowering pass re-derive from the AST the way the shipped emitter's own
/// dispatcher does, and this constructor falls short of that bar — a real,
/// accepted duplication (three independent copies of the same
/// `DurationLit`-extraction shape now exist: `context_checks.rs`,
/// `emitter/emit.rs`, and here), not one this slice is positioned to close,
/// since none of the three is `pub`/shaped for a shared caller today. Within
/// this function the `@ttl`/`@retain` cases at least share one call each to
/// [`duration_millis_annotation`], rather than repeating the extraction
/// inline a second time. `indexed` keeps the annotation's own declaration
/// order, deduplicated — no sort, unlike the shipped emitter's own
/// doubly-sorted `HashMap` intermediate ([DECISION E]'s own structural fix,
/// `ir.rs`'s own `StoreFieldIr::indexed` doc comment), but still guarding
/// against a duplicate `by:` key the checker admits (`validate_indexed_keys`
/// validates each `by:` argument independently, so `@indexed(by: k, by: k)`
/// certifies), mirroring the shipped emitter's own `store_map_indexes`
/// dedup guard.
///
/// `init` is constructed only for a `Cell` field ([DECISION D]) — no
/// checker pass types a non-`Cell` field's `init` expression (a real,
/// pre-existing gap this proposal's own grounding pass found:
/// `context_checks.rs`'s init-checking loop skips anything that isn't
/// `Cell`, and no other pass fills that gap either), so on a certified
/// program such an expression, if ever written, has no `expr_types` entry —
/// lowering it here would hit this pass's own ADR 0334 panic on a value the
/// checker never verified, not a recoverable state.
///
/// Deliberately not unified with `checker::StoreField` (`bynk-check/src/checker.rs`):
/// that dispatch is ephemeral, per-agent checking-time scratch, rebuilt
/// fresh inside `check_agent_decls` and discarded once that agent's
/// handler/invariant checking finishes; this constructor produces
/// persistent IR data with no consumer yet. Collapsing them would mean
/// either the checker producing IR-shaped data (crossing the phase-5/
/// phase-6 boundary `the-ir.md` §3.4 already drew deliberately) or this
/// pass consuming the checker's own discarded scratch state across a value
/// that no longer exists once `check_agent_decls` returns — a real, named
/// duplication (#1163's own Risks), not a gap this slice closes.
pub(crate) fn lower_store_field_ir(f: &StoreField, program: &CheckedProgram) -> StoreFieldIr {
    let mut cx = LowerIrCtx::new(program, HashSet::new());
    let (kind, indexed) = store_field_kind_and_indexed(f, &cx);
    // [DECISION D]: only a `Cell` field's `init` is ever lowered.
    let init = match &kind {
        StoreKindIr::Cell(_) => f.init.as_ref().map(|e| lower_expr_ir(e, &mut cx)),
        _ => None,
    };
    StoreFieldIr {
        field: f.name.name.clone(),
        kind,
        init,
        indexed,
    }
}

/// The shape half of [`lower_store_field_ir`] — its `kind`/`indexed`
/// computation, factored out so [`lower_store_field_shape_ir`] can share it
/// without either duplicating the `Cell`/`Map`/`Set`/`Cache`/`Log` dispatch
/// or paying for a `&mut LowerIrCtx` it never needs (nothing here lowers an
/// expression).
fn store_field_kind_and_indexed(f: &StoreField, cx: &LowerIrCtx) -> (StoreKindIr, Vec<IndexIr>) {
    let head = f.kind.head.name.as_str();
    let kind = match head {
        "Cell" => StoreKindIr::Cell(resolve_store_field_ty(cx, &f.kind.args[0])),
        "Map" => StoreKindIr::Map(
            resolve_store_field_ty(cx, &f.kind.args[0]),
            resolve_store_field_ty(cx, &f.kind.args[1]),
        ),
        "Set" => StoreKindIr::Set(resolve_store_field_ty(cx, &f.kind.args[0])),
        "Cache" => {
            let k = resolve_store_field_ty(cx, &f.kind.args[0]);
            let v = resolve_store_field_ty(cx, &f.kind.args[1]);
            let ttl = duration_millis_annotation(&f.annotations, "ttl").unwrap_or_else(|| {
                panic!(
                    "bynk internal error (ADR 0334): `Cache` field `{}` has no resolvable \
                     `@ttl` millis, but the checker already accepted this declaration — \
                     bynk.store.cache_ttl_required gates a missing or malformed `@ttl` \
                     before certify",
                    f.name.name
                )
            });
            StoreKindIr::Cache(k, v, ttl)
        }
        "Log" => {
            let elem = resolve_store_field_ty(cx, &f.kind.args[0]);
            let retain = duration_millis_annotation(&f.annotations, "retain");
            StoreKindIr::Log(elem, retain)
        }
        other => panic!(
            "bynk internal error (ADR 0334): store field `{}` has storage kind `{other}`, which \
             cannot reach a certified program — only Cell/Map/Set/Cache/Log are functional \
             (Queue is gated by bynk.store.kind_unsupported before certify)",
            f.name.name
        ),
    };
    // [DECISION C]/[DECISION E]: one entry per distinct `by:` argument,
    // declaration order, no sort — legal only on `Map` (`ANNOTATIONS`'s own
    // registry), so this is empty by construction for every other kind.
    // Deduplicated: `validate_indexed_keys` (`context_checks.rs`) validates
    // each `by:` argument independently with no duplicate check, so
    // `@indexed(by: k, by: k)` certifies — mirrors the shipped emitter's own
    // `store_map_indexes` guard (`emit.rs`'s `!fields.contains(&k.name)`),
    // grounded during P6.7's own review (#1163): dropping this would mean a
    // duplicate `by:` produces the same sibling index table twice.
    let mut indexed: Vec<IndexIr> = Vec::new();
    for arg in f
        .annotations
        .iter()
        .filter(|a| a.name.name == "indexed")
        .flat_map(|a| &a.args)
    {
        if let (Some(l), ExprKind::Ident(k)) = (&arg.label, &arg.value.kind)
            && l.name == "by"
            && !indexed.contains(&k.name)
        {
            indexed.push(k.name.clone());
        }
    }
    (kind, indexed)
}

/// #1187's Agent state-field slice: [`lower_store_field_ir`]'s shape-only
/// sibling — same `kind`/`indexed` (via [`store_field_kind_and_indexed`]),
/// `init` always `None`. This is the entry point `emit_agent`'s own state
/// section actually needs: a field's storage *shape* (its `Cell`/`Map`/
/// `Set`/`Cache`/`Log` kind and `@indexed` keys), never its `Cell` zero/
/// initial value expression. Deliberately never lowers `init`, unlike
/// [`lower_store_field_ir`] — a `Cell` field's initializer can be an
/// `is`-expression (`= x is SomeVariant`), which still hits `ExprKind::Is`'s
/// own `todo!()` a few hundred lines below (`1029_agent_static_init_hoist`'s
/// `store active: Cell[Bool] = if true { 5 is PositiveInt } else { false }`
/// hits exactly this on `lower_store_field_ir`'s own `init` arm). The
/// sibling `= None` shape (`223_store_cell_agent`'s own `store paymentRef:
/// Cell[Option[AuthId]] = None`) no longer needs this workaround as of
/// #1225's own ADR — `lower_store_field_ir` lowers it directly now
/// (`store_field_cell_option_init_none_lowers_without_panicking`). This
/// function's callers never need `init` at all regardless, so neither gap
/// is a risk for them.
pub(crate) fn lower_store_field_shape_ir(f: &StoreField, program: &CheckedProgram) -> StoreFieldIr {
    let cx = LowerIrCtx::new(program, HashSet::new());
    let (kind, indexed) = store_field_kind_and_indexed(f, &cx);
    StoreFieldIr {
        field: f.name.name.clone(),
        kind,
        init: None,
        indexed,
    }
}

/// P6.8 ([DECISION A]/[DECISION E], #1165): lower an agent invariant into a
/// real [`IrPredicate`] — seeds the predicate's own scope from `store_cells`
/// exactly as `checker::check_invariants` does (`bynk-check/src/checker.rs`:
/// each `store` `Cell` field in scope by bare name, reading as its element
/// type), then lowers `predicate` through the ordinary [`lower_expr_ir`]
/// machinery unchanged. Takes `store_cells` as a parameter rather than
/// re-deriving it from `program` ([DECISION E]) — a certified
/// `CheckedProgram` carries no persisted "this agent's store cells" table;
/// that scope is `check_agent_decls`'s own transient scratch, never
/// persisted to `TypedCommons`. Called once per agent's own invariant list,
/// by whichever future slice builds `IrItem::Agent` for real — not once per
/// handler, mirroring `check_invariants`'s own once-per-agent posture.
pub(crate) fn lower_invariant_ir(
    inv: &Invariant,
    store_cells: &HashMap<String, TyId>,
    program: &CheckedProgram,
) -> IrPredicate {
    let mut cx = LowerIrCtx::new(program, HashSet::new());
    for (name, ty) in store_cells {
        cx.bind(name.clone(), *ty);
    }
    IrPredicate {
        name: inv.name.name.clone(),
        predicate: lower_expr_ir(&inv.predicate, &mut cx),
    }
}

/// P6.8 ([DECISION A]/[DECISION E], #1165): lower a step invariant
/// (`Transition`) into a real [`IrPredicate`] — seeds `old`/`new`, both
/// bound to the agent's own synthetic state-record type, exactly as
/// `checker::check_transitions` does. `state_ty` is a parameter, not
/// re-derived, for the same reason [`lower_invariant_ir`]'s own
/// `store_cells` is: no persisted "this agent's state type" table survives
/// past `check_agent_decls`'s own transient scope. Called once per agent's
/// own transition list, by the same future caller [`lower_invariant_ir`]
/// names.
pub(crate) fn lower_transition_ir(
    tr: &Transition,
    state_ty: TyId,
    program: &CheckedProgram,
) -> IrPredicate {
    let mut cx = LowerIrCtx::new(program, HashSet::new());
    cx.bind("old".to_string(), state_ty);
    cx.bind("new".to_string(), state_ty);
    IrPredicate {
        name: tr.name.name.clone(),
        predicate: lower_expr_ir(&tr.predicate, &mut cx),
    }
}

/// [DECISION B]/[DECISION C] (#1165): does `body` reach a mutating
/// `Callee::Store` write, or an unconditional `Statement::Assign` (`:=`),
/// anywhere — including inside a nested `if`/`match`/lambda? Drives
/// [`lower_commit_shape_ir`]'s own `Transactional` decision, and, as of
/// #1196 (the #1187 emitter-cutover track's own R6.5 stake), `emit_agent`'s
/// (`bynk-emit/src/emitter/emit.rs`) own real implicit-commit-wrapper
/// decision too — its previous own name-matching `block_writes_state`
/// (`emitter.rs`) is deleted, this function is its sole, direct
/// replacement. The walk's own shape is that deleted function's own
/// already-correct skeleton reused structurally, not re-derived:
/// `Block`/`If`/`Match` are hand-matched so crossing a nested block
/// re-enters the statement-aware case (an `expr_children` descent alone
/// flattens a block straight to its statements' *values*, losing the
/// `Statement::Assign` tag), everywhere else recurses over
/// `expr_children`'s total child iterator.
///
/// Unlike the deleted function's own name-based `mutating_op`, this walk
/// needs no per-kind receiver-name set: a `Callee::Store { op, .. }` already
/// carries the field's own resolved identity (the checker only ever records
/// one for a method the field's own kind actually declares), so `op`'s
/// membership in the shared mutating-verb constants ([DECISION C],
/// `emitter.rs`) is unambiguous checked flat, across all four kinds' lists
/// at once — a locally-shadowed name that would false-positive
/// `mutating_op` cannot false-positive here at all, the exact fix Decision
/// B's own Risk names, and the exact defect `#1196_agent_write_detection_
/// via_resolved_callee`'s own fixture pins at the emitted-output level.
pub(crate) fn body_writes_state(body: &Block, program: &TypedCommons) -> bool {
    fn is_mutating_store_write(e: &Expr, program: &TypedCommons) -> bool {
        match program.callees.get(&e.id) {
            Some(Callee::Store { op, .. }) => {
                MUTATING_MAP_CACHE_OPS.contains(&op.as_str())
                    || MUTATING_SET_OPS.contains(&op.as_str())
                    || MUTATING_LOG_OPS.contains(&op.as_str())
                    || MUTATING_CELL_OPS.contains(&op.as_str())
            }
            _ => false,
        }
    }
    fn stmt(s: &Statement, program: &TypedCommons) -> bool {
        match s {
            Statement::Assign(_) => true,
            Statement::Let(l) | Statement::EffectLet(l) => expr(&l.value, program),
            Statement::Expect(a) => expr(&a.value, program),
            Statement::Send(s) => expr(&s.value, program),
            Statement::Do(d) => expr(&d.value, program),
        }
    }
    fn expr(e: &Expr, program: &TypedCommons) -> bool {
        if is_mutating_store_write(e, program) {
            return true;
        }
        match &e.kind {
            ExprKind::Block(b) => body_writes_state(b, program),
            ExprKind::If {
                cond,
                then_block,
                else_block,
            } => {
                expr(cond, program)
                    || body_writes_state(then_block, program)
                    || body_writes_state(else_block, program)
            }
            ExprKind::Match { discriminant, arms } => {
                expr(discriminant, program)
                    || arms.iter().any(|a| match &a.body {
                        MatchBody::Expr(e) => expr(e, program),
                        MatchBody::Block(b) => body_writes_state(b, program),
                    })
            }
            _ => expr_children(e).into_iter().any(|c| expr(c, program)),
        }
    }
    body.statements.iter().any(|s| stmt(s, program)) || expr(&body.tail, program)
}

/// P6.8 ([DECISION B]/[DECISION D]/[DECISION F], #1165): decide a handler
/// body's own one-of-three [`CommitShape`] from resolved data —
/// [`body_writes_state`]'s own write-detection walk, plus `emits` ([DECISION
/// D]: the caller's own `crate::emitter::block_uses_emit(body)` call, not
/// re-derived here — no `Callee` classification exists for `Events.emit` to
/// consume instead, see that decision's own grounding). Does not lower
/// `body` into an `IrExpr` tree first ([DECISION B]) — deciding the shape
/// only needs the two booleans, and lowering the whole body just to throw
/// the result away would be pure waste; a real `IrHandler.body` lowering is
/// a separate, not-yet-commissioned step (see [`crate::ir::IrItem`]'s own
/// doc comment).
///
/// Shape-agnostic between an agent and a service handler ([DECISION F]) — no
/// `is_store_agent` flag: a service handler's own call site simply passes
/// empty `invariants`/`transitions` slices (a service has no `store` block
/// to declare them against), and [`body_writes_state`] naturally finds
/// neither a mutating `Callee::Store` nor a bare `:=` in a service body (no
/// store fields to write), so `Transactional` is never constructed for one —
/// matching the shipped emitter's own `emit_service`, which already only
/// ever produces the other two shapes.
pub(crate) fn lower_commit_shape_ir(
    body: &Block,
    invariants: &[IrPredicate],
    transitions: &[IrPredicate],
    emits: bool,
    program: &CheckedProgram,
) -> CommitShape {
    if body_writes_state(body, program.program()) {
        CommitShape::Transactional {
            invariants: invariants.to_vec(),
            transitions: transitions.to_vec(),
        }
    } else if emits {
        CommitShape::FlushEvents
    } else {
        CommitShape::ReadOnly
    }
}

/// P6.11 ([DECISION C], #1171): reshape a `from Events(E { … })` pattern
/// into a real [`EventPatternIr`] — pure structural reshaping, no
/// `&CheckedProgram` parameter and no resolution: every field's own
/// matched value is either already a literal or an unresolved variant tag,
/// neither needing a `TyId`. `EventPatternValue::Literal` lowers through
/// the same `LiteralValue -> ConstVal` match [`lower_pattern_ir`] already
/// uses for `Pattern::Literal`'s identical closed set.
fn lower_event_pattern_ir(pattern: &EventPattern) -> EventPatternIr {
    EventPatternIr {
        fields: pattern
            .fields
            .iter()
            .map(|f| {
                let value = match &f.value {
                    EventPatternValue::Literal { value, .. } => {
                        EventPatternValueIr::Const(match value {
                            LiteralValue::Int(n) => ConstVal::Int(*n),
                            LiteralValue::Str(s) => ConstVal::Str(s.clone()),
                            LiteralValue::Bool(b) => ConstVal::Bool(*b),
                        })
                    }
                    EventPatternValue::Variant { variant, .. } => EventPatternValueIr::Variant {
                        tag: variant.name.clone(),
                    },
                };
                (f.name.name.clone(), value)
            })
            .collect(),
    }
}

/// P6.11 ([DECISION A], #1171): lower a service's own `from <protocol>`
/// header into a real [`ProtocolIr`] — standalone, takes the sub-node
/// rather than the owning `ServiceDecl` (mirrors [`lower_store_field_ir`]),
/// so a `from websocket`/`from Events` fixture can pin the descriptor by
/// itself even where [`lower_service_handler_ir`] cannot yet lower every
/// handler on the same service (the WebSocket lifecycle-body deferral —
/// see that function's own doc comment).
///
/// `WebSocket`/`Events`'s own type refs resolve through the `Ty::Unit`
/// fallback, not an ADR 0334 panic — deliberately, and for the same reason
/// [`lower_agent_item_ir`]'s own `key_ty` does: `resolver.rs` skips
/// `CommonsItem::Service` in every one of its own type-ref-resolution
/// passes (`resolver.rs:303-304`/`493-494`/`577-578`), and the one checker
/// site that does resolve a WebSocket frame type itself falls back to
/// `Ty::Unit` on a miss rather than erroring (`context_checks.rs:775-778`).
/// Panicking here would make this the second ADR-0334 site in this module
/// asserting a guarantee the checker doesn't actually give — the first
/// being `lower_agent_item_ir`'s own `key_ty`, review of #1169.
pub(crate) fn lower_protocol_ir(
    protocol: &ServiceProtocol,
    program: &CheckedProgram,
) -> ProtocolIr {
    let cx = LowerIrCtx::new(program, HashSet::new());
    match protocol {
        ServiceProtocol::Call => ProtocolIr::Call,
        ServiceProtocol::Http => ProtocolIr::Http,
        ServiceProtocol::Cron => ProtocolIr::Cron,
        ServiceProtocol::Queue { name } => ProtocolIr::Queue { name: name.clone() },
        ServiceProtocol::WebSocket { in_type, out_type } => ProtocolIr::WebSocket {
            in_ty: cx.resolve_type_ref(in_type).unwrap_or_else(|| cx.unit_ty()),
            out_ty: cx
                .resolve_type_ref(out_type)
                .unwrap_or_else(|| cx.unit_ty()),
        },
        ServiceProtocol::Events {
            event_type,
            pattern,
            schema_dispatch,
        } => ProtocolIr::Events {
            event: cx
                .resolve_type_ref(event_type)
                .unwrap_or_else(|| cx.unit_ty()),
            pattern: pattern.as_ref().map(lower_event_pattern_ir),
            schema_dispatch: schema_dispatch.as_ref().map(|d| d.pattern.clone()),
        },
    }
}

/// P6.11 ([DECISION D], #1171): interpret a service's own `cors`/
/// `security`/`limits` blocks into a real [`PolicyIr`] — private and takes
/// no `&CheckedProgram`, deliberately: this function resolves no type,
/// reads no `expr_types`, and cannot panic, so threading a `&CheckedProgram`
/// through it just to satisfy this module's own "every entry point takes a
/// certified program" header rule would be cargo-culting a rule about
/// *which failures may panic*, not about which fields are read (the same
/// distinction [`lower_type_item_ir`]'s own doc comment already draws).
/// Precedent for a private, non-`&CheckedProgram` helper in this module:
/// [`body_writes_state`], [`duration_millis_annotation`].
///
/// `None` whenever `service.protocol` is not `ServiceProtocol::Http` — the
/// checker itself gates all three blocks to HTTP only
/// (`bynk.http.cors_not_http`/`security_not_http`/`limits_not_http`,
/// `context_checks.rs`), so a `cors { }` block on, say, a `from cron`
/// service parses but never certifies as meaningful; not just when the
/// source declares none of the three blocks.
///
/// Every field here is exactly one already-shipped typed accessor's return
/// value (`CorsPolicy::origins()`/`credentials()`/`allow_headers()`/
/// `max_age_secs()`, `SecurityPolicy::nosniff()`/`hsts_max_age_secs()`,
/// `LimitsPolicy::max_body()`) — see [`crate::ir::PolicyIr`]'s own doc
/// comment for why interpreting through these, not passing the raw AST
/// struct through, is the point of this constructor. The `security: None`
/// arm materialises `SecurityIr { nosniff: true, hsts_max_age_secs: None }`
/// — the shipped emitter's own already-established default
/// (`emitter/workers_entry.rs`), reproduced verbatim, not invented here.
fn lower_policy_ir(service: &ServiceDecl) -> Option<PolicyIr> {
    if !matches!(service.protocol, ServiceProtocol::Http) {
        return None;
    }
    Some(PolicyIr {
        cors: service.cors.as_ref().map(|p| CorsIr {
            origins: p.origins(),
            credentials: p.credentials(),
            allow_headers: p.allow_headers(),
            max_age_secs: p.max_age_secs(),
        }),
        security: match &service.security {
            Some(p) => SecurityIr {
                nosniff: p.nosniff(),
                hsts_max_age_secs: p.hsts_max_age_secs(),
            },
            None => SecurityIr {
                nosniff: true,
                hsts_max_age_secs: None,
            },
        },
        max_body_bytes: service.limits.as_ref().and_then(|p| p.max_body()),
    })
}

/// #1228: a GET handler's own `@cache(maxAge:, scope:)` freshness policy —
/// [`crate::ir::CacheIr`]'s own doc comment has the full grounding for why
/// this is a standalone reader rather than a `PolicyIr` field. Field-for-
/// field the same extraction `emitter/workers_entry.rs`'s own (now
/// superseded) `cache_policy_for` did: only a `GET` yields a policy;
/// project validation (`bynk.http.cache_*`) has already rejected a
/// `@cache` anywhere else, and a malformed `maxAge` there, so a missing or
/// ill-formed annotation here simply yields `None` — no `&CheckedProgram`
/// needed, the same posture [`lower_policy_ir`]'s own doc comment already
/// argues for: `maxAge`/`scope` are already-resolved syntactic literals
/// (`ExprKind::DurationLit`/`Ident`), not a type this pass would ever need
/// to resolve.
pub(crate) fn lower_route_cache_ir(h: &Handler) -> Option<CacheIr> {
    if !matches!(
        h.kind,
        HandlerKind::Http {
            method: HttpMethod::Get,
            ..
        }
    ) {
        return None;
    }
    let ann = h.annotations.iter().find(|a| a.name.name == "cache")?;
    let mut max_age_millis: Option<i64> = None;
    let mut scope = "private";
    for arg in &ann.args {
        match arg.label.as_ref().map(|l| l.name.as_str()) {
            Some("maxAge") => {
                if let ExprKind::DurationLit { millis, .. } = &arg.value.kind {
                    max_age_millis = Some(*millis);
                }
            }
            Some("scope") => {
                if let ExprKind::Ident(id) = &arg.value.kind
                    && id.name == "public"
                {
                    scope = "public";
                }
            }
            _ => {}
        }
    }
    Some(CacheIr {
        max_age_secs: max_age_millis? / 1000,
        scope,
    })
}

/// #1228: a route's own `@limit(maxBody:)` annotation, if present — the
/// override half of `emitter/workers_entry.rs`'s own (now superseded)
/// `effective_max_body`; the service-wide `limits { maxBody }` fallback
/// stays that function's own concern (already IR-native via
/// `PolicyIr::max_body_bytes`, but read from a *service*, not a per-route
/// `Handler`, so it does not move here). Project validation
/// (`bynk.http.limit_*`/`limits_*`) has already rejected a malformed or
/// misplaced `@limit`, so an absent/ill-formed annotation here simply
/// yields `None` — the caller's own service-default fallback still
/// applies. No `&CheckedProgram` needed, same reasoning as
/// [`lower_route_cache_ir`]: `maxBody` is an already-resolved
/// `ExprKind::IntLit`, not a type.
pub(crate) fn lower_route_limit_ir(h: &Handler) -> Option<i64> {
    let ann = h.annotations.iter().find(|a| a.name.name == "limit")?;
    for arg in &ann.args {
        if arg.label.as_ref().map(|l| l.name.as_str()) == Some("maxBody")
            && let ExprKind::IntLit { value: n, .. } = &arg.value.kind
            && *n > 0
        {
            return Some(*n);
        }
    }
    None
}

/// P6.10 (#1169): assemble an agent declaration into a real
/// [`crate::ir::IrItem::Agent`] — wires every prior slice's own standalone
/// constructor ([`lower_store_field_ir`] since P6.7, [`lower_invariant_ir`]/
/// [`lower_transition_ir`]/[`lower_commit_shape_ir`] since P6.8,
/// [`lower_handler_ir`] since P6.9) rather than re-deriving any of their
/// logic — the first `IrItem` variant assembled from other already-real IR
/// data rather than lowered fresh from the AST by itself.
///
/// Computes `state`/`store_cells`/`state_ty` once and reuses them for every
/// downstream call that needs them — the "future `IrItem::Agent` builder"
/// every one of those constructors' own doc comments already named as the
/// job that would do this. `store_cells` is derived from `state`'s own
/// already-lowered [`StoreKindIr::Cell`] entries, not re-resolved
/// independently a second time from the AST — the one place this function
/// could have re-derived something it already has, and doesn't.
/// `invariants`/`transitions` are lowered once, then passed to *every*
/// handler's own [`lower_handler_ir`] call, not just the store-writing
/// ones: [`lower_commit_shape_ir`] only ever reads them for a
/// `Transactional` shape, so a read-only or event-flushing handler simply
/// carries the slices it was handed without using them, the same "pass
/// what a callee needs, let it decide whether to use it" posture
/// [`lower_commit_shape_ir`]'s own `emits` parameter already established.
pub(crate) fn lower_agent_item_ir(agent: &AgentDecl, program: &CheckedProgram) -> IrItem {
    let cx = LowerIrCtx::new(program, HashSet::new());
    // Not an ADR 0334 `.expect()`-style panic, deliberately: unlike a
    // handler param/return type, `agent.key_type` is never actually
    // resolution-checked by the checker at all — `resolver.rs` skips
    // `CommonsItem::Agent` in every one of its own `check_type_ref_resolves*`
    // passes, and `context_checks.rs:2854` itself falls back to `Ty::Unit`
    // on a miss rather than erroring (`agent Ledger { key id: Bogus … }`
    // certifies today, silently). Panicking here on a state the checker
    // itself accepts would make this the first ADR 0334 site in this module
    // to assert a guarantee that does not actually hold — mirror the
    // checker's own fallback instead (review of #1169).
    let key_ty = cx
        .resolve_type_ref(&agent.key_type)
        .unwrap_or_else(|| cx.unit_ty());
    let state: Vec<StoreFieldIr> = agent
        .store_fields
        .iter()
        .map(|f| lower_store_field_ir(f, program))
        .collect();
    let store_cells: HashMap<String, TyId> = state
        .iter()
        .filter_map(|f| match f.kind {
            StoreKindIr::Cell(ty) => Some((f.field.clone(), ty)),
            _ => None,
        })
        .collect();
    let state_ty = program.program().ty_intern.intern(Ty::Named {
        name: format!("{}State", agent.name.name),
        kind: checker::NamedKind::Record,
        args: Vec::new(),
    });
    let invariants: Vec<IrPredicate> = agent
        .invariants
        .iter()
        .map(|inv| lower_invariant_ir(inv, &store_cells, program))
        .collect();
    let transitions: Vec<IrPredicate> = agent
        .transitions
        .iter()
        .map(|tr| lower_transition_ir(tr, state_ty, program))
        .collect();
    let handlers: Vec<IrHandler> = agent
        .handlers
        .iter()
        .map(|h| {
            lower_handler_ir(
                h,
                &store_cells,
                state_ty,
                &invariants,
                &transitions,
                program,
            )
        })
        .collect();
    IrItem::Agent {
        def: agent.name.name.clone(),
        key: (agent.key_name.name.clone(), key_ty),
        state,
        handlers,
        invariants,
        transitions,
    }
}

/// P6.11 (#1171): assemble a service declaration into a real
/// [`crate::ir::IrItem::Service`] — structural mirror of
/// [`lower_agent_item_ir`], wiring [`lower_protocol_ir`],
/// [`lower_service_handler_ir`] and [`lower_policy_ir`] rather than
/// re-deriving any of their logic. Unlike the agent case, there is no
/// shared per-declaration context to compute once: a service has no
/// `store_cells`/`state_ty`/invariants/transitions (none of those concepts
/// exist for a service at all), so `&service.protocol`, already on the
/// declaration, is threaded to each handler call directly — the "compute
/// once, reuse" step [`lower_agent_item_ir`]'s own doc comment describes
/// degenerates to a borrow here, not because this function skips a step,
/// but because a service's own handlers have nothing else to share.
///
/// **No default-`by`/default-`given` inheritance logic, deliberately.**
/// `project_model::inject_service_defaults` (`bynk-check/src/project_model.rs`)
/// already mutated `handler.by_clause`/`handler.given` in place at pipeline
/// phase 2b (`bynk-check/src/analysis.rs`), overriding — not merging —
/// before `check_service_decls`, let alone this pass, ever runs. `h.by_clause`/
/// `h.given` are already final by the time [`lower_service_handler_ir`] sees
/// them; this is stated so a future reader who sees `ServiceDecl::default_by`/
/// `default_given` unread by this function knows that is correct, not an
/// omission.
pub(crate) fn lower_service_item_ir(service: &ServiceDecl, program: &CheckedProgram) -> IrItem {
    IrItem::Service {
        def: service.name.name.clone(),
        protocol: lower_protocol_ir(&service.protocol, program),
        handlers: service
            .handlers
            .iter()
            .map(|h| lower_service_handler_ir(h, &service.protocol, program))
            .collect(),
        policy: lower_policy_ir(service),
    }
}

/// P6.12 (#1173): assemble a capability declaration into a real
/// [`crate::ir::IrItem::Capability`] — structural mirror of
/// [`lower_service_item_ir`]/[`lower_agent_item_ir`], but with nothing to
/// compute once and share: `CapabilityDecl` carries no state/invariants/
/// transitions/protocol of its own, only `ops`, so each op lowers
/// independently through [`lower_op_sig_ir`].
pub(crate) fn lower_capability_item_ir(cap: &CapabilityDecl, program: &CheckedProgram) -> IrItem {
    IrItem::Capability {
        def: cap.name.name.clone(),
        ops: cap
            .ops
            .iter()
            .map(|op| lower_op_sig_ir(op, program))
            .collect(),
    }
}

/// P6.12 (#1173): lower one capability operation's own signature into a real
/// [`crate::ir::OpSig`] — the reference's own `IrItem::Capability` sketch
/// names this type (`ops: Vec<OpSig>`) but never defines it. Resolves
/// `params`/`return_ty` in the scope `op.type_params` names, the same
/// per-op rigid-variable seeding `context_checks::build_capability_op_info`
/// (`bynk-check/src/context_checks.rs`) already gives a generic op for the
/// checker-facing `CapabilityOpInfo` — an op's own `[T, …]` list is scoped to
/// the op itself, not the capability (`CapabilityDecl` carries no
/// `type_params` of its own), so this seeds a fresh [`LowerIrCtx`] per op
/// rather than reusing [`fn_rigid_type_vars`]'s fn/method-shaped
/// receiver-widening, which does not apply here.
///
/// **Not an ADR 0334 `.expect()`-style panic on a resolve miss,
/// deliberately** — the same posture [`lower_agent_item_ir`]'s own `key_ty`
/// doc comment already argues for `agent.key_type`, and for the identical
/// reason: a capability op's own `params`/`return_type` are never actually
/// resolution-checked by the checker at all. The resolver skips
/// `CommonsItem::Capability` outright (`resolver.rs:301/491/575`, "v0.5
/// items are resolved via a separate context-level pass"); the context-level
/// pass that replaces it, `check_capability_decls`, only calls
/// `checker::record_type_refs`, which silently does nothing on a name absent
/// from `types` rather than erroring (`checker.rs:2593-2597`); and
/// `build_capability_op_info` itself, the checker-facing constructor this
/// pass mirrors, degrades to `Ty::Unit` on the same miss rather than
/// treating it as impossible (`context_checks.rs:36,40`). `capability Store
/// { fn get(k: Bogus) -> Effect[Int] }` certifies today, silently. Panicking
/// here on a state the checker itself accepts would make this the first ADR
/// 0334 site in this module to assert a guarantee that does not actually
/// hold — mirror the checker's own fallback instead (review of #1182).
fn lower_op_sig_ir(op: &CapabilityOp, program: &CheckedProgram) -> OpSig {
    lower_op_sig_ir_from_commons(op, program.program())
}

/// #1187's own closing scoping pass: a `TypedCommons`-only sibling of
/// [`lower_op_sig_ir`], for the one real call site that never has a
/// `&CheckedProgram` — `emitter/lower.rs`'s `cap_op_param_names`, feeding
/// `trace(Cap.op)`/`with`-predicate observation lowering
/// (`bynk.test`'s DSL). That call path's own `TypedCommons` is a synthetic,
/// hand-assembled project-wide view (`project/tests_emit.rs`'s
/// `synthetic_typed_commons_for_target`, merging every consumed unit's own
/// `capability` declarations into one scratch commons for lookup) — never
/// itself the output of `certify`, so wrapping it as a `CheckedProgram`
/// here would misrepresent an uncertified value as certified
/// (`CheckedProgram`'s own doc comment, `bynk-check/src/checker.rs`, warns
/// against exactly this). Splitting this out is sound precisely because
/// this function never calls `LowerIrCtx::expr_ty` — the one method whose
/// `.expect()`-panic needs a genuinely certified program, the reason this
/// module's own file-level doc comment gives for taking `&CheckedProgram`
/// everywhere else. `resolve_type_ref`/`unit_ty()` (below) both degrade via
/// `.unwrap_or_else` and read nothing `TypedCommons` doesn't already expose
/// directly.
pub(crate) fn lower_op_sig_ir_from_commons(op: &CapabilityOp, commons: &TypedCommons) -> OpSig {
    let type_vars: HashSet<String> = op
        .type_params
        .iter()
        .map(|tp| tp.name.name.clone())
        .collect();
    let cx = LowerIrCtx::from_commons(commons, type_vars);
    let params: Vec<(String, TyId)> = op
        .params
        .iter()
        .map(|p| {
            let ty = cx
                .resolve_type_ref(&p.type_ref)
                .unwrap_or_else(|| cx.unit_ty());
            (p.name.name.clone(), ty)
        })
        .collect();
    let return_ty = cx
        .resolve_type_ref(&op.return_type)
        .unwrap_or_else(|| cx.unit_ty());
    OpSig {
        name: op.name.name.clone(),
        type_params: op
            .type_params
            .iter()
            .map(|tp| tp.name.name.clone())
            .collect(),
        params,
        return_ty,
    }
}

/// P6.14 (#1174): assemble a provider declaration into a real
/// [`crate::ir::IrItem::Provider`] — reads `ProviderDecl::external`
/// straight into [`ProviderBody`]'s own `Bynk`/`External` dispatch
/// ([`crate::ir::IrItem`]'s own doc comment has the full grounding for why
/// this, unlike `Actor`, was buildable this slice). `external: true` means
/// `ops` is empty by the field's own doc comment
/// (`bynk-syntax/src/ast.rs:592-595`) — nothing to lower. `given` lowers
/// unconditionally via [`lower_provider_given_ir`] (#1187's Provider
/// `given`/deps-wiring slice, fixing a real gap the `External` variant's own
/// bare-unit shape used to leave: `provider.given` is populated the same way
/// regardless of `external`, and [`ProviderBody::Bynk`]'s own doc comment
/// names why it, unlike `module`, is not deferrable — that reasoning always
/// applied to `External` too, just wasn't wired through).
pub(crate) fn lower_provider_item_ir(provider: &ProviderDecl, program: &CheckedProgram) -> IrItem {
    let given = lower_provider_given_ir(provider);
    let body = if provider.external {
        ProviderBody::External { given }
    } else {
        ProviderBody::Bynk {
            given,
            ops: provider
                .ops
                .iter()
                .map(|op| lower_provider_op_ir(op, program))
                .collect(),
        }
    };
    IrItem::Provider {
        def: provider.provider_name.name.clone(),
        cap: provider.capability.name.clone(),
        body,
    }
}

/// A provider's own `given` clause, resolved independent of
/// [`ProviderBody`]'s `external`/`Bynk` dispatch and independent of
/// [`lower_provider_item_ir`]'s own full assembly (#1187's Provider
/// `given`/deps-wiring slice) — the standalone entry point
/// `bynk-emit/src/project.rs`'s `instantiate_provider_expr` actually calls.
/// Building a real `IrItem::Provider` there would be unsafe for a `Bynk`
/// provider specifically: `ProviderBody::Bynk::ops` unconditionally lowers
/// every op's body through `lower_provider_op_ir` → `lower_expr_ir`, which
/// still can't handle every expression shape a real op body can contain —
/// `?` propagation (`ExprKind::Question`) and an `is`-expression
/// (`ExprKind::Is`) both remain `todo!()`. (#1225's own ADR closed the
/// sibling `Ok`/`Err`/`Some`/`None` construction gap this comment used to
/// cite here — not itself sufficient to make a real op body safe to build,
/// since `Question`/`Is` are unaffected, the identical remaining gap that
/// keeps `Agent`/`Service`'s own full `IrItem` construction impractical at
/// their emitter call sites, #1196/#1198's own findings.) This function
/// never touches `ops`/bodies, so it carries none of that risk;
/// [`lower_provider_item_ir`] itself now calls it too, rather than
/// hand-duplicating the one-line `map`.
pub(crate) fn lower_provider_given_ir(provider: &ProviderDecl) -> Vec<CapRefIr> {
    provider.given.iter().map(lower_cap_ref_ir).collect()
}

/// #1187's slice 6 plumbing (sibling of [`lower_provider_given_ir`]): a
/// handler's own `given` clause, resolved independent of any full
/// `IrHandler`/`IrItem` assembly — the standalone entry point for
/// `project.rs`'s `plan_agent_given_deps`, `EmitProjectCtx::
/// agent_method_givens`, and `emitter/workers.rs`'s own `given` collection.
/// Reuses [`lower_cap_ref_ir`] verbatim; a handler's `given` is syntactically
/// identical to a provider's (`bynk_syntax::ast::CapRef`), so this is the
/// same one-line adapter, not a new design.
pub(crate) fn lower_handler_given_ir(h: &Handler) -> Vec<CapRefIr> {
    h.given.iter().map(lower_cap_ref_ir).collect()
}

/// #1187's slice 3: a handler's resolved actor-verification seam — the same
/// "narrow, standalone reader of already-resolved data" precedent
/// [`body_writes_state`]/[`lower_service_handler_signature_ir`] established,
/// applied to `bynk-check`'s own five actor-seam resolvers
/// (`bynk-check/src/actors.rs`) instead of a full `IrHandler` assembly.
/// [`ActorSeamIr`]'s own doc comment has the full grounding for the
/// priority order and for the deliberately-missing `Signature` variant.
///
/// Replaces the hand-duplicated "try N resolvers, branch on which
/// returned `Some`" call sites this slice converts: `emit_service`
/// (`emitter/emit.rs`) and `emit_worker_compose`'s HTTP-dispatch match
/// (`emitter/workers.rs`). Deliberately does **not** yet replace every
/// caller of the five resolvers — `secrets.rs`'s `declared_secrets` unions
/// *all* matching seams' secrets rather than picking one (a different
/// shape this enum doesn't model), and the remaining call sites
/// (`emitter/workers_entry.rs`, `emitter/workers.rs`'s other two sites,
/// `emitter/emit.rs`'s `any_service_binds_caller`/`emit_make_surface`/
/// `ws_open_hosts_for`, `project/tests_emit.rs`) each call exactly one
/// resolver with nothing to collapse against — converting them to build a
/// five-variant enum just to immediately match out one arm would add
/// indirection without removing any real duplication.
pub(crate) fn lower_actor_seam_ir(
    handler: &Handler,
    actors: &HashMap<String, ActorDecl>,
) -> ActorSeamIr {
    if let Some(members) = bynk_check::actors::sum_members_for(handler, actors) {
        return ActorSeamIr::Sum(members);
    }
    if let Some(seam) = bynk_check::actors::bearer_seam_for(handler, actors) {
        return ActorSeamIr::Bearer(seam);
    }
    if let Some(seam) = bynk_check::actors::oidc_seam_for(handler, actors) {
        return ActorSeamIr::Oidc(seam);
    }
    if let Some(binder) = bynk_check::actors::caller_binder_for(handler, actors) {
        return ActorSeamIr::Caller(binder);
    }
    ActorSeamIr::None
}

/// P6.14 (#1174, review of #1186): adapt one `given` entry into a real
/// [`crate::ir::CapRefIr`] — [`CapRefIr`]'s own doc comment has the full
/// grounding for the `QualifiedName -> String` flattening and for why a
/// `Some` prefix is preserved unresolved.
fn lower_cap_ref_ir(cap_ref: &CapRef) -> CapRefIr {
    CapRefIr {
        context: cap_ref.context.as_ref().map(QualifiedName::joined),
        name: cap_ref.name.name.clone(),
    }
}

/// P6.14 (#1174): lower one provider operation's own signature *and* body
/// into a real [`crate::ir::ProviderOpIr`] — a new sibling of
/// [`lower_fn_body_ir`], not a widening of it, seeding a scope with just
/// this op's own `params` (no `self`, no store cells: `check_provider_decls`
/// checks every op via `checker::check_handler_body` with
/// `HandlerBodyCheck::new`'s own "everything optional empty" default,
/// `bynk-check/src/checker.rs:1055` — a provider op's `given` capabilities
/// need no scope entry of their own, resolved the same already-generic
/// `Callee`-wrapping [`lower_handler_body_ir`]'s own doc comment credits for
/// handler bodies). No rigid type variables: `ProviderOp` carries no
/// `type_params` of its own ([`crate::ir::ProviderOpIr`]'s own doc comment
/// has the full contrast with [`OpSig::type_params`]).
///
/// **ADR 0334 panic-on-miss, not [`lower_op_sig_ir`]'s lenient `Ty::Unit`
/// fallback** — unlike a bare `CapabilityOp` signature, a `ProviderOp`'s
/// `params`/`return_type` are resolved for real by `check_handler_body`
/// (the same machinery a fn body or handler body goes through), so a
/// resolve miss here is exactly the "checker accepted this, this pass
/// disagrees" internal-error case [`lower_fn_item_ir`]'s own panics guard
/// against, not a state the checker is known to accept silently.
fn lower_provider_op_ir(op: &ProviderOp, program: &CheckedProgram) -> ProviderOpIr {
    let mut cx = LowerIrCtx::new(program, HashSet::new());
    let params: Vec<(String, TyId)> = op
        .params
        .iter()
        .map(|p| {
            let ty = cx.resolve_type_ref(&p.type_ref).unwrap_or_else(|| {
                panic!(
                    "bynk internal error (ADR 0334): parameter `{}`'s type does not resolve in \
                     this pass's own scope, but the checker already accepted this provider op's \
                     body via check_handler_body — bynk-emit::ir::lower's resolution disagrees \
                     with bynk-check's",
                    p.name.name
                )
            });
            cx.bind(p.name.name.clone(), ty);
            (p.name.name.clone(), ty)
        })
        .collect();
    let return_ty = cx.resolve_type_ref(&op.return_type).unwrap_or_else(|| {
        panic!(
            "bynk internal error (ADR 0334): the return type of provider op `{}` does not \
             resolve in this pass's own scope, but the checker already accepted this provider \
             op's body via check_handler_body",
            op.name.name
        )
    });
    let body = wrap_body_return(lower_block_ir(&op.body, &mut cx));
    ProviderOpIr {
        name: op.name.name.clone(),
        params,
        return_ty,
        body,
    }
}

/// Lower a block as a value — no `Return` wrapping (see
/// [`lower_fn_body_ir`]'s doc comment for the distinction). A block's own
/// type is always its tail's type.
pub(crate) fn lower_block_ir(block: &Block, cx: &mut LowerIrCtx) -> IrExpr {
    cx.push_scope();
    let stmts: Vec<IrStmt> = block
        .statements
        .iter()
        .map(|s| lower_stmt_ir(s, cx))
        .collect();
    let tail = lower_expr_ir(&block.tail, cx);
    cx.pop_scope();
    let ty = tail.ty;
    IrExpr {
        kind: IrExprKind::Block {
            stmts,
            tail: Box::new(tail),
        },
        ty,
        span: block.span,
    }
}

fn lower_stmt_ir(s: &Statement, cx: &mut LowerIrCtx) -> IrStmt {
    match s {
        Statement::Let(l) => {
            let value = lower_expr_ir(&l.value, cx);
            // The checker binds the *annotation's* type when one is present
            // (`type_of_block`'s `Statement::Let` arm, `checker.rs:2829-2853`)
            // — `compatible()` admits a refined value under its base's
            // annotation, so the bound type can genuinely differ from the
            // RHS expression's own recorded type. `value.ty` stays the RHS's
            // own honest type (R6.1, this expression's real checked type);
            // only this pass's own scope bookkeeping (read back by a later
            // shorthand-field lookup) uses the annotation-preferred one, the
            // same distinction the checker itself draws between an
            // expression's type and a binding's type.
            let bound_ty = l.type_annot.as_ref().map_or(value.ty, |a| {
                cx.resolve_type_ref(a).unwrap_or_else(|| {
                    panic!(
                        "bynk internal error (ADR 0334): `let` annotation for `{}` does not \
                         resolve in this pass's own rigid-variable scope, but the checker \
                         already accepted this binding",
                        l.name.name
                    )
                })
            });
            // Mirrors `checker.rs:2854`: `_` is never bound.
            if l.name.name != "_" {
                cx.bind(l.name.name.clone(), bound_ty);
            }
            IrStmt::Let {
                local: l.name.name.clone(),
                value,
            }
        }
        Statement::EffectLet(l) => {
            let effect = lower_expr_ir(&l.value, cx);
            let span = effect.span;
            let ty = cx.peel_effect(effect.ty);
            // Same annotation-vs-RHS distinction as `Statement::Let` above,
            // peeled through `Effect[_]` (`checker.rs:2894-2953`'s own
            // `EffectLet` arm).
            let bound_ty = l.type_annot.as_ref().map_or(ty, |a| {
                cx.resolve_type_ref(a).unwrap_or_else(|| {
                    panic!(
                        "bynk internal error (ADR 0334): `let <-` annotation for `{}` does not \
                         resolve in this pass's own rigid-variable scope, but the checker \
                         already accepted this binding",
                        l.name.name
                    )
                })
            });
            if l.name.name != "_" {
                cx.bind(l.name.name.clone(), bound_ty);
            }
            IrStmt::Let {
                local: l.name.name.clone(),
                value: IrExpr {
                    kind: IrExprKind::Await {
                        effect: Box::new(effect),
                    },
                    ty,
                    span,
                },
            }
        }
        Statement::Send(send) => {
            let effect = lower_expr_ir(&send.value, cx);
            let span = effect.span;
            IrStmt::Expr {
                value: IrExpr {
                    kind: IrExprKind::Send {
                        effect: Box::new(effect),
                    },
                    ty: cx.unit_ty(),
                    span,
                },
            }
        }
        Statement::Do(d) => {
            let effect = lower_expr_ir(&d.value, cx);
            let span = effect.span;
            let ty = cx.peel_effect(effect.ty);
            IrStmt::Expr {
                value: IrExpr {
                    kind: IrExprKind::Await {
                        effect: Box::new(effect),
                    },
                    ty,
                    span,
                },
            }
        }
        // Not named by any P6.x rule (design/tracks/the-ir.md §6) — test-only,
        // no IR target proposed anywhere in this track.
        Statement::Expect(_) => todo!(
            "Statement::Expect has no IrStmt target — not named by any rule this track commissions"
        ),
        // `cell := expr` — the unconditional `Cell` write ([DECISION B],
        // P6.9, #1167). `checker.rs`'s own `Statement::Assign` arm resolves
        // `a.target.name` directly against `ctx.store_fields` by bare name
        // and never keys a `Callee` at all (only `a.value`, an ordinary
        // sub-expression, ever gets one) — no `ExprId`-keyed sink was ever
        // actually necessary, on a premise two prior slices (P6.7/P6.8) held
        // without re-checking it. `field` is this module's usual "no arena"
        // substitution; `value` lowers through the ordinary
        // [`lower_expr_ir`] path unchanged.
        Statement::Assign(a) => IrStmt::Assign {
            field: a.target.name.clone(),
            value: lower_expr_ir(&a.value, cx),
        },
    }
}

pub(crate) fn lower_expr_ir(e: &Expr, cx: &mut LowerIrCtx) -> IrExpr {
    let ty = cx.expr_ty(e.id);
    let span = e.span;
    match &e.kind {
        ExprKind::IntLit { value, .. } => IrExpr {
            kind: IrExprKind::Const(ConstVal::Int(*value)),
            ty,
            span,
        },
        ExprKind::FloatLit { value, .. } => IrExpr {
            kind: IrExprKind::Const(ConstVal::Float(*value)),
            ty,
            span,
        },
        ExprKind::DurationLit { millis, .. } => IrExpr {
            kind: IrExprKind::Const(ConstVal::DurationMillis(*millis)),
            ty,
            span,
        },
        ExprKind::StrLit(s) => IrExpr {
            kind: IrExprKind::Const(ConstVal::Str(s.clone())),
            ty,
            span,
        },
        ExprKind::BoolLit(b) => IrExpr {
            kind: IrExprKind::Const(ConstVal::Bool(*b)),
            ty,
            span,
        },
        ExprKind::UnitLit => IrExpr {
            kind: IrExprKind::Const(ConstVal::Unit),
            ty,
            span,
        },

        ExprKind::Ident(id) => IrExpr {
            kind: lower_ident_ir(&id.name, cx),
            ty,
            span,
        },

        ExprKind::RecordConstruction { fields, .. } => IrExpr {
            kind: IrExprKind::Record {
                // The checker already validated `type_name` and rejected a
                // mismatched/unknown one — `ty` above (a `Ty::Named` for this
                // exact construction) is this node's own resolved type, so
                // the declaring `TypeDecl` comes from there, not by
                // re-resolving `type_name` a second time.
                def: named_decl(ty, cx),
                fields: fields
                    .iter()
                    .map(|f| {
                        let value = match &f.value {
                            Some(v) => lower_expr_ir(v, cx),
                            // Shorthand `{ x }` — no `Expr`/`ExprId` of its
                            // own (the parser never builds one), so its type
                            // comes from this pass's own scope, exactly the
                            // local binding the checker's `ctx.lookup` read
                            // at `bynk-check/src/checker/expressions.rs:2427`.
                            None => {
                                let local_ty = cx.lookup(&f.name.name).unwrap_or_else(|| {
                                    panic!(
                                        "bynk internal error (ADR 0334): shorthand field `{}` has \
                                         no local binding in this pass's own scope — the checker \
                                         accepted it, so its own `ctx.lookup` must have found one",
                                        f.name.name
                                    )
                                });
                                IrExpr {
                                    kind: lower_ident_ir(&f.name.name, cx),
                                    ty: local_ty,
                                    span: f.name.span,
                                }
                            }
                        };
                        (f.name.name.clone(), value)
                    })
                    .collect(),
            },
            ty,
            span,
        },
        ExprKind::FieldAccess { receiver, field } => IrExpr {
            kind: IrExprKind::Field {
                base: Box::new(lower_expr_ir(receiver, cx)),
                field: field.name.clone(),
            },
            ty,
            span,
        },
        ExprKind::ListLit(elems) => IrExpr {
            kind: IrExprKind::List {
                elems: elems.iter().map(|el| lower_expr_ir(el, cx)).collect(),
            },
            ty,
            span,
        },
        ExprKind::Block(b) => lower_block_ir(b, cx),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => IrExpr {
            kind: IrExprKind::If {
                cond: Box::new(lower_expr_ir(cond, cx)),
                then_: Box::new(lower_block_ir(then_block, cx)),
                else_: Box::new(lower_block_ir(else_block, cx)),
            },
            ty,
            span,
        },
        ExprKind::BinOp(BinOp::And, lhs, rhs) => IrExpr {
            kind: IrExprKind::And {
                lhs: Box::new(lower_expr_ir(lhs, cx)),
                rhs: Box::new(lower_expr_ir(rhs, cx)),
            },
            ty,
            span,
        },
        ExprKind::BinOp(BinOp::Or, lhs, rhs) => IrExpr {
            kind: IrExprKind::Or {
                lhs: Box::new(lower_expr_ir(lhs, cx)),
                rhs: Box::new(lower_expr_ir(rhs, cx)),
            },
            ty,
            span,
        },
        ExprKind::UnaryOp(UnaryOp::Not, inner) => IrExpr {
            kind: IrExprKind::Not {
                operand: Box::new(lower_expr_ir(inner, cx)),
            },
            ty,
            span,
        },
        ExprKind::EffectPure(inner) => IrExpr {
            kind: IrExprKind::Pure {
                value: Box::new(lower_expr_ir(inner, cx)),
            },
            ty,
            span,
        },
        // Parens carry no semantic weight once parsed — not one of the
        // reference's own node kinds, unwrapped here so lowering a merely-
        // parenthesized covered-node subexpression doesn't panic.
        ExprKind::Paren(inner) => lower_expr_ir(inner, cx),

        // `p implies q` -> `Or { lhs: Not(p), rhs: q }` (P6.3, design/tracks/
        // the-ir.md §6, R6.7 — the reference's own Part 6.4 table). Peeled off
        // the comparison/arithmetic `BinOp` arm below (real as of #1189) —
        // `Implies` desugars away entirely rather than sharing that arm's
        // `IrBinOp` shape.
        //
        // This flat `Or`/`Not` pair has nowhere to carry an `is` binding from
        // the antecedent into the consequent (`p is Foo(x) implies f(x)`,
        // the reason the string emitter's own `Implies` handling exists,
        // `emitter/lower.rs:4332`'s `lower_and_with_is`) — unreachable today
        // since `ExprKind::Is` is still a `todo!()` three arms below, but
        // this arm needs revisiting, not inheriting by default, once P6.4
        // lands `Is`.
        ExprKind::BinOp(BinOp::Implies, lhs, rhs) => IrExpr {
            kind: IrExprKind::Or {
                // `Not`'s own type is Bool — the same type `Implies` itself
                // (and therefore `lhs`) already carries, so no new type is
                // synthesised here, only reused.
                lhs: Box::new(IrExpr {
                    kind: IrExprKind::Not {
                        operand: Box::new(lower_expr_ir(lhs, cx)),
                    },
                    ty,
                    span: lhs.span,
                }),
                rhs: Box::new(lower_expr_ir(rhs, cx)),
            },
            ty,
            span,
        },

        // Comparison/arithmetic (#1189, ir.rs's own Decision-D-extension
        // note): a shared `IrExprKind::BinOp { op, .. }` node, not ten
        // near-duplicate variants — see `IrExprKind::BinOp`'s own doc
        // comment for why that shape (not one variant per operator, unlike
        // `And`/`Or`/`Not` above) is the right call here. `lhs`/`rhs` lower
        // independently; no `is`-binding propagation applies (that's
        // `Implies`/`And`'s own concern, unaffected by this arm).
        ExprKind::BinOp(
            op @ (BinOp::Eq
            | BinOp::NotEq
            | BinOp::Lt
            | BinOp::LtEq
            | BinOp::Gt
            | BinOp::GtEq
            | BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div),
            lhs,
            rhs,
        ) => {
            let ir_op = match op {
                BinOp::Eq => IrBinOp::Eq,
                BinOp::NotEq => IrBinOp::NotEq,
                BinOp::Lt => IrBinOp::Lt,
                BinOp::LtEq => IrBinOp::LtEq,
                BinOp::Gt => IrBinOp::Gt,
                BinOp::GtEq => IrBinOp::GtEq,
                BinOp::Add => IrBinOp::Add,
                BinOp::Sub => IrBinOp::Sub,
                BinOp::Mul => IrBinOp::Mul,
                BinOp::Div => IrBinOp::Div,
                BinOp::And | BinOp::Or | BinOp::Implies => {
                    unreachable!("And/Or/Implies are matched by their own arms above")
                }
            };
            IrExpr {
                kind: IrExprKind::BinOp {
                    op: ir_op,
                    lhs: Box::new(lower_expr_ir(lhs, cx)),
                    rhs: Box::new(lower_expr_ir(rhs, cx)),
                },
                ty,
                span,
            }
        }
        // `-operand` (#1189) — `Not`'s own arithmetic counterpart, same
        // shape.
        ExprKind::UnaryOp(UnaryOp::Neg, inner) => IrExpr {
            kind: IrExprKind::Neg {
                operand: Box::new(lower_expr_ir(inner, cx)),
            },
            ty,
            span,
        },
        // Interpolated strings (#1189) — each hole lowers through the
        // ordinary `lower_expr_ir` machinery; chunks carry over verbatim.
        ExprKind::InterpStr(parts) => IrExpr {
            kind: IrExprKind::InterpStr {
                parts: parts.iter().map(|p| lower_interp_part_ir(p, cx)).collect(),
            },
            ty,
            span,
        },
        ExprKind::Call {
            type_args, args, ..
        } => lower_call_ir(e, None, type_args, args, cx),
        ExprKind::Lambda(lambda) => lower_lambda_ir(e, lambda, cx),
        // #1225's own ADR: the four built-in constructors lower to the same
        // `IrExprKind::Variant` a user-declared sum's own `Callee::Ctor`
        // does (`lower_call_ir`, above) — `IrExprKind::Variant`'s own doc
        // comment has the full grounding for why no `Callee` classification
        // is needed here either: `ty` (already resolved for every `IrExpr`,
        // R6.1) already carries the constructed sum's own identity, the
        // same way it does for a real `Callee::Ctor` call.
        ExprKind::Ok(inner) => IrExpr {
            kind: IrExprKind::Variant {
                tag: "Ok".to_string(),
                payload: vec![lower_expr_ir(inner, cx)],
            },
            ty,
            span,
        },
        ExprKind::Err(inner) => IrExpr {
            kind: IrExprKind::Variant {
                tag: "Err".to_string(),
                payload: vec![lower_expr_ir(inner, cx)],
            },
            ty,
            span,
        },
        ExprKind::Some(inner) => IrExpr {
            kind: IrExprKind::Variant {
                tag: "Some".to_string(),
                payload: vec![lower_expr_ir(inner, cx)],
            },
            ty,
            span,
        },
        ExprKind::None => IrExpr {
            kind: IrExprKind::Variant {
                tag: "None".to_string(),
                payload: Vec::new(),
            },
            ty,
            span,
        },
        ExprKind::Question(_) => todo!(
            "`?` propagation's real shipped desugar (emitter/lower.rs:1012-1042, ADR 0177/ADR \
             0178) branches on the operand's own checked type into three distinct shapes — an \
             `Option[T]?` early-returns HttpResult.NotFound (not any Err construction at all), \
             a `Result[T,E]?` either returns the scrutinee value unchanged or, under a declared \
             `embeds` conversion, constructs a new Err(EmbedType.Variant(..)) — not the single \
             generic Match{{Ok(v) => v, Err(e) => Return(Err(convert(e)))}} shape this comment \
             used to claim; modelling three divergent per-operand-type shapes as one \
             Match is its own design fork, not this row's own commission (P6.5, #1159, \
             Decision C — Match itself is real as of this slice, this gap is Question's own, \
             not a shared one)"
        ),
        ExprKind::ConstructorCall { args, .. } => lower_call_ir(e, None, &[], args, cx),
        ExprKind::MethodCall {
            receiver,
            type_args,
            args,
            ..
        } => lower_call_ir(e, Some(receiver), type_args, args, cx),
        ExprKind::Match { discriminant, arms } => {
            let scrutinee = Box::new(lower_expr_ir(discriminant, cx));
            // Deliberately the lowered `IrExpr`'s own `ty`, not
            // `cx.expr_ty(discriminant.id)` (the AST-keyed type P6.4's own
            // standalone fixtures read instead, `lower_match_fixture` below)
            // — the two agree everywhere reachable today, but `scrutinee.ty`
            // stays correct even if a future discriminant ever lowers
            // through a type-adjusting arm (a `peel_effect` site), where the
            // recorded AST type and the lowered node's own type can diverge.
            let scrutinee_ty = scrutinee.ty;
            let ir_arms: Vec<IrArm> = arms
                .iter()
                .map(|a| lower_arm_ir(a, scrutinee_ty, cx))
                .collect();
            let exhaustive = lower_exhaustive_ir(arms);
            let form = if match_needs_if_chain(arms) {
                MatchForm::IfChain
            } else {
                MatchForm::Flat
            };
            // Not recorded here: the string emitter also decides, at this
            // same site, whether the discriminant needs hoisting to a temp
            // before an if-chain re-evaluates it per arm
            // (`is_narrowable_path`, `emitter/lower.rs:5068` — re-evaluating
            // a non-ident/field discriminant could repeat side effects).
            // Left deliberately derivable from `scrutinee`'s own `Local`/
            // `Field` shape rather than mirrored onto `IrExprKind::Match` as
            // a second flag — the same "can never silently disagree"
            // argument Decision B makes for `form` applies here too, just
            // resolved by omission instead of reuse.
            IrExpr {
                kind: IrExprKind::Match {
                    scrutinee,
                    arms: ir_arms,
                    exhaustive,
                    form,
                },
                ty,
                span,
            }
        }
        ExprKind::Is { .. } => todo!(
            "`is`'s real shipped desugar (`lower_is`, emitter/lower.rs:5615) already diverges \
             from a plain two-arm boolean Match for a refined-type check: it skips \
             pattern-match machinery entirely (refined_check_as_bool plus a forced receiver \
             temp, is_receiver_ref_forced) — R5.9 (narrowing-scope recording) and R5.10 (the \
             forced-receiver-temp as an explicit IR Let), both already explicitly deferred by \
             #1157's own Decision D, unaffected by Match becoming real this slice (P6.5, #1159, \
             Decision C)"
        ),
        ExprKind::RecordSpread {
            base, overrides, ..
        } => lower_record_spread_ir(ty, span, base, overrides, cx),
        ExprKind::Expect(_) => todo!(
            "`expect` expression — test-body-only, gated by ctx.in_test_body \
             (bynk-check/src/checker.rs's check_body), unreachable through this pass's own \
             single-file checked_program/find_fn/lower_fn harness without first building or \
             routing through bynk-check's heavier project-level test machinery (P6.3, #1145, \
             Decision C)"
        ),
        ExprKind::Val { .. } => todo!(
            "`Val[T]` — test-body-only, same unreachable-through-this-harness gap as `expect` \
             above (P6.3, #1145, Decision C)"
        ),
        ExprKind::Wire(_) => todo!(
            "`Wire(...)` — test-body-only, same unreachable-through-this-harness gap as `expect` \
             above (P6.3, #1145, Decision C)"
        ),
        ExprKind::Observation(_) => todo!(
            "capability-call observation — test-body-only, same unreachable-through-this-harness \
             gap as `expect` above (P6.3, #1145, Decision C)"
        ),
        ExprKind::Trace { .. } => todo!(
            "`trace(...)` — test-body-only, same unreachable-through-this-harness gap as \
             `expect` above (P6.3, #1145, Decision C)"
        ),
    }
}

/// [`IrExprKind::InterpStr`]'s own per-part lowering (#1189) — a literal
/// chunk carries over verbatim; a hole lowers through the ordinary
/// [`lower_expr_ir`] machinery, same as any other subexpression.
fn lower_interp_part_ir(part: &InterpPart, cx: &mut LowerIrCtx) -> IrInterpPart {
    match part {
        InterpPart::Chunk(s) => IrInterpPart::Chunk(s.clone()),
        InterpPart::Hole(e) => IrInterpPart::Hole(Box::new(lower_expr_ir(e, cx))),
    }
}

/// Shared by `Call`/`MethodCall`/qualified-`ConstructorCall`: read back the
/// `Callee` P6.0 already recorded for `e.id` and build the matching
/// `IrExprKind` — never re-derive which dispatch branch fired. `receiver`
/// is `Some` only for `MethodCall`; `Call`/`ConstructorCall` have none, so
/// `type_args`/`args` alone determine the call's own arguments.
///
/// A `Callee::Ctor` becomes [`IrExprKind::Variant`], not `Call` — sum-variant
/// construction is a distinct node in Part 6.2's own shape, even though
/// `check_call`/`check_static_call` dispatch it identically to a real call.
/// Every other `Callee` becomes [`IrExprKind::Call`]. A method call's own
/// `receiver` is lowered and prepended to `args` *only* when the `Callee`
/// says the receiver is a genuine value the call operates on — `Method`
/// (`self`), `Kernel` (the collection/etc. value), `Agent` (the instance) —
/// the same way UFCS already treats `self` as an ordinary leading argument
/// (`check_method_call`'s own doc comment: "UFCS-style call resolution").
/// Every other `Callee` reached via `MethodCall` (`Static`, `Capability`,
/// `CrossCap`, `Cross`, `TestService`, `Store`, `Query`) has a receiver that
/// is a pure namespace/field reference already fully captured by the
/// `Callee`'s own fields (a type name, a capability name, a store field's
/// own name) — lowering and prepending it would be redundant at best (its
/// identity is already recorded) and wrong at worst (a bare type name like
/// `Point` in `Point.origin()` is not a `Local`, not a `Global`-shaped
/// nullary variant, and not a free function — lowering it as an ordinary
/// expression would hit `lower_ident_ir`'s own fallback `todo!()` for no
/// good reason). This is a lowering-time convention with no consumer yet to
/// validate it against (Decision A, #1143) — worth a second look once a
/// real `Ir → TS` printer is proposed.
fn lower_call_ir(
    e: &Expr,
    receiver: Option<&Expr>,
    type_args: &[bynk_syntax::ast::TypeRef],
    args: &[Expr],
    cx: &mut LowerIrCtx,
) -> IrExpr {
    let ty = cx.expr_ty(e.id);
    let span = e.span;
    let Some(callee) = cx.callee(e.id).cloned() else {
        todo!(
            "no Callee recorded for this call at {span:?} — one of the shapes Decision C (#1143) \
             left out on purpose (HttpResult/QueueResult bare-variant construction, Events.emit, \
             the production is_system_http_service address), or a genuine, newly-discovered gap"
        )
    };
    if let Callee::Ctor { tag, .. } = callee {
        // `IrExprKind::Variant` has no `targs` slot, unlike `Call` below —
        // deliberate, not dropped: `type_args` can never be non-empty here.
        // `check_call`'s own gate rejects any explicit type argument before
        // it even considers whether `name` is a variant constructor
        // ("`{name}` is not a generic function — it takes no type
        // arguments", `calls.rs:484-495`), and `ConstructorCall` (the
        // qualified form, `Opt.Some(x)`) has no `type_args` slot on the AST
        // at all — a generic sum's own instantiation is inferred from the
        // payload's argument types instead (`check_variant_construction`),
        // never named explicitly at a call site.
        //
        // `Callee::Ctor`'s own `sum: Arc<TypeDecl>` field is read no
        // further than this match's own destructure — #1225's own ADR
        // (`IrExprKind::Variant`'s own doc comment has the full grounding)
        // found `ty` (already computed above) already carries the
        // identical identity as a `TyId`, so `IrExprKind::Variant` itself
        // carries no separate `sum` field to populate.
        return IrExpr {
            kind: IrExprKind::Variant {
                tag,
                payload: args.iter().map(|a| lower_expr_ir(a, cx)).collect(),
            },
            ty,
            span,
        };
    }
    let receiver_is_a_value = matches!(
        callee,
        Callee::Method(_) | Callee::Kernel { .. } | Callee::Agent { .. }
    );
    let mut ir_args: Vec<IrExpr> = Vec::with_capacity(args.len() + receiver_is_a_value as usize);
    if receiver_is_a_value && let Some(r) = receiver {
        ir_args.push(lower_expr_ir(r, cx));
    }
    ir_args.extend(args.iter().map(|a| lower_expr_ir(a, cx)));
    let targs = type_args
        .iter()
        .map(|t| {
            cx.resolve_type_ref(t).unwrap_or_else(|| {
                panic!(
                    "bynk internal error (ADR 0334): an explicit call-site type argument does \
                     not resolve in this pass's own rigid-variable scope, but the checker \
                     already accepted this call"
                )
            })
        })
        .collect();
    IrExpr {
        kind: IrExprKind::Call {
            callee,
            targs,
            args: ir_args,
        },
        ty,
        span,
    }
}

/// A lambda's own recorded type is always `Ty::Fn { params, ret }`
/// (`check_lambda`, `bynk-check/src/checker/expressions.rs:974`) — that
/// `params` list is this pass's only source for each parameter's type,
/// since `LambdaParam::type_ref` is optional and, when absent, the
/// checker infers it from context (an expected function type) rather than
/// resolving it from an annotation this pass could re-derive.
fn lower_lambda_ir(e: &Expr, lambda: &LambdaExpr, cx: &mut LowerIrCtx) -> IrExpr {
    let ty = cx.expr_ty(e.id);
    let param_tys: Vec<TyId> = match &*cx.program.ty_intern.get(ty) {
        Ty::Fn { params, .. } => params.clone(),
        _ => panic!(
            "bynk internal error (ADR 0334): a Lambda's own recorded type is not Ty::Fn — \
             check_lambda only ever types one as a function type"
        ),
    };
    assert_eq!(
        lambda.params.len(),
        param_tys.len(),
        "bynk internal error (ADR 0334): a Lambda's own recorded Ty::Fn has a different arity \
         than its own AST params — bynk-emit::ir::lower and bynk-check disagree about this \
         lambda's shape"
    );
    cx.push_scope();
    for (p, pty) in lambda.params.iter().zip(&param_tys) {
        cx.bind(p.name.name.clone(), *pty);
    }
    let body = lower_expr_ir(&lambda.body, cx);
    cx.pop_scope();
    IrExpr {
        kind: IrExprKind::Lambda {
            params: lambda.params.iter().map(|p| p.name.name.clone()).collect(),
            body: Box::new(body),
            // Free-variable analysis isn't built here — nothing consumes
            // Lambda's IR yet (Decision A, #1143); a later slice computes
            // this once a real need (closure-conversion, a printer) exists
            // to validate it against.
            captures: Vec::new(),
        },
        ty,
        span: e.span,
    }
}

/// Classify a bare `Ident` as `Local` or `Global` — Decision C's narrow
/// scope (refined during implementation, see [`GlobalRef`]'s doc comment for
/// why the `HttpResult`/`QueueResult` case named in the original proposal
/// was dropped).
///
/// The `Global` probe is a pure name-shaped lookup with nothing tying it to
/// "this name is not one of the other, unmigrated forms" — a real
/// collision risk a review of this slice named directly. Only one of those
/// forms is cheaply excludable here (a bare free-function name, `ctx.input.
/// fns` in `check_ident`'s own ladder, `checker/expressions.rs:56-101` —
/// checked *before* its own nullary-variant fallback): excluding it first
/// closes the one case this pass can actually reach today, since this
/// entry point only ever runs over a `fn`/method body ([`lower_fn_body_ir`]'s
/// own doc comment) where a free function is the one non-local, non-variant
/// bare ident that legitimately occurs (a fn-value reference, `Callee`/
/// `Lambda`-adjacent, P6.2 territory). The remaining forms —
/// `bynk-emit/src/emitter/lower.rs`'s `lower_ident` still special-cases
/// `old`/`new` transition binders, invariant state-field reads, agent
/// store-cell/store-map/store-log reads, the multi-actor `deps.who` binder —
/// are handler-body-only and structurally unreachable through this entry
/// point (it has no `store_fields`/`agent_state_ty`/`actor_binding`
/// parameter to carry them), not merely unexcluded; each needs its own
/// resolved-identity plumbing this slice does not commission (Decision C).
fn lower_ident_ir(name: &str, cx: &LowerIrCtx) -> IrExprKind {
    if cx.lookup(name).is_some() {
        return IrExprKind::Local(name.to_string());
    }
    if cx.program.fns.contains_key(name) {
        todo!(
            "bare ident `{name}` names a free function used as a value — Callee/Lambda-adjacent, \
             P6.2 territory, not a Global reference"
        )
    }
    if let Some(sum) = nullary_variant_owner(name, cx) {
        return IrExprKind::Global(GlobalRef {
            sum,
            tag: name.to_string(),
        });
    }
    todo!(
        "bare ident `{name}` is neither a locally-bound name, a free function, nor a bare \
         nullary sum-variant reference — one of lower_ident's other special cases (store field, \
         agent `self`, actor binder, transition `old`/`new`), structurally unreachable through \
         lower_fn_body_ir (see its own doc comment) but left unhandled here defensively"
    )
}

/// The unique sum type owning a nullary (empty-payload) variant named
/// `name`, if exactly one exists. **Not** the same test `check_ident`'s own
/// fallback arm uses (`bynk-check/src/checker/expressions.rs:102-130`) —
/// that arm filters candidate owners by name *only*, requires exactly one,
/// and only then checks the matched variant's payload (a non-empty payload
/// there is a diagnostic, `bynk.types.variant_missing_payload`, not a
/// non-match). This filters by name *and* empty payload before the
/// uniqueness test, so a name matching one sum's nullary variant and a
/// second sum's non-nullary variant of the same name resolves here
/// (uniquely nullary) where `check_ident` would reject it (two owners).
/// Unreachable on a certified program today — the checker's own stricter
/// ladder already rejected that source — so this is a documented
/// divergence, not a live bug; re-run here (rather than read back) because
/// `check_ident`'s own verdict isn't recorded anywhere a later reader can
/// read.
fn nullary_variant_owner(name: &str, cx: &LowerIrCtx) -> Option<Arc<bynk_syntax::ast::TypeDecl>> {
    let mut owners = cx.program.types.values().filter(|t| {
        matches!(&t.body, TypeBody::Sum(s) if s.variants.iter().any(|v| v.name.name == name && v.payload.is_empty()))
    });
    let owner = owners.next()?;
    if owners.next().is_some() {
        return None;
    }
    Some(Arc::clone(owner))
}

/// The declaring `TypeDecl` for a `Ty::Named` type — used by `Record`'s own
/// lowering, where `ty` (this node's already-resolved type) already names
/// the type the checker resolved `RecordConstruction`'s `type_name` against.
fn named_decl(ty: TyId, cx: &LowerIrCtx) -> Arc<bynk_syntax::ast::TypeDecl> {
    let Ty::Named { name, .. } = &*cx.program.ty_intern.get(ty) else {
        panic!(
            "bynk internal error (ADR 0334): a RecordConstruction's own resolved type is not \
             Ty::Named — the checker only ever types one as its declaring record type"
        )
    };
    Arc::clone(cx.program.types.get(name).unwrap_or_else(|| {
        panic!(
            "bynk internal error (ADR 0334): `{name}` has no TypedCommons::types entry, but a \
             RecordConstruction just resolved to it"
        )
    }))
}

/// `TypeName { ...base, field: value, ... }` -> `Block { stmts: [Let(tmp,
/// base), <discarded shadowed-override effects>], tail: Record {
/// <complete, resolved field list> } }` (P6.3, design/tracks/the-ir.md §6,
/// R6.7 — the reference's own Part 6.4 table, Decision E). Every field the
/// target record declares is present in the tail `Record`, each resolved
/// from `overrides` when named there, or otherwise from a synthesised
/// `tmp.<field>` read — the same complete-by-construction shape
/// `RecordConstruction` already lowers to, not the current string emitter's
/// own raw `...spread` splice (`emitter/lower.rs`'s `lower_record_spread`).
///
/// `fields`' own order is *evaluation* order, not declared-field order (the
/// convention `RecordConstruction`'s own lowering above already sets, by
/// simply preserving whatever order its own source `fields` were written
/// in): every overridden field lands in `fields` in the *source* order its
/// override was written, ahead of every spread-through field (declared
/// order, since a bare `tmp.<field>` read has no side effect of its own, so
/// its exact position among the others is not observable). This matters
/// because an override's value may be effectful (`body_performs_effects`,
/// `bynk-check/src/checker/expressions.rs:1303`, walks `overrides` for
/// exactly that reason) — the current string emitter's own splice already
/// evaluates overrides in source order, and a future consumer reading
/// `fields` left-to-right must reproduce that, not silently reorder it to
/// match field *declaration* order instead.
fn lower_record_spread_ir(
    ty: TyId,
    span: Span,
    base: &Expr,
    overrides: &[FieldInit],
    cx: &mut LowerIrCtx,
) -> IrExpr {
    // `ty` is this spread's own resolved type — `check_record_spread`
    // returns the base's own type unchanged (Some(base_ty)), so the
    // declaring `TypeDecl` and its applied type arguments both come from
    // here, the same way `RecordConstruction`'s lowering reads `def` back
    // from its own resolved type rather than re-resolving `type_name`.
    let def = named_decl(ty, cx);
    let base_args = match &*cx.program.ty_intern.get(ty) {
        Ty::Named { args, .. } => args.clone(),
        _ => unreachable!("named_decl above already panics on a non-Ty::Named `ty`"),
    };
    let TypeBody::Record(record_body) = &def.body else {
        panic!(
            "bynk internal error (ADR 0334): `{}` is a RecordSpread's own resolved record type, \
             but its declaration is not TypeBody::Record",
            def.name.name
        )
    };
    let declared: HashSet<&str> = record_body
        .fields
        .iter()
        .map(|f| f.name.name.as_str())
        .collect();

    let base_ir = lower_expr_ir(base, cx);
    let base_ty = base_ir.ty;
    let base_span = base_ir.span;
    let tmp = cx.fresh_spread_tmp();
    let mut stmts = vec![IrStmt::Let {
        local: tmp.clone(),
        value: base_ir,
    }];

    // Lower every override's value in source order — same order the string
    // emitter's own splice and `body_performs_effects`'s own walk already
    // use. `check_record_spread` (`bynk-check/src/checker/expressions.rs:2269`)
    // has no duplicate-name diagnostic, so a field named more than once
    // type-checks today; `overridden` keeps every occurrence (not just the
    // last) so a shadowed occurrence's own effect isn't silently dropped
    // below.
    let overridden: Vec<(String, IrExpr)> = overrides
        .iter()
        .map(|f| {
            if !declared.contains(f.name.name.as_str()) {
                // check_record_spread's own bynk.record_spread.unknown_field
                // diagnostic already rejects this on any program that
                // reaches lowering — ADR 0334 discipline: a live mismatch
                // here is this pass and the checker disagreeing about an
                // already-certified program, a compiler bug, not a silent
                // skip.
                panic!(
                    "bynk internal error (ADR 0334): record spread override `{}` names a field \
                     `{}` does not declare, but the checker already accepted this spread",
                    f.name.name, def.name.name
                )
            }
            let value = match &f.value {
                // An override's own value is lowered exactly like
                // `RecordConstruction`'s explicit/shorthand field above —
                // same two forms, same rules for each.
                Some(v) => lower_expr_ir(v, cx),
                None => {
                    let local_ty = cx.lookup(&f.name.name).unwrap_or_else(|| {
                        panic!(
                            "bynk internal error (ADR 0334): shorthand spread override `{}` has \
                             no local binding in this pass's own scope — the checker accepted \
                             it, so its own `ctx.lookup` must have found one",
                            f.name.name
                        )
                    });
                    IrExpr {
                        kind: lower_ident_ir(&f.name.name, cx),
                        ty: local_ty,
                        span: f.name.span,
                    }
                }
            };
            (f.name.name.clone(), value)
        })
        .collect();

    // A name's *last* occurrence is the one whose value the checker actually
    // admits into the resulting record (`check_record_spread` type-checks
    // every occurrence but the emitted value is always the last-written
    // one) — every earlier occurrence still ran, so it becomes a discarded
    // statement here, preserving its own effect without contributing a
    // field.
    let mut last_index: HashMap<String, usize> = HashMap::new();
    for (i, (name, _)) in overridden.iter().enumerate() {
        last_index.insert(name.clone(), i);
    }
    let mut fields: Vec<(String, IrExpr)> = Vec::with_capacity(record_body.fields.len());
    let mut overridden_names: HashSet<String> = HashSet::new();
    for (i, (name, value)) in overridden.into_iter().enumerate() {
        if last_index[&name] == i {
            overridden_names.insert(name.clone());
            fields.push((name, value));
        } else {
            stmts.push(IrStmt::Expr { value });
        }
    }

    // Spread-through fields — every declared field `overrides` didn't name
    // — appended after, in declared order; a bare `tmp.<field>` read has no
    // effect of its own, so no ordering among these is observable. The
    // field's own type is the declared type instantiated at this spread's
    // own base type arguments (v0.157/ADR 0183's `instantiate_field_ty`,
    // the same substitution `check_record_spread` already applies to
    // type-check an override against a generic record).
    for decl_field in &record_body.fields {
        if overridden_names.contains(decl_field.name.name.as_str()) {
            continue;
        }
        let field_ty = checker::instantiate_field_ty(
            &def,
            &base_args,
            &decl_field.type_ref,
            &cx.program.types,
            &cx.program.ty_intern,
        )
        .unwrap_or_else(|| {
            panic!(
                "bynk internal error (ADR 0334): declared field `{}` of `{}` does not resolve \
                 against this spread's own base type arguments, but the checker already \
                 accepted this record spread",
                decl_field.name.name, def.name.name
            )
        });
        fields.push((
            decl_field.name.name.clone(),
            IrExpr {
                kind: IrExprKind::Field {
                    base: Box::new(IrExpr {
                        kind: IrExprKind::Local(tmp.clone()),
                        ty: base_ty,
                        span: base_span,
                    }),
                    field: decl_field.name.name.clone(),
                },
                ty: field_ty,
                span: decl_field.span,
            },
        ));
    }

    IrExpr {
        kind: IrExprKind::Block {
            stmts,
            tail: Box::new(IrExpr {
                kind: IrExprKind::Record { def, fields },
                ty,
                span,
            }),
        },
        ty,
        span,
    }
}

/// P6.4 (design/tracks/the-ir.md §6, #1157): `&Pattern -> IrPat`, tested
/// standalone against real certified programs. Since P6.5 (#1159), also
/// reached indirectly through [`lower_expr_ir`]'s `Match` arm (via
/// [`lower_arm_ir`]) — `Question`/`Is` stay `todo!()`, each for its own
/// reason, unaffected by `Match` becoming real.
///
/// `scrutinee_ty` is the type of whatever value this particular pattern
/// matches against — the match's own discriminant at the top level, or a
/// payload field's own type one level down inside a `Variant` pattern.
/// Every leaf but `Variant` ignores it structurally (a literal/binding/
/// wildcard pattern's own shape never depends on the scrutinee's type); a
/// `Variant` pattern uses it to resolve `tag`/`fields` through the
/// checker's own `variants_of` (Decision A) — the one place `bynk-check`
/// state is required at all, since `IrPat` otherwise needs nothing beyond
/// the AST `Pattern` it lowers.
fn lower_pattern_ir(pattern: &Pattern, scrutinee_ty: TyId, program: &TypedCommons) -> IrPat {
    match pattern {
        Pattern::Wildcard(_) => IrPat::Wild,
        Pattern::Binding(id) => IrPat::Bind {
            local: id.name.clone(),
        },
        Pattern::Literal { value, .. } => IrPat::Const {
            value: match value {
                LiteralValue::Int(n) => ConstVal::Int(*n),
                LiteralValue::Str(s) => ConstVal::Str(s.clone()),
                LiteralValue::Bool(b) => ConstVal::Bool(*b),
            },
        },
        Pattern::Variant {
            variant, bindings, ..
        } => {
            let variant_info = variant_info_of(scrutinee_ty, &variant.name, program);
            let fields = bindings
                .iter()
                .enumerate()
                .map(|(idx, b)| match &b.kind {
                    PatternBindingKind::Named { field, pattern } => {
                        let field_ty = variant_info
                            .payload
                            .iter()
                            .find(|(name, _)| name == &field.name)
                            .map(|(_, ty)| *ty)
                            .unwrap_or_else(|| {
                                panic!(
                                    "bynk internal error (ADR 0334): named pattern field `{}` \
                                     does not resolve against variant `{}`'s own payload, but \
                                     the checker already accepted this pattern",
                                    field.name, variant.name
                                )
                            });
                        (
                            field.name.clone(),
                            Box::new(lower_pattern_ir(pattern, field_ty, program)),
                        )
                    }
                    PatternBindingKind::Positional { pattern } => {
                        let (name, field_ty) =
                            variant_info.payload.get(idx).cloned().unwrap_or_else(|| {
                                panic!(
                                    "bynk internal error (ADR 0334): positional pattern binding \
                                     {idx} has no matching payload field on variant `{}`, but \
                                     the checker already accepted this pattern's arity",
                                    variant.name
                                )
                            });
                        (name, Box::new(lower_pattern_ir(pattern, field_ty, program)))
                    }
                })
                .collect();
            IrPat::Variant {
                scrutinee_ty,
                tag: variant.name.clone(),
                fields,
            }
        }
        Pattern::Refined {
            inner, predicate, ..
        } => IrPat::Refined {
            inner: Box::new(lower_pattern_ir(inner, scrutinee_ty, program)),
            refinement: predicate.clone(),
        },
        Pattern::Or(alts, _) => IrPat::Or {
            alts: alts
                .iter()
                .map(|p| lower_pattern_ir(p, scrutinee_ty, program))
                .collect(),
        },
    }
}

/// Shared by [`lower_pattern_ir`]'s `Variant` arm and
/// [`collect_pattern_binding_tys`]'s own mirror walk: resolve `tag` against
/// `scrutinee_ty` through the checker's `variants_of` (Decision A, R5.11).
/// `.expect()`-not-fallback (ADR 0334) — both call sites are reached only
/// from a certified program's own pattern, which `check_pattern`
/// (`bynk-check/src/checker/expressions.rs:3162`) already required to name
/// a real variant of a real variant-kind scrutinee.
fn variant_info_of(scrutinee_ty: TyId, tag: &str, program: &TypedCommons) -> checker::VariantInfo {
    checker::variants_of(scrutinee_ty, &program.types, program.tys())
        .unwrap_or_else(|| {
            panic!(
                "bynk internal error (ADR 0334): variant pattern `{tag}` matches against a \
                 non-variant-kind scrutinee, but the checker already accepted this pattern"
            )
        })
        .into_iter()
        .find(|v| v.name == tag)
        .unwrap_or_else(|| {
            panic!(
                "bynk internal error (ADR 0334): scrutinee has no variant `{tag}`, but the \
                 checker already accepted this pattern"
            )
        })
}

/// Mirrors [`lower_pattern_ir`]'s own recursive walk but collects `(name,
/// TyId)` pairs instead of building `IrPat` nodes — [`lower_arm_ir`]'s own
/// need, not `IrPat`'s: a bound name's type has nowhere to live on `IrPat`
/// itself (R6.1 is an `IrExpr` rule, not a pattern rule; the reference's own
/// `IrPat::Bind` carries no type either), yet this pass's `guard`/`body`
/// lowering needs every bound name in scope to resolve as a `Local` the
/// same way a `Block`'s own `Let` does. The checker's own equivalent table
/// (`Ctx::pattern_binding_types`) is transient — never persisted onto
/// `TypedCommons` — so this pass re-derives it here rather than reading a
/// checked-output field.
fn collect_pattern_binding_tys(
    pattern: &Pattern,
    ty: TyId,
    program: &TypedCommons,
    out: &mut Vec<(String, TyId)>,
) {
    match pattern {
        Pattern::Wildcard(_) | Pattern::Literal { .. } => {}
        Pattern::Binding(id) => out.push((id.name.clone(), ty)),
        Pattern::Variant {
            variant, bindings, ..
        } => {
            let variant_info = variant_info_of(ty, &variant.name, program);
            // `.expect()`-not-fallback (ADR 0334), matching `lower_pattern_ir`'s
            // identical lookups on the identical condition — this walk runs
            // over the same certified pattern, so a miss here is the same
            // checker/lowering disagreement, not a softer case.
            for (idx, b) in bindings.iter().enumerate() {
                match &b.kind {
                    PatternBindingKind::Named { field, pattern } => {
                        let field_ty = variant_info
                            .payload
                            .iter()
                            .find(|(name, _)| name == &field.name)
                            .map(|(_, ty)| *ty)
                            .unwrap_or_else(|| {
                                panic!(
                                    "bynk internal error (ADR 0334): named pattern field `{}` \
                                     does not resolve against variant `{}`'s own payload, but \
                                     the checker already accepted this pattern",
                                    field.name, variant.name
                                )
                            });
                        collect_pattern_binding_tys(pattern, field_ty, program, out);
                    }
                    PatternBindingKind::Positional { pattern } => {
                        let (_, field_ty) =
                            variant_info.payload.get(idx).cloned().unwrap_or_else(|| {
                                panic!(
                                    "bynk internal error (ADR 0334): positional pattern binding \
                                     {idx} has no matching payload field on variant `{}`, but \
                                     the checker already accepted this pattern's arity",
                                    variant.name
                                )
                            });
                        collect_pattern_binding_tys(pattern, field_ty, program, out);
                    }
                }
            }
        }
        Pattern::Refined { inner, .. } => collect_pattern_binding_tys(inner, ty, program, out),
        // Every alternative binds the same names at the same types
        // (`check_or_pattern_bindings`'s own Rule 1/2) — the first
        // alternative only, matching `Pattern::bound_names`'s own
        // defensive default.
        Pattern::Or(alts, _) => {
            if let Some(first) = alts.first() {
                collect_pattern_binding_tys(first, ty, program, out);
            }
        }
    }
}

/// R5.5 (Decision C): `true` iff `pat` contains an `Or` anywhere in its own
/// tree — the fact [`IrArm::binding_mode`] records once rather than the
/// emitter re-discovering it at emission time.
fn ir_pat_contains_or(pat: &IrPat) -> bool {
    match pat {
        IrPat::Wild | IrPat::Bind { .. } | IrPat::Const { .. } => false,
        IrPat::Variant { fields, .. } => fields.iter().any(|(_, p)| ir_pat_contains_or(p)),
        IrPat::Refined { inner, .. } => ir_pat_contains_or(inner),
        IrPat::Or { .. } => true,
    }
}

/// P6.4 (#1157): the non-form-deciding parts of a `MatchArm` —
/// `pat`/`guard`/`body`/`binds`/`binding_mode` — everything but the
/// `MatchForm` policy R5.2 decides, which P6.5 (#1159) builds separately
/// (`match_needs_if_chain`, reused from the string emitter — Decision B).
/// Tested standalone the same way [`lower_pattern_ir`] is; since P6.5, also
/// called per-arm by [`lower_expr_ir`]'s real `Match` arm.
///
/// R5.4's ordering (structural test, then refinement, then bindings, then
/// guard) is why `guard`/`body` are lowered with the pattern's own bound
/// names already pushed into scope — a guard must be able to read them.
fn lower_arm_ir(arm: &MatchArm, scrutinee_ty: TyId, cx: &mut LowerIrCtx) -> IrArm {
    let pat = lower_pattern_ir(&arm.pattern, scrutinee_ty, cx.program);
    let binds: Vec<String> = arm
        .pattern
        .bound_names()
        .into_iter()
        .map(|id| id.name.clone())
        .collect();

    let mut bind_tys = Vec::new();
    collect_pattern_binding_tys(&arm.pattern, scrutinee_ty, cx.program, &mut bind_tys);

    cx.push_scope();
    for (name, ty) in bind_tys {
        cx.bind(name, ty);
    }
    let guard = arm.guard.as_ref().map(|g| lower_expr_ir(g, cx));
    let body = match &arm.body {
        MatchBody::Expr(e) => lower_expr_ir(e, cx),
        MatchBody::Block(b) => lower_block_ir(b, cx),
    };
    cx.pop_scope();

    let binding_mode = if ir_pat_contains_or(&pat) {
        BindingMode::OrDispatch
    } else {
        BindingMode::Direct
    };

    IrArm {
        pat,
        guard,
        body,
        binds,
        binding_mode,
    }
}

/// P6.4 (#1157, Decision B — extends ADR 0334's `.expect()`-not-fallback
/// discipline to a second rule): a certified `CheckedProgram` can never
/// contain a structurally non-exhaustive match — every diagnostic path that
/// would produce one (`bynk.types.non_exhaustive_match`) is error-severity,
/// and `certify` (R3.10) rejects any unit carrying one. This pass therefore
/// never re-derives the verdict `bynk-check`'s own `saw_wildcard`/
/// `missing_patterns` machinery already computed — it trusts it, checking
/// only the one structural fact that verdict *implies* and this function
/// can cheaply confirm without rebuilding `missing_patterns`' own witness
/// machinery: a guarded arm never contributes to coverage
/// (`bynk-check/src/checker/expressions.rs`'s own `unguarded` filter,
/// `:3016`/`:3034`), so a certified match always has at least one unguarded
/// arm. `Exhaustive::Partial` stays real, inhabited code (a future
/// non-certified producer's own lane) — just never constructed here.
fn lower_exhaustive_ir(arms: &[MatchArm]) -> Exhaustive {
    if arms.iter().any(|a| a.guard.is_none()) {
        Exhaustive::Total
    } else {
        unreachable!(
            "bynk internal error (ADR 0334, extended by Decision B / #1157): a certified \
             program's match is never empty (the parser itself already requires at least one \
             arm) and always has at least one unguarded arm — bynk-check's own missing_patterns \
             gate (bynk.types.non_exhaustive_match, error-severity, rejected by certify per \
             R3.10) guarantees it"
        )
    }
}

/// Decision E: targeted minimal fixtures, one per node kind this slice
/// covers, staying strictly inside the subset [`lower_expr_ir`]/
/// [`lower_stmt_ir`] actually implement — not a walk over the real
/// `bynkc/tests/fixtures/positive` corpus, which hits an unimplemented
/// `Match`/`Call` arm within a few lines of almost any real fixture.
#[cfg(test)]
mod tests {
    use super::*;
    use bynk_check::checker::CheckedProgram;
    use bynk_check::hints::HintSink;
    use bynk_check::index::RefSink;
    use bynk_check::locals::LocalsSink;
    use bynk_check::requirements::RequirementSink;
    use bynk_check::{checker, context_checks, resolver, symbols};
    use bynk_project::UnitKind;
    use bynk_syntax::ast::{Commons, CommonsItem, FnDecl, SourceUnit};
    use bynk_syntax::{lexer, parser};

    fn checked_program(source: &str) -> CheckedProgram {
        let tokens = lexer::tokenize(source).expect("lex");
        let (commons, warnings) = parser::parse_with_warnings(&tokens, source).expect("parse");
        let resolved = resolver::resolve(commons).expect("resolve");
        let typed = checker::check(resolved).expect("check");
        checker::certify(typed, warnings).expect("certify")
    }

    fn find_fn<'a>(program: &'a CheckedProgram, name: &str) -> &'a FnDecl {
        program
            .program()
            .commons
            .items
            .iter()
            .find_map(|item| match item {
                CommonsItem::Fn(f) if f.name.display() == name => Some(f),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no fn named `{name}` in this fixture"))
    }

    fn lower_fn(program: &CheckedProgram, name: &str) -> IrExpr {
        let f = find_fn(program, name);
        lower_fn_body_ir(f, program)
    }

    fn find_type<'a>(program: &'a CheckedProgram, name: &str) -> &'a Arc<TypeDecl> {
        program
            .program()
            .types
            .get(name)
            .unwrap_or_else(|| panic!("no type named `{name}` in this fixture"))
    }

    /// Like [`find_fn`], but returns the program's own `Arc<FnDecl>` handle
    /// (from `TypedCommons.fns`/`.methods`, not re-walked from
    /// `commons.items`) — [`lower_fn_item_ir`] takes `&Arc<FnDecl>`
    /// specifically so `IrItem::Fn::def` can reuse this same `Arc`.
    fn find_fn_arc<'a>(program: &'a CheckedProgram, name: &str) -> &'a Arc<FnDecl> {
        if let Some((type_name, method_name)) = name.split_once('.') {
            let table = program.program().methods.get(type_name).unwrap_or_else(|| {
                panic!("no method table for type `{type_name}` in this fixture")
            });
            return table
                .instance
                .get(method_name)
                .or_else(|| table.statics.get(method_name))
                .unwrap_or_else(|| panic!("no method named `{name}` in this fixture"));
        }
        program
            .program()
            .fns
            .get(name)
            .unwrap_or_else(|| panic!("no fn named `{name}` in this fixture"))
    }

    /// Every fixture's `fn`-body wrapping — asserts the outer shape once so
    /// each node-kind test below only asserts its own tail, not this too.
    fn fn_tail(ir: &IrExpr) -> &IrExpr {
        let IrExprKind::Block { stmts, tail } = &ir.kind else {
            panic!(
                "lower_fn_body_ir always returns IrExprKind::Block, got {:?}",
                ir.kind
            )
        };
        assert!(
            stmts.is_empty(),
            "this helper is for single-tail bodies only"
        );
        let IrExprKind::Return { value } = &tail.kind else {
            panic!(
                "a fn body's own tail is always wrapped in Return, got {:?}",
                tail.kind
            )
        };
        value
    }

    #[test]
    fn const_covers_every_bynk_literal_form() {
        let program = checked_program(
            r#"
commons demo {
  fn int_lit() -> Int { 1 }
  fn float_lit() -> Float { 1.5 }
  fn duration_lit() -> Duration { 5.minutes }
  fn str_lit() -> String { "hi" }
  fn bool_lit() -> Bool { true }
  fn unit_lit() -> () { () }
}
"#,
        );
        let cases: &[(&str, ConstVal)] = &[
            ("int_lit", ConstVal::Int(1)),
            ("float_lit", ConstVal::Float(1.5)),
            ("duration_lit", ConstVal::DurationMillis(5 * 60 * 1000)),
            ("str_lit", ConstVal::Str("hi".to_string())),
            ("bool_lit", ConstVal::Bool(true)),
            ("unit_lit", ConstVal::Unit),
        ];
        for (fn_name, expected) in cases {
            let ir = lower_fn(&program, fn_name);
            let tail = fn_tail(&ir);
            let IrExprKind::Const(actual) = &tail.kind else {
                panic!("{fn_name}: expected Const, got {:?}", tail.kind)
            };
            assert_eq!(actual, expected, "{fn_name}");
        }
    }

    #[test]
    fn local_reads_a_bound_param() {
        let program = checked_program(
            r#"
commons demo {
  fn identity(n: Int) -> Int { n }
}
"#,
        );
        let ir = lower_fn(&program, "identity");
        let tail = fn_tail(&ir);
        assert!(matches!(&tail.kind, IrExprKind::Local(name) if name == "n"));
    }

    #[test]
    fn generic_fn_type_parameters_are_rigid_variables_not_unresolvable_declared_types() {
        // `resolve_type_ref` (no `vars` set) resolves a fn's own type
        // parameter `T` as an unknown *declared* type and fails — the same
        // failure mode `checker.rs`'s own `Ctx::type_vars` +
        // `resolve_type_ref_in` exists to avoid. Without it, `x`'s own type
        // never binds, and this test's own body — a bare `Local` — would
        // wrongly fall through to `lower_ident_ir`'s `todo!()`.
        let program = checked_program(
            r#"
commons demo {
  fn identity[T](x: T) -> T { x }
}
"#,
        );
        let ir = lower_fn(&program, "identity");
        let tail = fn_tail(&ir);
        assert!(matches!(&tail.kind, IrExprKind::Local(name) if name == "x"));
    }

    #[test]
    fn global_covers_a_bare_nullary_sum_variant() {
        let program = checked_program(
            r#"
commons demo {
  type Outcome =
    | Hit(score: Int)
    | Miss

  fn make() -> Outcome { Miss }
}
"#,
        );
        let ir = lower_fn(&program, "make");
        let tail = fn_tail(&ir);
        let IrExprKind::Global(g) = &tail.kind else {
            panic!("expected Global, got {:?}", tail.kind)
        };
        assert_eq!(g.sum.name.name, "Outcome");
        assert_eq!(g.tag, "Miss");
    }

    #[test]
    #[should_panic(expected = "Callee/Lambda-adjacent")]
    fn bare_free_function_reference_is_excluded_from_the_global_probe() {
        // A bare function-value reference (not a call) is the one non-local,
        // non-variant ident this pass can actually reach — `lower_ident_ir`
        // must stop here rather than risk `nullary_variant_owner` matching a
        // same-named variant by coincidence (a real collision risk a review
        // of this slice named directly).
        let program = checked_program(
            r#"
commons demo {
  fn double(n: Int) -> Int { n * 2 }

  fn get_double() -> (Int) -> Int { double }
}
"#,
        );
        let _ = lower_fn(&program, "get_double");
    }

    #[test]
    fn record_construction_covers_explicit_and_shorthand_fields() {
        let program = checked_program(
            r#"
commons demo {
  type Point = { x: Int, y: Int }

  fn explicit() -> Point { Point { x: 1, y: 2 } }
  fn shorthand(x: Int, y: Int) -> Point { Point { x, y } }
}
"#,
        );
        let explicit_ir = lower_fn(&program, "explicit");
        let explicit_tail = fn_tail(&explicit_ir);
        let IrExprKind::Record {
            def: explicit_def,
            fields: explicit_fields,
        } = &explicit_tail.kind
        else {
            panic!("explicit: expected Record, got {:?}", explicit_tail.kind)
        };
        assert_eq!(explicit_def.name.name, "Point");
        assert_eq!(explicit_fields.len(), 2);
        assert_eq!(explicit_fields[0].0, "x");
        assert!(matches!(
            &explicit_fields[0].1.kind,
            IrExprKind::Const(ConstVal::Int(1))
        ));
        assert_eq!(explicit_fields[1].0, "y");
        assert!(matches!(
            &explicit_fields[1].1.kind,
            IrExprKind::Const(ConstVal::Int(2))
        ));

        let shorthand_ir = lower_fn(&program, "shorthand");
        let shorthand_tail = fn_tail(&shorthand_ir);
        let IrExprKind::Record {
            def: shorthand_def,
            fields: shorthand_fields,
        } = &shorthand_tail.kind
        else {
            panic!("shorthand: expected Record, got {:?}", shorthand_tail.kind)
        };
        assert_eq!(shorthand_def.name.name, "Point");
        assert_eq!(shorthand_fields.len(), 2);
        assert_eq!(shorthand_fields[0].0, "x");
        assert!(matches!(&shorthand_fields[0].1.kind, IrExprKind::Local(n) if n == "x"));
        assert_eq!(shorthand_fields[1].0, "y");
        assert!(matches!(&shorthand_fields[1].1.kind, IrExprKind::Local(n) if n == "y"));
        // The shorthand path is the only reason `LowerIrCtx` tracks a scope
        // stack at all — assert the thing it uniquely produces (a field's
        // `ty`, taken from `cx.lookup` since a shorthand field has no
        // `ExprId` of its own) actually matches the param's real type.
        assert!(matches!(
            &*program.program().ty_intern.get(shorthand_fields[0].1.ty),
            Ty::Base(bynk_syntax::ast::BaseType::Int)
        ));
        assert!(matches!(
            &*program.program().ty_intern.get(shorthand_fields[1].1.ty),
            Ty::Base(bynk_syntax::ast::BaseType::Int)
        ));
    }

    #[test]
    fn field_access_reads_a_record_field() {
        let program = checked_program(
            r#"
commons demo {
  type Point = { x: Int, y: Int }

  fn get_x(p: Point) -> Int { p.x }
}
"#,
        );
        let ir = lower_fn(&program, "get_x");
        let tail = fn_tail(&ir);
        let IrExprKind::Field { base, field } = &tail.kind else {
            panic!("expected Field, got {:?}", tail.kind)
        };
        assert_eq!(field, "x");
        assert!(matches!(&base.kind, IrExprKind::Local(name) if name == "p"));
    }

    #[test]
    fn list_literal_lowers_every_element() {
        let program = checked_program(
            r#"
commons demo {
  fn make() -> List[Int] { [1, 2, 3] }
}
"#,
        );
        let ir = lower_fn(&program, "make");
        let tail = fn_tail(&ir);
        let IrExprKind::List { elems } = &tail.kind else {
            panic!("expected List, got {:?}", tail.kind)
        };
        assert_eq!(elems.len(), 3);
        assert!(matches!(
            &elems[0].kind,
            IrExprKind::Const(ConstVal::Int(1))
        ));
    }

    #[test]
    fn if_lowers_both_branches_as_blocks_not_return_wrapped() {
        // Also this slice's only real coverage of nested (non-fn-body) Block
        // lowering: `bynk_syntax::parser` constructs `ExprKind::Block` only
        // as a lambda body (`parser/statements.rs:421-428`, P6.2 territory) —
        // an `if`/`else` branch's `Block` is a bare AST field
        // (`ExprKind::If { then_block: Box<Block>, .. }`), not wrapped in
        // `ExprKind::Block`, but it is lowered through the exact same
        // `lower_block_ir` an `ExprKind::Block` would be, so this is the
        // real, reachable test for "a nested block is not Return-wrapped,
        // unlike a fn body's own outermost block" (`lower_fn_body_ir`'s own
        // doc comment).
        let program = checked_program(
            r#"
commons demo {
  fn choose(b: Bool) -> Int {
    if b { 1 } else { 2 }
  }
}
"#,
        );
        let ir = lower_fn(&program, "choose");
        let tail = fn_tail(&ir);
        let IrExprKind::If { cond, then_, else_ } = &tail.kind else {
            panic!("expected If, got {:?}", tail.kind)
        };
        assert!(matches!(&cond.kind, IrExprKind::Local(name) if name == "b"));
        let IrExprKind::Block {
            tail: then_tail, ..
        } = &then_.kind
        else {
            panic!("expected then_ to be a Block, got {:?}", then_.kind)
        };
        assert!(matches!(
            &then_tail.kind,
            IrExprKind::Const(ConstVal::Int(1))
        ));
        let IrExprKind::Block {
            tail: else_tail, ..
        } = &else_.kind
        else {
            panic!("expected else_ to be a Block, got {:?}", else_.kind)
        };
        assert!(matches!(
            &else_tail.kind,
            IrExprKind::Const(ConstVal::Int(2))
        ));
    }

    #[test]
    fn and_or_not_are_real_tree_nodes() {
        let program = checked_program(
            r#"
commons demo {
  fn conj(a: Bool, b: Bool) -> Bool { a && b }
  fn disj(a: Bool, b: Bool) -> Bool { a || b }
  fn negate(a: Bool) -> Bool { !a }
}
"#,
        );
        let and_ir = lower_fn(&program, "conj");
        assert!(matches!(fn_tail(&and_ir).kind, IrExprKind::And { .. }));
        let or_ir = lower_fn(&program, "disj");
        assert!(matches!(fn_tail(&or_ir).kind, IrExprKind::Or { .. }));
        let not_ir = lower_fn(&program, "negate");
        assert!(matches!(fn_tail(&not_ir).kind, IrExprKind::Not { .. }));
    }

    #[test]
    fn pure_wraps_a_synchronous_value_as_effect() {
        let program = checked_program(
            r#"
commons demo {
  fn make() -> Effect[Int] { Effect.pure(1) }
}
"#,
        );
        let ir = lower_fn(&program, "make");
        let tail = fn_tail(&ir);
        let IrExprKind::Pure { value } = &tail.kind else {
            panic!("expected Pure, got {:?}", tail.kind)
        };
        assert!(matches!(&value.kind, IrExprKind::Const(ConstVal::Int(1))));
    }

    #[test]
    fn await_peels_effect_from_an_effect_let_binding() {
        let program = checked_program(
            r#"
commons demo {
  fn use_it() -> Effect[Int] {
    let x <- Effect.pure(1)
    Effect.pure(x)
  }
}
"#,
        );
        let f = find_fn(&program, "use_it");
        let ir = lower_fn_body_ir(f, &program);
        let IrExprKind::Block { stmts, .. } = &ir.kind else {
            panic!("expected Block")
        };
        assert_eq!(stmts.len(), 1);
        let IrStmt::Let { local, value } = &stmts[0] else {
            panic!("expected Let, got {:?}", stmts[0])
        };
        assert_eq!(local, "x");
        assert!(matches!(&value.kind, IrExprKind::Await { .. }));
    }

    #[test]
    fn let_annotation_widens_the_bound_scope_type_not_just_the_rhs_expression() {
        // `compatible()` admits a refined value under its own base's
        // annotation — `let n: Int = p` with `p: Reps` leaves the checker's
        // scope holding `Int`, not the narrower `Reps`. The `let` statement's
        // own `value` keeps the RHS expression's own honest (refined) type
        // (R6.1); only the *bound* name — read back here by the shorthand
        // field `{ n }`, which has no `ExprId` of its own to fall back on —
        // must reflect the annotation's widened type instead.
        let program = checked_program(
            r#"
commons demo {
  type Reps = Int where InRange(1, 100)
  type Wrapper = { n: Int }

  fn make(p: Reps) -> Wrapper {
    let n: Int = p
    Wrapper { n }
  }
}
"#,
        );
        let f = find_fn(&program, "make");
        let ir = lower_fn_body_ir(f, &program);
        let IrExprKind::Block { stmts, tail } = &ir.kind else {
            panic!("expected Block")
        };
        let IrStmt::Let { local, value } = &stmts[0] else {
            panic!("expected Let, got {:?}", stmts[0])
        };
        assert_eq!(local, "n");
        assert!(matches!(
            &*program.program().ty_intern.get(value.ty),
            Ty::Named {
                kind: bynk_check::checker::NamedKind::Refined(_),
                ..
            }
        ));
        let IrExprKind::Return { value: wrapper } = &tail.kind else {
            panic!("expected Return, got {:?}", tail.kind)
        };
        let IrExprKind::Record { fields, .. } = &wrapper.kind else {
            panic!("expected Record, got {:?}", wrapper.kind)
        };
        assert_eq!(fields[0].0, "n");
        assert!(matches!(&fields[0].1.kind, IrExprKind::Local(name) if name == "n"));
        assert!(matches!(
            &*program.program().ty_intern.get(fields[0].1.ty),
            Ty::Base(bynk_syntax::ast::BaseType::Int)
        ));
    }

    #[test]
    fn do_statement_lowers_to_a_discarded_await() {
        let program = checked_program(
            r#"
commons demo {
  fn use_it() -> Effect[()] {
    do Effect.pure(())
    Effect.pure(())
  }
}
"#,
        );
        let f = find_fn(&program, "use_it");
        let ir = lower_fn_body_ir(f, &program);
        let IrExprKind::Block { stmts, .. } = &ir.kind else {
            panic!("expected Block")
        };
        assert_eq!(stmts.len(), 1);
        let IrStmt::Expr { value } = &stmts[0] else {
            panic!("expected Expr, got {:?}", stmts[0])
        };
        assert!(matches!(&value.kind, IrExprKind::Await { .. }));
    }

    #[test]
    fn send_statement_lowers_to_a_fire_and_forget_send_typed_unit() {
        let program = checked_program(
            r#"
commons demo {
  fn use_it() -> Effect[()] {
    ~> Effect.pure(())
    Effect.pure(())
  }
}
"#,
        );
        let f = find_fn(&program, "use_it");
        let ir = lower_fn_body_ir(f, &program);
        let IrExprKind::Block { stmts, .. } = &ir.kind else {
            panic!("expected Block")
        };
        assert_eq!(stmts.len(), 1);
        let IrStmt::Expr { value } = &stmts[0] else {
            panic!("expected Expr, got {:?}", stmts[0])
        };
        assert!(matches!(&value.kind, IrExprKind::Send { .. }));
        assert!(matches!(
            &*program.program().ty_intern.get(value.ty),
            bynk_check::checker::Ty::Unit
        ));
    }

    // P6.2 (#1143): Call/Lambda/Variant, driven entirely by the Callee P6.0
    // already recorded. `Callee::Store`/`Callee::Query` are deliberately
    // untested here — both dispatch only inside an agent handler body,
    // which needs the full context/project pipeline
    // (`context_checks::check_context_declarations`, a `UnitTable`,
    // `CrossContextInfo`) to check at all; `checked_program`'s own
    // single-file `resolver::resolve`/`checker::check` pipeline never
    // walks a `CommonsItem::Agent` (only `CommonsItem::Fn`), the same
    // "needs the full pipeline" limitation P6.0's own differential test
    // (`bynk-check/tests/callee_classification.rs`) already documented for
    // `Capability`/`CrossCap`/`Cross`/`Agent`.

    #[test]
    fn call_driven_by_callee_fn_lowers_a_free_function_call() {
        let program = checked_program(
            r#"
commons demo {
  fn double(n: Int) -> Int { n }

  fn use_it() -> Int { double(1) }
}
"#,
        );
        let ir = lower_fn(&program, "use_it");
        let tail = fn_tail(&ir);
        let IrExprKind::Call {
            callee,
            targs,
            args,
        } = &tail.kind
        else {
            panic!("expected Call, got {:?}", tail.kind)
        };
        assert!(matches!(callee, Callee::Fn(f) if f.name.display() == "double"));
        assert!(targs.is_empty());
        assert_eq!(args.len(), 1);
        assert!(matches!(&args[0].kind, IrExprKind::Const(ConstVal::Int(1))));
    }

    #[test]
    fn call_with_an_explicit_type_argument_resolves_targs() {
        let program = checked_program(
            r#"
commons demo {
  fn identity[T](x: T) -> T { x }

  fn use_it() -> Int { identity[Int](1) }
}
"#,
        );
        let ir = lower_fn(&program, "use_it");
        let tail = fn_tail(&ir);
        let IrExprKind::Call { callee, targs, .. } = &tail.kind else {
            panic!("expected Call, got {:?}", tail.kind)
        };
        assert!(matches!(callee, Callee::Fn(f) if f.name.display() == "identity"));
        assert_eq!(targs.len(), 1);
        assert!(matches!(
            &*program.program().ty_intern.get(targs[0]),
            Ty::Base(bynk_syntax::ast::BaseType::Int)
        ));
    }

    #[test]
    fn call_with_an_explicit_type_argument_naming_the_enclosing_fns_own_rigid_var() {
        // The case `resolve_type_ref_in`'s own `type_vars` set exists for
        // (`lower.rs`'s own `LowerIrCtx::resolve_type_ref`) — a wrong
        // `type_vars` seed for `wrap`'s own body turns into a panic here
        // (`identity[U]`'s own `U` fails to resolve as a declared type)
        // rather than a silently wrong result.
        let program = checked_program(
            r#"
commons demo {
  fn identity[T](x: T) -> T { x }

  fn wrap[U](x: U) -> U { identity[U](x) }
}
"#,
        );
        let ir = lower_fn(&program, "wrap");
        let tail = fn_tail(&ir);
        let IrExprKind::Call { callee, targs, .. } = &tail.kind else {
            panic!("expected Call, got {:?}", tail.kind)
        };
        assert!(matches!(callee, Callee::Fn(f) if f.name.display() == "identity"));
        assert_eq!(targs.len(), 1);
        assert!(matches!(
            &*program.program().ty_intern.get(targs[0]),
            Ty::Var(name) if name == "U"
        ));
    }

    #[test]
    fn call_driven_by_callee_value_applies_a_function_typed_local() {
        let program = checked_program(
            r#"
commons demo {
  fn apply(f: (Int) -> Int, x: Int) -> Int { f(x) }
}
"#,
        );
        let ir = lower_fn(&program, "apply");
        let tail = fn_tail(&ir);
        let IrExprKind::Call { callee, args, .. } = &tail.kind else {
            panic!("expected Call, got {:?}", tail.kind)
        };
        assert!(matches!(callee, Callee::Value(name) if name == "f"));
        assert_eq!(args.len(), 1);
        assert!(matches!(&args[0].kind, IrExprKind::Local(n) if n == "x"));
    }

    #[test]
    fn bare_and_qualified_variant_construction_both_lower_to_variant() {
        let program = checked_program(
            r#"
commons demo {
  type Shape =
    | Circle(radius: Int)
    | Square(side: Int)

  fn bare(n: Int) -> Shape { Circle(n) }
  fn qualified(n: Int) -> Shape { Shape.Circle(n) }
}
"#,
        );
        for fn_name in ["bare", "qualified"] {
            let ir = lower_fn(&program, fn_name);
            let tail = fn_tail(&ir);
            let IrExprKind::Variant { tag, payload } = &tail.kind else {
                panic!("{fn_name}: expected Variant, got {:?}", tail.kind)
            };
            // #1225's own ADR: no `sum` field on `Variant` itself — the
            // constructed sum's own identity is the wrapping `IrExpr::ty`.
            assert!(
                matches!(
                    &*program.program().ty_intern.get(tail.ty),
                    Ty::Named { name, .. } if name == "Shape"
                ),
                "{fn_name}: expected tail.ty to resolve to the Shape sum, got {:?}",
                program.program().ty_intern.get(tail.ty)
            );
            assert_eq!(tag, "Circle");
            assert_eq!(payload.len(), 1);
            assert!(matches!(&payload[0].kind, IrExprKind::Local(n) if n == "n"));
        }
    }

    /// #1225's own ADR resolution: `Ok`/`Err`/`Some`/`None` each lower to a
    /// real `IrExprKind::Variant`, the same node shape a user-declared
    /// sum's own `Callee::Ctor` construction lowers to
    /// (`bare_and_qualified_variant_construction_both_lower_to_variant`,
    /// above) — no separate `IrExprKind` needed for the built-in case, and
    /// no `Callee` classification either (confirmed during scoping: neither
    /// `check_ok`/`check_err`/`check_some`/`check_none` ever records one).
    ///
    /// Review of #1227: `tag`/`payload` alone would pass even if `tail.ty`
    /// carried the wrong identity (or none at all) — the actual claim the
    /// ADR rests on is that the *wrapping* `IrExpr::ty` is what a consumer
    /// reads instead of a `sum` field, so each case also asserts `tail.ty`
    /// resolves to the built-in sum actually constructed. `ok_http_case`
    /// pins the fifth, easy-to-miss shape: `Ok` is overloaded between
    /// `Result` and `HttpResult` (`check_ok`, `bynk-check/src/checker/
    /// expressions.rs`, peels the surrounding return type to decide), so a
    /// handler-shaped `Ok(...)` reaches this same arm with `ty =
    /// Ty::HttpResult(_)`, a third sum identity the claim has to cover —
    /// `variants_of`'s own `Ty::HttpResult` arm (`checker.rs`, backed by
    /// `HTTP_VARIANTS`, `bynk-syntax/src/ast.rs`) resolves it the same way.
    #[test]
    fn ok_err_some_none_all_lower_to_variant_by_tag_and_carry_their_sum_identity_on_ty() {
        let program = checked_program(
            r#"
commons demo {
  fn ok_case() -> Result[Int, String] { Ok(1) }
  fn err_case() -> Result[Int, String] { Err("bad") }
  fn some_case() -> Option[Int] { Some(1) }
  fn none_case() -> Option[Int] { None }
  fn ok_http_case() -> HttpResult[Int] { Ok(1) }
}
"#,
        );
        for (fn_name, expect_tag, expect_payload_len) in [
            ("ok_case", "Ok", 1),
            ("err_case", "Err", 1),
            ("some_case", "Some", 1),
            ("none_case", "None", 0),
            ("ok_http_case", "Ok", 1),
        ] {
            let ir = lower_fn(&program, fn_name);
            let tail = fn_tail(&ir);
            let IrExprKind::Variant { tag, payload } = &tail.kind else {
                panic!("{fn_name}: expected Variant, got {:?}", tail.kind)
            };
            assert_eq!(tag, expect_tag, "{fn_name}");
            assert_eq!(payload.len(), expect_payload_len, "{fn_name}");
            let resolved = &*program.program().ty_intern.get(tail.ty);
            let sum_matches = match fn_name {
                "ok_case" | "err_case" => matches!(resolved, Ty::Result(..)),
                "some_case" | "none_case" => matches!(resolved, Ty::Option(_)),
                "ok_http_case" => matches!(resolved, Ty::HttpResult(_)),
                _ => unreachable!(),
            };
            assert!(
                sum_matches,
                "{fn_name}: expected tail.ty to resolve to the constructed sum, got {resolved:?}"
            );
        }
    }

    #[test]
    fn method_call_driven_by_callee_method_prepends_the_receiver() {
        let program = checked_program(
            r#"
commons demo {
  type Point = { x: Int, y: Int }

  fn Point.shiftX(self, dx: Int) -> Point {
    Point { x: self.x, y: self.y }
  }

  fn use_it(p: Point, dx: Int) -> Point { p.shiftX(dx) }
}
"#,
        );
        let ir = lower_fn(&program, "use_it");
        let tail = fn_tail(&ir);
        let IrExprKind::Call { callee, args, .. } = &tail.kind else {
            panic!("expected Call, got {:?}", tail.kind)
        };
        assert!(matches!(callee, Callee::Method(f) if f.name.display().ends_with("shiftX")));
        assert_eq!(args.len(), 2);
        assert!(matches!(&args[0].kind, IrExprKind::Local(n) if n == "p"));
        assert!(matches!(&args[1].kind, IrExprKind::Local(n) if n == "dx"));
    }

    #[test]
    fn static_call_driven_by_callee_static_has_no_prepended_receiver() {
        let program = checked_program(
            r#"
commons demo {
  type Point = { x: Int, y: Int }

  fn Point.origin() -> Point { Point { x: 0, y: 0 } }

  fn use_it() -> Point { Point.origin() }
}
"#,
        );
        let ir = lower_fn(&program, "use_it");
        let tail = fn_tail(&ir);
        let IrExprKind::Call { callee, args, .. } = &tail.kind else {
            panic!("expected Call, got {:?}", tail.kind)
        };
        assert!(matches!(callee, Callee::Static(f) if f.name.display().ends_with("origin")));
        // `Point` (the receiver) is a type name, not a value — must not be
        // lowered and prepended, unlike the Method case above.
        assert!(args.is_empty());
    }

    #[test]
    fn kernel_method_call_prepends_the_receiver() {
        let program = checked_program(
            r#"
commons demo {
  fn identity_all(xs: List[Int]) -> List[Int] { xs.map((y) => y) }
}
"#,
        );
        let ir = lower_fn(&program, "identity_all");
        let tail = fn_tail(&ir);
        let IrExprKind::Call { callee, args, .. } = &tail.kind else {
            panic!("expected Call, got {:?}", tail.kind)
        };
        assert!(matches!(callee, Callee::Kernel { op, .. } if op == "map"));
        assert_eq!(args.len(), 2);
        assert!(matches!(&args[0].kind, IrExprKind::Local(n) if n == "xs"));
        let IrExprKind::Lambda {
            params,
            body,
            captures,
        } = &args[1].kind
        else {
            panic!("expected Lambda, got {:?}", args[1].kind)
        };
        assert_eq!(params, &["y".to_string()]);
        assert!(captures.is_empty());
        assert!(matches!(&body.kind, IrExprKind::Local(n) if n == "y"));
    }

    // P6.3 (#1145): Implies/RecordSpread desugaring — the only two of the
    // reference's own Part 6.4 desugaring-table rows this slice covers
    // (Decision D); every other row named there stays a `todo!()` citing its
    // own specific blocker (Decisions A–C).

    #[test]
    fn implies_desugars_to_or_not() {
        let program = checked_program(
            r#"
commons demo {
  fn imp(p: Bool, q: Bool) -> Bool { p implies q }
}
"#,
        );
        let ir = lower_fn(&program, "imp");
        let tail = fn_tail(&ir);
        let IrExprKind::Or { lhs, rhs } = &tail.kind else {
            panic!("expected Or, got {:?}", tail.kind)
        };
        let IrExprKind::Not { operand } = &lhs.kind else {
            panic!("expected Or's lhs to be Not, got {:?}", lhs.kind)
        };
        assert!(matches!(&operand.kind, IrExprKind::Local(n) if n == "p"));
        assert!(matches!(&rhs.kind, IrExprKind::Local(n) if n == "q"));
        assert!(matches!(
            &*program.program().ty_intern.get(lhs.ty),
            Ty::Base(bynk_syntax::ast::BaseType::Bool)
        ));
    }

    // #1189: comparison/arithmetic `BinOp`, `UnaryOp::Neg`, `InterpStr` —
    // the gap P6.2/P6.3 each confirmed and left `todo!()`, closed here.

    #[test]
    fn comparison_and_arithmetic_binops_lower_to_a_shared_dedicated_node() {
        let program = checked_program(
            r#"
commons demo {
  fn nonneg(balance: Int) -> Bool { balance >= 0 }
  fn total(a: Int, b: Int) -> Int { a + b }
}
"#,
        );
        let cmp_ir = lower_fn(&program, "nonneg");
        let IrExprKind::BinOp { op, lhs, rhs } = &fn_tail(&cmp_ir).kind else {
            panic!("expected BinOp, got {:?}", fn_tail(&cmp_ir).kind)
        };
        assert_eq!(*op, IrBinOp::GtEq);
        assert!(matches!(&lhs.kind, IrExprKind::Local(n) if n == "balance"));
        assert!(matches!(&rhs.kind, IrExprKind::Const(ConstVal::Int(0))));
        assert!(matches!(
            &*program.program().ty_intern.get(fn_tail(&cmp_ir).ty),
            Ty::Base(bynk_syntax::ast::BaseType::Bool)
        ));

        let arith_ir = lower_fn(&program, "total");
        let IrExprKind::BinOp { op, lhs, rhs } = &fn_tail(&arith_ir).kind else {
            panic!("expected BinOp, got {:?}", fn_tail(&arith_ir).kind)
        };
        assert_eq!(*op, IrBinOp::Add);
        assert!(matches!(&lhs.kind, IrExprKind::Local(n) if n == "a"));
        assert!(matches!(&rhs.kind, IrExprKind::Local(n) if n == "b"));
        assert!(matches!(
            &*program.program().ty_intern.get(fn_tail(&arith_ir).ty),
            Ty::Base(bynk_syntax::ast::BaseType::Int)
        ));
    }

    #[test]
    fn unary_neg_lowers_to_its_own_dedicated_node() {
        let program = checked_program(
            r#"
commons demo {
  fn negate(x: Int) -> Int { -x }
}
"#,
        );
        let ir = lower_fn(&program, "negate");
        let IrExprKind::Neg { operand } = &fn_tail(&ir).kind else {
            panic!("expected Neg, got {:?}", fn_tail(&ir).kind)
        };
        assert!(matches!(&operand.kind, IrExprKind::Local(n) if n == "x"));
        assert!(matches!(
            &*program.program().ty_intern.get(fn_tail(&ir).ty),
            Ty::Base(bynk_syntax::ast::BaseType::Int)
        ));
    }

    #[test]
    fn every_binop_tag_maps_to_its_own_ir_binop_variant() {
        // The hand-written ten-arm `BinOp -> IrBinOp` mapping is the one
        // place this change can be silently wrong (a transposition compiles
        // and clippy-passes) — every tag gets its own assertion here, not
        // just the two `comparison_and_arithmetic_binops_lower_to_a_shared_
        // dedicated_node` happens to cover incidentally (review of #1195).
        let program = checked_program(
            r#"
commons demo {
  fn eq(a: Int, b: Int) -> Bool { a == b }
  fn neq(a: Int, b: Int) -> Bool { a != b }
  fn lt(a: Int, b: Int) -> Bool { a < b }
  fn lteq(a: Int, b: Int) -> Bool { a <= b }
  fn gt(a: Int, b: Int) -> Bool { a > b }
  fn gteq(a: Int, b: Int) -> Bool { a >= b }
  fn add(a: Int, b: Int) -> Int { a + b }
  fn sub(a: Int, b: Int) -> Int { a - b }
  fn mul(a: Int, b: Int) -> Int { a * b }
  fn div(a: Int, b: Int) -> Int { a / b }
}
"#,
        );
        let cases = [
            ("eq", IrBinOp::Eq),
            ("neq", IrBinOp::NotEq),
            ("lt", IrBinOp::Lt),
            ("lteq", IrBinOp::LtEq),
            ("gt", IrBinOp::Gt),
            ("gteq", IrBinOp::GtEq),
            ("add", IrBinOp::Add),
            ("sub", IrBinOp::Sub),
            ("mul", IrBinOp::Mul),
            ("div", IrBinOp::Div),
        ];
        for (fn_name, expected_op) in cases {
            let ir = lower_fn(&program, fn_name);
            let IrExprKind::BinOp { op, .. } = &fn_tail(&ir).kind else {
                panic!("{fn_name}: expected BinOp, got {:?}", fn_tail(&ir).kind)
            };
            assert_eq!(*op, expected_op, "{fn_name}: wrong IrBinOp tag");
        }
    }

    #[test]
    fn interp_str_lowers_chunks_verbatim_and_holes_as_ordinary_expressions() {
        let program = checked_program(
            r#"
commons demo {
  fn greet(name: String) -> String { "hi \(name)!" }
}
"#,
        );
        let ir = lower_fn(&program, "greet");
        let IrExprKind::InterpStr { parts } = &fn_tail(&ir).kind else {
            panic!("expected InterpStr, got {:?}", fn_tail(&ir).kind)
        };
        assert_eq!(parts.len(), 3);
        assert!(matches!(&parts[0], IrInterpPart::Chunk(s) if s == "hi "));
        let IrInterpPart::Hole(hole) = &parts[1] else {
            panic!("expected Hole, got {:?}", parts[1])
        };
        assert!(matches!(&hole.kind, IrExprKind::Local(n) if n == "name"));
        assert!(matches!(&parts[2], IrInterpPart::Chunk(s) if s == "!"));
        assert!(matches!(
            &*program.program().ty_intern.get(fn_tail(&ir).ty),
            Ty::Base(bynk_syntax::ast::BaseType::String)
        ));
    }

    #[test]
    fn interp_str_covers_leading_hole_hole_only_and_adjacent_holes() {
        // `split_interp` (`bynk-syntax/src/lexer.rs`) never pushes an empty
        // `Chunk` — a leading/trailing/adjacent hole has no empty-string
        // sibling segment either side of it. Pins that `lower_interp_part_ir`
        // sees exactly the segments the lexer produces, not a padded shape.
        let program = checked_program(
            r#"
commons demo {
  fn lead(name: String) -> String { "\(name) hi" }
  fn only(name: String) -> String { "\(name)" }
  fn adjacent(a: String, b: String) -> String { "\(a)\(b)" }
}
"#,
        );

        let lead = lower_fn(&program, "lead");
        let IrExprKind::InterpStr { parts } = &fn_tail(&lead).kind else {
            panic!("expected InterpStr, got {:?}", fn_tail(&lead).kind)
        };
        assert_eq!(parts.len(), 2, "no leading empty Chunk before the hole");
        assert!(matches!(&parts[0], IrInterpPart::Hole(h)
            if matches!(&h.kind, IrExprKind::Local(n) if n == "name")));
        assert!(matches!(&parts[1], IrInterpPart::Chunk(s) if s == " hi"));

        let only = lower_fn(&program, "only");
        let IrExprKind::InterpStr { parts } = &fn_tail(&only).kind else {
            panic!("expected InterpStr, got {:?}", fn_tail(&only).kind)
        };
        assert_eq!(
            parts.len(),
            1,
            "a hole-only string is just the one Hole segment"
        );
        assert!(matches!(&parts[0], IrInterpPart::Hole(_)));

        let adjacent = lower_fn(&program, "adjacent");
        let IrExprKind::InterpStr { parts } = &fn_tail(&adjacent).kind else {
            panic!("expected InterpStr, got {:?}", fn_tail(&adjacent).kind)
        };
        assert_eq!(parts.len(), 2, "no empty Chunk between adjacent holes");
        assert!(matches!(&parts[0], IrInterpPart::Hole(h)
            if matches!(&h.kind, IrExprKind::Local(n) if n == "a")));
        assert!(matches!(&parts[1], IrInterpPart::Hole(h)
            if matches!(&h.kind, IrExprKind::Local(n) if n == "b")));
    }

    #[test]
    fn record_spread_resolves_every_declared_field_override_and_spread_through() {
        let program = checked_program(
            r#"
commons demo {
  type Point = { x: Int, y: Int, z: Int }

  fn shift(p: Point, y: Int) -> Point { Point { ...p, x: 0, y } }
}
"#,
        );
        let ir = lower_fn(&program, "shift");
        let tail = fn_tail(&ir);
        let IrExprKind::Block {
            stmts,
            tail: block_tail,
        } = &tail.kind
        else {
            panic!("expected Block, got {:?}", tail.kind)
        };
        assert_eq!(stmts.len(), 1);
        let IrStmt::Let { local, value } = &stmts[0] else {
            panic!("expected Let, got {:?}", stmts[0])
        };
        assert_eq!(local, "__spread_base_0");
        assert!(matches!(&value.kind, IrExprKind::Local(n) if n == "p"));

        let IrExprKind::Record { def, fields } = &block_tail.kind else {
            panic!("expected Record, got {:?}", block_tail.kind)
        };
        assert_eq!(def.name.name, "Point");
        assert_eq!(fields.len(), 3);

        // `x: 0` — a full-form override.
        assert_eq!(fields[0].0, "x");
        assert!(matches!(
            &fields[0].1.kind,
            IrExprKind::Const(ConstVal::Int(0))
        ));

        // `y` — a shorthand override, reading the `y` parameter, not `p.y`.
        assert_eq!(fields[1].0, "y");
        assert!(matches!(&fields[1].1.kind, IrExprKind::Local(n) if n == "y"));

        // `z` — not overridden, spread through as a synthesised `tmp.z` read.
        assert_eq!(fields[2].0, "z");
        let IrExprKind::Field { base, field } = &fields[2].1.kind else {
            panic!("expected Field, got {:?}", fields[2].1.kind)
        };
        assert_eq!(field, "z");
        assert!(matches!(&base.kind, IrExprKind::Local(n) if n == "__spread_base_0"));
        assert!(matches!(
            &*program.program().ty_intern.get(fields[2].1.ty),
            Ty::Base(bynk_syntax::ast::BaseType::Int)
        ));
    }

    #[test]
    fn record_spread_orders_fields_by_evaluation_order_not_declaration_order() {
        // Overrides land in `fields` in *source* order, not the record's own
        // declared field order — `y`'s override is written before `x`'s, so
        // it must come first, since an override's value may be effectful
        // and a future reader walks `fields` left to right to reproduce
        // that ordering (see `lower_record_spread_ir`'s own doc comment).
        let program = checked_program(
            r#"
commons demo {
  type Point = { x: Int, y: Int, z: Int }

  fn shift(p: Point) -> Point { Point { ...p, y: 1, x: 2 } }
}
"#,
        );
        let ir = lower_fn(&program, "shift");
        let tail = fn_tail(&ir);
        let IrExprKind::Block {
            tail: block_tail, ..
        } = &tail.kind
        else {
            panic!("expected Block, got {:?}", tail.kind)
        };
        let IrExprKind::Record { fields, .. } = &block_tail.kind else {
            panic!("expected Record, got {:?}", block_tail.kind)
        };
        let names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["y", "x", "z"]);
    }

    #[test]
    fn record_spread_duplicate_override_keeps_the_last_value_and_still_runs_the_earlier_one() {
        // `check_record_spread` has no duplicate-name diagnostic, so `x` is
        // named twice here and type-checks — the resulting field must take
        // `x`'s *last* value (2), but `x`'s *first* value (1) must still
        // run, as a discarded statement, since it may have been effectful.
        let program = checked_program(
            r#"
commons demo {
  type Point = { x: Int, y: Int }

  fn shift(p: Point) -> Point { Point { ...p, x: 1, x: 2 } }
}
"#,
        );
        let ir = lower_fn(&program, "shift");
        let tail = fn_tail(&ir);
        let IrExprKind::Block {
            stmts,
            tail: block_tail,
        } = &tail.kind
        else {
            panic!("expected Block, got {:?}", tail.kind)
        };
        assert_eq!(stmts.len(), 2, "the base Let plus one discarded duplicate");
        let IrStmt::Expr { value: discarded } = &stmts[1] else {
            panic!(
                "expected the second stmt to be a discarded Expr, got {:?}",
                stmts[1]
            )
        };
        assert!(matches!(
            &discarded.kind,
            IrExprKind::Const(ConstVal::Int(1))
        ));

        let IrExprKind::Record { fields, .. } = &block_tail.kind else {
            panic!("expected Record, got {:?}", block_tail.kind)
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].0, "x");
        assert!(matches!(
            &fields[0].1.kind,
            IrExprKind::Const(ConstVal::Int(2))
        ));
    }

    #[test]
    fn record_spread_instantiates_a_generic_records_spread_through_field_type() {
        // `Point`-shaped fixtures above are all monomorphic, so
        // `instantiate_field_ty`'s own substitution (ADR 0183/v0.157) is a
        // no-op there — a generic record's spread-through field is the one
        // case that actually exercises it, since `item`'s declared type is
        // the record's own rigid `T`, only resolvable via `base_args`.
        let program = checked_program(
            r#"
commons demo {
  type Boxed[T] = { item: T, tag: String }

  fn retag(b: Boxed[Int]) -> Boxed[Int] { Boxed { ...b, tag: "x" } }
}
"#,
        );
        let ir = lower_fn(&program, "retag");
        let tail = fn_tail(&ir);
        let IrExprKind::Block {
            tail: block_tail, ..
        } = &tail.kind
        else {
            panic!("expected Block, got {:?}", tail.kind)
        };
        let IrExprKind::Record { fields, .. } = &block_tail.kind else {
            panic!("expected Record, got {:?}", block_tail.kind)
        };
        let (_, item) = fields
            .iter()
            .find(|(n, _)| n == "item")
            .expect("item field present");
        let IrExprKind::Field { .. } = &item.kind else {
            panic!(
                "expected item to spread through as Field, got {:?}",
                item.kind
            )
        };
        assert!(matches!(
            &*program.program().ty_intern.get(item.ty),
            Ty::Base(bynk_syntax::ast::BaseType::Int)
        ));
    }

    // ==== P6.4 (#1157): IrPat/IrArm/Exhaustive, tested standalone ====
    //
    // `lower_pattern_ir`/`lower_arm_ir`/`lower_exhaustive_ir` are exercised
    // here at their own standalone granularity — each fixture below digs its
    // own `match` straight out of a fixture fn's body tail and lowers it
    // through these three entry points directly, bypassing `lower_expr_ir`.
    // Since P6.5 (#1159), the same three are also reached through the
    // ordinary `lower_expr_ir`/`lower_fn_body_ir` path — see the `match_*`
    // tests below this section for that coverage.

    /// Every fixture below is a single-expression `fn` body whose tail is
    /// `ExprKind::Match` — no general-purpose statement walk is needed.
    fn find_match_arms(f: &FnDecl) -> (&Expr, &[MatchArm]) {
        let ExprKind::Match { discriminant, arms } = &f.body.tail.kind else {
            panic!(
                "fixture's fn body tail is not a Match, got {:?}",
                f.body.tail.kind
            )
        };
        (discriminant, arms)
    }

    /// Binds the fn's own params into scope first (mirroring
    /// `lower_fn_body_ir`'s own setup) so a guard/body that reads an outer
    /// param — not just a pattern binding — resolves too, then lowers every
    /// arm of the fixture's own top-level `match` plus its `Exhaustive`.
    fn lower_match_fixture(program: &CheckedProgram, fn_name: &str) -> (Vec<IrArm>, Exhaustive) {
        let f = find_fn(program, fn_name);
        let (discriminant, arms) = find_match_arms(f);
        let disc_ty = program
            .program()
            .expr_types
            .get(&discriminant.id)
            .unwrap_or_else(|| panic!("{fn_name}: discriminant has no recorded type"))
            .ty;
        let mut cx = LowerIrCtx::new(program, HashSet::new());
        for p in &f.params {
            let ty = cx.resolve_type_ref(&p.type_ref).unwrap_or_else(|| {
                panic!("{fn_name}: param `{}`'s type does not resolve", p.name.name)
            });
            cx.bind(p.name.name.clone(), ty);
        }
        let ir_arms: Vec<IrArm> = arms
            .iter()
            .map(|a| lower_arm_ir(a, disc_ty, &mut cx))
            .collect();
        let exhaustive = lower_exhaustive_ir(arms);
        (ir_arms, exhaustive)
    }

    #[test]
    fn pattern_ir_covers_wildcard_binding_and_literal_leaves() {
        let program = checked_program(
            r#"
commons demo {
  fn wildcard_case(n: Int) -> Int {
    match n {
      _ => 0
    }
  }

  fn binding_case(n: Int) -> Int {
    match n {
      m => m
    }
  }

  fn literal_case(n: Int) -> String {
    match n {
      0 => "zero"
      other => "other"
    }
  }
}
"#,
        );

        let (wild_arms, wild_exhaustive) = lower_match_fixture(&program, "wildcard_case");
        assert_eq!(wild_arms.len(), 1);
        assert!(matches!(&wild_arms[0].pat, IrPat::Wild));
        assert!(wild_arms[0].binds.is_empty());
        assert_eq!(wild_arms[0].binding_mode, BindingMode::Direct);
        assert!(matches!(wild_exhaustive, Exhaustive::Total));

        let (binding_arms, _) = lower_match_fixture(&program, "binding_case");
        assert!(matches!(&binding_arms[0].pat, IrPat::Bind { local } if local == "m"));
        assert_eq!(binding_arms[0].binds, vec!["m".to_string()]);
        assert!(matches!(&binding_arms[0].body.kind, IrExprKind::Local(n) if n == "m"));

        let (literal_arms, literal_exhaustive) = lower_match_fixture(&program, "literal_case");
        assert!(matches!(
            &literal_arms[0].pat,
            IrPat::Const {
                value: ConstVal::Int(0)
            }
        ));
        assert!(matches!(&literal_arms[1].pat, IrPat::Bind { local } if local == "other"));
        assert!(matches!(literal_exhaustive, Exhaustive::Total));
    }

    #[test]
    fn pattern_ir_variant_over_a_user_sum_resolves_tag_and_payload_fields() {
        let program = checked_program(
            r#"
commons demo {
  type Outcome =
    | Hit(score: Int)
    | Miss

  fn variant_user_sum(o: Outcome) -> Int {
    match o {
      Hit(score) => score
      Miss => 0
    }
  }
}
"#,
        );

        let (arms, exhaustive) = lower_match_fixture(&program, "variant_user_sum");
        assert_eq!(arms.len(), 2);

        let IrPat::Variant { tag, fields, .. } = &arms[0].pat else {
            panic!("expected Variant, got {:?}", arms[0].pat)
        };
        assert_eq!(tag, "Hit");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "score");
        assert!(matches!(&*fields[0].1, IrPat::Bind { local } if local == "score"));
        assert_eq!(arms[0].binds, vec!["score".to_string()]);
        assert!(matches!(&arms[0].body.kind, IrExprKind::Local(n) if n == "score"));

        let IrPat::Variant { tag, fields, .. } = &arms[1].pat else {
            panic!("expected Variant, got {:?}", arms[1].pat)
        };
        assert_eq!(tag, "Miss");
        assert!(fields.is_empty());
        assert!(arms[1].binds.is_empty());

        assert!(matches!(exhaustive, Exhaustive::Total));
    }

    #[test]
    fn pattern_ir_variant_over_result_and_option_resolves_via_variants_of() {
        let program = checked_program(
            r#"
commons demo {
  fn variant_result(r: Result[Int, String]) -> Int {
    match r {
      Ok(v) => v
      Err(_) => 0
    }
  }

  fn variant_option(o: Option[Int]) -> Int {
    match o {
      Some(v) => v
      None => 0
    }
  }
}
"#,
        );

        let (result_arms, _) = lower_match_fixture(&program, "variant_result");
        let IrPat::Variant { tag, fields, .. } = &result_arms[0].pat else {
            panic!("expected Variant, got {:?}", result_arms[0].pat)
        };
        assert_eq!(tag, "Ok");
        assert_eq!(fields[0].0, "value");
        assert!(matches!(&*fields[0].1, IrPat::Bind { local } if local == "v"));
        let IrPat::Variant { tag, fields, .. } = &result_arms[1].pat else {
            panic!("expected Variant, got {:?}", result_arms[1].pat)
        };
        assert_eq!(tag, "Err");
        assert_eq!(fields[0].0, "error");
        assert!(matches!(&*fields[0].1, IrPat::Wild));

        let (option_arms, _) = lower_match_fixture(&program, "variant_option");
        let IrPat::Variant { tag, fields, .. } = &option_arms[0].pat else {
            panic!("expected Variant, got {:?}", option_arms[0].pat)
        };
        assert_eq!(tag, "Some");
        assert_eq!(fields[0].0, "value");
        let IrPat::Variant { tag, fields, .. } = &option_arms[1].pat else {
            panic!("expected Variant, got {:?}", option_arms[1].pat)
        };
        assert_eq!(tag, "None");
        assert!(fields.is_empty());
    }

    #[test]
    fn pattern_ir_refined_wraps_the_inner_pattern_and_stays_direct_mode() {
        let program = checked_program(
            r#"
commons demo {
  fn refined_case(n: Int) -> Int {
    match n {
      _ where Positive => 1
      _ => 0
    }
  }
}
"#,
        );

        let (arms, exhaustive) = lower_match_fixture(&program, "refined_case");
        let IrPat::Refined { inner, refinement } = &arms[0].pat else {
            panic!("expected Refined, got {:?}", arms[0].pat)
        };
        assert!(matches!(&**inner, IrPat::Wild));
        assert_eq!(refinement.predicates.len(), 1);
        assert_eq!(refinement.predicates[0].kind.name(), "Positive");
        assert_eq!(arms[0].binding_mode, BindingMode::Direct);
        assert!(matches!(exhaustive, Exhaustive::Total));
    }

    #[test]
    fn pattern_ir_or_pattern_records_or_dispatch_binding_mode_and_shared_binds() {
        let program = checked_program(
            r#"
commons demo {
  type Outcome =
    | Hit(score: Int)
    | Boost(score: Int)
    | Miss

  fn or_case(o: Outcome) -> Int {
    match o {
      Hit(score) | Boost(score) => score
      Miss => 0
    }
  }
}
"#,
        );

        let (arms, exhaustive) = lower_match_fixture(&program, "or_case");
        let IrPat::Or { alts } = &arms[0].pat else {
            panic!("expected Or, got {:?}", arms[0].pat)
        };
        assert_eq!(alts.len(), 2);
        assert!(matches!(&alts[0], IrPat::Variant { tag, .. } if tag == "Hit"));
        assert!(matches!(&alts[1], IrPat::Variant { tag, .. } if tag == "Boost"));
        // The whole point of R5.5/Decision C — a name shared across
        // alternatives, at different structural paths, still surfaces as
        // one `binds` entry and flips the arm's own dispatch mode.
        assert_eq!(arms[0].binds, vec!["score".to_string()]);
        assert_eq!(arms[0].binding_mode, BindingMode::OrDispatch);
        assert!(matches!(&arms[0].body.kind, IrExprKind::Local(n) if n == "score"));

        // `Miss`'s own arm has no `Or` anywhere in its pattern — stays
        // `Direct`, proving the mode is per-arm, not a whole-match property.
        assert_eq!(arms[1].binding_mode, BindingMode::Direct);

        assert!(matches!(exhaustive, Exhaustive::Total));
    }

    #[test]
    fn pattern_ir_arm_guard_lowers_and_sees_the_patterns_own_bindings() {
        // R5.4's ordering claim (a guard must be able to read the pattern's
        // own bound names) needs a guard that is itself an expression this
        // pass can lower. A `Bool` payload field read straight back as the
        // guard exercises the exact same scope-ordering property with the
        // simplest possible guard expression — no need for a comparison
        // (real as of #1189) to pin this claim.
        let program = checked_program(
            r#"
commons demo {
  type Outcome =
    | Hit(flag: Bool)
    | Miss

  fn guarded(o: Outcome) -> Int {
    match o {
      Hit(flag) if flag => 1
      _ => 0
    }
  }
}
"#,
        );

        let (arms, exhaustive) = lower_match_fixture(&program, "guarded");
        let guard = arms[0]
            .guard
            .as_ref()
            .unwrap_or_else(|| panic!("expected a lowered guard, got {:?}", arms[0].guard));
        assert!(matches!(&guard.kind, IrExprKind::Local(n) if n == "flag"));
        assert!(matches!(exhaustive, Exhaustive::Total));
    }

    #[test]
    fn pattern_ir_named_bindings_resolve_by_field_name_not_position() {
        let program = checked_program(
            r#"
commons demo {
  type Pair =
    | Pair(a: Int, b: String)
    | Empty

  fn named_reordered(p: Pair) -> String {
    match p {
      Pair(b: s, a: n) => s
      Empty => "none"
    }
  }

  fn named_subset(p: Pair) -> Int {
    match p {
      Pair(a: n) => n
      Empty => 0
    }
  }
}
"#,
        );

        // Out-of-order named bindings: `b` (String) is written first in
        // source order, `a` (Int) second — an index-keyed lookup would bind
        // `s` against `a`'s own `Int` field instead of `b`'s `String` one.
        let (reordered_arms, _) = lower_match_fixture(&program, "named_reordered");
        let IrPat::Variant { fields, .. } = &reordered_arms[0].pat else {
            panic!("expected Variant, got {:?}", reordered_arms[0].pat)
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].0, "b");
        assert!(matches!(&*fields[0].1, IrPat::Bind { local } if local == "s"));
        assert_eq!(fields[1].0, "a");
        assert!(matches!(&*fields[1].1, IrPat::Bind { local } if local == "n"));

        // The named form's documented strict-subset case: `Pair(a: n)`
        // binds only `a`, leaving `b` entirely out of `fields` — not padded
        // with a synthesised wildcard for the field it doesn't name.
        let (subset_arms, _) = lower_match_fixture(&program, "named_subset");
        let IrPat::Variant { fields, .. } = &subset_arms[0].pat else {
            panic!("expected Variant, got {:?}", subset_arms[0].pat)
        };
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "a");
        assert!(matches!(&*fields[0].1, IrPat::Bind { local } if local == "n"));
    }

    #[test]
    fn pattern_ir_nested_variant_pattern_threads_a_derived_field_ty() {
        // The one shape where the recursive `lower_pattern_ir` call re-enters
        // `variant_info_of` against a *derived* `field_ty` (`Ok`'s own
        // `value` field, `Option[Int]`) rather than the top-level scrutinee
        // — the substance of Decision A, otherwise only exercised to depth 1.
        let program = checked_program(
            r#"
commons demo {
  fn nested_variant(x: Result[Option[Int], String]) -> Int {
    match x {
      Ok(Some(v)) => v
      Ok(None) => 0
      Err(_) => 0
    }
  }
}
"#,
        );

        let (arms, exhaustive) = lower_match_fixture(&program, "nested_variant");
        let IrPat::Variant {
            tag: outer_tag,
            fields: outer_fields,
            ..
        } = &arms[0].pat
        else {
            panic!("expected Variant, got {:?}", arms[0].pat)
        };
        assert_eq!(outer_tag, "Ok");
        assert_eq!(outer_fields[0].0, "value");
        let IrPat::Variant {
            tag: inner_tag,
            fields: inner_fields,
            ..
        } = &*outer_fields[0].1
        else {
            panic!("expected a nested Variant, got {:?}", outer_fields[0].1)
        };
        assert_eq!(inner_tag, "Some");
        assert!(matches!(&*inner_fields[0].1, IrPat::Bind { local } if local == "v"));
        assert!(matches!(&arms[0].body.kind, IrExprKind::Local(n) if n == "v"));

        assert!(matches!(exhaustive, Exhaustive::Total));
    }

    #[test]
    #[should_panic(expected = "always has at least one unguarded arm")]
    fn exhaustive_ir_panics_if_every_arm_is_guarded() {
        // Never reachable through a real certified program — the checker's
        // own `missing_patterns` gate (`bynk.types.non_exhaustive_match`)
        // already rejects an all-guarded arm list as non-exhaustive before
        // `certify` ever produces a `CheckedProgram` (finding 3, #1157).
        // Hand-built here, bypassing `checked_program` entirely, to prove
        // the ADR 0334-style discipline Decision B commissions is real, not
        // decorative.
        let dummy_expr = Expr {
            id: ExprId(0),
            kind: ExprKind::BoolLit(true),
            span: Span::default(),
        };
        let arm = MatchArm {
            pattern: Pattern::Wildcard(Span::default()),
            guard: Some(dummy_expr.clone()),
            body: MatchBody::Expr(dummy_expr),
            span: Span::default(),
        };
        let _ = lower_exhaustive_ir(std::slice::from_ref(&arm));
    }

    // ==== P6.5 (#1159): Match wired to a real consumer ====
    //
    // Unlike the P6.4 section above, every fixture below goes through the
    // ordinary `lower_expr_ir`/`lower_fn_body_ir` path — no digging a
    // fixture's own `match` out by hand and calling `lower_arm_ir`/
    // `lower_exhaustive_ir` directly.

    #[test]
    fn match_through_lower_expr_ir_builds_a_real_match_node_with_flat_form() {
        // Also this slice's own end-to-end coverage of `Exhaustive::Total`
        // reachable through `lower_fn_body_ir` — the P6.4 section above only
        // ever reaches it through the standalone `lower_exhaustive_ir` entry
        // point, not the real `ExprKind::Match` arm this slice wires up.
        let program = checked_program(
            r#"
commons demo {
  type Outcome =
    | Hit(score: Int)
    | Miss

  fn describe(o: Outcome) -> Int {
    match o {
      Hit(score) => score
      Miss => 0
    }
  }
}
"#,
        );
        let ir = lower_fn(&program, "describe");
        let tail = fn_tail(&ir);
        let IrExprKind::Match {
            scrutinee,
            arms,
            exhaustive,
            form,
        } = &tail.kind
        else {
            panic!("expected Match, got {:?}", tail.kind)
        };
        assert!(matches!(&scrutinee.kind, IrExprKind::Local(n) if n == "o"));
        assert_eq!(arms.len(), 2);
        assert!(matches!(&arms[0].pat, IrPat::Variant { tag, .. } if tag == "Hit"));
        assert!(matches!(&arms[0].body.kind, IrExprKind::Local(n) if n == "score"));
        assert!(matches!(&arms[1].pat, IrPat::Variant { tag, .. } if tag == "Miss"));
        assert!(matches!(exhaustive, Exhaustive::Total));
        assert_eq!(*form, MatchForm::Flat);
    }

    #[test]
    fn match_with_a_guarded_arm_gets_if_chain_form() {
        let program = checked_program(
            r#"
commons demo {
  type Outcome =
    | Hit(flag: Bool)
    | Miss

  fn describe(o: Outcome) -> Int {
    match o {
      Hit(flag) if flag => 1
      _ => 0
    }
  }
}
"#,
        );
        let ir = lower_fn(&program, "describe");
        let tail = fn_tail(&ir);
        let IrExprKind::Match { arms, form, .. } = &tail.kind else {
            panic!("expected Match, got {:?}", tail.kind)
        };
        assert_eq!(*form, MatchForm::IfChain);
        // Not just `form` — the guard itself must actually lower, and
        // (R5.4 ordering) must see the pattern's own bound name `flag`
        // already in scope, resolving as a `Local` rather than falling
        // through to `lower_ident_ir`'s `todo!()`. `lower_arm_ir` already
        // covers this standalone (`pattern_ir_arm_guard_lowers_and_sees_the_
        // patterns_own_bindings`); this pins the same property reached
        // through the real `ExprKind::Match` arm instead.
        let guard = arms[0]
            .guard
            .as_ref()
            .unwrap_or_else(|| panic!("expected a lowered guard, got {:?}", arms[0].guard));
        assert!(matches!(&guard.kind, IrExprKind::Local(n) if n == "flag"));
    }

    #[test]
    fn match_with_a_nested_refutable_payload_pattern_gets_if_chain_form() {
        let program = checked_program(
            r#"
commons demo {
  fn describe(x: Result[Option[Int], String]) -> Int {
    match x {
      Ok(Some(v)) => v
      Ok(None) => 0
      Err(_) => 0
    }
  }
}
"#,
        );
        let ir = lower_fn(&program, "describe");
        let tail = fn_tail(&ir);
        let IrExprKind::Match { form, .. } = &tail.kind else {
            panic!("expected Match, got {:?}", tail.kind)
        };
        assert_eq!(*form, MatchForm::IfChain);
    }

    #[test]
    fn match_with_a_refined_arm_gets_if_chain_form() {
        // The third of `match_needs_if_chain`'s three disjuncts
        // (`Pattern::Refined`), untested through `lower_expr_ir` by the two
        // tests above — also pins the IR counterpart this same slice builds
        // for a `where`-refined arm (`IrPat::Refined`), so `form` and the
        // pattern shape are checked together rather than in isolation.
        let program = checked_program(
            r#"
commons demo {
  fn describe(n: Int) -> Int {
    match n {
      _ where Positive => 1
      _ => 0
    }
  }
}
"#,
        );
        let ir = lower_fn(&program, "describe");
        let tail = fn_tail(&ir);
        let IrExprKind::Match { arms, form, .. } = &tail.kind else {
            panic!("expected Match, got {:?}", tail.kind)
        };
        assert!(matches!(&arms[0].pat, IrPat::Refined { .. }));
        assert_eq!(*form, MatchForm::IfChain);
    }

    #[test]
    fn match_with_a_bindingless_or_pattern_stays_flat() {
        // The counter-intuitive half of `pattern_has_nested_test`'s own
        // `Pattern::Or` rule: a bindingless or-pattern (`Hit | Boost`) stays
        // `Flat` — only an or-pattern *with* bindings, or one with a nested
        // refutable payload, needs the if-chain. A distinct axis from
        // `IrArm::binding_mode` (`OrDispatch` the moment `IrPat::Or` occurs
        // anywhere, bindings or not, R5.5 — see `ir_pat_contains_or`'s own
        // doc comment), so no assertion on `binding_mode` here. The
        // cheapest way to catch a future "any `Or` => if-chain" regression,
        // in either the emitter predicate or a re-derivation of it, is
        // pinning the case that would silently flip first.
        let program = checked_program(
            r#"
commons demo {
  type Outcome =
    | Hit
    | Boost
    | Miss

  fn describe(o: Outcome) -> Int {
    match o {
      Hit | Boost => 1
      Miss => 0
    }
  }
}
"#,
        );
        let ir = lower_fn(&program, "describe");
        let tail = fn_tail(&ir);
        let IrExprKind::Match { arms, form, .. } = &tail.kind else {
            panic!("expected Match, got {:?}", tail.kind)
        };
        assert!(matches!(&arms[0].pat, IrPat::Or { .. }));
        assert_eq!(*form, MatchForm::Flat);
    }

    #[test]
    fn match_construction_is_position_agnostic_tail_vs_nested() {
        // Mirrors `if_lowers_both_branches_as_blocks_not_return_wrapped`'s
        // own precedent: `Match`'s own scrutinee/arms/exhaustive/form don't
        // depend on whether the match sits in a fn body's own tail (wrapped
        // in `Return` by `lower_fn_body_ir`) or nested inside another
        // expression (here, a `let`'s own value) — the same source-level
        // match lowers to the same shape either way, since `MatchForm`
        // deliberately doesn't record a position axis (Decision A).
        let program = checked_program(
            r#"
commons demo {
  type Outcome =
    | Hit(score: Int)
    | Miss

  fn tail_position(o: Outcome) -> Int {
    match o {
      Hit(score) => score
      Miss => 0
    }
  }

  fn nested_position(o: Outcome) -> Int {
    let result = match o {
      Hit(score) => score
      Miss => 0
    }
    result
  }
}
"#,
        );

        let tail_ir = lower_fn(&program, "tail_position");
        let tail = fn_tail(&tail_ir);
        let IrExprKind::Match {
            arms: tail_arms,
            exhaustive: tail_exhaustive,
            form: tail_form,
            ..
        } = &tail.kind
        else {
            panic!("tail_position: expected Match, got {:?}", tail.kind)
        };

        let nested_ir = lower_fn(&program, "nested_position");
        let IrExprKind::Block { stmts, .. } = &nested_ir.kind else {
            panic!(
                "lower_fn_body_ir always returns IrExprKind::Block, got {:?}",
                nested_ir.kind
            )
        };
        let IrStmt::Let {
            value: nested_match,
            ..
        } = &stmts[0]
        else {
            panic!("nested_position: expected a Let statement, got {stmts:?}")
        };
        let IrExprKind::Match {
            arms: nested_arms,
            exhaustive: nested_exhaustive,
            form: nested_form,
            ..
        } = &nested_match.kind
        else {
            panic!(
                "nested_position: expected Match, got {:?}",
                nested_match.kind
            )
        };

        fn tags(arms: &[IrArm]) -> Vec<&str> {
            arms.iter()
                .map(|a| match &a.pat {
                    IrPat::Variant { tag, .. } => tag.as_str(),
                    other => panic!("expected a Variant pattern, got {other:?}"),
                })
                .collect()
        }
        assert_eq!(tags(tail_arms), tags(nested_arms));
        assert_eq!(*tail_form, *nested_form);
        assert!(matches!(tail_exhaustive, Exhaustive::Total));
        assert!(matches!(nested_exhaustive, Exhaustive::Total));
    }

    #[test]
    fn type_item_record_resolves_fields_and_generic_rigid_vars() {
        let program = checked_program(
            r#"
commons demo {
  type Box[T] = { value: T }
}
"#,
        );
        let decl = find_type(&program, "Box");
        let item = lower_type_item_ir(decl, &program);
        let IrItem::Type { def, shape } = &item else {
            panic!("expected IrItem::Type, got {item:?}")
        };
        assert_eq!(def.name.name, "Box");
        let TypeShape::Record { fields } = shape else {
            panic!("expected TypeShape::Record, got {shape:?}")
        };
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "value");
        assert!(matches!(
            &*program.program().ty_intern.get(fields[0].1),
            Ty::Var(name) if name == "T"
        ));
    }

    #[test]
    fn type_item_sum_resolves_variant_payloads_and_embeds() {
        let program = checked_program(
            r#"
commons demo {
  type PaymentError = enum { Declined, InsufficientFunds }

  type OrderError =
    | OutOfStock(sku: String, qty: Int)
    | Payment(reason: PaymentError)
    embeds PaymentError as Payment
}
"#,
        );
        let decl = find_type(&program, "OrderError");
        let item = lower_type_item_ir(decl, &program);
        let IrItem::Type { shape, .. } = &item else {
            panic!("expected IrItem::Type, got {item:?}")
        };
        let TypeShape::Sum { variants, embeds } = shape else {
            panic!("expected TypeShape::Sum, got {shape:?}")
        };
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].0, "OutOfStock");
        assert_eq!(variants[0].1.len(), 2);
        assert_eq!(variants[0].1[0].0, "sku");
        assert!(matches!(
            &*program.program().ty_intern.get(variants[0].1[0].1),
            Ty::Base(bynk_syntax::ast::BaseType::String)
        ));
        assert_eq!(variants[0].1[1].0, "qty");
        assert!(matches!(
            &*program.program().ty_intern.get(variants[0].1[1].1),
            Ty::Base(bynk_syntax::ast::BaseType::Int)
        ));
        assert_eq!(variants[1].0, "Payment");
        assert_eq!(variants[1].1.len(), 1);
        assert_eq!(variants[1].1[0].0, "reason");

        assert_eq!(embeds.len(), 1);
        let (source, tag) = &embeds[0];
        assert_eq!(tag, "Payment");
        let Ty::Named { name, .. } = &*program.program().ty_intern.get(*source) else {
            panic!("expected embeds source to resolve to a named type")
        };
        assert_eq!(name, "PaymentError");
    }

    #[test]
    fn type_item_refined_and_opaque_cover_bare_and_predicated_and_opaque_forms() {
        let program = checked_program(
            r#"
commons demo {
  type Age = Int where Positive
  type UserId = opaque Int
  type Bare = Int
}
"#,
        );
        let age = find_type(&program, "Age");
        let IrItem::Type {
            shape: age_shape, ..
        } = lower_type_item_ir(age, &program)
        else {
            unreachable!()
        };
        let TypeShape::Refined {
            base,
            refinement,
            opaque,
        } = age_shape
        else {
            panic!("expected TypeShape::Refined for Age")
        };
        assert_eq!(base, bynk_syntax::ast::BaseType::Int);
        assert!(refinement.is_some());
        assert!(!opaque);

        let user_id = find_type(&program, "UserId");
        let IrItem::Type {
            shape: user_id_shape,
            ..
        } = lower_type_item_ir(user_id, &program)
        else {
            unreachable!()
        };
        let TypeShape::Refined {
            refinement, opaque, ..
        } = user_id_shape
        else {
            panic!("expected TypeShape::Refined for UserId")
        };
        assert!(refinement.is_none());
        assert!(opaque);

        let bare = find_type(&program, "Bare");
        let IrItem::Type {
            shape: bare_shape, ..
        } = lower_type_item_ir(bare, &program)
        else {
            unreachable!()
        };
        let TypeShape::Refined {
            refinement, opaque, ..
        } = bare_shape
        else {
            panic!("expected TypeShape::Refined for Bare")
        };
        assert!(refinement.is_none());
        assert!(!opaque);
    }

    #[test]
    fn fn_item_covers_effectful_and_pure_and_generic_and_method() {
        let program = checked_program(
            r#"
commons demo {
  type Box[A] = { value: A }

  fn Box.get(self) -> A { self.value }

  fn Box.fetch(self) -> Effect[A] { Effect.pure(self.value) }

  fn two_params(a: Int, b: Int) -> Int { a }

  fn identity[T](x: T) -> T { x }

  fn fetch() -> Effect[Int] { Effect.pure(1) }
}
"#,
        );

        let two_params = find_fn_arc(&program, "two_params");
        let IrItem::Fn {
            def,
            receiver,
            params,
            ret,
            effectful,
            ..
        } = lower_fn_item_ir(two_params, &program)
        else {
            unreachable!()
        };
        assert!(
            Arc::ptr_eq(&def, two_params),
            "def should reuse the program's own Arc, not a fresh clone"
        );
        assert_eq!(def.name.display(), "two_params");
        assert!(receiver.is_none(), "a free function has no receiver");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].0, "a");
        assert_eq!(params[1].0, "b");
        assert!(matches!(
            &*program.program().ty_intern.get(ret),
            Ty::Base(bynk_syntax::ast::BaseType::Int)
        ));
        assert!(!effectful);

        let fetch = find_fn_arc(&program, "fetch");
        let IrItem::Fn { effectful, .. } = lower_fn_item_ir(fetch, &program) else {
            unreachable!()
        };
        assert!(effectful);

        let identity = find_fn_arc(&program, "identity");
        let IrItem::Fn { params, ret, .. } = lower_fn_item_ir(identity, &program) else {
            unreachable!()
        };
        assert!(matches!(
            &*program.program().ty_intern.get(params[0].1),
            Ty::Var(n) if n == "T"
        ));
        assert!(matches!(
            &*program.program().ty_intern.get(ret),
            Ty::Var(n) if n == "T"
        ));

        let method = find_fn_arc(&program, "Box.get");
        let IrItem::Fn {
            def,
            receiver,
            params,
            ret,
            effectful,
            ..
        } = lower_fn_item_ir(method, &program)
        else {
            unreachable!()
        };
        assert!(
            Arc::ptr_eq(&def, method),
            "def should reuse the program's own Arc, not a fresh clone"
        );
        assert_eq!(def.name.display(), "Box.get");
        assert!(params.is_empty(), "self is not a param — see receiver");
        let Some(receiver_ty) = receiver else {
            panic!("expected Box.get to carry a receiver")
        };
        let Ty::Named { name, args, .. } = &*program.program().ty_intern.get(receiver_ty) else {
            panic!("expected the receiver to resolve to a named type")
        };
        assert_eq!(name, "Box");
        assert_eq!(args.len(), 1);
        assert!(matches!(
            &*program.program().ty_intern.get(args[0]),
            Ty::Var(n) if n == "A"
        ));
        assert!(matches!(
            &*program.program().ty_intern.get(ret),
            Ty::Var(n) if n == "A"
        ));
        assert!(!effectful);

        let effectful_method = find_fn_arc(&program, "Box.fetch");
        let IrItem::Fn {
            receiver,
            effectful,
            ..
        } = lower_fn_item_ir(effectful_method, &program)
        else {
            unreachable!()
        };
        assert!(
            receiver.is_some(),
            "Box.fetch is also a method with a receiver"
        );
        assert!(effectful, "Effect[A] return makes Box.fetch effectful");
    }

    #[test]
    fn type_item_sum_covers_a_payload_less_variant() {
        let program = checked_program(
            r#"
commons demo {
  type PaymentError = enum { Declined, InsufficientFunds }
}
"#,
        );
        let decl = find_type(&program, "PaymentError");
        let item = lower_type_item_ir(decl, &program);
        let IrItem::Type { shape, .. } = &item else {
            panic!("expected IrItem::Type, got {item:?}")
        };
        let TypeShape::Sum { variants, embeds } = shape else {
            panic!("expected TypeShape::Sum, got {shape:?}")
        };
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].0, "Declined");
        assert!(
            variants[0].1.is_empty(),
            "a bare variant carries no payload"
        );
        assert_eq!(variants[1].0, "InsufficientFunds");
        assert!(variants[1].1.is_empty());
        assert!(embeds.is_empty());
    }

    #[test]
    fn type_item_record_drops_a_fields_own_inline_refinement() {
        // Decision B extension, `ir.rs`'s own `TypeShape::Record` doc
        // comment: a field's inline `where` clause is a construction-time
        // constraint the checker already enforces, not part of the emitted
        // shape — pin that the field still lowers to `(name, ty)` with the
        // refinement silently absent, not that lowering rejects it.
        let program = checked_program(
            r#"
commons demo {
  type Account = { balance: Int where NonNegative }
}
"#,
        );
        let decl = find_type(&program, "Account");
        let item = lower_type_item_ir(decl, &program);
        let IrItem::Type { shape, .. } = &item else {
            panic!("expected IrItem::Type, got {item:?}")
        };
        let TypeShape::Record { fields } = shape else {
            panic!("expected TypeShape::Record, got {shape:?}")
        };
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "balance");
        assert!(matches!(
            &*program.program().ty_intern.get(fields[0].1),
            Ty::Base(bynk_syntax::ast::BaseType::Int)
        ));
    }

    #[test]
    fn type_item_record_resolves_a_generic_type_application_field() {
        // The `TypeRef::App` arm of `resolve_type_ref_in` — the one arm
        // that returns `None` for an unknown/unapplied name, and so the one
        // most likely to silently hit this pass's own ADR 0334 panic if the
        // field's resolution were ever wired up wrong.
        let program = checked_program(
            r#"
commons demo {
  type Box[T] = { value: T }
  type Wrapper = { boxed: Box[Int] }
}
"#,
        );
        let decl = find_type(&program, "Wrapper");
        let item = lower_type_item_ir(decl, &program);
        let IrItem::Type { shape, .. } = &item else {
            panic!("expected IrItem::Type, got {item:?}")
        };
        let TypeShape::Record { fields } = shape else {
            panic!("expected TypeShape::Record, got {shape:?}")
        };
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "boxed");
        let Ty::Named { name, args, .. } = &*program.program().ty_intern.get(fields[0].1) else {
            panic!("expected `boxed` to resolve to a named type")
        };
        assert_eq!(name, "Box");
        assert_eq!(args.len(), 1);
        assert!(matches!(
            &*program.program().ty_intern.get(args[0]),
            Ty::Base(bynk_syntax::ast::BaseType::Int)
        ));
    }

    /// Like [`checked_program`], but for source that declares an `agent`
    /// and/or a `service` — both are only legal inside a `context`, not a
    /// bare `commons` (`bynk.agent.outside_context`/the service
    /// equivalent), so `source` is parsed as a context unit and its items
    /// re-wrapped into a [`Commons`] value before re-using the same
    /// `resolve`/`check` pipeline `checked_program` does.
    /// `resolver::resolve`/`checker::check` both already treat
    /// `CommonsItem::Agent`/`Service` as inert (v0.5 declaration kinds "go
    /// through the context-level v0.5 path", `resolver.rs`'s own comment)
    /// — real agent/service checking (`store` field kinds, handler bodies,
    /// `expr_types` for a `Cell` initialiser, actor bindings) only happens
    /// via `context_checks::check_context_declarations`, called here by
    /// hand with a [`symbols::UnitTable`] built directly from the checked
    /// commons' own agent/service/actor and local `capability` items (a
    /// handler's `given <Cap>` resolves against `table.capabilities` —
    /// populated here so a fixture can declare its own local capability,
    /// but still no cross-context `uses`/`consumes` in any fixture this
    /// helper is given, so `resolver::CrossContextInfo::default()` is
    /// exact, not an approximation — in particular, no `from Events(E)`
    /// fixture is possible here, since a real subscription needs
    /// `consumes bynk { Events }`).
    ///
    /// **P6.11 (#1171) adds `services`/`actors` to the table and a
    /// pre-`resolve` `inject_service_defaults` pass.** `table.actors`
    /// matters even for a fixture using only the prelude (`Caller`,
    /// `Visitor`): `actor_identity_ty` resolves a *local* `actor`
    /// declaration through `table.actors` and silently falls through to
    /// the prelude/`Ty::Unit` otherwise
    /// (`context_checks.rs::actor_identity_ty`), so a `by u: Buyer`
    /// fixture without this would assert against a wrong `TyId` with no
    /// error at all. `inject_service_defaults` stands in for
    /// `bynk-check/src/analysis.rs`'s own pipeline-phase-2b call — this
    /// reduced harness has no such phase — so a fixture relying on a
    /// service-level `by`/`given` default (rather than declaring one per
    /// handler) would otherwise silently see it un-inherited, pinning the
    /// wrong fact with no failure to signal it.
    fn checked_context_program(source: &str) -> CheckedProgram {
        let tokens = lexer::tokenize(source).expect("lex");
        let unit = parser::parse_unit(&tokens, source).expect("parse");
        let SourceUnit::Context(mut ctx) = unit else {
            panic!("expected a context unit, got {unit:?}")
        };
        for item in &mut ctx.items {
            if let CommonsItem::Service(svc) = item {
                bynk_check::project_model::inject_service_defaults(svc);
            }
        }
        let commons = Commons {
            name: ctx.name,
            items: ctx.items,
            uses: ctx.uses,
            documentation: ctx.documentation,
            form: ctx.form,
            span: ctx.span,
            trivia: ctx.trivia,
            trailing_comments: ctx.trailing_comments,
        };
        let resolved = resolver::resolve(commons).expect("resolve");
        let mut typed = checker::check(resolved).expect("check");
        let agents: HashMap<String, AgentDecl> = typed
            .commons
            .items
            .iter()
            .filter_map(|item| match item {
                CommonsItem::Agent(a) => Some((a.name.name.clone(), a.clone())),
                _ => None,
            })
            .collect();
        let services: HashMap<String, ServiceDecl> = typed
            .commons
            .items
            .iter()
            .filter_map(|item| match item {
                CommonsItem::Service(s) => Some((s.name.name.clone(), s.clone())),
                _ => None,
            })
            .collect();
        let actors: HashMap<String, bynk_syntax::ast::ActorDecl> = typed
            .commons
            .items
            .iter()
            .filter_map(|item| match item {
                CommonsItem::Actor(a) => Some((a.name.name.clone(), a.clone())),
                _ => None,
            })
            .collect();
        // A `given <Cap>` clause on a handler resolves against
        // `table.capabilities` (`context_checks.rs`'s own `capability_info_map`
        // construction) — populated here from this fixture's own local
        // `capability` declarations so a handler can legitimately declare one
        // (e.g. `Log.append`'s own `given Clock` requirement), the same
        // "no cross-context uses/consumes" scope this helper's own doc
        // comment already names for `agents`/`types`.
        let capabilities: HashMap<String, bynk_syntax::ast::CapabilityDecl> = typed
            .commons
            .items
            .iter()
            .filter_map(|item| match item {
                CommonsItem::Capability(c) => Some((c.name.name.clone(), c.clone())),
                _ => None,
            })
            .collect();
        // P6.14 (#1174): `check_provider_decls` (the pass that actually
        // type-checks a `ProviderOp`'s own body via `check_handler_body`)
        // reads `table.providers`, keyed by capability name — same "one
        // provider per capability in v0.5" convention
        // `symbols::UnitTable::providers`'s own doc comment names. Populated
        // here for the same reason `capabilities` is (above): without it, a
        // fixture's own `provides` declaration is silently never checked at
        // all, not merely under-checked (feedback memory
        // "bynk-emit test harness scope").
        let providers: HashMap<String, ProviderDecl> = typed
            .commons
            .items
            .iter()
            .filter_map(|item| match item {
                CommonsItem::Provider(p) => Some((p.capability.name.clone(), p.clone())),
                _ => None,
            })
            .collect();
        let table = symbols::UnitTable {
            kind: Some(UnitKind::Context),
            types: typed.types.clone(),
            agents,
            services,
            actors,
            capabilities,
            providers,
            ..symbols::UnitTable::default()
        };
        let tys = typed.ty_intern.clone();
        let errors = context_checks::check_context_declarations(
            &mut typed,
            &table,
            &resolver::CrossContextInfo::default(),
            true,
            &HashSet::new(),
            &HashMap::new(),
            &mut RefSink::new(),
            &mut HintSink::new(),
            &mut LocalsSink::new(),
            &mut RequirementSink::new(),
            &tys,
        );
        checker::certify(typed, errors).expect("certify")
    }

    fn find_agent<'a>(program: &'a CheckedProgram, name: &str) -> &'a AgentDecl {
        program
            .program()
            .commons
            .items
            .iter()
            .find_map(|item| match item {
                CommonsItem::Agent(a) if a.name.name == name => Some(a),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no agent named `{name}` in this fixture"))
    }

    /// `lower_actor_seam_ir`'s own `actors` map — rebuilt the same way
    /// `checked_context_program` builds its own throwaway `table.actors`
    /// (not itself exposed on `CheckedProgram`), since the resolvers it
    /// wraps take the same `HashMap<String, ActorDecl>` shape the real
    /// `table.actors`/`ctx.actors` emitter-side callers already carry.
    fn actors_map(program: &CheckedProgram) -> HashMap<String, ActorDecl> {
        program
            .program()
            .commons
            .items
            .iter()
            .filter_map(|item| match item {
                CommonsItem::Actor(a) => Some((a.name.name.clone(), a.clone())),
                _ => None,
            })
            .collect()
    }

    fn find_service<'a>(program: &'a CheckedProgram, name: &str) -> &'a ServiceDecl {
        program
            .program()
            .commons
            .items
            .iter()
            .find_map(|item| match item {
                CommonsItem::Service(s) if s.name.name == name => Some(s),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no service named `{name}` in this fixture"))
    }

    fn find_capability<'a>(program: &'a CheckedProgram, name: &str) -> &'a CapabilityDecl {
        program
            .program()
            .commons
            .items
            .iter()
            .find_map(|item| match item {
                CommonsItem::Capability(c) if c.name.name == name => Some(c),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no capability named `{name}` in this fixture"))
    }

    fn find_provider<'a>(program: &'a CheckedProgram, name: &str) -> &'a ProviderDecl {
        program
            .program()
            .commons
            .items
            .iter()
            .find_map(|item| match item {
                CommonsItem::Provider(p) if p.provider_name.name == name => Some(p),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no provider named `{name}` in this fixture"))
    }

    /// `find_handler` (below) matches on `method_name`, which is always
    /// `None` for a service handler — useless here. Matches on
    /// `HandlerKind` equality instead; not a unique identity on its own (a
    /// service may declare several handlers sharing one `HandlerKind`, all
    /// `on call`), so a fixture with more than one same-kind handler must
    /// index `service.handlers`/the lowered `handlers` slice directly
    /// instead of calling this twice.
    fn find_service_handler<'a>(service: &'a ServiceDecl, kind: &HandlerKind) -> &'a Handler {
        service
            .handlers
            .iter()
            .find(|h| &h.kind == kind)
            .unwrap_or_else(|| {
                panic!(
                    "no handler of kind {kind:?} on service `{}`",
                    service.name.name
                )
            })
    }

    fn find_store_field<'a>(agent: &'a AgentDecl, name: &str) -> &'a StoreField {
        agent
            .store_fields
            .iter()
            .find(|f| f.name.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "no store field named `{name}` on agent `{}`",
                    agent.name.name
                )
            })
    }

    #[test]
    fn store_field_cell_with_initialiser_and_without() {
        let program = checked_context_program(
            r#"
context demo

agent Counter {
  key id: String
  store balance: Cell[Int] = 0
  store hint: Cell[String]

  on call touch() -> Effect[()] {
    Effect.pure(())
  }
}
"#,
        );
        let agent = find_agent(&program, "Counter");

        let balance = find_store_field(agent, "balance");
        let ir = lower_store_field_ir(balance, &program);
        assert_eq!(ir.field, "balance");
        assert!(matches!(ir.kind, StoreKindIr::Cell(_)));
        assert!(
            ir.init.is_some(),
            "a Cell field with an initialiser lowers a real init expression"
        );
        assert!(ir.indexed.is_empty());

        let hint = find_store_field(agent, "hint");
        let ir = lower_store_field_ir(hint, &program);
        assert_eq!(ir.field, "hint");
        assert!(matches!(ir.kind, StoreKindIr::Cell(_)));
        assert!(
            ir.init.is_none(),
            "Decision D's own boundary: a Cell field with no initialiser lowers to None, \
             not merely reached by omission"
        );
    }

    /// #1187's Agent state-field slice: [`lower_store_field_shape_ir`]
    /// exists precisely because [`lower_store_field_ir`] could not lower
    /// these two shapes without hitting an `IrExprKind` gap — `= None`
    /// (`ExprKind::None`) hit the `Ok`/`Err`/`Some`/`None` gap, closed as of
    /// #1225's own ADR; an `is`-expression initialiser (`ExprKind::Is`)
    /// still hits its own separate, still-open `todo!()` a few hundred
    /// lines below. Both are real, certified fixtures, not hypothetical:
    /// `223_store_cell_agent` (`store paymentRef: Cell[Option[AuthId]] =
    /// None`, now lowered directly by `lower_store_field_ir` too — see
    /// `store_field_cell_option_init_none_lowers_without_panicking`) and
    /// `1029_agent_static_init_hoist` (`store active: Cell[Bool] = if true
    /// { 5 is PositiveInt } else { false }`, still `is`-blocked). This pins
    /// that the shape-only reader never touches `init` at all regardless —
    /// unconditionally true, not contingent on either gap's own status — so
    /// it lowers both fields cleanly whether or not `lower_store_field_ir`
    /// itself still would panic on them.
    #[test]
    fn store_field_shape_ir_does_not_panic_on_none_or_is_initialisers() {
        let program = checked_context_program(
            r#"
context demo

type AuthId = String where NonEmpty

agent Order {
  key id: String
  store paymentRef: Cell[Option[AuthId]] = None

  on call touch() -> Effect[()] {
    Effect.pure(())
  }
}
"#,
        );
        let agent = find_agent(&program, "Order");
        let payment_ref = find_store_field(agent, "paymentRef");
        let ir = lower_store_field_shape_ir(payment_ref, &program);
        assert_eq!(ir.field, "paymentRef");
        assert!(matches!(ir.kind, StoreKindIr::Cell(_)));
        assert!(
            ir.init.is_none(),
            "the shape-only reader never lowers init, regardless of the source field"
        );

        let program = checked_context_program(
            r#"
context demo

type PositiveInt = Int where Positive

agent Meter {
  key id: String
  store active: Cell[Bool] = if true { 5 is PositiveInt } else { false }

  on call touch() -> Effect[()] {
    Effect.pure(())
  }
}
"#,
        );
        let agent = find_agent(&program, "Meter");
        let active = find_store_field(agent, "active");
        let ir = lower_store_field_shape_ir(active, &program);
        assert_eq!(ir.field, "active");
        assert!(matches!(ir.kind, StoreKindIr::Cell(_)));
        assert!(ir.init.is_none());
    }

    #[test]
    fn store_field_falls_back_to_unit_on_an_unresolvable_type_like_the_checker_does() {
        // #1187's Agent state-field slice, step 0: no checker pass validates
        // a store field's own type reference (only its shape — `Cell`/`Map`/
        // `Set`/`Cache`/`Log` — and `@ttl`/`@indexed` legality), so
        // `store x: Cell[Bogus] = "hello"` certifies today (exit 0, no
        // diagnostic, verified empirically against the real `bynkc` binary)
        // even though `Bogus` is undeclared and the initialiser's own type
        // doesn't match it. `resolve_store_field_ty` must mirror
        // `lower_op_sig_ir`'s own `Ty::Unit` fallback rather than panic on a
        // state that is, in fact, reachable from source — shared by
        // `lower_store_field_ir` and `lower_store_field_shape_ir` alike.
        // Only the shape reader is exercised end-to-end here: with the
        // field's own type unresolved, the checker's init-checking loop
        // leaves `"hello"` untyped too (a second, separate gap in the
        // dormant, not-yet-wired `lower_store_field_ir`'s `init` arm — out
        // of scope for this slice, which never lowers `init` at all).
        let program = checked_context_program(
            r#"
context demo

agent Widget {
  key id: String
  store x: Cell[Bogus] = "hello"

  on call touch() -> Effect[()] {
    Effect.pure(())
  }
}
"#,
        );
        let agent = find_agent(&program, "Widget");
        let x = find_store_field(agent, "x");
        // Would already have panicked inside `checked_context_program`'s own
        // `.expect("certify")` if this were rejected upstream — reaching
        // here at all is part of what this test pins.
        let shape = lower_store_field_shape_ir(x, &program);
        let StoreKindIr::Cell(ty) = shape.kind else {
            panic!("expected StoreKindIr::Cell, got {:?}", shape.kind)
        };
        assert!(matches!(&*program.program().ty_intern.get(ty), Ty::Unit));
    }

    #[test]
    fn store_field_cell_init_qualified_constructor_call_lowers_without_panicking() {
        // A `Cell` field's own initialiser is checked via
        // `checker::check_state_initialiser`, which types call-shaped
        // sub-expressions (`T.unsafe(lit)`, a qualified sum-variant
        // constructor) through the same `Ctx` every other body check uses —
        // including recording a `Callee` for the call. `lower_store_field_ir`
        // lowers that same init expression through `lower_expr_ir`, which
        // reads `program.callees` unconditionally for a call-shaped node
        // (ADR 0334): this pins that `check_static_initialiser` persists its
        // own `Callee` into the real `typed.callees` sink rather than a
        // throwaway local, so this doesn't panic on a certified program the
        // checker legitimately accepted.
        let program = checked_context_program(
            r#"
context demo

type PositiveId = opaque Int where NonNegative

agent Counter {
  key id: String
  store n: Cell[PositiveId] = PositiveId.unsafe(1)

  on call touch() -> Effect[()] {
    Effect.pure(())
  }
}
"#,
        );
        let agent = find_agent(&program, "Counter");
        let n = find_store_field(agent, "n");
        let ir = lower_store_field_ir(n, &program);
        assert!(matches!(ir.kind, StoreKindIr::Cell(_)));
        assert!(ir.init.is_some());
    }

    /// #1225's own ADR resolution, verified against the real regression it
    /// closes: `223_store_cell_agent` (`bynkc/tests/fixtures/positive`) has
    /// `store paymentRef: Cell[Option[AuthId]] = None` — before this ADR,
    /// `lower_store_field_ir`'s own `init` lowering hit the `ExprKind::None`
    /// `todo!()` this module used to carry (`lower_store_field_shape_ir`,
    /// #1187's Agent state-field slice 2a, was built specifically to avoid
    /// ever lowering `init` at all, precisely because of this gap — see its
    /// own doc comment). This pins that the *full*, `init`-lowering
    /// `lower_store_field_ir` no longer panics on the exact shape that
    /// fixture carries.
    #[test]
    fn store_field_cell_option_init_none_lowers_without_panicking() {
        let program = checked_context_program(
            r#"
context demo

type AuthId = String where NonEmpty

agent Order {
  key id: String
  store paymentRef: Cell[Option[AuthId]] = None

  on call touch() -> Effect[()] {
    Effect.pure(())
  }
}
"#,
        );
        let agent = find_agent(&program, "Order");
        let payment_ref = find_store_field(agent, "paymentRef");
        let ir = lower_store_field_ir(payment_ref, &program);
        assert!(matches!(ir.kind, StoreKindIr::Cell(_)));
        let Some(init) = &ir.init else {
            panic!("expected a lowered `None` initialiser, got None")
        };
        assert!(matches!(
            &init.kind,
            IrExprKind::Variant { tag, payload } if tag == "None" && payload.is_empty()
        ));
    }

    #[test]
    fn store_field_map_indexed_zero_one_and_multiple() {
        let program = checked_context_program(
            r#"
context demo

type Reservation = {
  id: String,
  orderId: String,
  status: String,
}

agent Inventory {
  key sku: String
  store noIndex: Map[String, Reservation]
  store oneIndex: Map[String, Reservation] @indexed(by: orderId)
  store twoIndex: Map[String, Reservation] @indexed(by: orderId, by: status)
  store dupIndex: Map[String, Reservation] @indexed(by: orderId, by: orderId)

  on call touch() -> Effect[()] {
    Effect.pure(())
  }
}
"#,
        );
        let agent = find_agent(&program, "Inventory");

        let no_index = find_store_field(agent, "noIndex");
        let ir = lower_store_field_ir(no_index, &program);
        assert!(matches!(ir.kind, StoreKindIr::Map(_, _)));
        assert!(ir.indexed.is_empty());
        assert!(
            ir.init.is_none(),
            "Decision D: init is None for a non-Cell kind"
        );

        let one_index = find_store_field(agent, "oneIndex");
        let ir = lower_store_field_ir(one_index, &program);
        assert_eq!(ir.indexed, vec!["orderId".to_string()]);

        let two_index = find_store_field(agent, "twoIndex");
        let ir = lower_store_field_ir(two_index, &program);
        assert_eq!(
            ir.indexed,
            vec!["orderId".to_string(), "status".to_string()],
            "Vec<IndexIr>'s own multi-entry case, in the annotation's own by: order"
        );

        // `validate_indexed_keys` (bynk-check) validates each `by:`
        // argument independently with no duplicate check, so
        // `@indexed(by: orderId, by: orderId)` certifies — this pins that
        // lowering still dedupes, mirroring the shipped emitter's own
        // `store_map_indexes` guard, rather than emitting the same sibling
        // index table twice.
        let dup_index = find_store_field(agent, "dupIndex");
        let ir = lower_store_field_ir(dup_index, &program);
        assert_eq!(
            ir.indexed,
            vec!["orderId".to_string()],
            "a duplicate by: key collapses to one sibling-table entry"
        );
    }

    #[test]
    fn store_field_set_element_type() {
        let program = checked_context_program(
            r#"
context demo

agent Tags {
  key id: String
  store tags: Set[String]

  on call touch() -> Effect[()] {
    Effect.pure(())
  }
}
"#,
        );
        let agent = find_agent(&program, "Tags");
        let tags = find_store_field(agent, "tags");
        let ir = lower_store_field_ir(tags, &program);
        assert_eq!(ir.field, "tags");
        assert!(matches!(ir.kind, StoreKindIr::Set(_)));
        assert!(ir.init.is_none());
        assert!(ir.indexed.is_empty());
    }

    #[test]
    fn store_field_cache_resolves_ttl_millis() {
        let program = checked_context_program(
            r#"
context demo

agent Sessions {
  key id: String
  store live: Cache[String, Int] @ttl(5.minutes)

  on call touch() -> Effect[()] {
    Effect.pure(())
  }
}
"#,
        );
        let agent = find_agent(&program, "Sessions");
        let live = find_store_field(agent, "live");
        let ir = lower_store_field_ir(live, &program);
        assert_eq!(ir.field, "live");
        let StoreKindIr::Cache(_, _, ttl) = ir.kind else {
            panic!("expected StoreKindIr::Cache, got {:?}", ir.kind)
        };
        assert_eq!(
            ttl,
            5 * 60 * 1000,
            "5.minutes' own DurationLit.millis, not re-derived arithmetic"
        );
    }

    #[test]
    fn store_field_log_with_and_without_retain() {
        let program = checked_context_program(
            r#"
context demo

agent Inventory {
  key id: String
  store historyWithRetain: Log[String] @retain(30.days)
  store historyNoRetain: Log[String]

  on call touch() -> Effect[()] {
    Effect.pure(())
  }
}
"#,
        );
        let agent = find_agent(&program, "Inventory");

        let with_retain = find_store_field(agent, "historyWithRetain");
        let ir = lower_store_field_ir(with_retain, &program);
        let StoreKindIr::Log(_, retain) = ir.kind else {
            panic!("expected StoreKindIr::Log, got {:?}", ir.kind)
        };
        assert_eq!(retain, Some(30 * 24 * 60 * 60 * 1000));

        let no_retain = find_store_field(agent, "historyNoRetain");
        let ir = lower_store_field_ir(no_retain, &program);
        let StoreKindIr::Log(_, retain) = ir.kind else {
            panic!("expected StoreKindIr::Log, got {:?}", ir.kind)
        };
        assert_eq!(retain, None, "@retain is genuinely optional on Log");
    }

    fn find_handler<'a>(agent: &'a AgentDecl, name: &str) -> &'a Handler {
        agent
            .handlers
            .iter()
            .find(|h| h.method_name.as_ref().is_some_and(|m| m.name == name))
            .unwrap_or_else(|| panic!("no handler named `{name}` on agent `{}`", agent.name.name))
    }

    /// The agent's own synthetic `<Agent>State` record type — re-derives
    /// `context_checks.rs`'s own `state_ty` construction (`tys.intern(Ty::Named {
    /// name: "<Agent>State", .. })`) rather than looking it up: the synthetic
    /// declaration itself lives only in `check_agent_decls`'s own transient
    /// `types_for_handler` clone, never persisted to `TypedCommons::types`, but
    /// `Types::intern` is content-addressed, so re-interning the identical `Ty`
    /// value here returns the exact `TyId` the checker already minted.
    fn agent_state_ty(program: &CheckedProgram, agent_name: &str) -> TyId {
        program.program().ty_intern.intern(Ty::Named {
            name: format!("{agent_name}State"),
            kind: checker::NamedKind::Record,
            args: Vec::new(),
        })
    }

    /// [`lower_invariant_ir`]'s own `store_cells` parameter, built the same
    /// way [`lower_invariant_ir`]'s own doc comment says a future `IrItem::Agent`
    /// caller must: from each `Cell` field's own already-lowered
    /// [`StoreFieldIr::kind`], not re-resolved independently.
    fn agent_store_cells(program: &CheckedProgram, agent: &AgentDecl) -> HashMap<String, TyId> {
        agent
            .store_fields
            .iter()
            .filter(|f| f.kind.head.name == "Cell")
            .map(|f| {
                let ir = lower_store_field_ir(f, program);
                let StoreKindIr::Cell(ty) = ir.kind else {
                    unreachable!("filtered to Cell fields above")
                };
                (ir.field, ty)
            })
            .collect()
    }

    #[test]
    fn invariant_ir_references_a_store_cell_by_bare_name() {
        let program = checked_context_program(
            r#"
context demo

agent Counter {
  key id: String
  store active: Cell[Bool] = true

  invariant staysKnown: active

  on call touch() -> Effect[()] {
    Effect.pure(())
  }
}
"#,
        );
        let agent = find_agent(&program, "Counter");
        let store_cells = agent_store_cells(&program, agent);
        let inv = agent
            .invariants
            .iter()
            .find(|i| i.name.name == "staysKnown")
            .expect("invariant `staysKnown` in this fixture");
        let ir = lower_invariant_ir(inv, &store_cells, &program);
        assert_eq!(ir.name, "staysKnown");
        assert!(
            matches!(&ir.predicate.kind, IrExprKind::Local(n) if n == "active"),
            "a bare-name read of a store Cell field lowers to Local, got {:?}",
            ir.predicate.kind
        );
    }

    #[test]
    fn transition_ir_references_both_old_and_new() {
        let program = checked_context_program(
            r#"
context demo

agent Counter {
  key id: String
  store active: Cell[Bool] = true

  transition activeImplication: old.active implies new.active

  on call touch() -> Effect[()] {
    Effect.pure(())
  }
}
"#,
        );
        let agent = find_agent(&program, "Counter");
        let state_ty = agent_state_ty(&program, "Counter");
        let tr = agent
            .transitions
            .iter()
            .find(|t| t.name.name == "activeImplication")
            .expect("transition `activeImplication` in this fixture");
        let ir = lower_transition_ir(tr, state_ty, &program);
        assert_eq!(ir.name, "activeImplication");
        // `p implies q` lowers to `Or { lhs: Not(p), rhs: q }` (P6.3) — pin
        // that both `old`/`new` reach this predicate as `Field` accesses on
        // a `Local` bound to `state_ty`, not left unresolved.
        let IrExprKind::Or { lhs, rhs } = &ir.predicate.kind else {
            panic!(
                "expected `implies` to lower to Or, got {:?}",
                ir.predicate.kind
            )
        };
        let IrExprKind::Not { operand } = &lhs.kind else {
            panic!("expected Or's own lhs to be Not, got {:?}", lhs.kind)
        };
        let IrExprKind::Field { base, field } = &operand.kind else {
            panic!(
                "expected `old.active` to lower to Field, got {:?}",
                operand.kind
            )
        };
        assert!(matches!(&base.kind, IrExprKind::Local(n) if n == "old"));
        assert_eq!(field, "active");
        let IrExprKind::Field { base, field } = &rhs.kind else {
            panic!(
                "expected `new.active` to lower to Field, got {:?}",
                rhs.kind
            )
        };
        assert!(matches!(&base.kind, IrExprKind::Local(n) if n == "new"));
        assert_eq!(field, "active");
    }

    /// Every [`CommitShape`] "Done when" case (#1165) lives on one agent —
    /// each handler exercises exactly one classification so a failing
    /// assertion names its own scenario unambiguously.
    fn commit_shape_fixture() -> CheckedProgram {
        checked_context_program(
            r#"
context demo

type Box = { n: Int }

fn Box.put(self, x: Int) -> Effect[()] {
  Effect.pure(())
}

capability Clock {
  fn now() -> Effect[Int]
}

provides Clock = FixedClock {
  fn now() -> Effect[Int] {
    42
  }
}

agent Widget {
  key id: String
  store items: Map[String, Int]
  store active: Cell[Bool] = true
  store tags: Set[String]
  store history: Log[String]

  on call readOnlyPlain() -> Effect[()] {
    Effect.pure(())
  }

  on call readOnlyQuery() -> Effect[Int] {
    items.size()
  }

  on call nestedMutation(xs: List[String], flag: Bool) -> Effect[()] {
    if flag {
      match flag {
        true => xs.forEach((x) => items.put(x, 1))
        false => Effect.pure(())
      }
    } else {
      Effect.pure(())
    }
  }

  on call bareAssign(v: Bool) -> Effect[()] {
    active := v
    Effect.pure(())
  }

  on call shadowedName(items: Box, x: Int) -> Effect[()] {
    let _ <- items.put(x)
    Effect.pure(())
  }

  on call cellUpdate() -> Effect[()] {
    let _ <- active.update((b) => !b)
    Effect.pure(())
  }

  on call setAdd(t: String) -> Effect[()] {
    let _ <- tags.add(t)
    Effect.pure(())
  }

  on call logAppend(t: String) -> Effect[()] given Clock {
    let _ <- history.append(t)
    Effect.pure(())
  }
}
"#,
        )
    }

    fn commit_shape_of(program: &CheckedProgram, handler_name: &str) -> CommitShape {
        let agent = find_agent(program, "Widget");
        let handler = find_handler(agent, handler_name);
        let emits = block_uses_emit(&handler.body, &program.program().callees);
        lower_commit_shape_ir(&handler.body, &[], &[], emits, program)
    }

    #[test]
    fn commit_shape_read_only_for_a_plain_body() {
        let program = commit_shape_fixture();
        assert!(matches!(
            commit_shape_of(&program, "readOnlyPlain"),
            CommitShape::ReadOnly
        ));
    }

    #[test]
    fn commit_shape_read_only_for_a_non_mutating_store_read() {
        // `items.size()` records a `Callee::Store { op: "size", .. }` — not in
        // any of the shared mutating-verb constants — so this pins that a
        // real, resolved store read never false-positives into `Transactional`.
        let program = commit_shape_fixture();
        assert!(matches!(
            commit_shape_of(&program, "readOnlyQuery"),
            CommitShape::ReadOnly
        ));
    }

    #[test]
    fn commit_shape_flush_events_for_an_emit_only_body() {
        // `emits` is a plain parameter ([DECISION D]) — the shipped
        // `crate::emitter::block_uses_emit(body)` is the real caller's own
        // computation, reused verbatim rather than re-derived here, so this
        // pins `lower_commit_shape_ir`'s own decision given a non-writing
        // body and `emits: true` directly, independent of exercising
        // `block_uses_emit` itself (which has no `Events` capability to
        // resolve in this reduced test harness — `checked_context_program`'s
        // own doc comment names the `uses`/`consumes` limitation).
        let program = commit_shape_fixture();
        let agent = find_agent(&program, "Widget");
        let handler = find_handler(agent, "readOnlyPlain");
        let shape = lower_commit_shape_ir(&handler.body, &[], &[], true, &program);
        assert!(matches!(shape, CommitShape::FlushEvents));
    }

    #[test]
    fn commit_shape_transactional_for_a_write_nested_in_if_match_lambda() {
        // Reaches `items.put(x, 1)` through If -> Match -> Lambda (forEach's
        // own closure argument) — pins that this walk's descent matches
        // `block_writes_state`'s already-correct one, including through a
        // lambda (R6.5's own `#54` worked example).
        let program = commit_shape_fixture();
        assert!(matches!(
            commit_shape_of(&program, "nestedMutation"),
            CommitShape::Transactional { .. }
        ));
    }

    #[test]
    fn commit_shape_transactional_for_a_bare_cell_assign() {
        let program = commit_shape_fixture();
        assert!(matches!(
            commit_shape_of(&program, "bareAssign"),
            CommitShape::Transactional { .. }
        ));
    }

    #[test]
    fn commit_shape_read_only_for_a_locally_shadowed_store_field_name() {
        // `shadowedName`'s own `items: Box` parameter shadows the agent's
        // `items` store field for the checker's own store-dispatch guard
        // (`ctx.lookup(id.name).is_none()`, `checker.rs`) — `items.put(x)`
        // resolves to `Box`'s own user-declared `put` method, `Callee::Method`,
        // never `Callee::Store`. Pins Decision B's own named improvement over
        // `block_writes_state`'s receiver-name matching, which cannot tell
        // this apart from a real store write and would over-approximate to
        // `Transactional`.
        let program = commit_shape_fixture();
        assert!(matches!(
            commit_shape_of(&program, "shadowedName"),
            CommitShape::ReadOnly
        ));
    }

    #[test]
    fn commit_shape_transactional_for_a_cell_update_method_call() {
        // `active.update(f)` (ADR 0125) is the one `Cell` write that reaches
        // this walk as a method call rather than `Statement::Assign` — pins
        // `MUTATING_CELL_OPS`, otherwise unexercised by `bareAssign`'s own
        // `:=` case.
        let program = commit_shape_fixture();
        assert!(matches!(
            commit_shape_of(&program, "cellUpdate"),
            CommitShape::Transactional { .. }
        ));
    }

    #[test]
    fn commit_shape_transactional_for_a_set_add_method_call() {
        // Pins `MUTATING_SET_OPS`, otherwise unexercised — `nestedMutation`'s
        // own write is a `Map.put`.
        let program = commit_shape_fixture();
        assert!(matches!(
            commit_shape_of(&program, "setAdd"),
            CommitShape::Transactional { .. }
        ));
    }

    #[test]
    fn commit_shape_transactional_for_a_log_append_method_call() {
        // Pins `MUTATING_LOG_OPS`, otherwise unexercised.
        let program = commit_shape_fixture();
        assert!(matches!(
            commit_shape_of(&program, "logAppend"),
            CommitShape::Transactional { .. }
        ));
    }

    #[test]
    fn commit_shape_transactional_carries_its_own_invariants_and_transitions_unswapped() {
        // The only place this constructor moves data rather than deciding a
        // boolean: `Transactional { invariants, transitions }` must carry
        // exactly the slices it was given, in the field they were given
        // under — not swapped, not dropped.
        let program = commit_shape_fixture();
        let agent = find_agent(&program, "Widget");
        let handler = find_handler(agent, "bareAssign");
        let bool_ty = program
            .program()
            .ty_intern
            .intern(Ty::Base(bynk_syntax::ast::BaseType::Bool));
        let mk = |name: &str| IrPredicate {
            name: name.to_string(),
            predicate: IrExpr {
                kind: IrExprKind::Const(ConstVal::Bool(true)),
                ty: bool_ty,
                span: Span::new(0, 0),
            },
        };
        let invariants = vec![mk("invOnly")];
        let transitions = vec![mk("transitionOnly")];
        let shape =
            lower_commit_shape_ir(&handler.body, &invariants, &transitions, false, &program);
        let CommitShape::Transactional {
            invariants: got_inv,
            transitions: got_tr,
        } = shape
        else {
            panic!("expected Transactional, got a different CommitShape")
        };
        assert_eq!(got_inv.len(), 1);
        assert_eq!(got_inv[0].name, "invOnly");
        assert_eq!(got_tr.len(), 1);
        assert_eq!(got_tr[0].name, "transitionOnly");
    }

    /// Every [`IrHandler`] "Done when" case (#1167) lives on one agent — a
    /// read-only handler, a store-writing handler exercising `:=` with a
    /// declared param, a handler with a `given` capability actually called
    /// in the body, and (review of #1167) a handler exercising a
    /// `Callee::Store` method call on a non-`Cell` field — the one
    /// [DECISION F] claim the other three don't reach, since none of them
    /// calls a store method at all, and `entries` (a `Map`, never bound into
    /// [`lower_handler_body_ir`]'s own scope — only `Cell` fields are) pins
    /// that the never-bound path really is unreachable rather than merely
    /// untested.
    fn handler_ir_fixture() -> CheckedProgram {
        checked_context_program(
            r#"
context demo

capability Clock {
  fn now() -> Effect[Int]
}

provides Clock = FixedClock {
  fn now() -> Effect[Int] {
    42
  }
}

agent Ledger {
  key id: String
  store balance: Cell[Int] = 0
  store entries: Map[String, Int]

  on call peek() -> Effect[Int] {
    Effect.pure(balance)
  }

  on call deposit(amount: Int) -> Effect[()] {
    balance := amount
    Effect.pure(())
  }

  on call touchClock() -> Effect[Int] given Clock {
    let t <- Clock.now()
    Effect.pure(t)
  }

  on call addEntry(k: String, v: Int) -> Effect[()] {
    let _ <- entries.put(k, v)
    Effect.pure(())
  }
}
"#,
        )
    }

    fn handler_ir_of(program: &CheckedProgram, handler_name: &str) -> IrHandler {
        handler_ir_of_with_predicates(program, handler_name, &[], &[])
    }

    fn handler_ir_of_with_predicates(
        program: &CheckedProgram,
        handler_name: &str,
        invariants: &[IrPredicate],
        transitions: &[IrPredicate],
    ) -> IrHandler {
        let agent = find_agent(program, "Ledger");
        let handler = find_handler(agent, handler_name);
        let store_cells = agent_store_cells(program, agent);
        let state_ty = agent_state_ty(program, "Ledger");
        lower_handler_ir(
            handler,
            &store_cells,
            state_ty,
            invariants,
            transitions,
            program,
        )
    }

    #[test]
    fn handler_ir_read_only_handler_has_no_binder_and_read_only_commit() {
        let program = handler_ir_fixture();
        let ir = handler_ir_of(&program, "peek");
        assert!(
            ir.binder.is_none(),
            "Decision D: an agent handler's own binder is always None, pinned explicitly rather \
             than left to omission"
        );
        assert_eq!(ir.kind, HandlerKind::Call);
        assert!(matches!(ir.commit, CommitShape::ReadOnly));
        assert_eq!(ir.method_name.as_deref(), Some("peek"));
        assert!(ir.effectful, "peek returns Effect[Int]");
        assert!(ir.params.is_empty());
        assert!(ir.given.is_empty());
        let IrExprKind::Block { stmts, tail } = &ir.body.kind else {
            panic!("expected a Block, got {:?}", ir.body.kind)
        };
        assert!(stmts.is_empty());
        let IrExprKind::Return { value } = &tail.kind else {
            panic!(
                "expected the tail to be wrapped in Return, got {:?}",
                tail.kind
            )
        };
        let IrExprKind::Pure { value } = &value.kind else {
            panic!(
                "expected `Effect.pure(balance)` to lower to Pure, got {:?}",
                value.kind
            )
        };
        assert!(
            matches!(&value.kind, IrExprKind::Local(n) if n == "balance"),
            "a bare-name read of a store Cell field lowers to Local, got {:?}",
            value.kind
        );
    }

    #[test]
    fn handler_ir_store_writing_handler_lowers_assign_and_transactional_commit() {
        let program = handler_ir_fixture();
        let ir = handler_ir_of(&program, "deposit");
        assert!(ir.binder.is_none());
        let int_ty = program
            .program()
            .ty_intern
            .intern(Ty::Base(bynk_syntax::ast::BaseType::Int));
        assert_eq!(
            ir.params,
            vec![("amount".to_string(), int_ty)],
            "a declared param's type must resolve and bind into the body's own scope"
        );
        assert_eq!(ir.method_name.as_deref(), Some("deposit"));
        let CommitShape::Transactional {
            invariants,
            transitions,
        } = &ir.commit
        else {
            panic!("expected Transactional, got {:?}", ir.commit)
        };
        // `handler_ir_of` (unlike `handler_ir_of_with_predicates`) passes no
        // predicates — empty here reflects this test's own call, not a
        // limit of `lower_handler_ir` itself; see
        // `handler_ir_threads_invariants_and_transitions_into_commit_unswapped`
        // for the threading pin.
        assert!(invariants.is_empty());
        assert!(transitions.is_empty());
        let IrExprKind::Block { stmts, .. } = &ir.body.kind else {
            panic!("expected a Block, got {:?}", ir.body.kind)
        };
        assert_eq!(
            stmts.len(),
            1,
            "the `:=` write is the body's only statement"
        );
        let IrStmt::Assign { field, value } = &stmts[0] else {
            panic!("expected IrStmt::Assign, got {:?}", stmts[0])
        };
        assert_eq!(field, "balance");
        assert!(
            matches!(&value.kind, IrExprKind::Local(n) if n == "amount"),
            "the assigned value reads the handler's own `amount` param, got {:?}",
            value.kind
        );
    }

    #[test]
    fn handler_ir_given_capability_recorded_and_call_lowers_as_ordinary_callee() {
        let program = handler_ir_fixture();
        let ir = handler_ir_of(&program, "touchClock");
        assert!(ir.binder.is_none());
        assert_eq!(ir.given, vec!["Clock".to_string()]);
        let IrExprKind::Block { stmts, .. } = &ir.body.kind else {
            panic!("expected a Block, got {:?}", ir.body.kind)
        };
        assert_eq!(stmts.len(), 1);
        let IrStmt::Let { local, value } = &stmts[0] else {
            panic!("expected IrStmt::Let, got {:?}", stmts[0])
        };
        assert_eq!(local, "t");
        let IrExprKind::Await { effect } = &value.kind else {
            panic!(
                "expected `let t <- Clock.now()` to lower to Await, got {:?}",
                value.kind
            )
        };
        let IrExprKind::Call { callee, args, .. } = &effect.kind else {
            panic!(
                "expected `Clock.now()` to lower as an ordinary Callee-classified Call \
                 (Decision F — no new call-lowering logic needed), got {:?}",
                effect.kind
            )
        };
        assert!(
            matches!(callee, Callee::Capability { cap, op } if cap == "Clock" && op == "now"),
            "expected Callee::Capability {{ cap: \"Clock\", op: \"now\" }}, got {callee:?}"
        );
        assert!(args.is_empty());
    }

    #[test]
    fn handler_ir_store_method_call_on_a_non_cell_field_lowers_as_ordinary_callee() {
        // The one Decision F claim `peek`/`deposit`/`touchClock` don't reach:
        // a `Callee::Store` method call, on a `Map` field never bound into
        // `lower_handler_body_ir`'s own scope (only `Cell` fields are) —
        // pins that `entries` is never looked up as an ident at all, not
        // merely that it happens not to be, since `receiver_is_a_value`
        // excludes `Callee::Store` from ever lowering its own receiver.
        let program = handler_ir_fixture();
        let ir = handler_ir_of(&program, "addEntry");
        assert!(ir.binder.is_none());
        assert!(
            matches!(ir.commit, CommitShape::Transactional { .. }),
            "Map.put is a mutating Callee::Store op"
        );
        let IrExprKind::Block { stmts, .. } = &ir.body.kind else {
            panic!("expected a Block, got {:?}", ir.body.kind)
        };
        assert_eq!(stmts.len(), 1);
        let IrStmt::Let { local, value } = &stmts[0] else {
            panic!("expected IrStmt::Let, got {:?}", stmts[0])
        };
        assert_eq!(local, "_");
        let IrExprKind::Await { effect } = &value.kind else {
            panic!(
                "expected `let _ <- entries.put(k, v)` to lower to Await, got {:?}",
                value.kind
            )
        };
        let IrExprKind::Call { callee, args, .. } = &effect.kind else {
            panic!(
                "expected `entries.put(k, v)` to lower as an ordinary Callee-classified Call, \
                 got {:?}",
                effect.kind
            )
        };
        assert!(
            matches!(callee, Callee::Store { field, op } if field == "entries" && op == "put"),
            "expected Callee::Store {{ field: \"entries\", op: \"put\" }}, got {callee:?}"
        );
        assert_eq!(
            args.len(),
            2,
            "the receiver is never prepended for Callee::Store"
        );
        assert!(matches!(&args[0].kind, IrExprKind::Local(n) if n == "k"));
        assert!(matches!(&args[1].kind, IrExprKind::Local(n) if n == "v"));
    }

    #[test]
    fn handler_ir_threads_invariants_and_transitions_into_commit_unswapped() {
        // Review of #1167: `lower_handler_ir` originally passed
        // `lower_commit_shape_ir` empty `invariants`/`transitions` slices
        // unconditionally — indistinguishable from an agent that genuinely
        // declares neither. Now threaded through as ordinary parameters,
        // mirroring `commit_shape_transactional_carries_its_own_invariants_and_transitions_unswapped`'s
        // own pin one layer up.
        let program = handler_ir_fixture();
        let bool_ty = program
            .program()
            .ty_intern
            .intern(Ty::Base(bynk_syntax::ast::BaseType::Bool));
        let mk = |name: &str| IrPredicate {
            name: name.to_string(),
            predicate: IrExpr {
                kind: IrExprKind::Const(ConstVal::Bool(true)),
                ty: bool_ty,
                span: Span::new(0, 0),
            },
        };
        let invariants = vec![mk("invOnly")];
        let transitions = vec![mk("transitionOnly")];
        let ir = handler_ir_of_with_predicates(&program, "deposit", &invariants, &transitions);
        let CommitShape::Transactional {
            invariants: got_inv,
            transitions: got_tr,
        } = ir.commit
        else {
            panic!("expected Transactional, got {:?}", ir.commit)
        };
        assert_eq!(got_inv.len(), 1);
        assert_eq!(got_inv[0].name, "invOnly");
        assert_eq!(got_tr.len(), 1);
        assert_eq!(got_tr[0].name, "transitionOnly");
    }

    /// Every [`IrItem::Agent`] "Done when" case (#1169) lives on one agent —
    /// a `Cell` field an invariant/transition and a handler all reference by
    /// bare name, a `Map` field only a handler reaches (via `Callee::Store`),
    /// one read-only handler, and one store-writing handler whose own
    /// `commit` must carry the agent's real invariants/transitions, not an
    /// empty pair.
    ///
    /// **Named gap, not silently absorbed (review of #1169):** no handler
    /// here exercises `CommitShape::FlushEvents` (a non-writing handler that
    /// emits) — the one arm [`lower_commit_shape_ir`] can produce that this
    /// fixture never reaches. A real `Events.emit` call needs an `event`
    /// declaration plus `consumes bynk { Events }`, which
    /// [`checked_context_program`]'s own doc comment already names as out
    /// of scope for this reduced harness (`resolver::CrossContextInfo`
    /// hardcoded to `::default()`) — extending it is a deliberate, separate
    /// change, not a two-line addition here. The risk this would pin —
    /// threading real, non-empty `invariants`/`transitions` into every
    /// handler somehow pushing a non-writing one into `Transactional` — is
    /// already structurally impossible: [`lower_commit_shape_ir`]'s own
    /// branch is decided by [`body_writes_state`] alone, never by whether
    /// `invariants`/`transitions` are empty, and `peek`'s own `ReadOnly`
    /// assertion below already exercises exactly that (non-empty
    /// predicates, still `ReadOnly`) for the `body_writes_state == false`
    /// case `FlushEvents` shares.
    fn agent_item_fixture() -> CheckedProgram {
        checked_context_program(
            r#"
context demo

agent Ledger {
  key id: String
  store active: Cell[Bool] = true
  store balance: Cell[Int] = 0
  store entries: Map[String, Int]

  invariant staysKnown: active

  transition activeImplication: old.active implies new.active

  on call peek() -> Effect[Int] {
    Effect.pure(balance)
  }

  on call deposit(amount: Int) -> Effect[()] {
    balance := amount
    Effect.pure(())
  }

  on call addEntry(k: String, v: Int) -> Effect[()] {
    let _ <- entries.put(k, v)
    Effect.pure(())
  }
}
"#,
        )
    }

    #[test]
    fn agent_item_ir_assembles_key_state_and_threads_invariants_transitions_into_handlers() {
        let program = agent_item_fixture();
        let agent = find_agent(&program, "Ledger");
        let ir = lower_agent_item_ir(agent, &program);
        let IrItem::Agent {
            def,
            key,
            state,
            handlers,
            invariants,
            transitions,
        } = &ir
        else {
            panic!("expected IrItem::Agent, got {:?}", ir)
        };

        assert_eq!(def, "Ledger");
        let string_ty = program
            .program()
            .ty_intern
            .intern(Ty::Base(bynk_syntax::ast::BaseType::String));
        assert_eq!(key, &("id".to_string(), string_ty));

        assert_eq!(
            state.len(),
            3,
            "active, balance, entries — declaration order"
        );
        assert!(matches!(state[0].kind, StoreKindIr::Cell(_)));
        assert_eq!(state[0].field, "active");
        assert!(matches!(state[1].kind, StoreKindIr::Cell(_)));
        assert_eq!(state[1].field, "balance");
        assert!(matches!(state[2].kind, StoreKindIr::Map(_, _)));
        assert_eq!(state[2].field, "entries");

        assert_eq!(invariants.len(), 1);
        assert_eq!(invariants[0].name, "staysKnown");
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].name, "activeImplication");

        assert_eq!(handlers.len(), 3, "peek, deposit, addEntry");

        let peek = handlers
            .iter()
            .find(|h| h.method_name.as_deref() == Some("peek"))
            .expect("peek handler");
        assert!(matches!(peek.commit, CommitShape::ReadOnly));

        // The load-bearing assertion: `deposit`'s own commit carries the
        // *agent's real* invariants/transitions, lowered once here and
        // threaded through — not the empty pair `lower_handler_ir` would
        // produce on its own if this function forgot to pass them.
        let deposit = handlers
            .iter()
            .find(|h| h.method_name.as_deref() == Some("deposit"))
            .expect("deposit handler");
        let CommitShape::Transactional {
            invariants: got_inv,
            transitions: got_tr,
        } = &deposit.commit
        else {
            panic!(
                "expected deposit's own commit to be Transactional, got {:?}",
                deposit.commit
            )
        };
        assert_eq!(got_inv.len(), 1);
        assert_eq!(got_inv[0].name, "staysKnown");
        assert_eq!(got_tr.len(), 1);
        assert_eq!(got_tr[0].name, "activeImplication");

        // `addEntry` writes through `Callee::Store` (Map.put), not `:=` —
        // pins that a non-Cell-field write also reaches Transactional and
        // also carries the same real invariants/transitions, not just the
        // `:=` case `deposit` already covers.
        let add_entry = handlers
            .iter()
            .find(|h| h.method_name.as_deref() == Some("addEntry"))
            .expect("addEntry handler");
        let CommitShape::Transactional {
            invariants: got_inv,
            transitions: got_tr,
        } = &add_entry.commit
        else {
            panic!(
                "expected addEntry's own commit to be Transactional, got {:?}",
                add_entry.commit
            )
        };
        assert_eq!(got_inv.len(), 1);
        assert_eq!(got_tr.len(), 1);

        // Every `IrHandler` this slice builds is still agent-only —
        // pinning Decision D's own guarantee one level up, through the
        // assembled `IrItem::Agent` rather than only through
        // `lower_handler_ir` directly.
        assert!(handlers.iter().all(|h| h.binder.is_none()));
    }

    /// #1189's own named breakage point: `agent_item_fixture`'s own
    /// invariant/transition are deliberately comparison-free
    /// (`active`/`old.active implies new.active`) — hand-picked, per #1189's
    /// own finding, the same way every P6.4-P6.9 fixture happened to avoid
    /// the gap. This is the real shape #1189 named as blocked before this
    /// slice (`balance >= 0`, `bynkc/tests/fixtures/positive/248_history_property`'s
    /// own `nonneg` invariant) — pins that `lower_agent_item_ir` no longer
    /// panics on it.
    #[test]
    fn agent_invariant_with_a_real_comparison_lowers_without_panicking() {
        let program = checked_context_program(
            r#"
context demo

agent Ledger {
  key id: String
  store balance: Cell[Int] = 0

  invariant nonneg: balance >= 0

  on call deposit(amount: Int) -> Effect[()] {
    balance := amount
    Effect.pure(())
  }
}
"#,
        );
        let agent = find_agent(&program, "Ledger");
        let ir = lower_agent_item_ir(agent, &program);
        let IrItem::Agent { invariants, .. } = &ir else {
            panic!("expected IrItem::Agent, got {:?}", ir)
        };
        assert_eq!(invariants.len(), 1);
        let IrExprKind::BinOp { op, .. } = &invariants[0].predicate.kind else {
            panic!("expected BinOp, got {:?}", invariants[0].predicate.kind)
        };
        assert_eq!(*op, IrBinOp::GtEq);
    }

    /// P6.11's own `IrItem::Service` "Done when" case (#1171): a plain
    /// `call`-protocol service, three handlers — one bare, one with a real
    /// actor binder, one with a `given` capability actually called in the
    /// body. Every service handler's return type is protocol-mandated to a
    /// sum for HTTP/queue/cron fixtures elsewhere in this module — `Call`
    /// is the one protocol that isn't, so this fixture needs none of the
    /// free-`fn` indirection those do.
    fn call_service_fixture() -> CheckedProgram {
        checked_context_program(
            r#"
context demo

type UserId = String

actor Buyer { auth = Internal, identity = UserId }

capability Clock {
  fn now() -> Effect[Int]
}

provides Clock = FixedClock {
  fn now() -> Effect[Int] {
    42
  }
}

service Api {
  on call(ping: String) -> Effect[String] {
    Effect.pure(ping)
  }
  on call(ping: String) -> Effect[String] by u: Buyer {
    Effect.pure(ping)
  }
  on call() -> Effect[Int] given Clock {
    let t <- Clock.now()
    Effect.pure(t)
  }
}
"#,
        )
    }

    #[test]
    fn service_item_ir_assembles_a_call_protocol_service() {
        let program = call_service_fixture();
        let service = find_service(&program, "Api");
        let ir = lower_service_item_ir(service, &program);
        let IrItem::Service {
            def,
            protocol,
            handlers,
            policy,
        } = &ir
        else {
            panic!("expected IrItem::Service, got {:?}", ir)
        };
        assert_eq!(def, "Api");
        assert!(matches!(protocol, ProtocolIr::Call));
        assert!(
            policy.is_none(),
            "policy is only ever Some for a from http service"
        );
        assert_eq!(handlers.len(), 3, "declaration order preserved");

        assert!(handlers[0].binder.is_none());
        assert!(handlers[0].given.is_empty());
        assert!(
            handlers[0].actors.is_empty(),
            "no `by` clause at all on this handler"
        );
        assert!(handlers[1].binder.is_some());
        assert_eq!(
            handlers[1].actors,
            vec!["Buyer".to_string()],
            "the gate itself, read straight off the `by` clause — see IrHandler::actors' own \
             doc comment for why this can't be recovered from `binder` alone"
        );
        assert_eq!(handlers[2].given, vec!["Clock".to_string()]);

        for h in handlers {
            assert!(
                h.method_name.is_none(),
                "a service's on call handler carries no method_name"
            );
            assert!(h.effectful, "every service handler returns Effect[T]");
            assert!(matches!(h.commit, CommitShape::ReadOnly));
        }
    }

    #[test]
    fn service_handler_binder_is_recorded_and_bound_into_the_body_scope() {
        // `Caller` (v0.54) is a prelude actor — no local `actor` declaration
        // needed — whose identity is the calling-context id, `String`.
        let program = checked_context_program(
            r#"
context demo

service Api {
  on call(ping: String) -> Effect[String] by c: Caller {
    Effect.pure(c.identity)
  }
}
"#,
        );
        let service = find_service(&program, "Api");
        let handler = find_service_handler(service, &HandlerKind::Call);
        let ir = lower_service_handler_ir(handler, &service.protocol, &program);

        let binder = ir
            .binder
            .as_ref()
            .unwrap_or_else(|| panic!("expected a persisted actor binding for this handler"));
        assert_eq!(binder.binder, "c");
        let tys = &program.program().ty_intern;
        let Ty::Actor(identity_ty) = &*tys.get(binder.ty) else {
            panic!("expected Ty::Actor, got {:?}", tys.get(binder.ty))
        };
        assert_eq!(identity_ty.display(tys), "String");
        assert_eq!(ir.actors, vec!["Caller".to_string()]);

        // The load-bearing half: `binder` recorded on the struct is not the
        // same claim as `binder` actually reaching the body's own scope —
        // walk the lowered body and confirm a real `Local("c")` read
        // reached the tree where `c.identity` was written, rather than
        // `lower_ident_ir`'s own unresolved-ident `todo!()`.
        let IrExprKind::Block { tail, .. } = &ir.body.kind else {
            panic!("expected a Block, got {:?}", ir.body.kind)
        };
        let IrExprKind::Return { value } = &tail.kind else {
            panic!("expected Return, got {:?}", tail.kind)
        };
        let IrExprKind::Pure { value } = &value.kind else {
            panic!("expected Pure, got {:?}", value.kind)
        };
        let IrExprKind::Field { base, field } = &value.kind else {
            panic!(
                "expected `c.identity` to lower to Field, got {:?}",
                value.kind
            )
        };
        assert_eq!(field, "identity");
        assert!(
            matches!(&base.kind, IrExprKind::Local(n) if n == "c"),
            "expected the binder to be bound into the body's own scope as Local(\"c\"), got {:?}",
            base.kind
        );
    }

    #[test]
    fn sum_actor_binder_lowers_an_actor_sum() {
        // Mirrors `bynk-check`'s own `sum_by_clause_persists_an_actor_sum_binding`
        // test: a sum needs distinguishable schemes, so it must be
        // `from http`. The free `fn ok` indirection (see
        // `http_service_fixture`'s own doc comment) was originally a
        // workaround for the `Ok`/`Err`/`Some`/`None` construction gap in
        // `lower_expr_ir`, closed as of #1225's own ADR — kept as the
        // fixture's own shape regardless. The body's own `match who { … }` was initially
        // planned as a separate, riskier claim — no existing test drove
        // pattern lowering over an `ActorSum` scrutinee — but verified
        // empirically during implementation to lower correctly (`User(_)`'s
        // own positional payload binding included), so it stays in rather
        // than being degraded away.
        let program = checked_context_program(
            r#"
context demo

type UserId = String

actor User { auth = Bearer(secret = "AUTH_SECRET"), identity = UserId }

fn ok(s: String) -> HttpResult[String] { Ok(s) }

service Api from http {
  on GET("/whoami") () -> Effect[HttpResult[String]] by who: User | Visitor {
    match who {
      User(_) => Effect.pure(ok("user"))
      Visitor => Effect.pure(ok("visitor"))
    }
  }
}
"#,
        );
        let service = find_service(&program, "Api");
        let handler = &service.handlers[0];
        let ir = lower_service_handler_ir(handler, &service.protocol, &program);
        let binder = ir
            .binder
            .as_ref()
            .unwrap_or_else(|| panic!("expected a persisted actor binding for this handler"));
        assert_eq!(binder.binder, "who");
        let tys = &program.program().ty_intern;
        let Ty::ActorSum(members) = &*tys.get(binder.ty) else {
            panic!("expected Ty::ActorSum, got {:?}", tys.get(binder.ty))
        };
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].0, "User");
        assert_eq!(members[0].1.display(tys), "UserId");
        assert_eq!(members[1].0, "Visitor");
        assert_eq!(
            members[1].1.display(tys),
            "()",
            "Visitor is a unit-identity prelude actor"
        );
        assert_eq!(
            ir.actors,
            vec!["User".to_string(), "Visitor".to_string()],
            "actors is redundant with Ty::ActorSum's own member names here, but present \
             uniformly regardless of shape — see IrHandler::actors' own doc comment"
        );

        // The body itself really did lower — a real `Match` node, not the
        // simpler single-arm body an earlier draft of this test used
        // before the `ActorSum`-scrutinee path was verified.
        let IrExprKind::Block { tail, .. } = &ir.body.kind else {
            panic!("expected a Block, got {:?}", ir.body.kind)
        };
        let IrExprKind::Return { value } = &tail.kind else {
            panic!("expected Return, got {:?}", tail.kind)
        };
        assert!(
            matches!(&value.kind, IrExprKind::Match { .. }),
            "expected `match who {{ … }}` to lower to a real Match node, got {:?}",
            value.kind
        );
    }

    /// Review of #1209: pins the one load-bearing ordering decision
    /// `ActorSeamIr`'s own doc comment argues for — `sum_members_for`
    /// ahead of `bearer_seam_for` — at `lower_actor_seam_ir` itself, not
    /// only four fixture-hops away via a full `emit_service`/`bless` run.
    /// `bearer_seam_for` has no `by.is_sum()` guard of its own and resolves
    /// off `by.primary()`, so a Bearer-first sum (`by who: User | Visitor`
    /// with `User`'s own scheme `Bearer`) would resolve as `ActorSeamIr::
    /// Bearer` instead of `ActorSeamIr::Sum` if the two resolvers were ever
    /// tried in the other order.
    #[test]
    fn lower_actor_seam_ir_tries_sum_ahead_of_bearer_for_a_bearer_first_sum() {
        let program = checked_context_program(
            r#"
context demo

type UserId = String

actor User { auth = Bearer(secret = "AUTH_SECRET"), identity = UserId }

service Api from http {
  on GET("/whoami") () -> Effect[HttpResult[String]] by who: User | Visitor {
    Effect.pure(Ok("ok"))
  }
}
"#,
        );
        let service = find_service(&program, "Api");
        let handler = &service.handlers[0];
        let actors = actors_map(&program);
        let seam = lower_actor_seam_ir(handler, &actors);
        let ActorSeamIr::Sum(members) = &seam else {
            panic!("expected ActorSeamIr::Sum for a Bearer-first sum `by` clause, got {seam:?}");
        };
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].actor_name, "User");
        assert_eq!(members[1].actor_name, "Visitor");
    }

    #[test]
    fn a_binderless_by_clause_still_records_the_actor_gate() {
        // Review of #1180's own core scenario: `binder` alone erases a
        // binder-less `by <Actor>` (verify-and-discard) entirely — nothing
        // in `kind`/`params`/`given`/`binder` would otherwise distinguish
        // this actor-gated handler from a public one with no `by` clause
        // at all. `actors` is what a consumer must read to recover the
        // gate.
        let program = checked_context_program(
            r#"
context demo

type UserId = String

actor Buyer { auth = Internal, identity = UserId }

service Api {
  on call(ping: String) -> Effect[String] by Buyer {
    Effect.pure(ping)
  }
}
"#,
        );
        let service = find_service(&program, "Api");
        let handler = find_service_handler(service, &HandlerKind::Call);
        let ir = lower_service_handler_ir(handler, &service.protocol, &program);
        assert!(
            ir.binder.is_none(),
            "a binder-less `by <Actor>` verifies-and-discards — no identity is bound"
        );
        assert_eq!(
            ir.actors,
            vec!["Buyer".to_string()],
            "the gate itself survives even though no binder does"
        );
    }

    /// Every `from http` policy test lives on one service — a full `cors`/
    /// `security`/`limits` block, asserted against the *interpreted*
    /// `PolicyIr`, not the raw AST. The handler body goes through a free
    /// `fn ok` rather than a bare `Ok(s)` construction — a holdover from
    /// when `Ok`/`Err`/`Some`/`None` construction was still `todo!()` in
    /// `lower_expr_ir` (closed as of #1225's own ADR; kept as-is here since
    /// it's still a correct, working fixture shape and every test built on
    /// it is unaffected either way, not because a bare `Ok(s)` would fail
    /// now).
    fn http_service_fixture() -> CheckedProgram {
        checked_context_program(
            r#"
context demo

fn ok(s: String) -> HttpResult[String] { Ok(s) }

service Api from http {
  cors { origins: ["https://app.example.com"], credentials: true, maxAge: 1.hours }
  security { hsts: 365.days }
  limits { maxBody: 1048576 }
  on GET("/ping") () -> Effect[HttpResult[String]] by v: Visitor {
    Effect.pure(ok("pong"))
  }
}
"#,
        )
    }

    #[test]
    fn http_policy_lowers_every_accessor_to_interpreted_values() {
        let program = http_service_fixture();
        let service = find_service(&program, "Api");
        let ir = lower_service_item_ir(service, &program);
        let IrItem::Service {
            protocol,
            handlers,
            policy,
            ..
        } = &ir
        else {
            panic!("expected IrItem::Service, got {:?}", ir)
        };
        assert!(matches!(protocol, ProtocolIr::Http));
        assert_eq!(
            handlers[0].kind,
            HandlerKind::Http {
                method: bynk_syntax::ast::HttpMethod::Get,
                path: "/ping".to_string(),
            },
            "the route binding lives per-handler — this is why ProtocolIr::Http itself \
             carries no payload"
        );

        let policy = policy
            .as_ref()
            .unwrap_or_else(|| panic!("expected a real PolicyIr for a from http service"));
        let cors = policy
            .cors
            .as_ref()
            .unwrap_or_else(|| panic!("expected Some(CorsIr) — this fixture declares cors {{ }}"));
        assert_eq!(cors.origins, vec!["https://app.example.com".to_string()]);
        assert!(cors.credentials);
        assert_eq!(
            cors.allow_headers, None,
            "no `headers:` field written — the author-override distinction, not the \
             emitter's own smart default"
        );
        assert_eq!(cors.max_age_secs, Some(3600), "1.hours in whole seconds");

        assert!(policy.security.nosniff);
        assert_eq!(
            policy.security.hsts_max_age_secs,
            Some(365 * 24 * 60 * 60),
            "365.days in whole seconds"
        );
        assert_eq!(policy.max_body_bytes, Some(1_048_576));
    }

    #[test]
    fn an_http_service_with_no_security_block_still_lowers_the_safe_defaults() {
        // The single test pinning ADR 0164's own asymmetry: `security: None`
        // on the AST means *defaults*, not *no headers* — a real, easy
        // regression if `PolicyIr::security` were ever "simplified" to
        // `Option<SecurityIr>`.
        let program = checked_context_program(
            r#"
context demo

fn ok(s: String) -> HttpResult[String] { Ok(s) }

service Api from http {
  on GET("/ping") () -> Effect[HttpResult[String]] by v: Visitor {
    Effect.pure(ok("pong"))
  }
}
"#,
        );
        let service = find_service(&program, "Api");
        let ir = lower_service_item_ir(service, &program);
        let IrItem::Service { policy, .. } = &ir else {
            panic!("expected IrItem::Service, got {:?}", ir)
        };
        let policy = policy
            .as_ref()
            .unwrap_or_else(|| panic!("expected Some(PolicyIr) for a from http service"));
        assert!(policy.cors.is_none(), "no cors {{ }} block was declared");
        assert!(
            policy.security.nosniff,
            "the safe default, even with no security {{ }} block"
        );
        assert_eq!(policy.security.hsts_max_age_secs, None);
        assert_eq!(
            policy.max_body_bytes, None,
            "no limits {{ }} block was declared"
        );
    }

    /// A queue consumer's own body is the one shape this module's own
    /// pre-existing gaps make genuinely unlowerable today, not just
    /// awkward to fixture around: `Effect[QueueResult]` is mandatory
    /// (`bynk.queue.return_not_https`-adjacent gate, `context_checks.rs:
    /// 3730-3744`), and every `QueueResult` value — `Ack`, `NotFound`,
    /// `Retry(reason)` — is a bare or qualified built-in-sum variant
    /// reference, the exact case `GlobalRef`'s own doc comment (`ir.rs`)
    /// already names as dropped from P6.1's Decision C on purpose
    /// (contextual, `expected`-type-driven disambiguation this pass has no
    /// sink to read back). Unlike `HttpResult`'s `Ok`/`Err`, `QueueResult`'s
    /// own variants also don't resolve inside an ordinary free `fn` body at
    /// all (confirmed empirically: `bynk.resolve.unknown_name`) — the
    /// checker's own special-case for them (`checker.rs:3507`) is reached
    /// only via a real handler body's own `Ctx::return_ty`, which the
    /// resolver's eager pass over an ordinary `fn` never sets up — so the
    /// `fn ok(s) -> HttpResult[String] { Ok(s) }` indirection every other
    /// HTTP/cron fixture in this module uses has no queue-shaped
    /// equivalent. Two tests, not one, cover what's actually true here.
    fn queue_service_fixture() -> CheckedProgram {
        checked_context_program(
            r#"
context demo

type EmailJob = { to: String }

service Outbox from queue("orders") {
  on message(m: EmailJob) -> Effect[QueueResult] {
    Ack
  }
}
"#,
        )
    }

    #[test]
    fn a_queue_services_protocol_and_handler_signature_lower_correctly() {
        // Everything that *doesn't* need the handler body lowered: the
        // protocol descriptor (standalone, mirroring `lower_protocol_ir`'s
        // own precedent for `from websocket`) and the handler's own
        // `params`/`given`/`effectful` via `lower_handler_signature_ir`
        // directly, the same shared helper `lower_service_handler_ir`
        // itself calls before ever reaching the body.
        let program = queue_service_fixture();
        let service = find_service(&program, "Outbox");
        assert!(matches!(
            lower_protocol_ir(&service.protocol, &program),
            ProtocolIr::Queue { name } if name == "orders"
        ));
        let handler = find_service_handler(service, &HandlerKind::Message);
        let cx = LowerIrCtx::new(&program, HashSet::new());
        let (params, given, _ret, effectful) = lower_handler_signature_ir(handler, &cx);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].0, "m");
        assert!(given.is_empty());
        assert!(effectful, "every service handler returns Effect[T]");
    }

    /// #1187's slice 5 (the `Service` emitter cutover, review of #1196):
    /// `lower_service_handler_signature_ir` is `emit_service`'s own real
    /// call site's entry point, not `lower_handler_signature_ir` directly —
    /// this pins it against the exact shape that motivated it: an ordinary
    /// `from http` handler body constructing `Ok(...)` directly (not routed
    /// through the `fn ok(s) -> HttpResult[String] { Ok(s) }` indirection
    /// every other fixture in this module uses). Originally chosen because
    /// building a real `IrHandler` here (`lower_service_handler_ir`) would
    /// panic on this exact body, on P6.2/P6.3's own `Ok`/`Err`/`Some`/`None`
    /// gap (#1143/#1145) — closed as of #1225's own ADR, so this specific
    /// body no longer panics `lower_service_handler_ir` either. The general
    /// claim this test's own name makes still holds regardless (a real
    /// `IrHandler` is still unsafe to build unconditionally at
    /// `emit_service`'s call site — an ordinary `from http` body just as
    /// routinely hits the still-open `?`/`is` gaps, `ExprKind::Question`/
    /// `ExprKind::Is`, instead), so the fixture stays as-is rather than
    /// chasing a body shape that still panics today.
    #[test]
    fn service_handler_signature_lowers_without_touching_a_body_that_constructs_ok() {
        let program = checked_context_program(
            r#"
context demo

service Api from http {
  on GET("/ping") () -> Effect[HttpResult[String]] by v: Visitor {
    Effect.pure(Ok("pong"))
  }
}
"#,
        );
        let service = find_service(&program, "Api");
        let handler = find_service_handler(
            service,
            &HandlerKind::Http {
                method: bynk_syntax::ast::HttpMethod::Get,
                path: "/ping".to_string(),
            },
        );
        let (params, _given, ret, effectful) =
            lower_service_handler_signature_ir(handler, &program);
        assert!(params.is_empty(), "`() -> ...` declares no parameters");
        assert!(effectful, "an `Effect[...]` return type");
        assert!(matches!(
            &*program.program().ty_intern.get(ret),
            Ty::Effect(_)
        ));
    }

    #[test]
    #[should_panic(expected = "neither a locally-bound name")]
    fn a_queue_services_on_message_handler_reaches_ordinary_body_lowering_not_the_websocket_deferral()
     {
        // The highest-value protocol test: proves the WebSocket deferral
        // gate in `lower_service_handler_ir` keys on the `(kind, protocol)`
        // *pair*, not on `HandlerKind::Message` alone — the same literal
        // variant is also a WebSocket inbound frame. This panics on the
        // pre-existing bare-nullary-built-in-variant gap (`Ack`,
        // `lower_ident_ir`'s own final `todo!()` — see
        // `queue_service_fixture`'s own doc comment), **not** on the
        // WebSocket-specific "synthetic leading" message — proof this
        // handler got past the gate the WebSocket deferral guards, not
        // caught by it.
        let program = queue_service_fixture();
        let service = find_service(&program, "Outbox");
        let handler = find_service_handler(service, &HandlerKind::Message);
        let _ = lower_service_handler_ir(handler, &service.protocol, &program);
    }

    #[test]
    fn a_cron_service_lowers_its_schedule_from_the_handler_not_the_protocol() {
        let program = checked_context_program(
            r#"
context demo

fn done() -> Result[(), String] { Ok(()) }

service Sweeper from cron {
  on schedule("*/5 * * * *") () -> Effect[Result[(), String]] {
    Effect.pure(done())
  }
}
"#,
        );
        let service = find_service(&program, "Sweeper");
        let ir = lower_service_item_ir(service, &program);
        let IrItem::Service {
            protocol, handlers, ..
        } = &ir
        else {
            panic!("expected IrItem::Service, got {:?}", ir)
        };
        assert!(matches!(protocol, ProtocolIr::Cron));
        assert_eq!(
            handlers[0].kind,
            HandlerKind::Cron {
                expr: "*/5 * * * *".to_string()
            }
        );
    }

    /// Mirrors `bynkc/tests/fixtures/positive/236_websocket_chatroom` in
    /// full — `on open`/`on message`/`on close` all present, the same
    /// shape the real fixture uses — so this fixture's own tests can cover
    /// both the owned (`on open`) and borrowed (`on message`/`on close`,
    /// P6.13, #1179) `connection` cases. A held `Connection` needs real
    /// disposal to certify (the linearity pass), so `on open` transfers it
    /// into a trivial `Room` agent rather than dropping it.
    fn websocket_service_fixture() -> CheckedProgram {
        checked_context_program(
            r#"
context demo

type RoomId = String
type UserId = String
type ServerFrame = { text: String }
type ClientFrame = { text: String }

actor Participant { auth = Bearer(secret = "AUTH_SECRET"), identity = UserId }

service ChatGateway from websocket(in: ClientFrame, out: ServerFrame) {
  on open (roomId: RoomId) -> Effect[()] by user: Participant {
    let _ <- connection.send(ServerFrame { text: "welcome" })
    let _ <- Room(roomId).join(user.identity, connection)
    ()
  }

  on message (roomId: RoomId, frame: ClientFrame) -> Effect[()] by user: Participant {
    let _ <- connection.send(ServerFrame { text: frame.text })
    let _ <- Room(roomId).post(user.identity, frame.text)
    ()
  }

  on close (roomId: RoomId) -> Effect[()] by user: Participant {
    let _ <- Room(roomId).leave(user.identity)
    ()
  }
}

agent Room {
  key id: RoomId
  store members: Set[UserId]
  store conns: Map[UserId, Connection[ServerFrame]]

  on call join(u: UserId, conn: Connection[ServerFrame]) -> Effect[()] {
    let _ <- members.add(u)
    let _ <- conns.put(u, conn)
    ()
  }

  on call leave(u: UserId) -> Effect[()] {
    let _ <- members.remove(u)
    let _ <- conns.remove(u)
    ()
  }

  on call post(sender: UserId, text: String) -> Effect[()] {
    let _ <- conns.parTraverse((c: Connection[ServerFrame]) => c.send(ServerFrame { text: text }))
    ()
  }
}
"#,
        )
    }

    #[test]
    fn websocket_protocol_descriptor_lowers_its_frame_types() {
        let program = websocket_service_fixture();
        let service = find_service(&program, "ChatGateway");
        let ir = lower_protocol_ir(&service.protocol, &program);
        let ProtocolIr::WebSocket { in_ty, out_ty } = ir else {
            panic!("expected ProtocolIr::WebSocket, got {:?}", ir)
        };
        let tys = &program.program().ty_intern;
        assert_eq!(in_ty.display(tys), "ClientFrame");
        assert_eq!(out_ty.display(tys), "ServerFrame");
    }

    #[test]
    fn websocket_open_handler_lowers_an_owned_connection_binding() {
        // P6.13 (#1179): the named deferral is gone — `on open` now
        // assembles a real `IrHandler` whose body's `connection.send(…)`
        // read resolves through the synthetic binding, and whose own
        // `connection` field records the owned (non-borrowed) case.
        let program = websocket_service_fixture();
        let service = find_service(&program, "ChatGateway");
        let handler = find_service_handler(service, &HandlerKind::Open);
        let ir = lower_service_handler_ir(handler, &service.protocol, &program);
        let conn = ir
            .connection
            .as_ref()
            .expect("on open must carry a ConnectionBinder");
        assert!(
            !conn.borrowed,
            "on open's connection is a fresh owned socket, not borrowed"
        );
        let tys = &program.program().ty_intern;
        assert_eq!(conn.ty.display(tys), "Connection[ServerFrame]");
        // `connection` is never in the AST-derived params — mirrors the
        // checker's own `handler.params`/`params_for_check` asymmetry.
        assert!(ir.params.iter().all(|(name, _)| name != "connection"));
        let IrExprKind::Block { stmts, .. } = &ir.body.kind else {
            panic!("expected a Block body, got {:?}", ir.body.kind)
        };
        let has_connection_local = stmts
            .iter()
            .any(|stmt| format!("{stmt:?}").contains("Local(\"connection\")"));
        assert!(
            has_connection_local,
            "expected the lowered body to resolve `connection` as a Local, got {stmts:?}"
        );
    }

    #[test]
    fn websocket_message_and_close_handlers_lower_a_borrowed_connection_binding() {
        // P6.13 (#1179): `on message`/`on close` receive the same
        // `connection` binding as `on open`, but borrowed — no disposal
        // obligation, mirroring the checker's own `borrowed_held`.
        let program = websocket_service_fixture();
        let service = find_service(&program, "ChatGateway");
        for kind in [HandlerKind::Message, HandlerKind::Close] {
            let handler = find_service_handler(service, &kind);
            let ir = lower_service_handler_ir(handler, &service.protocol, &program);
            let conn = ir
                .connection
                .as_ref()
                .unwrap_or_else(|| panic!("{kind:?} must carry a ConnectionBinder"));
            assert!(
                conn.borrowed,
                "{kind:?}'s connection is the borrowed firing socket"
            );
        }
        // Review of #1185: `on open`'s own test already pins that a
        // `connection.…` read resolves to `Local("connection")` in the
        // lowered body — but that alone only proves the *owned* case
        // reaches scope. `on message`'s own non-consuming
        // `connection.send(…)` (`borrowed_held`'s whole reason to exist)
        // pins the borrowed case identically, so a future change that
        // gated `cx.bind` on `!borrowed` would fail here, not just leave
        // an unused synthetic binding.
        let handler = find_service_handler(service, &HandlerKind::Message);
        let ir = lower_service_handler_ir(handler, &service.protocol, &program);
        let IrExprKind::Block { stmts, .. } = &ir.body.kind else {
            panic!("expected a Block body, got {:?}", ir.body.kind)
        };
        let has_connection_local = stmts
            .iter()
            .any(|stmt| format!("{stmt:?}").contains("Local(\"connection\")"));
        assert!(
            has_connection_local,
            "expected on message's lowered body to resolve `connection` as a Local, got {stmts:?}"
        );
    }

    #[test]
    fn websocket_service_item_assembles_with_every_lifecycle_handler_lowered() {
        // The named deferral used to make `lower_service_item_ir`
        // unbuildable for any real websocket service (issue #1179's own
        // framing) — this is that end-to-end assembly, now real.
        let program = websocket_service_fixture();
        let service = find_service(&program, "ChatGateway");
        let ir = lower_service_item_ir(service, &program);
        let IrItem::Service { handlers, .. } = &ir else {
            panic!("expected IrItem::Service, got {:?}", ir)
        };
        assert_eq!(handlers.len(), 3);
        assert!(handlers.iter().all(|h| h.connection.is_some()));
    }

    #[test]
    fn service_level_by_and_given_defaults_are_already_injected_before_lowering() {
        // Pins the *harness* change (the `inject_service_defaults` call in
        // `checked_context_program`) as much as the lowering itself:
        // without it, this test would fail, silently pinning the wrong
        // fact instead of catching a real gap. `lower_service_item_ir`
        // itself contains no default-inheritance logic at all — see its
        // own doc comment.
        let program = checked_context_program(
            r#"
context demo

type UserId = String

actor Buyer { auth = Internal, identity = UserId }

capability Clock {
  fn now() -> Effect[Int]
}

provides Clock = FixedClock {
  fn now() -> Effect[Int] {
    42
  }
}

service Api by u: Buyer given Clock {
  on call(ping: String) -> Effect[String] {
    Effect.pure(ping)
  }
}
"#,
        );
        let service = find_service(&program, "Api");
        let ir = lower_service_item_ir(service, &program);
        let IrItem::Service { handlers, .. } = &ir else {
            panic!("expected IrItem::Service, got {:?}", ir)
        };
        assert_eq!(handlers.len(), 1);
        let binder = handlers[0]
            .binder
            .as_ref()
            .unwrap_or_else(|| panic!("expected the service-level `by u: Buyer` to be inherited"));
        assert_eq!(binder.binder, "u");
        assert_eq!(handlers[0].given, vec!["Clock".to_string()]);
    }

    #[test]
    fn lower_event_pattern_ir_reshapes_literal_and_variant_fields() {
        // Review of #1180: the `Events` protocol path had zero coverage.
        // `lower_protocol_ir`'s own `Events` arm genuinely can't be driven
        // through `checked_context_program` — a real `from Events(E)`
        // subscription needs `consumes bynk { Events }`, which this
        // reduced harness's `CrossContextInfo::default()` doesn't support
        // (the same limitation named in `checked_context_program`'s own
        // doc comment, and in #1169's own Risks for `CommitShape::
        // FlushEvents`). `lower_event_pattern_ir` itself has no such
        // excuse: it takes no `&CheckedProgram`, resolves nothing, and
        // cannot panic — pure AST reshaping — so it's pinned directly
        // against a parsed (not checked or certified) `EventPattern`.
        let source = r#"
context demo

type Status = | Active | Inactive

event OrderPlaced = {
  status: Status,
  count: Int,
}

service Subscriber from Events(OrderPlaced { status: Active, count: 3, .. }) {
  on event(o: OrderPlaced) -> Effect[()] {
    Effect.pure(())
  }
}
"#;
        let tokens = lexer::tokenize(source).expect("lex");
        let unit = parser::parse_unit(&tokens, source).expect("parse");
        let SourceUnit::Context(ctx) = unit else {
            panic!("expected a context unit, got {unit:?}")
        };
        let service = ctx
            .items
            .iter()
            .find_map(|item| match item {
                CommonsItem::Service(s) if s.name.name == "Subscriber" => Some(s),
                _ => None,
            })
            .expect("no service named `Subscriber` in this fixture");
        let ServiceProtocol::Events { pattern, .. } = &service.protocol else {
            panic!(
                "expected ServiceProtocol::Events, got {:?}",
                service.protocol
            )
        };
        let pattern = pattern
            .as_ref()
            .expect("expected a structural pattern on this Events subscription");

        let ir = lower_event_pattern_ir(pattern);
        assert_eq!(ir.fields.len(), 2, "declaration order preserved");
        assert_eq!(ir.fields[0].0, "status");
        assert!(
            matches!(&ir.fields[0].1, EventPatternValueIr::Variant { tag } if tag == "Active"),
            "expected a bare nullary variant tag (the AST's own optional qualifying type_name \
             dropped), got {:?}",
            ir.fields[0].1
        );
        assert_eq!(ir.fields[1].0, "count");
        assert!(
            matches!(
                &ir.fields[1].1,
                EventPatternValueIr::Const(ConstVal::Int(3))
            ),
            "expected a literal Int constant, got {:?}",
            ir.fields[1].1
        );
    }

    #[test]
    fn lower_capability_item_ir_assembles_ops_in_declaration_order() {
        let program = checked_context_program(
            r#"
context demo

capability Store {
  fn get(key: String) -> Effect[Int]
  fn put(key: String, value: Int) -> Effect[()]
}
"#,
        );
        let cap = find_capability(&program, "Store");
        let ir = lower_capability_item_ir(cap, &program);
        let IrItem::Capability { def, ops } = &ir else {
            panic!("expected IrItem::Capability, got {:?}", ir)
        };
        assert_eq!(def, "Store");
        assert_eq!(ops.len(), 2, "declaration order preserved");

        assert_eq!(ops[0].name, "get");
        assert!(ops[0].type_params.is_empty());
        assert_eq!(ops[0].params.len(), 1);
        assert_eq!(ops[0].params[0].0, "key");
        assert!(matches!(
            &*program.program().ty_intern.get(ops[0].params[0].1),
            Ty::Base(bynk_syntax::ast::BaseType::String)
        ));
        // `return_ty` mirrors `IrItem::Fn::ret`'s own convention (Effect-
        // wrapped, not peeled) — `get`'s declared `Effect[Int]` resolves
        // whole, same as a fn's `ret`.
        assert!(matches!(
            &*program.program().ty_intern.get(ops[0].return_ty),
            Ty::Effect(inner) if matches!(
                &*program.program().ty_intern.get(*inner),
                Ty::Base(bynk_syntax::ast::BaseType::Int)
            )
        ));

        assert_eq!(ops[1].name, "put");
        assert_eq!(ops[1].params.len(), 2);
        assert_eq!(ops[1].params[0].0, "key");
        assert_eq!(ops[1].params[1].0, "value");
        assert!(matches!(
            &*program.program().ty_intern.get(ops[1].params[1].1),
            Ty::Base(bynk_syntax::ast::BaseType::Int)
        ));
    }

    #[test]
    fn lower_op_sig_ir_resolves_generic_op_type_params_as_rigid_vars() {
        // Review-relevant: a capability op's own `[T, …]` list is scoped to
        // the op, not the capability (`CapabilityDecl` has no `type_params`
        // of its own) — pins that `lower_op_sig_ir` seeds a fresh rigid-var
        // scope per op rather than sharing one across every op on the same
        // capability, and that a wrong scope would show up as `Ty::Unit`
        // (ADR 0334's own silent-wrong-answer failure mode), not a panic.
        let program = checked_context_program(
            r#"
context demo

capability Store {
  fn get[T](key: String) -> Effect[T]
  fn now() -> Effect[Int]
}
"#,
        );
        let cap = find_capability(&program, "Store");
        let ir = lower_capability_item_ir(cap, &program);
        let IrItem::Capability { ops, .. } = &ir else {
            panic!("expected IrItem::Capability, got {:?}", ir)
        };

        let get = ops.iter().find(|o| o.name == "get").expect("op `get`");
        assert_eq!(get.type_params, vec!["T".to_string()]);
        assert!(matches!(
            &*program.program().ty_intern.get(get.params[0].1),
            Ty::Base(bynk_syntax::ast::BaseType::String)
        ));
        assert!(
            matches!(
                &*program.program().ty_intern.get(get.return_ty),
                Ty::Effect(inner) if matches!(
                    &*program.program().ty_intern.get(*inner),
                    Ty::Var(n) if n == "T"
                )
            ),
            "expected the op's own `T` to survive as Ty::Var, not collapse to Ty::Unit"
        );

        let now = ops.iter().find(|o| o.name == "now").expect("op `now`");
        assert!(
            now.type_params.is_empty(),
            "a sibling non-generic op must not see `get`'s own `T` in scope"
        );
    }

    #[test]
    fn lower_op_sig_ir_resolves_a_generic_op_type_param_inside_a_type_argument() {
        // Review of #1182: `Effect[T]` alone only exercises `TypeRef::Effect`'s
        // arm — a bare wrapper around the var. `Box[T]` exercises
        // `TypeRef::App`'s arm instead, where `T` is a *type argument*, not
        // the whole ref, and a wrong rigid-var scope fails differently
        // (`types.get(&name.name)?` on the *bare var itself*, not on the
        // outer `Box`) — the other half of the resolution surface
        // `lower_op_sig_ir`'s own doc comment claims to cover.
        let program = checked_context_program(
            r#"
context demo

type Box[A] = { value: A }

capability Store {
  fn get[T](box: Box[T]) -> Effect[T]
}
"#,
        );
        let cap = find_capability(&program, "Store");
        let ir = lower_capability_item_ir(cap, &program);
        let IrItem::Capability { ops, .. } = &ir else {
            panic!("expected IrItem::Capability, got {:?}", ir)
        };
        let get = &ops[0];
        assert!(
            matches!(
                &*program.program().ty_intern.get(get.params[0].1),
                Ty::Named { name, args, .. }
                    if name == "Box"
                        && args.len() == 1
                        && matches!(
                            &*program.program().ty_intern.get(args[0]),
                            Ty::Var(n) if n == "T"
                        )
            ),
            "expected Box[T] with T resolved as a rigid Ty::Var argument, got {:?}",
            program.program().ty_intern.get(get.params[0].1)
        );
    }

    #[test]
    fn lower_op_sig_ir_falls_back_to_unit_on_an_unresolvable_type_like_the_checker_does() {
        // Review of #1182: a capability op's own param/return types are
        // never actually resolution-checked upstream (the resolver skips
        // `CommonsItem::Capability`, and `check_capability_decls` only
        // records refs, never errors on a miss) — so `Bogus` here
        // certifies today, and `lower_op_sig_ir` must mirror the checker's
        // own `build_capability_op_info` fallback (`Ty::Unit`) rather than
        // panic on a state that is, in fact, reachable from source.
        let program = checked_context_program(
            r#"
context demo

capability Store {
  fn get(key: Bogus) -> Effect[Bogus]
}
"#,
        );
        let cap = find_capability(&program, "Store");
        // Would already have panicked inside `checked_context_program`'s own
        // `.expect("certify")` if this were rejected upstream — reaching
        // here at all is part of what this test pins.
        let ir = lower_capability_item_ir(cap, &program);
        let IrItem::Capability { ops, .. } = &ir else {
            panic!("expected IrItem::Capability, got {:?}", ir)
        };
        assert!(matches!(
            &*program.program().ty_intern.get(ops[0].params[0].1),
            Ty::Unit
        ));
        assert!(matches!(
            &*program.program().ty_intern.get(ops[0].return_ty),
            Ty::Unit
        ));
    }

    #[test]
    fn lower_op_sig_ir_agrees_with_the_checkers_own_capability_op_info() {
        // Review of #1182: `OpSig`'s own doc comment states this mirrors
        // `CapabilityOpInfo` — same per-op var seeding, same `types` map,
        // same non-peeled `return_ty`. Checks the stated invariant directly
        // against the checker's own constructor rather than leaving it only
        // asserted in prose, so a later change to either side's rigid-var
        // seeding fails this test instead of silently drifting.
        let program = checked_context_program(
            r#"
context demo

capability Store {
  fn get[T](key: String) -> Effect[T]
}
"#,
        );
        let cap = find_capability(&program, "Store");
        let ir = lower_capability_item_ir(cap, &program);
        let IrItem::Capability { ops, .. } = &ir else {
            panic!("expected IrItem::Capability, got {:?}", ir)
        };
        let op = &ops[0];

        let info = context_checks::build_capability_op_info(
            &cap.ops[0],
            &program.program().types,
            &program.program().ty_intern,
        );

        assert_eq!(op.name, info.name);
        assert_eq!(op.type_params, info.type_params);
        assert_eq!(
            op.params.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
            info.param_names
        );
        assert_eq!(
            op.params.iter().map(|(_, t)| *t).collect::<Vec<_>>(),
            info.params
        );
        assert_eq!(op.return_ty, info.return_ty);
    }

    #[test]
    fn lower_provider_item_ir_assembles_bynk_ops_and_given_in_declaration_order() {
        // `given Random, Clock` (reverse-alphabetical, deliberately) pins
        // that `given`'s own declaration order survives — neither op body
        // below calls either capability, pinning review of #1186's point
        // that an *unused* `given` entry must still appear (it feeds R8.1's
        // own `deps` constructor, not anything the op bodies reference).
        let program = checked_context_program(
            r#"
context demo

capability Clock {
  fn now() -> Effect[Int]
}

capability Random {
  fn next() -> Effect[Int]
}

capability Store {
  fn get(key: String) -> Effect[Int]
  fn put(key: String, value: Int) -> Effect[()]
}

provides Store = MemStore given Random, Clock {
  fn get(key: String) -> Effect[Int] {
    Effect.pure(0)
  }
  fn put(key: String, value: Int) -> Effect[()] {
    Effect.pure(())
  }
}
"#,
        );
        let provider = find_provider(&program, "MemStore");
        let ir = lower_provider_item_ir(provider, &program);
        let IrItem::Provider { def, cap, body } = &ir else {
            panic!("expected IrItem::Provider, got {:?}", ir)
        };
        assert_eq!(def, "MemStore");
        assert_eq!(cap, "Store");
        let ProviderBody::Bynk { given, ops } = body else {
            panic!("expected ProviderBody::Bynk, got {:?}", body)
        };
        assert_eq!(
            given.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
            vec!["Random", "Clock"],
            "given's own declaration order preserved, unused entries included"
        );
        assert!(given.iter().all(|g| g.context.is_none()));
        assert_eq!(ops.len(), 2, "declaration order preserved");

        assert_eq!(ops[0].name, "get");
        assert_eq!(ops[0].params.len(), 1);
        assert_eq!(ops[0].params[0].0, "key");
        assert!(matches!(
            &*program.program().ty_intern.get(ops[0].params[0].1),
            Ty::Base(bynk_syntax::ast::BaseType::String)
        ));
        // `return_ty` mirrors `OpSig::return_ty`'s own convention
        // (Effect-wrapped, not peeled).
        assert!(matches!(
            &*program.program().ty_intern.get(ops[0].return_ty),
            Ty::Effect(inner) if matches!(
                &*program.program().ty_intern.get(*inner),
                Ty::Base(bynk_syntax::ast::BaseType::Int)
            )
        ));

        assert_eq!(ops[1].name, "put");
        assert_eq!(ops[1].params.len(), 2);
        assert_eq!(ops[1].params[0].0, "key");
        assert_eq!(ops[1].params[1].0, "value");
    }

    #[test]
    fn lower_provider_item_ir_external_provider_has_no_ops() {
        // v0.17: an external (bodiless) provider is only legal inside an
        // `adapter` unit (`bynk-check/src/symbols.rs:438`), which this test
        // harness's own `checked_context_program` cannot build (it only
        // ever parses a `context` — feedback memory "bynk-emit test harness
        // scope"). `lower_provider_item_ir`'s own `External` branch never
        // reads `program` at all, so this hand-constructs the `ProviderDecl`
        // the parser would produce for `provides Store = ExternalStore`
        // inside an adapter, the same way `project_model.rs`'s own
        // `provider()` test helper does, rather than growing a second,
        // adapter-shaped fixture builder for a branch this pass never
        // touches `program` on.
        let program = checked_context_program("context demo\n");
        let provider = ProviderDecl {
            capability: bynk_syntax::ast::Ident {
                name: "Store".to_string(),
                span: Span::default(),
            },
            provider_name: bynk_syntax::ast::Ident {
                name: "ExternalStore".to_string(),
                span: Span::default(),
            },
            // Review of #1187's own Provider given/deps-wiring slice: an
            // external provider's own `given` is populated the same way a
            // Bynk one's is (nothing in the grammar or checker gates it on
            // `external`) — non-empty here specifically to pin that
            // `ProviderBody::External` now carries it, where it used to be
            // silently dropped (a bare-unit variant with nowhere to put it).
            given: vec![CapRef {
                context: None,
                name: bynk_syntax::ast::Ident {
                    name: "Clock".to_string(),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            ops: Vec::new(),
            external: true,
            documentation: None,
            span: Span::default(),
            trivia: Default::default(),
        };

        let ir = lower_provider_item_ir(&provider, &program);
        let IrItem::Provider { def, cap, body } = &ir else {
            panic!("expected IrItem::Provider, got {:?}", ir)
        };
        assert_eq!(def, "ExternalStore");
        assert_eq!(cap, "Store");
        let ProviderBody::External { given } = body else {
            panic!("expected ProviderBody::External, got {:?}", body)
        };
        assert_eq!(given.len(), 1);
        assert_eq!(given[0].context, None);
        assert_eq!(given[0].name, "Clock");
    }

    #[test]
    fn lower_provider_op_ir_lowers_a_given_capability_call_via_the_ordinary_callee_path() {
        // Pins `IrItem`'s own doc comment: a provider op's `given`
        // capabilities need no scope entry of their own — a
        // `Callee::Capability`-classified call already lowers correctly
        // through the ordinary `lower_block_ir`/`lower_expr_ir` path, the
        // same claim `lower_handler_body_ir`'s own doc comment makes for
        // handler bodies.
        let program = checked_context_program(
            r#"
context demo

capability Clock {
  fn now() -> Effect[Int]
}

capability Store {
  fn get(key: String) -> Effect[Int]
}

provides Store = MemStore given Clock {
  fn get(key: String) -> Effect[Int] {
    Clock.now()
  }
}
"#,
        );
        let provider = find_provider(&program, "MemStore");
        let ir = lower_provider_item_ir(provider, &program);
        let IrItem::Provider { body, .. } = &ir else {
            panic!("expected IrItem::Provider, got {:?}", ir)
        };
        let ProviderBody::Bynk { given, ops } = body else {
            panic!("expected ProviderBody::Bynk, got {:?}", body)
        };
        assert_eq!(given.len(), 1);
        assert_eq!(given[0].name, "Clock");
        let get = &ops[0];
        let tail = fn_tail(&get.body);
        let IrExprKind::Call { callee, args, .. } = &tail.kind else {
            panic!("expected Call, got {:?}", tail.kind)
        };
        assert!(matches!(
            callee,
            Callee::Capability { cap, op } if cap == "Clock" && op == "now"
        ));
        assert!(args.is_empty());
    }

    #[test]
    fn lower_cap_ref_ir_local_capability_has_no_context() {
        let cap_ref = CapRef {
            context: None,
            name: bynk_syntax::ast::Ident {
                name: "Clock".to_string(),
                span: Span::default(),
            },
            span: Span::default(),
        };
        let ir = lower_cap_ref_ir(&cap_ref);
        assert_eq!(ir.context, None);
        assert_eq!(ir.name, "Clock");
    }

    #[test]
    fn lower_cap_ref_ir_preserves_a_cross_context_prefix() {
        // `given B.Cap` (v0.15) is out of `checked_context_program`'s own
        // fixture scope (no cross-context `uses`/`consumes`, feedback
        // memory "bynk-emit test harness scope") — pins `lower_cap_ref_ir`'s
        // own `QualifiedName -> String` flattening directly against a
        // hand-built `CapRef`, the same posture the external-provider test
        // above already takes for a branch the fixture cannot reach.
        let cap_ref = CapRef {
            context: Some(QualifiedName {
                parts: vec![bynk_syntax::ast::Ident {
                    name: "Billing".to_string(),
                    span: Span::default(),
                }],
                span: Span::default(),
            }),
            name: bynk_syntax::ast::Ident {
                name: "Ledger".to_string(),
                span: Span::default(),
            },
            span: Span::default(),
        };
        let ir = lower_cap_ref_ir(&cap_ref);
        assert_eq!(ir.context.as_deref(), Some("Billing"));
        assert_eq!(ir.name, "Ledger");
    }

    #[test]
    fn lower_provider_op_ir_binds_its_own_param_into_scope() {
        // Review of #1186: every other provider-op test's own body ignores
        // its params, so `cx.bind` (the one line seeding the scope
        // `lower_ident_ir` needs — the reason this function is a sibling of
        // `lower_fn_body_ir` rather than a widening of it) could regress
        // silently. Pins a param reference resolving to `Local`, not the
        // unresolved-ident `todo!()` or a same-named-global misclassification.
        let program = checked_context_program(
            r#"
context demo

capability Store {
  fn get(key: String) -> Effect[String]
}

provides Store = MemStore {
  fn get(key: String) -> Effect[String] {
    Effect.pure(key)
  }
}
"#,
        );
        let provider = find_provider(&program, "MemStore");
        let ir = lower_provider_item_ir(provider, &program);
        let IrItem::Provider { body, .. } = &ir else {
            panic!("expected IrItem::Provider, got {:?}", ir)
        };
        let ProviderBody::Bynk { ops, .. } = body else {
            panic!("expected ProviderBody::Bynk, got {:?}", body)
        };
        let get = &ops[0];
        let key_ty = get.params[0].1;
        let tail = fn_tail(&get.body);
        // `Effect.pure(key)` desugars to `IrExprKind::Pure`, wrapping `key`
        // as its own sole value.
        let IrExprKind::Pure { value } = &tail.kind else {
            panic!("expected Pure, got {:?}", tail.kind)
        };
        assert!(
            matches!(
                &value.kind,
                IrExprKind::Local(name) if name == "key"
            ),
            "expected the op's own `key` param to resolve as a bound Local, got {:?}",
            value.kind
        );
        assert_eq!(
            value.ty, key_ty,
            "the resolved Local's type must be the same TyId bind() seeded"
        );
    }

    /// Review of #1229 (#1228): `lower_route_cache_ir`/`lower_route_limit_ir`
    /// take no `&CheckedProgram`, resolve nothing, and cannot panic — the same
    /// posture `lower_event_pattern_ir`'s own test above already established a
    /// parsed-not-checked fixture for, and for the identical reason here: their
    /// defensive branches (a non-`GET` handler, a `maxAge`-less `@cache`, a
    /// non-positive `maxBody`) are exactly the shapes `bynk-check`'s
    /// `bynk.http.cache_on_non_get`/`cache_bad_max_age`/`limit_bad_max_body`
    /// (`bynk-check/src/context_checks.rs`) already reject, so a real checked
    /// program can never reach them and the fixture bless run never exercises
    /// them either.
    fn parsed_only_context(source: &str) -> bynk_syntax::ast::Context {
        let tokens = lexer::tokenize(source).expect("lex");
        let unit = parser::parse_unit(&tokens, source).expect("parse");
        let SourceUnit::Context(ctx) = unit else {
            panic!("expected a context unit, got {unit:?}")
        };
        ctx
    }

    fn parsed_handler<'a>(
        ctx: &'a bynk_syntax::ast::Context,
        service: &str,
        index: usize,
    ) -> &'a Handler {
        let service = ctx
            .items
            .iter()
            .find_map(|item| match item {
                CommonsItem::Service(s) if s.name.name == service => Some(s),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no service named `{service}` in this fixture"));
        &service.handlers[index]
    }

    #[test]
    fn lower_route_cache_ir_reads_maxage_and_scope_off_a_get_handler() {
        let ctx = parsed_only_context(
            r#"
context demo

service Api from http {
  @cache(maxAge: 5.minutes, scope: public)
  on GET("/config") () -> Effect[HttpResult[String]] by v: Visitor {
    Ok("cfg")
  }

  @cache(maxAge: 30.seconds)
  on GET("/private") () -> Effect[HttpResult[String]] by v: Visitor {
    Ok("priv")
  }

  on GET("/plain") () -> Effect[HttpResult[String]] by v: Visitor {
    Ok("plain")
  }
}
"#,
        );
        let public_cache = lower_route_cache_ir(parsed_handler(&ctx, "Api", 0))
            .unwrap_or_else(|| panic!("expected Some(CacheIr) for a well-formed @cache"));
        assert_eq!(public_cache.max_age_secs, 300, "5.minutes in whole seconds");
        assert_eq!(public_cache.scope, "public");

        let default_scope_cache = lower_route_cache_ir(parsed_handler(&ctx, "Api", 1))
            .unwrap_or_else(|| panic!("expected Some(CacheIr) with no explicit scope:"));
        assert_eq!(default_scope_cache.max_age_secs, 30);
        assert_eq!(
            default_scope_cache.scope, "private",
            "no scope: argument written — must default to private"
        );

        assert!(
            lower_route_cache_ir(parsed_handler(&ctx, "Api", 2)).is_none(),
            "no @cache annotation at all must yield None"
        );
    }

    #[test]
    fn lower_route_cache_ir_returns_none_for_a_non_get_handler_even_with_a_cache_annotation() {
        // `bynk.http.cache_on_non_get` already rejects this at check time — this
        // pins the lowering function's own independent guard, not reachable
        // through a real certified program.
        let ctx = parsed_only_context(
            r#"
context demo

service Api from http {
  @cache(maxAge: 5.minutes)
  on POST("/items") (body: String) -> Effect[HttpResult[String]] by v: Visitor {
    Created(body)
  }
}
"#,
        );
        assert!(
            lower_route_cache_ir(parsed_handler(&ctx, "Api", 0)).is_none(),
            "a @cache on a non-GET handler must not construct a CacheIr"
        );
    }

    #[test]
    fn lower_route_cache_ir_discards_an_otherwise_well_formed_scope_when_maxage_is_missing() {
        // `bynk.http.cache_bad_max_age` already rejects a maxAge-less @cache at
        // check time — this pins that `scope`'s own well-formedness does not
        // rescue a missing `maxAge` into a partial CacheIr.
        let ctx = parsed_only_context(
            r#"
context demo

service Api from http {
  @cache(scope: public)
  on GET("/broken") () -> Effect[HttpResult[String]] by v: Visitor {
    Ok("x")
  }
}
"#,
        );
        assert!(
            lower_route_cache_ir(parsed_handler(&ctx, "Api", 0)).is_none(),
            "a well-formed scope: must not survive a missing maxAge:"
        );
    }

    #[test]
    fn lower_route_limit_ir_reads_maxbody_off_a_route_annotation() {
        let ctx = parsed_only_context(
            r#"
context demo

service Api from http {
  @limit(maxBody: 26_214_400)
  on POST("/bulk") (body: String) -> Effect[HttpResult[String]] by v: Visitor {
    Created(body)
  }

  on POST("/upload") (body: String) -> Effect[HttpResult[String]] by v: Visitor {
    Created(body)
  }
}
"#,
        );
        assert_eq!(
            lower_route_limit_ir(parsed_handler(&ctx, "Api", 0)),
            Some(26_214_400)
        );
        assert!(
            lower_route_limit_ir(parsed_handler(&ctx, "Api", 1)).is_none(),
            "no @limit annotation at all must yield None — the caller applies the \
             service-wide default, this function does not know it"
        );
    }

    #[test]
    fn lower_route_limit_ir_returns_none_for_a_non_positive_maxbody() {
        // `bynk.http.limit_bad_max_body` already rejects a non-positive maxBody
        // at check time — this pins the lowering function's own independent
        // guard. `None` here matters specifically because the caller's own
        // fallback composition (`effective_max_body`) treats it as "no
        // route-level override," falling through to the service-wide default —
        // not as "an explicit zero-byte cap."
        let ctx = parsed_only_context(
            r#"
context demo

service Api from http {
  @limit(maxBody: 0)
  on POST("/zero") (body: String) -> Effect[HttpResult[String]] by v: Visitor {
    Created(body)
  }
}
"#,
        );
        assert!(
            lower_route_limit_ir(parsed_handler(&ctx, "Api", 0)).is_none(),
            "a non-positive maxBody must not construct Some(0)"
        );
    }
}
