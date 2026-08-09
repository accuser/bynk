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

use bynk_check::checker::{self, CheckedProgram, Ty, TyId, TypedCommons};
use bynk_syntax::ast::{
    BinOp, Block, Expr, ExprId, ExprKind, FnDecl, FnName, Statement, TypeBody, UnaryOp,
};

use crate::ir::{ConstVal, GlobalRef, IrExpr, IrExprKind, IrStmt};

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
}

impl<'a> LowerIrCtx<'a> {
    fn new(program: &'a CheckedProgram, type_vars: HashSet<String>) -> Self {
        Self {
            program: program.program(),
            scopes: vec![HashMap::new()],
            type_vars,
        }
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
}

/// Lower a function/method body: seeds scope from `f`'s params (and its own
/// rigid type variables — its own `[T, ...]` type parameters, plus a
/// generic receiver's, for a method), lowers the body as an ordinary value
/// block, then wraps the tail in [`IrExprKind::Return`] — the one place
/// this pass builds a `Return` node (Bynk has no `return` keyword; see that
/// variant's own doc comment). Distinct from [`lower_block_ir`], which
/// lowers a *nested* block as a bare value with no such wrapping.
///
/// Handler bodies are out of scope for this entry point: a handler's own
/// non-local bare-ident forms (store-field/cell reads, agent `self`, the
/// actor binder, transition `old`/`new`) need resolved-identity plumbing
/// (`store_fields`, `agent_state_ty`, `actor_binding` — the `Ctx` fields
/// `check_handler_body` seeds, `checker.rs:930-960`) this pass has no
/// parameter for and does not commission (Decision C) — calling this on a
/// handler body would silently misclassify any bare ident matching one of
/// those forms as a `Local`/`Global` miss (`todo!()`) rather than the
/// specific kind it actually is, which is why no such entry point exists
/// yet rather than one that would produce a wrong tree.
pub(crate) fn lower_fn_body_ir(f: &FnDecl, program: &CheckedProgram) -> IrExpr {
    let mut type_vars: HashSet<String> = f
        .type_params
        .iter()
        .map(|tp| tp.name.name.clone())
        .collect();
    if let FnName::Method { type_name, .. } = &f.name
        && let Some(decl) = program.program().types.get(&type_name.name)
    {
        type_vars.extend(decl.type_params.iter().map(|tp| tp.name.name.clone()));
    }
    let mut cx = LowerIrCtx::new(program, type_vars);
    // `self` is never in `f.params` (`FnDecl::has_self` gates it) — mirrors
    // `check_fn`'s own binding (`checker.rs`, the `f.has_self` arm): a
    // generic receiver's `self` carries the receiver applied to its own
    // rigid type variables (`Box[A]`'s `self`, not a fixed `Box[Int]`).
    if let FnName::Method { type_name, .. } = &f.name
        && f.has_self
        && let Some(decl) = program.program().types.get(&type_name.name)
    {
        let self_args = decl
            .type_params
            .iter()
            .map(|tp| cx.program.ty_intern.intern(Ty::Var(tp.name.name.clone())))
            .collect();
        let self_ty = checker::named_ty_with_args(decl, self_args, &cx.program.ty_intern);
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
        // A `Cell` store write — Callee::Store territory (P6.2's row names
        // `Callee::Store`-detected writes directly; P6.7 gives store fields
        // their own IR shape).
        Statement::Assign(_) => todo!(
            "Statement::Assign (a Cell store write) is Callee::Store/StoreFieldIr territory — P6.2/P6.7"
        ),
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

        // Every arm below is genuinely out of this slice's scope — see
        // design/tracks/the-ir.md §6 for which future slice covers it.
        ExprKind::BinOp(
            BinOp::Implies
            | BinOp::Eq
            | BinOp::NotEq
            | BinOp::Lt
            | BinOp::LtEq
            | BinOp::Gt
            | BinOp::GtEq
            | BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div,
            ..,
        ) => todo!(
            "comparison/arithmetic BinOp has no dedicated IrExprKind in Part 6.2 — routes through \
             Callee::Kernel once P6.2 lands (Implies desugars to Or(Not, _), P6.3's row)"
        ),
        ExprKind::UnaryOp(UnaryOp::Neg, _) => {
            todo!("UnaryOp::Neg — Callee::Kernel territory, P6.2")
        }
        ExprKind::InterpStr(_) => todo!("interpolated strings — not named by any P6.x rule yet"),
        ExprKind::Call { .. } => todo!("Call — P6.2, driven by Callee"),
        ExprKind::Lambda(_) => todo!("Lambda — P6.2"),
        ExprKind::Ok(_) | ExprKind::Err(_) | ExprKind::Some(_) | ExprKind::None => {
            todo!(
                "Result/Option constructors — P6.2 (Callee::Ctor-adjacent), not yet Callee-classified by P6.0 either"
            )
        }
        ExprKind::Question(_) => todo!("`?` propagation — P6.3's desugaring table (R6.7-R6.9)"),
        ExprKind::ConstructorCall { .. } => {
            todo!("qualified static/variant call — P6.2, driven by Callee")
        }
        ExprKind::MethodCall { .. } => todo!("method call — P6.2, driven by Callee"),
        ExprKind::Match { .. } => todo!("Match — P6.4/P6.5 (Pattern IR + Match lowering)"),
        ExprKind::Is { .. } => todo!("`is` pattern test — P6.3's desugaring table"),
        ExprKind::RecordSpread { .. } => todo!("record spread — P6.3's desugaring table (R6.7)"),
        ExprKind::Expect(_) => todo!("`expect` expression — not named by any P6.x rule yet"),
        ExprKind::Val { .. } => todo!("`Val[T]` — test-only, not named by any P6.x rule yet"),
        ExprKind::Wire(_) => todo!("`Wire(...)` — test-only, not named by any P6.x rule yet"),
        ExprKind::Observation(_) => {
            todo!("capability-call observation — not named by any P6.x rule yet")
        }
        ExprKind::Trace { .. } => todo!("`trace(...)` — not named by any P6.x rule yet"),
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

/// Decision E: targeted minimal fixtures, one per node kind this slice
/// covers, staying strictly inside the subset [`lower_expr_ir`]/
/// [`lower_stmt_ir`] actually implement — not a walk over the real
/// `bynkc/tests/fixtures/positive` corpus, which hits an unimplemented
/// `Match`/`Call` arm within a few lines of almost any real fixture.
#[cfg(test)]
mod tests {
    use super::*;
    use bynk_check::checker::CheckedProgram;
    use bynk_check::{checker, resolver};
    use bynk_syntax::ast::{CommonsItem, FnDecl};
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
}
