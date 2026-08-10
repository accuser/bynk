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
    BinOp, Block, Expr, ExprId, ExprKind, FieldInit, FnDecl, FnName, LambdaExpr, Statement,
    TypeBody, UnaryOp,
};
use bynk_syntax::span::Span;

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
    /// Bumped once per [`lower_record_spread_ir`] call — a fixed temp name
    /// is unique against every *source-level* name (`__` is unlexable) but
    /// not against another spread on the same nesting chain (a spread
    /// inside an override value, e.g. `Point { ...p, x: (Point { ...q, x:
    /// 0 }).x }`); a monotonic per-function suffix removes that hazard.
    spread_tmp_counter: usize,
}

impl<'a> LowerIrCtx<'a> {
    fn new(program: &'a CheckedProgram, type_vars: HashSet<String>) -> Self {
        Self {
            program: program.program(),
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
        // `cell := expr` — the unconditional `Cell` write. `Callee::Store`
        // now exists (P6.2, #1143), but `Statement` carries no `ExprId` of
        // its own to key a `Callee` sink by (the sink is `HashMap<ExprId,
        // Callee>`) — `:=`'s own checker-side validation
        // (`checker.rs`'s `Statement::Assign` arm) never reaches an
        // `Expr`-shaped call site the way a `.put(...)`-style store method
        // call does. A real `IrStmt` target for this needs its own
        // identity/sink shape, which P6.7 ("store-field state shape and
        // index tables derived in the IR") is better positioned to design
        // deliberately than re-deriving one here from a single statement
        // kind.
        Statement::Assign(_) => {
            todo!("Statement::Assign (a Cell `:=` write) has no ExprId to key a Callee by — P6.7")
        }
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
        // the bundled comparison/arithmetic `todo!()` below, which P6.2
        // (#1143) already confirmed has no dedicated IrExprKind and no Callee
        // classification either.
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

        // Every arm below is genuinely out of this slice's scope — see
        // design/tracks/the-ir.md §6 for which future slice covers it.
        ExprKind::BinOp(
            BinOp::Eq
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
            "comparison/arithmetic BinOp has no dedicated IrExprKind in Part 6.2 and no Callee \
             classification either — P6.2 (#1143) confirmed neither exists, and P6.3 (#1145) \
             didn't name it either (only Implies, the row's one BinOp, was in scope); stays a gap \
             for whichever future slice actually names it"
        ),
        ExprKind::UnaryOp(UnaryOp::Neg, _) => {
            todo!("UnaryOp::Neg has no Callee classification — same gap as arithmetic BinOp above")
        }
        ExprKind::InterpStr(_) => todo!("interpolated strings — not named by any P6.x rule yet"),
        ExprKind::Call {
            type_args, args, ..
        } => lower_call_ir(e, None, type_args, args, cx),
        ExprKind::Lambda(lambda) => lower_lambda_ir(e, lambda, cx),
        ExprKind::Ok(_) | ExprKind::Err(_) | ExprKind::Some(_) | ExprKind::None => {
            todo!(
                "Result/Option constructors — P6.2 (#1143) confirmed these are not yet \
                 Callee-classified by P6.0 either, so ir::lower still can't drive them; P6.3 \
                 (#1145, Decision B) named the deeper blocker too — Option/Result are built-in \
                 Ty variants with no Arc<TypeDecl> to put in IrExprKind::Variant's `sum` field \
                 the way a user-declared sum has, and no ADR has decided how to represent that \
                 identity yet"
            )
        }
        ExprKind::Question(_) => todo!(
            "`?` propagation desugars to a real Match (scrutinee: e, arms: Ok(v) => v, Err(e) \
             => Return(Err(...))) — genuinely gated on P6.4's Pattern IR, not P6.1: \
             IrArm/Exhaustive/MatchForm are still uninhabited placeholder enums (ir.rs), so no \
             value of Match's own arms field is constructible yet (P6.3, #1145, Decision A)"
        ),
        ExprKind::ConstructorCall { args, .. } => lower_call_ir(e, None, &[], args, cx),
        ExprKind::MethodCall {
            receiver,
            type_args,
            args,
            ..
        } => lower_call_ir(e, Some(receiver), type_args, args, cx),
        ExprKind::Match { .. } => todo!("Match — P6.4/P6.5 (Pattern IR + Match lowering)"),
        ExprKind::Is { .. } => todo!(
            "`is` pattern test desugars to a real Match (per the reference's own table) — same \
             P6.4 Pattern IR gate as `?` above, not buildable on P6.1 alone (P6.3, #1145, \
             Decision A)"
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
    if let Callee::Ctor { sum, tag } = callee {
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
        return IrExpr {
            kind: IrExprKind::Variant {
                sum,
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
            let IrExprKind::Variant { sum, tag, payload } = &tail.kind else {
                panic!("{fn_name}: expected Variant, got {:?}", tail.kind)
            };
            assert_eq!(sum.name.name, "Shape");
            assert_eq!(tag, "Circle");
            assert_eq!(payload.len(), 1);
            assert!(matches!(&payload[0].kind, IrExprKind::Local(n) if n == "n"));
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
}
