//! Expression and statement lowering — the `LowerCtx`-driven engine that
//! turns Bynk expressions, statements, matches, and mocks into TypeScript
//! source. Split out of `emitter.rs` (ADR 0060); `LowerCtx` and the `ts_*`
//! type renderers stay in the parent and are reached via `use super::*`.

use std::collections::HashSet;
use std::sync::Arc;

use bynk_check::checker::{Callee, NamedKind, Ty, TyId, TypedCommons};
use bynk_syntax::ast::{
    BaseType, BinOp, Block, Expr, ExprKind, FieldInit, Ident, InterpPart, LambdaExpr, LiteralValue,
    MatchArm, MatchBody, ObservationExpr, ObservationMatcher, Pattern, PatternBindingKind,
    PredKind, Refinement, Statement, TypeBody, TypeDecl, TypeRef, UnaryOp,
};

use crate::ir::{ConstVal, EventPatternIr, EventPatternValueIr};

use super::*;

/// Lower a block to a sequence of TypeScript statements suitable for use as
/// an async function body. Used by v0.7 mock-operation emission — today
/// exclusively test/property/contract scaffolding (`stub` rhs values, `where`
/// predicates, contract `requires` guards), never real production provider
/// bodies, so `test_scaffold` is set unconditionally (nothing here emits an
/// `assert`, so `assert_loc` stays `None`) purely to mark test-scaffold mode
/// for `in_test_scaffold()` — otherwise a refined-literal admitted here (e.g.
/// a `stub Cap.op() returns "lit"` rhs) brands via the production `(v as T)`
/// cast, which cannot resolve `T` in a stub class's `any`-typed destructure
/// (Locale capability track, slice 1, #844).
pub fn lower_block_to_async_body(
    block: &Block,
    return_type: &TypeRef,
    typed: &mut TypedCommons,
    cross_context: &bynk_check::resolver::CrossContextInfo,
    runtime_use: &RuntimeUse,
) -> (String, SourceMapBuilder) {
    let mut out = String::new();
    // v0.70: a sub-builder records body checkpoints relative to this local buffer;
    // the caller merges it into the module map at the splice offset.
    let smb = RefCell::new(SourceMapBuilder::new());
    {
        let mut cx = LowerCtx::new(
            ModuleCtx::new(typed, cross_context, runtime_use),
            BodyMode::PredicateScaffold {
                test: TestShared {
                    test_scaffold: true,
                    assert_loc: None,
                },
            },
        )
        .with_source_map(Some(&smb));
        let async_tail = is_effectful_return(return_type);
        emit_block_as_function_body_with_return(
            &mut out,
            block,
            &mut cx,
            0,
            async_tail,
            Some(return_type),
        );
    }
    (out, smb.into_inner())
}

/// Lower a test-case body: statements followed by a discarded tail expression
/// (the runner records success via the assertion mechanism, not via a return
/// value). Used by v0.7 test emission.
#[allow(clippy::too_many_arguments)]
pub fn lower_test_case_body(
    block: &Block,
    typed: &mut TypedCommons,
    cross_context: &bynk_check::resolver::CrossContextInfo,
    test_services: HashSet<String>,
    test_service_handlers: HashMap<String, Vec<crate::ir::IrHandlerKind>>,
    test_agents: HashSet<String>,
    source: &str,
    rel_path: &str,
    runtime_use: &RuntimeUse,
) -> (String, SourceMapBuilder) {
    let mut out = String::new();
    let smb = RefCell::new(SourceMapBuilder::new());
    {
        let mut cx = LowerCtx::new(
            ModuleCtx::new(typed, cross_context, runtime_use),
            BodyMode::TestCase {
                test: TestShared {
                    test_scaffold: true,
                    assert_loc: Some(crate::emitter::AssertLoc {
                        source: source.to_string(),
                        rel_path: rel_path.to_string(),
                    }),
                },
                // v0.117: observations and `trace` in the body read the per-case
                // recorded trace object declared by the runner scaffold.
                observation_trace: Some("__obs".to_string()),
                test_services,
                test_service_handlers,
            },
        )
        .with_source_map(Some(&smb));
        cx.local_agents = test_agents;
        for stmt in &block.statements {
            emit_statement(&mut out, stmt, &mut cx, 0);
        }
        // Evaluate the tail expression but discard its value; assertions inside
        // it still take effect via thrown AssertionErrors.
        cx.record_span(out.len(), block.tail.span);
        let mut pre = Pre::new();
        let tail = pre.lower(&block.tail, &mut cx);
        for s in pre.stmts() {
            write_line(&mut out, 0, s);
        }
        if !tail.is_empty() && tail != "undefined" {
            write_line(&mut out, 0, &format!("void ({tail});"));
        }
    }
    (out, smb.into_inner())
}

/// v0.16: lower an integration test case body. Like [`lower_test_case_body`],
/// but in **workers** mode and from a synthetic harness root: entry calls
/// (`ctx.service(args)`) are cross-context calls that lower to `callService(
/// deps.env.<BINDING>, …)` over the real wire. The harness root declares no
/// local services/agents, so those scoped sets stay empty; `cross_context`
/// carries every participant's service surface.
#[allow(clippy::too_many_arguments)]
pub fn lower_integration_case_body(
    block: &Block,
    typed: &mut TypedCommons,
    cross_context: &bynk_check::resolver::CrossContextInfo,
    system_http_services: std::collections::HashSet<String>,
    system_http_routes: std::collections::HashSet<(String, String, String)>,
    system_http_route_body: HashMap<(String, String, String), (usize, bynk_syntax::ast::TypeRef)>,
    system_http_type_ns: String,
    source: &str,
    rel_path: &str,
    runtime_use: &RuntimeUse,
) -> (String, SourceMapBuilder) {
    let mut out = String::new();
    let smb = RefCell::new(SourceMapBuilder::new());
    {
        let mut module = ModuleCtx::new(typed, cross_context, runtime_use);
        module.target = BuildTarget::Workers;
        let mut cx = LowerCtx::new(
            module,
            BodyMode::IntegrationCase {
                test: TestShared {
                    test_scaffold: true,
                    assert_loc: Some(crate::emitter::AssertLoc {
                        source: source.to_string(),
                        rel_path: rel_path.to_string(),
                    }),
                },
                system_http_services,
                system_http_routes,
                system_http_route_body,
                system_http_type_ns,
            },
        )
        .with_source_map(Some(&smb));
        for stmt in &block.statements {
            emit_statement(&mut out, stmt, &mut cx, 0);
        }
        cx.record_span(out.len(), block.tail.span);
        let mut pre = Pre::new();
        let tail = pre.lower(&block.tail, &mut cx);
        for s in pre.stmts() {
            write_line(&mut out, 0, s);
        }
        if !tail.is_empty() && tail != "undefined" {
            write_line(&mut out, 0, &format!("void ({tail});"));
        }
    }
    (out, smb.into_inner())
}

pub(crate) fn emit_block_as_function_body(
    out: &mut String,
    block: &Block,
    cx: &mut LowerCtx,
    indent: usize,
    async_tail: bool,
) {
    // v0.154 (ADR 0178): **preserve** the enclosing `return_ty` rather than
    // resetting it. Nested control-flow blocks (an `if`/`else` or `match` arm
    // *within* a function body) reach here, and a `return` from them returns
    // from the whole function — so an embedding `?` inside them must still see
    // the function's return type. Only a genuine function/handler boundary
    // (`_with_return`) rebinds it. A top-level body with no embedding context
    // (a mock/test that never called `_with_return`) simply inherits the
    // ambient `None`.
    emit_block_inner(out, block, cx, indent, async_tail);
}

/// As [`emit_block_as_function_body`], but records the enclosing return type on
/// the `LowerCtx` for the duration of the body so the `?` lowering can apply a
/// declared error embedding (v0.154, ADR 0178). The return type is set and then
/// restored, so a *sibling* body (a lambda, a mock) does not inherit the wrong
/// one; nested control-flow blocks reached via the shim above keep it.
pub(crate) fn emit_block_as_function_body_with_return(
    out: &mut String,
    block: &Block,
    cx: &mut LowerCtx,
    indent: usize,
    async_tail: bool,
    return_type: Option<&TypeRef>,
) {
    let tys = cx.commons().tys();
    let prev = cx.return_ty.take();
    cx.return_ty = return_type
        .and_then(|rt| bynk_check::checker::resolve_type_ref(rt, &cx.commons().types, tys));
    emit_block_inner(out, block, cx, indent, async_tail);
    cx.return_ty = prev;
}

/// v0.154 (ADR 0178): the `Err`-wrap a `?` on a `Result` operand needs, if the
/// operand's error type differs from the enclosing return's and a declared
/// embedding converts it. Returns `(sum_type, variant)` to emit
/// `Err(sum_type.variant(err))`, or `None` to propagate the `Err` unchanged.
/// Uses the checker's `embedding_for`, so it can never diverge from what the
/// checker accepted.
fn embed_conversion(
    operand_ty: Option<bynk_check::checker::TyId>,
    cx: &LowerCtx,
) -> Option<(String, String)> {
    let tys = cx.commons().tys();
    use bynk_check::checker::Ty;
    let operand_node = operand_ty.map(|t| tys.get(t));
    let Some(Ty::Result(_, e)) = operand_node.as_deref() else {
        return None;
    };
    let f_err = peel_result_err(cx.return_ty?, tys)?;
    // An exact/compatible match propagates as-is; only a genuine mismatch that a
    // declared embedding resolves gets wrapped.
    if bynk_check::checker::compatible(*e, f_err, tys) {
        return None;
    }
    bynk_check::checker::embedding_for(f_err, *e, &cx.commons().types, tys)
}

/// Peel `Result[_, E]` / `Effect[Result[_, E]]` to the error type `E`.
fn peel_result_err(
    ty: bynk_check::checker::TyId,
    tys: &Arc<Types>,
) -> Option<bynk_check::checker::TyId> {
    use bynk_check::checker::Ty;
    match &*tys.get(ty) {
        Ty::Result(_, e) => Some(*e),
        Ty::Effect(inner) => peel_result_err(*inner, tys),
        _ => None,
    }
}

fn emit_block_inner(
    out: &mut String,
    block: &Block,
    cx: &mut LowerCtx,
    indent: usize,
    async_tail: bool,
) {
    // #908: every nested block (if/else branch, match arm, lambda body) as
    // well as a function/handler's own body reaches here, so this is the one
    // place that needs to push/pop a shadow-rename frame to match the
    // language's actual block-scoping — a re-`let` frame must not survive
    // past the block it was declared in.
    cx.shadow_scopes.push(HashMap::new());
    for stmt in &block.statements {
        emit_statement(out, stmt, cx, indent);
    }
    // Tail position: match → inline switch, if → inline if, otherwise return expr.
    // Anchor the tail's generated lines to the tail expression's span (slice 1).
    cx.record_span(out.len(), block.tail.span);
    match &block.tail.kind {
        ExprKind::Match { discriminant, arms } => {
            emit_match_tail(out, discriminant, arms, cx, indent, async_tail);
        }
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } if ternary_shaped(cond, then_block, else_block, cx) => {
            // T2.1 (R6.2): the shape *could* be a ternary. Whether it actually
            // is depends on whether either branch hoists — which is now
            // answered by lowering it, not by predicting it. Both outcomes are
            // built from the same `Lowered` values, so nothing is lowered twice
            // and no temp number is burned on a discarded attempt.
            let mut pre = Pre::new();
            let cond_expr = pre.lower(cond, cx);
            let then_tail = lower_tail_expr(&then_block.tail, cx, async_tail);
            let else_tail = lower_tail_expr(&else_block.tail, cx, async_tail);
            for s in pre.stmts() {
                write_line(out, indent, s);
            }
            if then_tail.pre.is_empty() && else_tail.pre.is_empty() {
                write_line(
                    out,
                    indent,
                    &format!(
                        "return ({cond_expr} ? {t} : {e});",
                        t = then_tail.expr,
                        e = else_tail.expr
                    ),
                );
            } else {
                // A branch hoisted after all — the case `simple_expr` used to
                // mispredict (#945, and the `ListLit`/`InterpStr` fallthrough
                // this slice closes). A statement `if` is where a hoisted `?`'s
                // early return belongs, so emit that instead of a ternary that
                // has nowhere to put it. Byte-identical to what `emit_if_tail`
                // produces for this shape (both branches are statement-free and
                // their tails lower as plain expressions), rebuilt from the
                // values already in hand.
                write_line(out, indent, &format!("if ({cond_expr}) {{"));
                emit_pure_tail_branch(out, then_tail, indent + INDENT_STEP);
                write_line(out, indent, "} else {");
                emit_pure_tail_branch(out, else_tail, indent + INDENT_STEP);
                write_line(out, indent, "}");
            }
        }
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            emit_if_tail(out, cond, then_block, else_block, cx, indent, async_tail);
        }
        _ => {
            let tail = lower_tail_expr(&block.tail, cx, async_tail);
            for s in &tail.pre {
                write_line(out, indent, s);
            }
            write_line(out, indent, &format!("return {};", tail.expr));
        }
    }
    cx.shadow_scopes.pop();
}

/// Write one branch of the statement `if` above: the tail's hoisted statements
/// followed by its `return`. Exactly the body [`emit_block_inner`]'s own `_`
/// arm writes for a statement-free block, which is what `emit_if_tail` would
/// have recursed into.
fn emit_pure_tail_branch(out: &mut String, tail: Lowered, indent: usize) {
    for s in &tail.pre {
        write_line(out, indent, s);
    }
    write_line(out, indent, &format!("return {};", tail.expr));
}

/// Lower an expression that's in the tail position of a returning context.
///
/// In async-tail position (v0.7.1), an `async function` wraps its return value
/// as a Promise automatically, so `Effect.pure(...)` is redundant and should
/// emit as a bare value. Recurse through control-flow forms whose result is
/// the surrounding function's return value:
/// - `Effect.pure(x)` → lower `x` directly.
/// - A ternary-form `if`/`else` (simple branches) where each branch's tail is
///   itself an async-tail position.
/// - A pure-tail block (no statements) where the inner tail is the actual
///   returned expression.
/// - Parens (transparent).
///
/// In non-async-tail position, defer to [`lower_expr`] unchanged.
fn lower_tail_expr(e: &Expr, cx: &mut LowerCtx, async_tail: bool) -> Lowered {
    if !async_tail {
        return lower_expr(e, cx);
    }
    match &e.kind {
        ExprKind::EffectPure(inner) => lower_expr(inner, cx),
        ExprKind::Paren(inner) => lower_tail_expr(inner, cx, true),
        ExprKind::Block(b) if b.statements.is_empty() => lower_tail_expr(&b.tail, cx, true),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } if ternary_shaped(cond, then_block, else_block, cx) => {
            let mut pre = Pre::new();
            let cond_expr = pre.lower(cond, cx);
            let then_tail = lower_tail_expr(&then_block.tail, cx, true);
            let else_tail = lower_tail_expr(&else_block.tail, cx, true);
            if then_tail.pre.is_empty() && else_tail.pre.is_empty() {
                return pre.finish(format!(
                    "({cond_expr} ? {t} : {e})",
                    t = then_tail.expr,
                    e = else_tail.expr
                ));
            }
            let slot_ty = async_tail_slot_ty(e, cx);
            let value =
                hoist_if_as_statement(&mut pre, cond_expr, then_tail, else_tail, slot_ty, cx);
            pre.finish(value)
        }
        _ => lower_expr(e, cx),
    }
}

/// The slot annotation for a hoisted `if` in **async-tail** position, where
/// `checked_ty_ts` alone is wrong.
///
/// #1029 review: this function exists because an `async function` wraps its
/// return value, so `Effect.pure(x)` in tail position emits a bare `x` rather
/// than a promise. A branch therefore assigns `T` where the `if`'s recorded type
/// is `Effect[T]` — and `ts_ty` would render that `Promise<T>`, which `tsc
/// --strict` rejects (`Type 'boolean' is not assignable to type
/// 'Promise<boolean>'`). The ternary form never had to care: `return (c ? x : p)`
/// in an `async function` legally accepts either.
///
/// Both shapes are genuinely reachable — an `Effect.pure(x)` branch yields `T`
/// (the `EffectPure` arm above unwraps it) while a branch that is merely
/// `Effect`-typed falls through to `lower_expr` and yields `Promise<T>` — so the
/// annotation admits both rather than picking one.
fn async_tail_slot_ty(e: &Expr, cx: &LowerCtx) -> Option<String> {
    let tys = cx.commons().tys();
    match cx.commons().expr_ty(e.id).as_deref() {
        Some(Ty::Effect(inner)) => {
            let inner_ts = match &*tys.get(*inner) {
                Ty::Unit => "void".to_string(),
                _ => ts_ty(*inner, tys),
            };
            Some(format!("{inner_ts} | Promise<{inner_ts}>"))
        }
        _ => checked_ty_ts(e, cx),
    }
}

/// T2.1: a branch of a would-be ternary hoisted, so the ternary has nowhere to
/// put its statements. Hoist the whole `if` as a real statement assigning to a
/// fresh slot, and hand back the slot as the expression.
///
/// The reason this matters is the one R6.6 names: an arrow wrapper
/// (`(() => { … })()`, the only shape available before `pre` could travel with
/// the expression) makes a hoisted `?`'s `return` exit the *arrow* rather than
/// the enclosing function — the `lower_match_as_iife` miscompile (#946) in a
/// different costume, which `tsc --strict` rejects outright because the arrow's
/// type becomes `T | { tag: "Err" }`. Hoisting keeps the `return` where the
/// source put it.
///
/// Decision B of #1017 asked whether threading closes this for free. For the
/// `if`-expression it does, and this is where. It does **not** close
/// `lower_bin_op`'s short-circuit `rhs`, whose hoist must be *skipped* when the
/// operator doesn't reach it — a statement cannot be conditionally skipped, so
/// the arrow there is load-bearing rather than incidental. That one stays open.
fn hoist_if_as_statement(
    pre: &mut Pre,
    cond_expr: String,
    then_tail: Lowered,
    else_tail: Lowered,
    slot_ty: Option<String>,
    cx: &mut LowerCtx,
) -> String {
    let slot = cx.fresh();
    // The slot needs its type spelled out. TypeScript's "evolving `let`"
    // inference gives up once a branch also `return`s (TS7034/TS7005 under
    // `--strict`), which is exactly the shape a hoisted `?` produces — so take
    // the type the checker already recorded for the `if` expression.
    match &slot_ty {
        Some(ty) => pre.push(format!("let {slot}: {ty};")),
        None => pre.push(format!("let {slot};")),
    }
    pre.push(format!("if ({cond_expr}) {{"));
    pre.extend(assign_branch(then_tail, &slot));
    pre.push("} else {".to_string());
    pre.extend(assign_branch(else_tail, &slot));
    pre.push("}".to_string());
    slot
}

/// One branch of the hoisted statement `if` above: the branch's own hoisted
/// statements, then the assignment of its value to the shared slot. Each line
/// carries its own indent — a hoisted statement is written by whichever caller
/// owns the statement position, at one flat indent, so nesting has to travel
/// inside the string.
fn assign_branch(tail: Lowered, slot: &str) -> Vec<String> {
    let pad = " ".repeat(INDENT_STEP);
    let mut out: Vec<String> = tail.pre.iter().map(|s| format!("{pad}{s}")).collect();
    out.push(format!("{pad}{slot} = {};", tail.expr));
    out
}

/// The TypeScript rendering of the type the checker recorded for `e`, if any.
/// Used to annotate the slot [`hoist_if_as_statement`] declares.
fn checked_ty_ts(e: &Expr, cx: &LowerCtx) -> Option<String> {
    cx.commons()
        .expr_ty(e.id)
        .map(|_| ts_ty(cx.commons().expr_types[&e.id].ty, cx.commons().tys()))
}

/// T2.1 (R6.2): the ternary form's **structural** precondition — everything
/// about the shape that must hold before any lowering is attempted. Both
/// branches must be pure tails (a ternary has nowhere to host a statement)
/// whose own tails lower as plain expressions, and the condition must introduce
/// no `is` bindings (likewise nowhere to declare them).
///
/// The question this deliberately does *not* ask is the one `simple_expr` used
/// to guess: "will lowering this branch hoist a statement?" That is now
/// answered after the fact by [`Lowered::pre`], which cannot be wrong. The
/// predictive classifier — and the `debug_assert!(stmts.is_empty())` pair that
/// stood in for the guarantee it could not make — is deleted; the five defects
/// it produced (#945–#948 plus the `ListLit`/`InterpStr` fallthrough this slice
/// found live) are unrepresentable rather than merely fixed.
///
/// What remains here is only *routing*: which emitter shape a branch would take
/// if it were emitted as a block. That is a structural property of the AST, not
/// a prediction about lowering, and it never guessed wrong — every one of the
/// defects was hoisting, none was routing.
fn ternary_shaped(cond: &Expr, then_block: &Block, else_block: &Block, cx: &LowerCtx) -> bool {
    then_block.statements.is_empty()
        && else_block.statements.is_empty()
        && tail_lowers_as_expression(&then_block.tail, cx)
        && tail_lowers_as_expression(&else_block.tail, cx)
        && !cond_has_is_bindings(cond, cx)
}

/// True when [`emit_block_inner`] would lower this tail through its plain-
/// expression arm rather than routing it to a statement shape of its own. A
/// ternary branch — and the branch rebuild both hoisting fallbacks perform —
/// can host an expression and its hoisted statements, but not an emitted
/// `switch` or `if`/`else`.
fn tail_lowers_as_expression(e: &Expr, cx: &LowerCtx) -> bool {
    match &e.kind {
        ExprKind::Match { .. } => false,
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => ternary_shaped(cond, then_block, else_block, cx),
        _ => true,
    }
}

fn emit_statement(out: &mut String, stmt: &Statement, cx: &mut LowerCtx, indent: usize) {
    // Slice 1 (ADR 0103 D2): every generated line this statement emits — including
    // the multi-line `?` expansion (temp / Err-guard / unwrap) — anchors to the
    // statement's source span, so a source-map-aware stepper coalesces them into
    // one source step (the slice-0 spike confirmed this).
    cx.record_span(out.len(), stmt.span());
    match stmt {
        Statement::Let(l) => {
            // Track `let x = AgentName(key)` so subsequent `x.method(args)`
            // calls can dispatch through the agent class.
            if l.name.name != "_"
                && let ExprKind::Call {
                    name,
                    args: ctor_args,
                    ..
                } = &l.value.kind
                && cx.local_agents.contains(&name.name)
                && ctor_args.len() == 1
            {
                cx.local_agent_vars
                    .insert(l.name.name.clone(), name.name.clone());
            }
            let mut pre = Pre::new();
            let value = pre.lower(&l.value, cx);
            for s in pre.stmts() {
                write_line(out, indent, s);
            }
            // #908: also covers `_` (a fresh throwaway local; TS allows
            // `const _ = ...` only once per scope) and a re-`let` of a name
            // already bound in this or an enclosing block (which otherwise
            // either collides with the first `const` of that name — TS2451 —
            // or, one block deeper, shadows it into a TDZ read).
            let bind_name = cx.bind_local_name(&l.name.name);
            match &l.type_annot {
                Some(annot) => write_line(
                    out,
                    indent,
                    &format!(
                        "const {bind_name}: {ty} = {value};",
                        ty = ts_type_ref(annot),
                    ),
                ),
                None => write_line(out, indent, &format!("const {bind_name} = {value};")),
            }
        }
        Statement::EffectLet(l) => {
            // `let x <- expr` → `const x = await expr;`
            let mut pre = Pre::new();
            // v0.182 (#664): a call-site `by <Actor>(<identity>)` supplies the
            // identity the addressed handler reads as `deps.identity`. Lower it
            // (a test-only brand cast, like an agent key) and stash it for the
            // address-call lowering to fold into that call's deps.
            let saved_identity = cx.call_site_identity.take();
            let saved_no_credential = cx.call_site_no_credential;
            cx.call_site_no_credential = false;
            if let Some(principal) = &l.principal {
                // #706: `by Nobody` drives the route with no credential (the 401
                // path); it names no identity. Any other principal supplies its
                // identity — the *raw* lowered value (`"bob"`), which unit deps
                // folds with an `as any` brand cast and the system driver passes
                // as the JWT `sub` verbatim.
                if principal.actor.name == "Nobody" {
                    cx.call_site_no_credential = true;
                } else {
                    cx.call_site_identity = principal.identity.as_ref().map(|id| pre.lower(id, cx));
                }
            }
            let value = pre.lower(&l.value, cx);
            cx.call_site_identity = saved_identity;
            cx.call_site_no_credential = saved_no_credential;
            for s in pre.stmts() {
                write_line(out, indent, s);
            }
            let bind_name = cx.bind_local_name(&l.name.name);
            // T2.2 (R6.4): the literal `await` below is one of the two real
            // sources of effectfulness a synchronous-looking IIFE further out
            // needs to know about — see `emitted_await`.
            cx.emitted_await = true;
            match &l.type_annot {
                Some(annot) => write_line(
                    out,
                    indent,
                    &format!(
                        "const {bind_name}: {ty} = await {value};",
                        ty = ts_type_ref(annot),
                    ),
                ),
                None => write_line(out, indent, &format!("const {bind_name} = await {value};")),
            }
        }
        Statement::Expect(a) => {
            // Inside a test case body, `expect <pred>` lowers to a runtime check
            // that throws an ExpectationError so the surrounding test-case runner
            // catches it and records the failure (v0.112, renamed from `assert`).
            let span_start = a.value.span.start;
            let span_end = a.value.span.end;
            let location = expect_location(cx, span_start);
            let src = expect_source_text(cx, a.value.span);
            let mut pre = Pre::new();
            let cond = pre.lower(&a.value, cx);
            // Structural expected-vs-actual for a top-level comparison. The
            // predicate is pure (ADR 0144), so re-evaluating the operands for the
            // failure message is observationally identical to the condition.
            let detail = if let ExprKind::BinOp(op, l, r) = &a.value.kind
                && let Some(sym) = comparison_op_symbol(*op)
            {
                let lv = pre.lower(l, cx);
                let rv = pre.lower(r, cx);
                format!(
                    "\"expect {src}\\n  expected: {src}\\n  actual:   \" + __bynkShow(({lv})) + \" {sym} \" + __bynkShow(({rv}))"
                )
            } else {
                format!("\"expect {src}\"")
            };
            for s in pre.stmts() {
                write_line(out, indent, s);
            }
            write_line(
                out,
                indent,
                &format!(
                    "if (!({cond})) {{ throw __bynkExpectFailure(\"{location}\", {span_start}, {span_end}, {detail}); }}",
                ),
            );
        }
        Statement::Send(s) => {
            // v0.79: `~> expr` — fire-and-forget. The reply is `Effect[()]` and is
            // never awaited. On the Workers target the immediate tier hands the
            // promise to the execution context's `waitUntil`, so it settles after
            // the handler returns rather than being killed with the response. The
            // execution context rides in `deps.__exec` (threaded by `compose`).
            let mut pre = Pre::new();
            let value = pre.lower(&s.value, cx);
            for st in pre.stmts() {
                write_line(out, indent, st);
            }
            write_line(
                out,
                indent,
                &format!(
                    "{deps}.__exec.waitUntil({value});",
                    deps = cx.cap_deps_expr()
                ),
            );
        }
        Statement::Do(d) => {
            // v0.146 (ADR 0170): `do expr` → `await expr;`. The binder-free
            // `let _ <- expr` for a unit effect — the effect runs and joins the
            // handler, its `()` result discarded (no `const`).
            let mut pre = Pre::new();
            let value = pre.lower(&d.value, cx);
            for st in pre.stmts() {
                write_line(out, indent, st);
            }
            // T2.2 (R6.4): see the matching note at `EffectLet`.
            cx.emitted_await = true;
            write_line(out, indent, &format!("await {value};"));
        }
        Statement::Assign(a) => {
            // v0.81 (storage track, ADR 0109): `cell := expr` writes the mutable
            // working state in place (`__state.cell = <expr>`). It is staged in
            // memory — read-your-writes within the handler — and flushed once at
            // handler end via `commitState` (which runs the invariant gate before
            // the durable write). A fault before that flush persists nothing.
            let mut pre = Pre::new();
            let value = pre.lower(&a.value, cx);
            for st in pre.stmts() {
                write_line(out, indent, st);
            }
            let lhs = match cx.agent_store_cells() {
                Some((var, _)) => format!("{var}.{}", a.target.name),
                // Defensive: the checker resolves `:=` to a store cell, so a
                // write outside a store-agent handler does not reach emission.
                None => a.target.name.clone(),
            };
            write_line(out, indent, &format!("{lhs} = {value};"));
        }
    }
}

/// v0.59: the `location` string an `expect` failure carries. With a test-body
/// [`AssertLoc`](crate::emitter::AssertLoc) in scope it is a real, escaped
/// `path:line:col` (so `--format json` consumers can link to the source);
/// otherwise it falls back to the bare byte offset (`expect`s only appear in test
/// bodies, so the fallback is defensive).
fn expect_location(cx: &LowerCtx, offset: usize) -> String {
    match cx.assert_loc() {
        Some(loc) => {
            let (line, col) = bynk_syntax::span::line_col(&loc.source, offset);
            // Normalise to forward slashes so the location is identical on
            // Windows (where `PathBuf` joins with `\`) — matching the
            // diagnostic path rendering and the committed goldens.
            let path = loc.rel_path.replace('\\', "/");
            crate::emitter::escape_ts_string(&format!("{path}:{line}:{col}"))
        }
        None => format!("offset {offset}"),
    }
}

/// v0.112: the trimmed, TS-escaped source text of an `expect` predicate, embedded
/// in the structural failure report (`expect <src>`). Escaped for a
/// double-quoted TS string literal (no surrounding quotes); empty when no
/// test-body source is in scope (defensive — `expect`s only appear in test bodies).
fn expect_source_text(cx: &LowerCtx, span: bynk_syntax::span::Span) -> String {
    match cx.assert_loc() {
        Some(loc) => {
            let raw = loc.source.get(span.start..span.end).unwrap_or("").trim();
            crate::emitter::escape_ts_string(raw)
        }
        None => String::new(),
    }
}

/// v0.112: the source operator symbol for a comparison `BinOp`, or `None` for
/// non-comparison operators (which get the source-text-only failure report).
fn comparison_op_symbol(op: BinOp) -> Option<&'static str> {
    match op {
        BinOp::Eq => Some("=="),
        BinOp::NotEq => Some("!="),
        BinOp::Lt => Some("<"),
        BinOp::LtEq => Some("<="),
        BinOp::Gt => Some(">"),
        BinOp::GtEq => Some(">="),
        _ => None,
    }
}

fn write_line(out: &mut String, indent: usize, line: &str) {
    for _ in 0..indent {
        out.push(' ');
    }
    out.push_str(line);
    out.push('\n');
}

/// The raw TypeScript form of a compile-time literal that v0.9.4 may admit as a
/// refined type (int or string). `None` for anything else.
fn lower_const_literal_raw(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::IntLit { value: n, .. } => Some(n.to_string()),
        // v0.21: the stored lexeme verbatim.
        ExprKind::FloatLit { lexeme, .. } => Some(lexeme.clone()),
        ExprKind::StrLit(s) => Some(format!("\"{}\"", escape_ts_string(s))),
        // A negated numeric literal — admissible at compile time (the
        // checker folds the sign in `const_literal`), so the lowering must
        // route it through `unsafe` like any other admitted literal.
        ExprKind::UnaryOp(UnaryOp::Neg, inner) => match &inner.kind {
            ExprKind::IntLit { value: n, .. } => Some(format!("-{n}")),
            ExprKind::FloatLit { lexeme, .. } => Some(format!("-{lexeme}")),
            _ => None,
        },
        _ => None,
    }
}

/// Lower an interpolated string (v0.43, ADR 0075) to a TS template literal.
/// Chunks become escaped literal text; each hole becomes `${String(<expr>)}`.
/// `String(…)` is identity for a `String` hole and the display form for
/// `Int`/`Float`/`Bool` — and the checker guarantees only base scalars reach
/// here, so no `[object Object]` can be emitted.
fn lower_interp_str(parts: &[InterpPart], cx: &mut LowerCtx) -> Lowered {
    let mut pre = Pre::new();
    let mut out = String::from("`");
    for part in parts {
        match part {
            InterpPart::Chunk(text) => out.push_str(&escape_ts_template(text)),
            InterpPart::Hole(hole) => {
                let lowered = pre.lower(hole, cx);
                out.push_str(&format!("${{String({lowered})}}"));
            }
        }
    }
    out.push('`');
    pre.finish(out)
}

/// Escape a literal chunk for a TS template-literal context: backslash,
/// backtick, and `$` (to neutralise `${`), plus the control-char escapes
/// [`escape_ts_string`] applies. (v0.43.)
fn escape_ts_template(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '`' => out.push_str("\\`"),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// #1 review: an expression's lowered text, paired with the statements that
/// must run before it — the typed replacement for the `stmts: &mut
/// Vec<String>` sink `lower_expr_into` used to take. A caller that receives a
/// `Lowered` cannot accidentally drop `pre` (unlike the old sink, dropping it
/// took no code at all) or splice it into a string where a statement is
/// required (`Lowered` is not a `String`) — the two failure modes the sink
/// pattern actually produced (`lower_if`'s/`lower_tail_expr`'s ternary paths,
/// `lower_and_with_is`'s splice).
///
/// T2.1 (R6.2): this is now the *only* way a lowering function hands
/// statements to its caller. There is no sink parameter left in `bynk-emit`
/// to hand them to instead.
pub(crate) struct Lowered {
    pub pre: Vec<String>,
    pub expr: String,
}

impl Lowered {
    /// An expression that hoists nothing — the common leaf case.
    pub(crate) fn bare(expr: impl Into<String>) -> Self {
        Lowered {
            pre: Vec::new(),
            expr: expr.into(),
        }
    }
}

/// T2.1 (R6.2): the statements an expression under construction has hoisted so
/// far. This is the *owned* counterpart to the deleted sink: a `Pre` belongs to
/// the one function assembling an expression, and leaves it only by being
/// [finished](Pre::finish) into that expression's [`Lowered`]. It is
/// deliberately never a parameter — that is the whole point. A lowering
/// function cannot be handed somewhere to write statements, so it cannot write
/// them anywhere but its own return value, and the caller decides where they
/// land (a real statement position, or a hoist-safe wrapper).
pub(crate) struct Pre(Vec<String>);

impl Pre {
    pub(crate) fn new() -> Self {
        Pre(Vec::new())
    }

    /// Lower `e`, absorbing whatever it hoists, and hand back its text. The
    /// replacement for `lower_expr_into(e, stmts, cx)`: identical ordering —
    /// the callee's statements land here, in order, at the point of the call —
    /// but the callee never saw the buffer.
    pub(crate) fn lower(&mut self, e: &Expr, cx: &mut LowerCtx) -> String {
        let lowered = lower_expr(e, cx);
        self.absorb(lowered)
    }

    /// Absorb an already-built [`Lowered`] and hand back its text.
    pub(crate) fn absorb(&mut self, lowered: Lowered) -> String {
        self.0.extend(lowered.pre);
        lowered.expr
    }

    /// Absorb an optional [`Lowered`] — a kernel lowerer that may decline the
    /// shape. Declining hoists nothing, so `None` leaves `self` untouched.
    pub(crate) fn absorb_opt(&mut self, lowered: Option<Lowered>) -> Option<String> {
        lowered.map(|l| self.absorb(l))
    }

    /// Hoist a statement this function generated itself (a `?` temp, an
    /// `is`-receiver binding, a match IIFE's scaffolding).
    pub(crate) fn push(&mut self, stmt: String) {
        self.0.push(stmt);
    }

    pub(crate) fn extend(&mut self, stmts: impl IntoIterator<Item = String>) {
        self.0.extend(stmts);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The accumulated statements, for a caller that is writing them into a
    /// real statement position rather than hoisting them further.
    pub(crate) fn stmts(&self) -> &[String] {
        &self.0
    }

    /// Pair the accumulated statements with the expression they precede.
    pub(crate) fn finish(self, expr: impl Into<String>) -> Lowered {
        Lowered {
            pre: self.0,
            expr: expr.into(),
        }
    }
}

/// The canonical entry point: lower `e`, returning its text and whatever
/// statements must run first.
pub(crate) fn lower_expr(e: &Expr, cx: &mut LowerCtx) -> Lowered {
    let tys = cx.commons().tys();
    // v0.9.4: a literal the checker admitted as a refined type (expected-type-
    // directed construction) is branded directly — the refinement was already
    // verified at compile time, so there is no runtime check and no `Result`.
    // ADR 0182: a refined/alias type has no public `.unsafe`, so this brands with
    // an inline `as` cast (`unchecked_construct` with `is_opaque = false`) rather
    // than calling one; opaque is never admitted here (`NamedKind::Refined` only).
    if let Some(Ty::Named {
        name,
        kind: NamedKind::Refined(_),
        ..
    }) = cx.commons().expr_ty(e.id).as_deref()
        && let Some(raw) = lower_const_literal_raw(e)
    {
        // ADR 0182: in GENERATED TEST scaffolding a branded type is in scope only
        // as an `any`-typed value binding (`const { T } = ns as any`), never as a
        // type, so a `(raw as T)` cast fails to resolve `T` ("refers to a value,
        // but is being used as a type"). Brand via `unchecked_construct_test`
        // (refined → `(raw as any)`) there, exactly as the `Val[T](lit)` mock path
        // does. Production emission keeps the real `(raw as T)` brand.
        if cx.in_test_scaffold() {
            return Lowered::bare(unchecked_construct_test(name, &raw, false));
        }
        return Lowered::bare(unchecked_construct(name, &raw, false));
    }
    let mut pre = Pre::new();
    let expr = match &e.kind {
        ExprKind::IntLit { value: n, .. } => n.to_string(),
        // Slice C: `Wire(<String>)` in a generic position lowers to its raw inner
        // string. The system-http driver site intercepts `Wire` args before this
        // to route them raw (no serialisation) and switch to the outcome decoder.
        ExprKind::Wire(inner) => pre.lower(inner, cx),
        // v0.21: the stored lexeme verbatim — `1e10` must not normalise.
        ExprKind::FloatLit { lexeme, .. } => lexeme.clone(),
        // v0.86 (ADR 0112): a `Duration` literal lowers to its constant
        // milliseconds (the value `Duration` erases to).
        ExprKind::DurationLit { millis, .. } => millis.to_string(),
        ExprKind::StrLit(s) => format!("\"{}\"", escape_ts_string(s)),
        // v0.43 (ADR 0075): an interpolated string lowers to a TS template
        // literal — chunks as escaped literal text, holes as `${String(…)}`.
        ExprKind::InterpStr(parts) => pre.absorb(lower_interp_str(parts, cx)),
        ExprKind::BoolLit(b) => b.to_string(),
        // v0.20b: a list literal lowers to a TS array literal; `readonly` is
        // a type-level property and the checker owns the element typing.
        ExprKind::ListLit(elems) => {
            let lowered: Vec<String> = elems.iter().map(|el| pre.lower(el, cx)).collect();
            format!("[{}]", lowered.join(", "))
        }
        ExprKind::Ident(id) => lower_ident(e, id, cx),
        ExprKind::Call { name, args, .. } => pre.absorb(lower_call(e, name, args, cx)),
        ExprKind::UnaryOp(op, inner) => {
            let inner = pre.lower(inner, cx);
            let sym = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            };
            format!("{sym}{inner}")
        }
        ExprKind::BinOp(op, lhs, rhs) => pre.absorb(lower_bin_op(*op, lhs, rhs, cx)),
        ExprKind::Paren(inner) => {
            let s = pre.lower(inner, cx);
            format!("({s})")
        }
        ExprKind::Ok(inner) => {
            let s = pre.lower(inner, cx);
            // v0.9: `Ok` is overloaded — use the checker's recorded type to
            // decide between `Result.Ok` and `HttpResult.Ok`.
            if matches!(
                cx.commons().expr_ty(e.id).as_deref(),
                Some(Ty::HttpResult(_))
            ) {
                format!("HttpResult.Ok({s})")
            } else {
                format!("Ok({s})")
            }
        }
        ExprKind::Err(inner) => {
            let s = pre.lower(inner, cx);
            format!("Err({s})")
        }
        ExprKind::Some(inner) => {
            let s = pre.lower(inner, cx);
            format!("Some({s})")
        }
        ExprKind::None => "None".to_string(),
        ExprKind::Question(inner) => {
            // v0.153 (ADR 0177): an `Option[T]?` operand lifts into an
            // HttpResult handler — `None` early-returns `NotFound` (404),
            // `Some(v)` yields `v`. Any other operand is a `Result`: `Err`
            // propagates unchanged. Branch on the operand's checked type.
            // The checker rejects any other `?` operand, so a typed operand is
            // always present — assert it, so a future gap surfaces loudly in
            // tests rather than silently emitting the `Result` branch (which on
            // an untyped `Option` would leak `None` → `undefined`).
            let operand_ty = cx.commons().expr_types.get(&inner.id).map(|te| te.ty);
            let operand_node = operand_ty.map(|t| tys.get(t));
            debug_assert!(
                matches!(
                    operand_node.as_deref(),
                    Some(Ty::Option(_) | Ty::Result(_, _))
                ),
                "`?` operand has no `Option`/`Result` checked type at {:?}: {operand_node:?}",
                inner.span,
            );
            let is_option = matches!(operand_node.as_deref(), Some(Ty::Option(_)));
            // v0.154 (ADR 0178): a `Result` operand whose error type differs from
            // the enclosing return's is converted by a declared embedding
            // (`embeds E as V`) — the same rule the checker accepted it under.
            let embed = if is_option {
                None
            } else {
                embed_conversion(operand_ty, cx)
            };
            let inner_expr = pre.lower(inner, cx);
            let tmp = cx.fresh();
            pre.push(format!("const {tmp} = {inner_expr};"));
            if is_option {
                pre.push(format!(
                    "if ({tmp}.tag === \"None\") return HttpResult.NotFound;"
                ));
            } else if let Some((ty_name, variant)) = embed {
                pre.push(format!(
                    "if ({tmp}.tag === \"Err\") return Err({ty_name}.{variant}({tmp}.error));"
                ));
            } else {
                pre.push(format!("if ({tmp}.tag === \"Err\") return {tmp};"));
            }
            // T2.3 (R6.3): every branch above just pushed a `return` that must
            // exit the enclosing function, not whatever scope this `Lowered`
            // eventually lands in — see `LowerCtx::emitted_early_return`.
            cx.emitted_early_return = true;
            format!("{tmp}.value")
        }
        ExprKind::ConstructorCall {
            type_name,
            method,
            args,
        } => pre.absorb(lower_constructor_call(type_name, method, args, cx)),
        ExprKind::RecordConstruction { type_name, fields } => {
            pre.absorb(lower_record_construction(type_name, fields, cx))
        }
        ExprKind::FieldAccess { receiver, field } => {
            pre.absorb(lower_field_access(e, receiver, field, cx))
        }
        ExprKind::MethodCall {
            receiver,
            method,
            type_args,
            args,
        } => pre.absorb(lower_method_call(e, receiver, method, type_args, args, cx)),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => pre.absorb(lower_if(e, cond, then_block, else_block, cx)),
        // v0.20a: a lambda lowers to a TS arrow; `async` iff its checked type
        // is an effectful function. A lambda is deliberately its own return
        // scope (ADR 0178) — its body is lowered with no access to the outer
        // `pre`, so anything it hoists (match-as-IIFE etc.) stays local to
        // the arrow, correctly, because a `?`'s `return` there is supposed to
        // exit the lambda.
        ExprKind::Lambda(lambda) => lower_lambda(e, lambda, cx),
        ExprKind::Block(b) => lower_block_as_expr(b, cx),
        ExprKind::Match { discriminant, arms } => {
            pre.absorb(lower_match_as_iife(discriminant, arms, cx))
        }
        ExprKind::Is { value, pattern } => pre.absorb(lower_is(value, pattern, cx)),
        ExprKind::UnitLit => "undefined".to_string(),
        ExprKind::EffectPure(inner) => {
            let inner_expr = pre.lower(inner, cx);
            format!("Promise.resolve({inner_expr})")
        }
        ExprKind::RecordSpread {
            type_name: _,
            base,
            overrides,
        } => pre.absorb(lower_record_spread(base, overrides, cx)),
        ExprKind::Expect(inner) => {
            // v0.9.1: expect as an expression (v0.112, renamed from `assert`).
            // Emit a runtime helper call that returns void (i.e., evaluates to
            // `undefined` at runtime and is treated as the unit value `()` in Bynk
            // terms). The expression form reports the predicate source only — the
            // statement form carries the structural expected-vs-actual report.
            let value = pre.lower(inner, cx);
            let span_start = inner.span.start;
            let span_end = inner.span.end;
            let location = expect_location(cx, span_start);
            let src = expect_source_text(cx, inner.span);
            format!(
                "__bynkExpect(({value}), \"{location}\", {span_start}, {span_end}, \"expect {src}\")"
            )
        }
        ExprKind::Val { type_ref, args } => pre.absorb(lower_val(type_ref, args, cx)),
        ExprKind::Observation(o) => lower_observation(o, cx),
        ExprKind::Trace { cap, op } => {
            // `trace(Cap.op)` → the recorded calls mapped to per-call records
            // whose fields are the operation's parameters (positionally).
            let obs = cx
                .observation_trace()
                .map(str::to_string)
                .unwrap_or_else(|| "__obs".to_string());
            let key = format!("{}.{}", cap.name, op.name);
            let names = cap_op_param_names(cx, &cap.name, &op.name);
            let fields = names
                .iter()
                .enumerate()
                .map(|(i, n)| format!("{n}: __c.args[{i}]"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("(({obs}.log[{key:?}] ?? []).map((__c: any) => ({{ {fields} }})))")
        }
    };
    pre.finish(expr)
}

/// The parameter names of a capability operation, looked up from the capability
/// declarations in scope (v0.117). Used to destructure a recorded call's
/// arguments for a `with` predicate and to build `trace(Cap.op)` records.
/// P6.29 (design/tracks/the-ir.md §6a): the declaration lookup itself now reads
/// `crate::ir::lower::capability_op_sig_from_commons` — still a by-name walk
/// under the hood (nothing IR-native replaces "find the op named `op` on the
/// capability named `cap`"), but relocated to `ir/lower.rs` (excluded from
/// `ast_importers` — the `Ast → Ir` lowering pass's own job), so this file no
/// longer spells `bynk_syntax::ast::CommonsItem` itself.
fn cap_op_param_names(cx: &LowerCtx, cap: &str, op: &str) -> Vec<String> {
    crate::ir::lower::capability_op_sig_from_commons(cx.commons(), cap, op)
        .map(|sig| sig.params.into_iter().map(|(name, _)| name).collect())
        .unwrap_or_default()
}

/// Lower an observation (v0.117) to a `Bool` JavaScript expression over the
/// recorded trace object. `total`/`matching` counts drive the sugar; `before`
/// compares call order.
fn lower_observation(o: &ObservationExpr, cx: &mut LowerCtx) -> String {
    let obs = cx
        .observation_trace()
        .map(str::to_string)
        .unwrap_or_else(|| "__obs".to_string());
    let key = format!("{}.{}", o.cap.name, o.op.name);
    let calls = format!("({obs}.log[{key:?}] ?? [])");
    match &o.matcher {
        ObservationMatcher::NeverCalled => format!("({calls}.length === 0)"),
        ObservationMatcher::Before { cap, op } => {
            let key2 = format!("{}.{}", cap.name, op.name);
            let calls2 = format!("({obs}.log[{key2:?}] ?? [])");
            format!(
                "({calls}.length > 0 && {calls2}.length > 0 && {calls}[0].order < {calls2}[0].order)"
            )
        }
        ObservationMatcher::Called { count, with_pred } => match with_pred {
            None => match count {
                None => format!("({calls}.length >= 1)"),
                Some(c) => {
                    let n = lower_expr(c, cx).expr;
                    format!("({calls}.length === ({n}))")
                }
            },
            Some(p) => {
                let names = cap_op_param_names(cx, &o.cap.name, &o.op.name);
                let destructure = if names.is_empty() {
                    String::new()
                } else {
                    format!("const [{}] = __c.args; ", names.join(", "))
                };
                let pred_lowered = lower_expr(p, cx);
                let pre_src = pred_lowered.pre.join(" ");
                let pred = pred_lowered.expr;
                let matching = format!(
                    "{calls}.filter((__c: any) => {{ {destructure}{pre_src}return ({pred}); }}).length"
                );
                match count {
                    None => format!("(({matching}) >= 1)"),
                    Some(c) => {
                        let n = lower_expr(c, cx).expr;
                        format!("(({matching}) === ({n}))")
                    }
                }
            }
        },
    }
}

/// A default base-type literal (as TypeScript source) that satisfies a refined
/// type's predicates, for bare `Val[T]`. `None` when no default can be derived
/// (a `Matches` refinement — the checker rejects bare `Val` for those).
fn refined_default(decl: &TypeDecl) -> Option<String> {
    let (base, refinement) = match &decl.body {
        TypeBody::Refined {
            base, refinement, ..
        } => (*base, refinement.as_ref()),
        _ => return None,
    };
    match base {
        BaseType::Int => {
            let mut lo: i64 = 0;
            if let Some(r) = refinement {
                for p in &r.predicates {
                    match &p.kind {
                        PredKind::Positive => lo = lo.max(1),
                        PredKind::NonNegative => lo = lo.max(0),
                        PredKind::InRange(a, _) => lo = lo.max(a.value),
                        _ => {}
                    }
                }
            }
            Some(lo.to_string())
        }
        BaseType::String => {
            let mut len: i64 = 0;
            if let Some(r) = refinement {
                for p in &r.predicates {
                    match p.kind {
                        PredKind::NonEmpty => len = len.max(1),
                        PredKind::MinLength(k) | PredKind::Length(k) => len = len.max(k),
                        PredKind::Matches(_) => return None,
                        _ => {}
                    }
                }
            }
            if len < 1 {
                len = 1;
            }
            Some(format!("\"{}\"", "x".repeat(len as usize)))
        }
        BaseType::Bool => Some("true".to_string()),
        BaseType::Float => {
            let mut lo: f64 = 0.0;
            let mut hi = f64::INFINITY;
            if let Some(r) = refinement {
                for p in &r.predicates {
                    match &p.kind {
                        PredKind::Positive => lo = lo.max(1.0),
                        PredKind::NonNegative => lo = lo.max(0.0),
                        PredKind::InRangeF(a, b) => {
                            lo = lo.max(a.value);
                            hi = hi.min(b.value);
                        }
                        _ => {}
                    }
                }
            }
            // The `Positive` floor of 1.0 can overshoot a tight fractional
            // range (`InRange(0.0, 0.5)`); fall back to the upper bound.
            if lo > hi {
                lo = hi;
            }
            Some(lo.to_string())
        }
        // v0.86: `Duration` carries no refinement; `0` millis is its default.
        BaseType::Duration | BaseType::Instant => Some("0".to_string()),
        // v0.110: `Bytes` carries no refinement; the empty octet sequence is
        // its default.
        BaseType::Bytes => Some("new Uint8Array()".to_string()),
    }
}

/// v0.9.4 Part B (slice 1): lower a refined-type `Val[T]` / `Val[T](lit)` to
/// the branded `unsafe` constructor. The checker has already validated this is a
/// refined type in a test body, and recorded the refined type at `span`.
/// v0.9.4 slice 2 recursion cap for bare `Val` generation (mirrors the
/// checker's `MOCK_DEPTH`).
const MOCK_DEPTH: u32 = 12;

/// A TypeScript base-literal default for an opaque type's underlying base. Not
/// distinct per call in this increment — per-call distinctness via a runtime
/// counter is a follow-up.
fn base_default_ts(base: BaseType) -> String {
    match base {
        BaseType::Int => "0".to_string(),
        BaseType::String => "\"mock\"".to_string(),
        BaseType::Bool => "true".to_string(),
        BaseType::Float => "0".to_string(),
        BaseType::Duration | BaseType::Instant => "0".to_string(),
        BaseType::Bytes => "new Uint8Array()".to_string(),
    }
}

/// Generate a TypeScript expression for a bare `Val` of `ty` (v0.9.4 Part B,
/// slice 2). Recurses through sum payloads and record fields; refined types use
/// `refined_default`, opaque types wrap a base default, bare bases use 0/""/true.
fn mock_value(ty: TyId, cx: &LowerCtx, depth: u32) -> String {
    let tys = cx.commons().tys();
    if depth == 0 {
        return "undefined".to_string();
    }
    match &*tys.get(ty) {
        Ty::Base(BaseType::Int) => "0".to_string(),
        Ty::Base(BaseType::String) => "\"\"".to_string(),
        Ty::Base(BaseType::Bool) => "true".to_string(),
        Ty::Base(BaseType::Float) => "0".to_string(),
        Ty::Named { name, .. } => {
            let Some(decl) = cx.commons().types.get(name.as_str()) else {
                return "undefined".to_string();
            };
            match &decl.body {
                TypeBody::Refined { .. } => {
                    let d = refined_default(decl).unwrap_or_else(|| "0".to_string());
                    unchecked_construct_test(name, &d, false)
                }
                TypeBody::Opaque { base, .. } => {
                    unchecked_construct_test(name, &base_default_ts(*base), true)
                }
                TypeBody::Sum(s) => match s.variants.first() {
                    None => "undefined".to_string(),
                    Some(v) if v.payload.is_empty() => format!("{name}.{}", v.name.name),
                    Some(v) => {
                        let args: Vec<String> = v
                            .payload
                            .iter()
                            .map(|f| {
                                bynk_check::checker::resolve_type_ref(
                                    &f.type_ref,
                                    &cx.commons().types,
                                    tys,
                                )
                                .map(|t| mock_value(t, cx, depth - 1))
                                .unwrap_or_else(|| "undefined".to_string())
                            })
                            .collect();
                        format!("{name}.{}({})", v.name.name, args.join(", "))
                    }
                },
                TypeBody::Record(r) => {
                    let parts: Vec<String> = r
                        .fields
                        .iter()
                        .map(|f| {
                            let fv = bynk_check::checker::resolve_type_ref(
                                &f.type_ref,
                                &cx.commons().types,
                                tys,
                            )
                            .map(|t| mock_value(t, cx, depth - 1))
                            .unwrap_or_else(|| "undefined".to_string());
                            format!("{}: {}", f.name.name, fv)
                        })
                        .collect();
                    format!("{{ {} }}", parts.join(", "))
                }
            }
        }
        _ => "undefined".to_string(),
    }
}

/// Lower an `ExprKind::MethodCall`. This is a dispatcher: a sequence of
/// independent guard-and-`return` branches, tried in order (the order is
/// load-bearing — earlier guards take precedence), falling through to the
/// UFCS instance-call tail. The collection/numeric/string/option/result
/// kernels and the typed JSON codec delegate to dedicated helpers that
/// return `Option<String>`.
/// #926: render a capability call's explicit type argument(s) as a TS
/// generic argument list (`<T>`), or `""` when absent. Unlike
/// `Json.decode[T]` (`lower_json_codec_call`, below), which specialises a
/// runtime codec per call and needs no TS-level generic at all, a capability
/// operation's `T` is a pure type-level parameter on a real generic TS
/// interface method (Decision C) — with no argument of type `T`, TypeScript
/// cannot infer a return-position-only parameter, so the call site must
/// name it explicitly.
fn capability_call_type_args_ts(type_args: &[TypeRef]) -> String {
    if type_args.is_empty() {
        return String::new();
    }
    format!(
        "<{}>",
        type_args
            .iter()
            .map(ts_type_ref)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// #934 (call-site key scoping): `Idempotency.dedup[T](key)` and
/// `Idempotency.remember[T](key, value, expiresAfter)` both take the
/// developer-supplied dedup key as their first argument (already lowered, in
/// `args_lowered[0]`). Rewrite it in place to a template literal prefixed with
/// the calling handler's own qualified name, so two unrelated handlers that
/// happen to pick the same literal key never collide.
///
/// `is_first_party` must already have confirmed this is genuinely the
/// first-party `bynk.Idempotency` capability, not merely a same-named
/// capability some other adapter or context happens to declare (both call
/// sites resolve that separately, since the two lowering paths — flattened
/// vs. cross-context-qualified — carry the provenance differently).
///
/// `cx.handler_scope` is set at every real capability-call site (an ordinary
/// service handler, an agent handler, a composed provider op body, a
/// websocket lifecycle DO method — see `emit_service`/`emit_agent`/
/// `emit_provider`/`emit_ws_do_method`). Its absence here means some new
/// capability-call site was added without threading it through; that is a
/// compiler bug, not a user error, so this fails loudly rather than silently
/// shipping an unscoped key.
fn scope_idempotency_key(
    is_first_party: bool,
    method: &str,
    args_lowered: &mut [String],
    cx: &LowerCtx,
) {
    if !is_first_party || !matches!(method, "dedup" | "remember") {
        return;
    }
    let scope = cx
        .handler_scope()
        .unwrap_or_else(|| panic!("Idempotency.{method} lowered with no handler_scope set"));
    args_lowered[0] = format!("`{scope}::${{{}}}`", args_lowered[0]);
}

fn lower_method_call(
    e: &Expr,
    receiver: &Expr,
    method: &Ident,
    type_args: &[TypeRef],
    args: &[Expr],
    cx: &mut LowerCtx,
) -> Lowered {
    let tys = cx.commons().tys();
    let mut pre = Pre::new();
    // v0.104/v0.105 (real-time track slice 3b): a held-`Map` operation —
    // `<map>.<op>(…)` on a `store Map[K, Connection]` field on Workers. The live
    // socket cannot be persisted, so the durable record stores the **connection id**
    // (`__state.<map>` is a `Record<string(K), connId>`) and each `Connection` is
    // re-resolved from its connId via `resolveConnection` — so a stored connection
    // survives DO eviction (§2.9.6, slice 3b-ii). `put` records the connection's id
    // (`connIdOf`); `get` resolves it (or `None` if the socket has since closed);
    // `remove` resolves-closes-deletes (the §2.9 "removes-and-closes" contract). The
    // record mutation is staged in `__state` and flushed by the same end-of-handler
    // commit as any other persisted field.
    // P6.21 (partial, continued): the receiver test reads the checker's own
    // `Callee::Store`/`Callee::Query` (P6.0) instead of
    // `!cx.is_local(&id.name)` — a real instance of R6.5's name-matched-
    // receiver defect class. A held `store Map[K, Connection]` field's
    // method calls resolve through the same `StoreField::Map` checker
    // dispatch as any other storage map — entry ops (`put`/`remove`/
    // `contains`/`size`/`get`) as `Callee::Store`, lifted query ops (e.g.
    // `parTraverse`) as `Callee::Query` — so both variants are matched here,
    // mirroring the ordinary (non-held) Map branch's own guard just below.
    // `agent_held_map_frame` stays: it answers which *kind* of store field
    // this is (held vs ordinary), not the shadowing question `Callee`
    // already settles.
    if let ExprKind::Ident(id) = &receiver.kind
        && matches!(
            cx.commons().callee(e.id),
            Some(Callee::Store { .. } | Callee::Query { .. })
        )
        && let Some(f_ts) = cx.agent_held_map_frame(&id.name).cloned()
    {
        let var = cx.agent_store_var().to_string();
        let m = format!("{var}.{}", id.name);
        let a: Vec<String> = args.iter().map(|x| pre.lower(x, cx)).collect();
        return pre.finish(match method.name.as_str() {
            "put" => format!("(({m}[String({})] = connIdOf({})), undefined)", a[0], a[1]),
            "remove" => format!(
                "(async () => {{ const __k = String({0}); const __cid = {m}[__k]; if (__cid !== undefined) {{ const __c = resolveConnection<{f_ts}>(this.state, __cid); if (__c.tag === \"Some\") {{ await __c.value.close(); }} delete {m}[__k]; }} return undefined; }})()",
                a[0]
            ),
            "contains" => format!("(String({}) in {m})", a[0]),
            "size" => format!("Object.keys({m}).length"),
            "get" => format!(
                "(() => {{ const __k = String({0}); return (__k in {m}) ? resolveConnection<{f_ts}>(this.state, {m}[__k]) : None; }})()",
                a[0]
            ),
            // Any non-entry op is a lazy query lifting the map into a scan over its
            // **resolved** connections (the present ones — a connId whose socket has
            // closed drops out).
            _ => lower_query_method(format!(
                    "Object.values({m}).flatMap((__cid) => {{ const __c = resolveConnection<{f_ts}>(this.state, __cid); return __c.tag === \"Some\" ? [__c.value] : []; }})"
                ), method, &a, cx.commons().expr_types.get(&e.id).map(|te| te.ty), tys)
            .unwrap_or_else(|| {
                format!("(/* unsupported held Map op {} */ undefined)", method.name)
            }),
        });
    }
    // v0.82 (ADR 0110): a storage-`Map` operation — `<map>.<op>(…)` on a `store
    // Map[K, V]` field. Lowers to an entry op over `__state.<map>` (a
    // `Record<string, V>`): mutating the working record (`put`/`remove`/`update`/
    // `upsert`) or reading it (`get`/`contains`/`size`). The surface op is
    // `Effect`-typed and awaited with `<-`, but the working-record mutation is
    // synchronous (the durable write is the single end-of-handler flush), so an
    // awaited expression suffices. `update` on an absent key throws (a fault →
    // nothing commits).
    //
    // P6.21 (partial, continued): the receiver test reads the checker's own
    // `Callee::Store`/`Callee::Query` (P6.0) instead of `!cx.is_local(&id.name)`
    // — a real instance of R6.5's name-matched-receiver defect class, the same
    // one `body_writes_state`/#1196 already closed for write-detection.
    // `cx.is_agent_store_map` stays: it answers a different question (which
    // *kind* of store field this is, needed to pick this branch over
    // Set/Cache/Log/Cell's own), not the shadowing question `Callee` already
    // settles.
    if let ExprKind::Ident(id) = &receiver.kind
        && matches!(
            cx.commons().callee(e.id),
            Some(Callee::Store { .. } | Callee::Query { .. })
        )
        && cx.is_agent_store_map(&id.name)
    {
        let var = cx.agent_store_var().to_string();
        let m = format!("{var}.{}", id.name);
        // v0.93 (ADR 0118): the value-record fields this map is `@indexed(by:)` on.
        let idx_fields: Vec<String> = cx.agent_store_index_fields(&id.name);
        // Route an equality `filter` on an indexed field to a posting-list lookup
        // (`<map>__idx_<f>[v]`) instead of a full `Object.values` scan. The lookup
        // reads the staged map, so it stays read-your-writes (the index is
        // maintained in the same commit).
        if method.name == "filter"
            && let [arg] = args
            && let ExprKind::Lambda(lam) = &arg.kind
            && let Some(routed) = pre.absorb_opt(route_indexed_filter(
                &m,
                &var,
                &id.name,
                &idx_fields,
                lam,
                cx,
            ))
        {
            return pre.finish(routed);
        }
        let a: Vec<String> = args.iter().map(|x| pre.lower(x, cx)).collect();
        return pre.finish(match method.name.as_str() {
            // v0.93: an indexed map's mutators keep the sibling posting-lists exact
            // inside the same staged commit (re-index on last-write-wins).
            "put" if !idx_fields.is_empty() => idx_map_put(&m, &var, &id.name, &idx_fields, &a),
            "remove" if !idx_fields.is_empty() => {
                idx_map_remove(&m, &var, &id.name, &idx_fields, &a)
            }
            "update" if !idx_fields.is_empty() => {
                idx_map_update(&m, &var, &id.name, &idx_fields, &a)
            }
            "upsert" if !idx_fields.is_empty() => {
                idx_map_upsert(&m, &var, &id.name, &idx_fields, &a)
            }
            "put" => format!("(({m}[{}] = {}), undefined)", a[0], a[1]),
            "remove" => format!("((delete {m}[{}]), undefined)", a[0]),
            "contains" => format!("(({}) in {m})", a[0]),
            "size" => format!("Object.keys({m}).length"),
            "get" => format!(
                "(() => {{ const __k = {0}; return (__k in {m}) ? Some({m}[__k]) : None; }})()",
                a[0]
            ),
            "update" => format!(
                "(() => {{ const __k = {0}; if (!(__k in {m})) {{ throw new Error(\"Map.update: key absent\"); }} {m}[__k] = ({1})({m}[__k]); return undefined; }})()",
                a[0], a[1]
            ),
            "upsert" => format!(
                "(() => {{ const __k = {0}; {m}[__k] = ({2})((__k in {m}) ? {m}[__k] : ({1})); return undefined; }})()",
                a[0], a[1], a[2]
            ),
            // v0.91 (ADR 0119): any non-entry op is a lazy query that lifts the
            // map into a scan over its values (`Object.values`).
            _ => lower_query_method(format!("Object.values({m})"), method, &a, cx.commons().expr_types.get(&e.id).map(|te| te.ty), tys)
            .unwrap_or_else(|| format!("(/* unsupported Map op {} */ undefined)", method.name)),
        });
    }
    // v0.83: a storage-`Set` operation — `<set>.<op>(…)` on a `store Set[T]` field.
    // Lowers to an entry op over `__state.<set>` (a `Record<string, boolean>`):
    // `add`/`remove` mutate the working record, `contains`/`size` read it.
    //
    // P6.21 (partial, continued): reads `Callee::Store` (P6.0) instead of
    // `!cx.is_local(&id.name)` — see the storage-`Map` branch above for the
    // full reasoning. `Set` never gets `Callee::Query` (checker.rs's own
    // `StoreField::Set` arm records only `Store`), unlike `Map`/`Log`.
    if let ExprKind::Ident(id) = &receiver.kind
        && matches!(cx.commons().callee(e.id), Some(Callee::Store { .. }))
        && cx.is_agent_store_set(&id.name)
    {
        let var = cx.agent_store_var().to_string();
        let s = format!("{var}.{}", id.name);
        let a: Vec<String> = args.iter().map(|x| pre.lower(x, cx)).collect();
        return pre.finish(match method.name.as_str() {
            "add" => format!("(({s}[{}] = true), undefined)", a[0]),
            "remove" => format!("((delete {s}[{}]), undefined)", a[0]),
            "contains" => format!("(({}) in {s})", a[0]),
            "size" => format!("Object.keys({s}).length"),
            other => format!("(/* unsupported Set op {other} */ undefined)"),
        });
    }
    // v0.87 (ADR 0113): a storage-`Cache` operation — `<cache>.<op>(…)` on a
    // `store Cache[K, V]` field. Lowers to an entry op over `__state.<cache>` (a
    // `Record<string, { v, exp }>`) that applies TTL expiry against the injected
    // `Clock`: every op but `remove` reads `now()` (an awaited `Effect`), so the
    // op is an async IIFE. `put`/`update`/`upsert` stamp `exp = now() + ttl`;
    // `get`/`contains`/`size` treat an entry past `exp` as absent.
    //
    // P6.21 (partial, continued): reads `Callee::Store` instead of
    // `!cx.is_local(&id.name)` — see the storage-`Map` branch above. `Cache`
    // never gets `Callee::Query`, same as `Set`.
    if let ExprKind::Ident(id) = &receiver.kind
        && matches!(cx.commons().callee(e.id), Some(Callee::Store { .. }))
        && let Some(ttl) = cx.agent_store_cache_ttl(&id.name)
    {
        let var = cx.agent_store_var().to_string();
        let c = format!("{var}.{}", id.name);
        let now = format!("await {}.Clock.now()", cx.cap_deps_expr());
        let a: Vec<String> = args.iter().map(|x| pre.lower(x, cx)).collect();
        return pre.finish(match method.name.as_str() {
            "remove" => format!("((delete {c}[{}]), undefined)", a[0]),
            "put" => format!(
                "(async () => {{ const __now = {now}; {c}[{0}] = {{ v: {1}, exp: __now + {ttl} }}; return undefined; }})()",
                a[0], a[1]
            ),
            "get" => format!(
                "(async () => {{ const __now = {now}; const __k = {0}; return ((__k in {c}) && {c}[__k].exp > __now) ? Some({c}[__k].v) : None; }})()",
                a[0]
            ),
            "contains" => format!(
                "(async () => {{ const __now = {now}; const __k = {0}; return (__k in {c}) && {c}[__k].exp > __now; }})()",
                a[0]
            ),
            "size" => format!(
                "(async () => {{ const __now = {now}; return Object.values({c}).filter((__e) => __e.exp > __now).length; }})()"
            ),
            "update" => format!(
                "(async () => {{ const __now = {now}; const __k = {0}; if (!((__k in {c}) && {c}[__k].exp > __now)) {{ throw new Error(\"Cache.update: key absent\"); }} {c}[__k] = {{ v: ({1})({c}[__k].v), exp: __now + {ttl} }}; return undefined; }})()",
                a[0], a[1]
            ),
            "upsert" => format!(
                "(async () => {{ const __now = {now}; const __k = {0}; const __cur = ((__k in {c}) && {c}[__k].exp > __now) ? {c}[__k].v : ({1}); {c}[__k] = {{ v: ({2})(__cur), exp: __now + {ttl} }}; return undefined; }})()",
                a[0], a[1], a[2]
            ),
            other => format!("(/* unsupported Cache op {other} */ undefined)"),
        });
    }
    // v0.95 (ADR 0121): a storage-`Log` operation — `<log>.<op>(…)` on a
    // `store Log[T]` field (an array of `{ t, v }`). `append` stamps the clock
    // and pushes (pruning past `@retain`); the time-window roots and the general
    // query vocabulary lower to a lazy pipeline over the entry values.
    //
    // P6.21 (partial, continued): reads `Callee::Store`/`Callee::Query`
    // instead of `!cx.is_local(&id.name)` — see the storage-`Map` branch
    // above. `Log` gets both variants, same as `Map` (`append` is `Store`;
    // the window-root/query vocabulary is `Query`, checker.rs's own
    // `StoreField::Log` arm).
    if let ExprKind::Ident(id) = &receiver.kind
        && matches!(
            cx.commons().callee(e.id),
            Some(Callee::Store { .. } | Callee::Query { .. })
        )
        && let Some(retain) = cx.agent_store_log_retain(&id.name)
    {
        let var = cx.agent_store_var().to_string();
        let g = format!("{var}.{}", id.name);
        let a: Vec<String> = args.iter().map(|x| pre.lower(x, cx)).collect();
        if method.name == "append" {
            let now = format!("await {}.Clock.now()", cx.cap_deps_expr());
            let prune = match retain {
                Some(ms) => format!(
                    " for (let __i = {g}.length - 1; __i >= 0; __i--) {{ if ({g}[__i].t < __now - {ms}) {g}.splice(__i, 1); }}"
                ),
                None => String::new(),
            };
            return pre.finish(format!(
                "(async () => {{ const __now = {now}; {g}.push({{ t: __now, v: {0} }});{prune} return undefined; }})()",
                a[0]
            ));
        }
        // The values array feeding the general query pipeline.
        let values = format!("{g}.map((__e) => __e.v)");
        let thunk = |body: String| format!("(() => {body})");
        return pre.finish(match method.name.as_str() {
            // Time-window roots → a `Query` thunk over the windowed values.
            "since" => thunk(format!(
                "{g}.filter((__e) => __e.t >= ({0})).map((__e) => __e.v)",
                a[0]
            )),
            "before" => thunk(format!(
                "{g}.filter((__e) => __e.t < ({0})).map((__e) => __e.v)",
                a[0]
            )),
            "between" => thunk(format!(
                "{g}.filter((__e) => __e.t >= ({0}) && __e.t <= ({1})).map((__e) => __e.v)",
                a[0], a[1]
            )),
            "recent" => thunk(format!(
                "{g}.slice(Math.max(0, {g}.length - Math.max(0, {0}))).reverse().map((__e) => __e.v)",
                a[0]
            )),
            "reversed" => thunk(format!("[...{g}].reverse().map((__e) => __e.v)")),
            // The general query vocabulary over the entry values.
            _ => lower_query_method(values, method, &a, cx.commons().expr_types.get(&e.id).map(|te| te.ty), tys)
                .unwrap_or_else(|| format!("(/* unsupported Log op {} */ undefined)", method.name)),
        });
    }
    // v0.98 (ADR 0125): a storage-`Cell` operation — `<cell>.update(f)` on a
    // `store Cell[T]` field. Lowers to a staged read-modify-write over
    // `__state.<cell>` (`Map.update`'s lowering minus the key-absent guard — a
    // cell is always present, so there is no fault path). The mutation is
    // synchronous against the working state (read-your-writes); the single
    // end-of-handler `commitState` flush runs the invariant gate before the
    // durable write, exactly as `:=` does.
    // Note: unlike the other store kinds, a `Cell` field is deliberately bound
    // into checker scope (self_scope) as a bare local of its element type, so
    // the checker always resolves a same-named identifier to the cell — no
    // `is_local` guard belongs here without also changing that scope
    // construction, or the emitter would disagree with what the checker typed.
    if let ExprKind::Ident(id) = &receiver.kind
        && cx
            .agent_store_cells()
            .is_some_and(|(_, cells)| cells.contains(&id.name))
    {
        let var = cx.agent_store_var().to_string();
        let n = format!("{var}.{}", id.name);
        let a: Vec<String> = args.iter().map(|x| pre.lower(x, cx)).collect();
        return pre.finish(match method.name.as_str() {
            "update" => format!("(() => {{ {n} = ({0})({n}); return undefined; }})()", a[0]),
            other => format!("(/* unsupported Cell op {other} */ undefined)"),
        });
    }
    // v0.9: explicit `HttpResult.Variant(args)` construction. The
    // checker has already recorded the expression's type — emit it
    // directly through the runtime's HttpResult namespace.
    //
    // P6.21/P6.23 (review of #1251): reads `Callee::Intrinsic` (the sink
    // #1251 added) instead of `id.name == HTTP_RESULT` — closes the last
    // real instance of R6.5's name-matched-receiver defect class this
    // session's own P6.21 slices left open, since Decision C originally
    // excluded this shape for lack of a `Callee` to read.
    if let ExprKind::Ident(_) = &receiver.kind
        && matches!(
            cx.commons().callee(e.id),
            Some(Callee::Intrinsic { ns, .. }) if *ns == HTTP_RESULT
        )
    {
        let args_lowered: Vec<String> = args.iter().map(|a| pre.lower(a, cx)).collect();
        return pre.finish(format!(
            "HttpResult.{}({})",
            method.name,
            args_lowered.join(", ")
        ));
    }
    // v0.20b: built-in collection statics — `List.empty()` /
    // `Map.empty()`. The checker recorded the instantiated type;
    // emit it explicitly so the TS value doesn't infer as `never[]`
    // / `Map<unknown, unknown>` outside contextually-typed positions.
    //
    // P6.21 (partial, continued — the earlier #1247 attempt reverted this
    // exact conversion after a zero-diff bless caught it silently dropping
    // `Instant`/`Bytes` statics inside `.test.bynk` bodies; root-caused and
    // fixed separately — `typecheck_case_body` now returns its own
    // `callees` map instead of discarding it, so `Callee::Intrinsic` is
    // populated for test-case bodies too): reads `Callee::Intrinsic` (P6.0)
    // instead of `id.name == LIST || id.name == MAP` plus
    // `!cx.commons().types.contains_key` — the checker's own
    // `check_method_call` already resolves the identical shadowing question
    // (`ctx.lookup(...).is_none() && !ctx.input.types.contains_key(...)`,
    // `calls.rs`) before ever recording this `Callee`, closing a real gap
    // the emitter's own guard left open: no `cx.is_local(&id.name)` check,
    // unlike every other branch this session already converted.
    if let ExprKind::Ident(_) = &receiver.kind
        && matches!(
            cx.commons().callee(e.id),
            Some(Callee::Intrinsic { ns, op }) if (*ns == LIST || *ns == MAP) && op == "empty"
        )
        && args.is_empty()
    {
        match cx.commons().expr_ty(e.id).as_deref() {
            Some(Ty::List(t)) => {
                return pre.finish(format!("([] as readonly {}[])", ts_ty(*t, tys)));
            }
            Some(Ty::Map(k, v)) => {
                return pre.finish(format!("new Map<{}, {}>()", ts_ty(*k, tys), ts_ty(*v, tys)));
            }
            _ => {}
        }
    }
    // v0.22b: the typed JSON codec (ADR 0045).
    if let Some(s) = pre.absorb_opt(lower_json_codec_call(e, receiver, method, args, cx)) {
        return pre.finish(s);
    }
    // v0.22a: the numeric parse statics — `Int.parse(s)` /
    // `Float.parse(s)` (ADR 0048). Full-string parse via `Number(…)`
    // (which, unlike `parseFloat`, rejects trailing garbage); an
    // empty/whitespace-only string would coerce to `0`, so it is
    // rejected first. `Int` requires a safe integer (the honest
    // runtime "overflow → None"); `Float` requires finite (the 0040
    // posture).
    // P6.21 (partial, continued): reads `Callee::Intrinsic` instead of
    // `id.name == INT || id.name == FLOAT` — see the `List`/`Map` branch
    // above. `Int`/`Float` are lexically reserved (the parser only admits
    // them here followed by `.`), so unlike `List`/`Map` this specific guard
    // never had a real shadowing gap to close — converted for consistency
    // with every other static-constructor branch in this function.
    if let ExprKind::Ident(id) = &receiver.kind
        && matches!(
            cx.commons().callee(e.id),
            Some(Callee::Intrinsic { ns, op }) if (*ns == INT || *ns == FLOAT) && op == "parse"
        )
        && args.len() == 1
    {
        let s = pre.lower(&args[0], cx);
        let guard = if id.name == INT {
            "Number.isSafeInteger(__n)"
        } else {
            "Number.isFinite(__n)"
        };
        return pre.finish(format!(
            "((__s: string) => {{ const __n = __s.trim() === \"\" ? Number.NaN : Number(__s); return {guard} ? Some(__n) : None; }})({s})"
        ));
    }
    // v0.86 (ADR 0112): `Duration.millis(n)` — the runtime `Int`→`Duration`
    // constructor. A `Duration` lowers to its milliseconds, so this is the
    // identity on the argument.
    //
    // P6.21 (partial, continued): reads `Callee::Intrinsic` instead of
    // `id.name == DURATION` — see the `List`/`Map` branch above for why this
    // closes a real shadowing gap the bare name check left open (the checker
    // itself guards `ctx.lookup(DURATION).is_none()` before ever recording
    // this `Callee`; the same applies to `Instant`/`Bytes`/`Stream` below).
    if let ExprKind::Ident(_) = &receiver.kind
        && matches!(
            cx.commons().callee(e.id),
            Some(Callee::Intrinsic { ns, op }) if *ns == DURATION && op == "millis"
        )
        && args.len() == 1
    {
        let inner = pre.lower(&args[0], cx);
        return pre.finish(inner);
    }
    // v0.90 (ADR 0114): `Instant.fromEpochMillis(n)` — an `Instant` lowers to
    // its epoch milliseconds, so this is the identity on the argument.
    if let ExprKind::Ident(_) = &receiver.kind
        && matches!(
            cx.commons().callee(e.id),
            Some(Callee::Intrinsic { ns, op }) if *ns == INSTANT && op == "fromEpochMillis"
        )
        && args.len() == 1
    {
        let inner = pre.lower(&args[0], cx);
        return pre.finish(inner);
    }
    // v0.110 (ADR 0142 D2): the `Bytes` static constructors. `fromUtf8` is the
    // UTF-8 encoding of a string (total); `fromBase64` is a guarded base64
    // decode returning `Option` (`None` on invalid base64); `empty` is the
    // zero octet sequence.
    if let ExprKind::Ident(_) = &receiver.kind
        && matches!(cx.commons().callee(e.id), Some(Callee::Intrinsic { ns, .. }) if *ns == BYTES)
    {
        match (method.name.as_str(), args.len()) {
            ("fromUtf8", 1) => {
                let s = pre.lower(&args[0], cx);
                return pre.finish(format!("new TextEncoder().encode({s})"));
            }
            ("fromBase64", 1) => {
                let s = pre.lower(&args[0], cx);
                cx.note_bytes();
                return pre.finish(format!("__bynkBytesFromBase64({s})"));
            }
            ("empty", 0) => {
                return pre.finish("new Uint8Array()".to_string());
            }
            _ => {}
        }
    }
    // v0.100: `Stream.of(xs)` — the deterministic in-memory source. A `Stream`
    // lowers to a host async iterable; `of` wraps a list as an async generator.
    // Emitted inline (no runtime import), like the collection kernels.
    if let ExprKind::Ident(_) = &receiver.kind
        && matches!(
            cx.commons().callee(e.id),
            Some(Callee::Intrinsic { ns, op }) if *ns == STREAM && op == "of"
        )
        && args.len() == 1
    {
        let xs = pre.lower(&args[0], cx);
        return pre.finish(format!(
            "(async function* () {{ for (const __e of {xs}) {{ yield __e; }} }})()"
        ));
    }
    // v0.15 cross-context capability call: `B.Cap.op(args)` /
    // `Alias.Cap.op(args)`. The provider is instantiated locally in
    // the composition root, so this lowers to an in-process
    // `<deps>.<Cap>.op(args)` exactly like a local capability call —
    // the consumed-context prefix is resolved away.
    if let Some(chain) = flatten_emit_ident_chain(receiver)
        && let Some((consumed, cap)) = cx.cross_context().resolve_cross_capability(&chain)
    {
        let mut args_lowered: Vec<String> = args.iter().map(|a| pre.lower(a, cx)).collect();
        scope_idempotency_key(
            cap == "Idempotency" && consumed == "bynk",
            &method.name,
            &mut args_lowered,
            cx,
        );
        return pre.finish(format!(
            "{}.{}.{}{}({})",
            cx.cap_deps_expr(),
            cap,
            method.name,
            capability_call_type_args_ts(type_args),
            args_lowered.join(", ")
        ));
    }
    // v0.6 cross-context service call: receiver is an alias or the
    // dotted name of a consumed context.
    if let Some(s) = pre.absorb_opt(lower_cross_context_service_call(receiver, method, args, cx)) {
        return pre.finish(s);
    }
    // Events track, slice 0 (spine #936): `Events.emit[E](event)` never
    // calls through a provider like an ordinary capability op — release-
    // at-commit (events.md §3.0) buffers it into the handler's own
    // `__events` local instead, flushed only if the handler completes
    // without throwing. The local is declared by the outer body wrapper
    // whenever `block_uses_emit` is true (the same syntactic check this
    // receiver/method match mirrors), so the two stay in lockstep by
    // construction. The flush calls `deps.__eventsDispatch`, which the
    // publishing context's own compose wires to the Cloudflare fan-out DO
    // (Workers) or an in-process dispatch closure (Bundle) — see
    // `emit_events_fanout_do`/`project.rs`'s `__eventsDispatch` construction.
    // P6.21 (continued, review of #1254): reads `Callee::Capability` (P6.0)
    // instead of `id.name == "Events" && method.name == "emit"` — a real
    // instance of R6.5's name-matched-receiver defect class, the same one
    // every other branch in this module already closed. `unit_table_uses_
    // emit` (`project.rs`) already reads this exact `Callee` for the
    // identical classification, project-wide. `cx.is_first_party_events()`
    // stays: it answers a different question (is *this unit's* `Events`
    // capability actually the first-party runtime implementation, not
    // which provider/target the emitted TS should call at all) than the
    // one `Callee` settles.
    if let ExprKind::Ident(_) = &receiver.kind
        && matches!(
            cx.commons().callee(e.id),
            Some(Callee::Capability { cap, op }) if cap == "Events" && op == "emit"
        )
        && cx.is_first_party_events()
    {
        let event_name = type_args
            .first()
            .and_then(|t| match t {
                TypeRef::Named(ident) => Some(ident.name.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "unknown".to_string());
        let payload = pre.lower(&args[0], cx);
        // Events track, slice 2 (spine #936): the envelope is minted here,
        // once, before fan-out duplicates this single `__events` entry to
        // every subscriber — so one emission always carries one `eventId`/
        // `emittedAt` to all of them (the invariant the identical-eventId
        // fixture proves). `publisherId` is the emitting *context's*
        // qualified name, not the emitting agent's instance identity:
        // `Events.emit` is also legal from a plain, keyless service handler
        // with no agent to report (`negative/503_events_emit_outside_owner`),
        // so a context-scoped identifier is the only one available
        // uniformly at every legal emission site — an amendment to
        // design/bynk-design-notes.md §7's "the publisher is the emitting
        // agent" framing (events-envelope ADR). `emittedAt` uses bare
        // `Date.now()`, not `given Clock`, matching the runtime's existing
        // JWT exp/nbf precedent of bypassing the `Clock` capability for its
        // own internal timestamps — requiring `Clock` alongside `Events`
        // would be new ambient coupling and would break every existing
        // fixture that emits with `given Events` alone. `schemaVersion` is
        // the event's own declared `@schema(N)` (Events slice 3b, #978),
        // or `1` if it declares none — a real, per-type computed value is
        // still a future slice (3c, the cross-build schema registry).
        let schema_version = cx.event_schema_version(&event_name);
        let publisher_id = escape_ts_string(cx.owning_context().unwrap_or_default());
        return pre.finish(format!(
            "(async () => {{ __events.push({{ type: \"{event_name}\", payload: {payload}, envelope: {{ eventId: crypto.randomUUID(), publisherId: \"{publisher_id}\", emittedAt: Date.now(), schemaVersion: {schema_version} }} }}); }})()"
        ));
    }
    // Capability call: receiver is a bare ident naming a declared
    // capability in `given`. Lower to `<deps>.Capability.op(args)`,
    // where `<deps>` is `deps` in a handler body and `this.deps` in a
    // provider body (v0.12 provider composition).
    if let ExprKind::Ident(id) = &receiver.kind
        && cx.has_capability(&id.name)
    {
        let mut args_lowered: Vec<String> = args.iter().map(|a| pre.lower(a, cx)).collect();
        // #934: a flattened `consumes bynk { Idempotency }` local name is
        // first-party only if it's actually `bynk`'s own capability — either
        // declared right here (this unit *is* `bynk`) or flattened in from it
        // (`flattened_caps`). A same-named capability from any other adapter
        // or context (e.g. the #926/#931 generic-op test fixtures' own
        // illustrative `capability Idempotency`) must not be scoped.
        let is_first_party = id.name == "Idempotency"
            && (cx.in_bynk_unit()
                || cx
                    .cross_context()
                    .flattened_caps
                    .get(&id.name)
                    .map(String::as_str)
                    == Some("bynk"));
        scope_idempotency_key(is_first_party, &method.name, &mut args_lowered, cx);
        return pre.finish(format!(
            "{}.{}.{}{}({})",
            cx.cap_deps_expr(),
            id.name,
            method.name,
            capability_call_type_args_ts(type_args),
            args_lowered.join(", ")
        ));
    }
    // Static call: receiver is a bare ident naming a declared type.
    if let ExprKind::Ident(id) = &receiver.kind
        && cx.commons().types.contains_key(&id.name)
    {
        let args_lowered: Vec<String> = args.iter().map(|a| pre.lower(a, cx)).collect();
        return pre.finish(format!(
            "{}.{}({})",
            id.name,
            method.name,
            args_lowered.join(", ")
        ));
    }
    // v0.7 / v0.182: local service call inside a test case body. `svc.call(args)`
    // lowers to `svc.call(args, deps)`. An http address `svc.<VERB>("/path", …)`
    // (#664) lowers to the emitted handler key `svc.http_<VERB>_<path>(<rest>,
    // <deps>)`, where `<deps>` folds in the call-site identity when present.
    // Slice B: a `system`-tier http address drives a real `worker.fetch` through
    // a driver (`__sysdrive_<svc>_<key>(rest, sub)`), not a direct handler call.
    if let ExprKind::Ident(id) = &receiver.kind
        && cx.is_system_http_service(&id.name)
        && let Some(verb) = bynk_syntax::ast::HttpMethod::from_ident(&method.name)
        && let Some(first) = args.first()
        && let ExprKind::StrLit(path) = &first.kind
    {
        // #707: a `(method, path)` whose path is a declared route but whose
        // method has no handler is a **wrong-method** call — drive the generic
        // `405` driver. The checker has already rejected a path that is not
        // declared for any method, so an absent route here means wrong method.
        let route_declared =
            cx.has_system_http_route(&(id.name.clone(), verb.as_str().to_string(), path.clone()));
        if !route_declared {
            return pre.finish(format!(
                "__sysdrive_wrongmethod_{}({:?}, {:?})",
                id.name,
                verb.as_str(),
                path
            ));
        }
        let key = crate::emitter::http_handler_method_name(verb, path);
        let sub = cx
            .call_site_identity
            .clone()
            .unwrap_or_else(|| "\"\"".to_string());
        // #706: `by Nobody` drives the *no-auth* driver — no `Authorization`
        // header, so the real seam rejects it (`401` → `Rejected(Unauthorized)`);
        // it takes precedence over the body form (the seam rejects before the
        // body is read). Otherwise — Slice C: a `Wire(…)` argument drives the
        // *raw* driver (input sent unvalidated, result decodes to an
        // `HttpOutcome`); a fully typed call keeps the serialising driver.
        // #821: the two axes are independent — a `Wire(…)` argument combined
        // with `by Nobody` drives the raw *and* no-auth driver together, so
        // the missing-credential rejection and the unvalidated body both
        // apply, rather than one silently discarding the other.
        let has_wire = args[1..]
            .iter()
            .any(|a| matches!(&a.kind, ExprKind::Wire(_)));
        let driver = match (cx.call_site_no_credential, has_wire) {
            (true, true) => "__sysdrive_rawnoauth",
            (true, false) => "__sysdrive_noauth",
            (false, true) => "__sysdrive_raw",
            (false, false) => "__sysdrive",
        };
        let is_raw = driver == "__sysdrive_raw" || driver == "__sysdrive_rawnoauth";
        // #708/#821: the raw (and raw+no-auth) driver's every slot is a
        // `string`. `lower_expr_into` already lowers a `Wire(s)` to its raw inner
        // string, but a *typed* arg mixed into the same raw call must be
        // converted to that `string` slot: the body param serialises through
        // the same wire codec the typed driver uses (so a hand-typed body
        // matches a `Wire`d one byte-for-byte); any other (path) param just
        // coerces via `String(...)`.
        let body_info = if is_raw {
            cx.system_http_route_body(&(id.name.clone(), verb.as_str().to_string(), path.clone()))
                .cloned()
        } else {
            None
        };
        let rest: Vec<String> = args[1..]
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let lowered = pre.lower(a, cx);
                if !is_raw || matches!(&a.kind, ExprKind::Wire(_)) {
                    return lowered;
                }
                match &body_info {
                    // A named type's typed literal is unbranded in test-scaffold
                    // code (the checker already validated it against the
                    // handler's declared type — Slice A), the same reason
                    // `driver_param_ty` types a named driver param `any` rather
                    // than the branded type. Cast through `any` here too, so
                    // `serialise_<Name>` (which expects the branded shape)
                    // accepts the plain object literal.
                    Some((body_idx, ty)) if *body_idx == i => format!(
                        "JSON.stringify({})",
                        crate::emitter::serialisation::serialise_expr_via(
                            ty,
                            &format!("({lowered} as any)"),
                            cx.system_http_type_ns(),
                            cx.runtime_use()
                        )
                    ),
                    _ => format!("String({lowered})"),
                }
            })
            .collect();
        let mut all = rest;
        all.push(sub);
        return pre.finish(format!(
            "{}_{}_{}({})",
            driver,
            id.name,
            key,
            all.join(", ")
        ));
    }
    if let ExprKind::Ident(id) = &receiver.kind
        && cx.is_test_service(&id.name)
    {
        let deps_expr = match cx.call_site_identity.clone() {
            Some(identity) => format!("{{ ...deps, identity: ({identity} as any) }}"),
            None => "deps".to_string(),
        };
        // http address: the method is an HTTP verb and the first argument is the
        // route pattern string. The key is a pure function of verb + path; the
        // remaining args are the handler's positional params.
        if let Some(verb) = bynk_syntax::ast::HttpMethod::from_ident(&method.name)
            && let Some(first) = args.first()
            && let ExprKind::StrLit(path) = &first.kind
        {
            let key = crate::emitter::http_handler_method_name(verb, path);
            let rest: Vec<String> = args[1..].iter().map(|a| pre.lower(a, cx)).collect();
            let mut all = rest;
            all.push(deps_expr);
            return pre.finish(format!("{}.{}({})", id.name, key, all.join(", ")));
        }
        // cron/queue address: the emitted key is position-indexed among the
        // service's same-kind handlers, so recover the index from the handler
        // list. cron drops the leading schedule string; queue passes the
        // message. P6.37 (design/tracks/the-ir.md §6a): reads `IrHandlerKind`
        // (P6.24a's own pure, unconditional mirror) rather than the raw AST
        // `HandlerKind` this map used to store.
        if let Some(handlers) = cx.test_service_handlers(&id.name) {
            if method.name == "schedule"
                && let Some(ExprKind::StrLit(expr)) = args.first().map(|a| &a.kind)
            {
                let mut idx = 0usize;
                for h in handlers {
                    if let crate::ir::IrHandlerKind::Cron { expr: e } = h {
                        if e == expr {
                            break;
                        }
                        idx += 1;
                    }
                }
                let key = crate::emitter::cron_handler_method_name(&id.name, idx);
                let rest: Vec<String> = args[1..].iter().map(|a| pre.lower(a, cx)).collect();
                let mut all = rest;
                all.push(deps_expr);
                return pre.finish(format!("{}.{}({})", id.name, key, all.join(", ")));
            }
            if method.name == "message"
                && handlers
                    .iter()
                    .any(|h| matches!(h, crate::ir::IrHandlerKind::Message))
            {
                // A `from queue` service binds exactly one queue and declares one
                // `on message` handler, so the position index is 0.
                let key = crate::emitter::queue_handler_method_name(&id.name, 0);
                let args_lowered: Vec<String> = args.iter().map(|a| pre.lower(a, cx)).collect();
                let mut all = args_lowered;
                all.push(deps_expr);
                return pre.finish(format!("{}.{}({})", id.name, key, all.join(", ")));
            }
        }
        // `svc.call(args)` and other (non-http) forms: pass args through with deps.
        let args_lowered: Vec<String> = args.iter().map(|a| pre.lower(a, cx)).collect();
        let mut all = args_lowered;
        all.push(deps_expr);
        return pre.finish(format!("{}.{}({})", id.name, method.name, all.join(", ")));
    }
    // v0.9.2: inline agent invocation. Source form is
    // `Agent(<key>).method(args)`; receiver parses as
    // `Call(Agent, [<key>])`. Lower to
    // `__makeAgent(<key>).method(args, deps)`. Works in service and
    // agent-handler bodies (deps is the handler's deps parameter) and
    // test bodies (deps is the locally-built makeTestDeps record).
    //
    // P6.21 (partial, continued): reads `Callee::Agent` (P6.0) instead of
    // `cx.local_agents.contains(&name.name)` — the checker's own
    // `check_method_call` records this the moment a receiver's *checked
    // type* resolves to an agent, uniformly for this shape, the let-bound
    // shape below, and the self-agent WS-transfer case inside this branch —
    // closing a real instance of R6.5's name-matched-receiver defect class
    // (a free function or local sharing an agent's own name could misfire
    // the old check; the checker's own type-driven resolution cannot).
    if let ExprKind::Call {
        name,
        args: ctor_args,
        ..
    } = &receiver.kind
        && matches!(cx.commons().callee(e.id), Some(Callee::Agent { .. }))
    {
        // v0.104 (real-time track slice 3b): when lowering a `from websocket`
        // `on open` body inside its hosting Durable Object, a transfer to the
        // self-agent is a direct `this.method(args, deps)` self-call — the key
        // addresses *this* DO, and the held connection never crosses an RPC
        // boundary (DECISION A). The key expression is **not** emitted: the shape
        // constraint (`bynk.ws.open_transfer_shape`, D2) restricts it to a
        // request-derivable param ident — side-effect-free and equal to this
        // instance's own key — so dropping it is sound.
        if cx.ws_self_agent() == Some(name.name.as_str()) {
            cx.record_agent_call(&name.name, &method.name);
            let args_lowered: Vec<String> = args.iter().map(|a| pre.lower(a, cx)).collect();
            let mut all = args_lowered;
            all.push("deps".to_string());
            return pre.finish(format!(
                "this.{method}({args})",
                method = method.name,
                args = all.join(", ")
            ));
        }
        cx.record_agent_call(&name.name, &method.name);
        let key_arg = ctor_args
            .first()
            .map(|a| pre.lower(a, cx))
            .unwrap_or_else(|| "\"default\"".to_string());
        let instance = cx.agent_construct(&name.name, &key_arg);
        let args_lowered: Vec<String> = args.iter().map(|a| pre.lower(a, cx)).collect();
        let mut all = args_lowered;
        all.push("deps".to_string());
        return pre.finish(format!(
            "{instance}.{method}({args})",
            method = method.name,
            args = all.join(", ")
        ));
    }
    // Let-bound agent invocation. `let x = Agent(key); x.method(args)`
    // — the statement emitter recorded `x` as an agent variable when
    // it lowered the let. Method calls on `x` go straight to the
    // class instance with `deps` threaded through.
    //
    // P6.21 (partial, continued): reads `Callee::Agent` instead of
    // `cx.local_agent_vars.contains_key(&id.name)` — see the inline
    // agent-invocation branch above. `cx.local_agent_vars` stays: it still
    // answers "which agent does this bind name refer to" (needed for
    // `record_agent_call`'s own bookkeeping and the #908 rename-resolution
    // below), a different question from "is this receiver really an agent
    // instance at all," which `Callee` now settles.
    if let ExprKind::Ident(id) = &receiver.kind
        && matches!(cx.commons().callee(e.id), Some(Callee::Agent { .. }))
    {
        if let Some(agent) = cx.local_agent_vars.get(&id.name).cloned() {
            cx.record_agent_call(&agent, &method.name);
        }
        let args_lowered: Vec<String> = args.iter().map(|a| pre.lower(a, cx)).collect();
        let mut all = args_lowered;
        all.push("deps".to_string());
        // #908: `local_agent_vars` is keyed by the bynk source name, which a
        // re-`let` may have renamed — resolve to the actual JS binding rather
        // than assuming the source name is still it.
        let recv = cx
            .resolved_local_name(&id.name)
            .unwrap_or_else(|| id.name.clone());
        return pre.finish(format!("{}.{}({})", recv, method.name, all.join(", ")));
    }
    // v0.20b: built-in kernel methods on the collection types,
    // dispatched on the receiver's checked type. Emitted inline
    // (typed IIFEs / spreads) — no runtime imports, so files that
    // never touch collections emit byte-identically to v0.20a.
    if let Some(recv_ty) = cx.commons().expr_ty(receiver.id).as_deref().cloned() {
        match &recv_ty {
            Ty::List(elem) => {
                if let Some(s) =
                    pre.absorb_opt(lower_list_kernel(e, receiver, method, args, *elem, cx))
                {
                    return pre.finish(s);
                }
            }
            // v0.91 (ADR 0119): a chained op on a lazy `Query` — the source is
            // the receiver thunk, invoked (`(recv)()`).
            Ty::Query(_) => {
                let recv = pre.lower(receiver, cx);
                let a: Vec<String> = args.iter().map(|x| pre.lower(x, cx)).collect();
                let result_ty = cx.commons().expr_types.get(&e.id).map(|te| te.ty);
                if let Some(s) =
                    lower_query_method(format!("({recv})()"), method, &a, result_ty, tys)
                {
                    return pre.finish(s);
                }
            }
            // v0.100: a chained op on a `Stream` — the receiver already *is* an
            // async iterable, so it is the source directly. Emitted inline as
            // async-generator IIFEs (builders) / an async drain (`collect`).
            Ty::Stream(_) => {
                let recv = pre.lower(receiver, cx);
                let a: Vec<String> = args.iter().map(|x| pre.lower(x, cx)).collect();
                if let Some(s) = lower_stream_method(recv, method, &a) {
                    return pre.finish(s);
                }
            }
            // v0.102: the held-resource operations on a `Connection[F]` lower to
            // method calls on the runtime `Connection` object — `send(frame)` and
            // `close()`. The linearity pass has already verified ownership.
            Ty::Connection(_) => {
                let recv = pre.lower(receiver, cx);
                let a: Vec<String> = args.iter().map(|x| pre.lower(x, cx)).collect();
                return pre.finish(format!("({recv}).{}({})", method.name, a.join(", ")));
            }
            Ty::Map(key, val) => {
                if let Some(s) =
                    pre.absorb_opt(lower_map_kernel(receiver, method, args, *key, *val, cx))
                {
                    return pre.finish(s);
                }
            }
            // v0.21: the numeric kernel. `toFloat` is the identity
            // at runtime (the Int/Float distinction is erased);
            // the four `Float -> Int` roundings map onto `Math.*`.
            // v0.22a extends it (abs/min/max/clamp, isNaN/isFinite).
            Ty::Base(BaseType::Int | BaseType::Float) => {
                if let Some(s) = pre.absorb_opt(lower_numeric_kernel(receiver, method, args, cx)) {
                    return pre.finish(s);
                }
            }
            // v0.86 (ADR 0112): the `Duration` kernel. `toMillis` is the identity
            // at runtime (a `Duration` already *is* its milliseconds); `toString`
            // renders the number.
            Ty::Base(BaseType::Duration) => {
                if let Some(s) = pre.absorb_opt(lower_duration_kernel(receiver, method, args, cx)) {
                    return pre.finish(s);
                }
            }
            // v0.90 (ADR 0114): the `Instant` kernel. `toEpochMillis` is the
            // identity (an `Instant` lowers to its epoch millis); `toString`
            // renders the number.
            Ty::Base(BaseType::Instant) => {
                if let Some(s) = pre.absorb_opt(lower_instant_kernel(receiver, method, args, cx)) {
                    return pre.finish(s);
                }
            }
            // v0.110 (ADR 0142): the `Bytes` kernel. `length` is the octet
            // count; `toBase64` encodes; `decodeUtf8` is a guarded UTF-8 decode
            // returning `Option`.
            Ty::Base(BaseType::Bytes) => {
                if let Some(s) = pre.absorb_opt(lower_bytes_kernel(receiver, method, args, cx)) {
                    return pre.finish(s);
                }
            }
            // v0.22a: the string kernel (ADR 0046).
            Ty::Base(BaseType::String) => {
                if let Some(s) = pre.absorb_opt(lower_string_kernel(receiver, method, args, cx)) {
                    return pre.finish(s);
                }
            }
            // v0.22a: Option/Result combinators (ADR 0048).
            Ty::Option(inner) => {
                if let Some(s) =
                    pre.absorb_opt(lower_option_kernel(e, receiver, method, args, *inner, cx))
                {
                    return pre.finish(s);
                }
            }
            Ty::Result(ok, err) => {
                if let Some(s) = pre.absorb_opt(lower_result_kernel(
                    e, receiver, method, args, *ok, *err, cx,
                )) {
                    return pre.finish(s);
                }
            }
            // §2.8.3: the `Effect[Result[T, E]]` combinators — `mapOk`/`mapErr`/
            // `flatMapOk`/`flatMapErr`. Lowered as an `async` IIFE that awaits the
            // receiver `Promise<Result<…>>` and rebuilds the transformed Result.
            Ty::Effect(inner) => {
                if let Ty::Result(ok, err) = &*tys.get(*inner)
                    && let Some(s) = pre.absorb_opt(lower_effect_result_kernel(
                        e, receiver, method, args, *ok, *err, cx,
                    ))
                {
                    return pre.finish(s);
                }
            }
            // #561: a refined receiver inherits its base type's read-only kernel
            // methods (DECISION D). A refined value erases to its branded base
            // representation, so the inherited call lowers through the *same*
            // base-kernel helper as a plain base receiver — the emitted call is
            // byte-identical, with no unwrap/`.raw` step. Declared methods win:
            // when the refined type declares this method it takes the UFCS tail
            // below, so route to the kernel only when it is *not* declared.
            // `Bool` has no kernel and always falls through.
            Ty::Named {
                name,
                kind: NamedKind::Refined(base),
                ..
            } if !cx
                .commons()
                .methods
                .get(name)
                .is_some_and(|t| t.instance.contains_key(&method.name)) =>
            {
                let lowered = match base {
                    BaseType::Int | BaseType::Float => {
                        pre.absorb_opt(lower_numeric_kernel(receiver, method, args, cx))
                    }
                    BaseType::String => {
                        pre.absorb_opt(lower_string_kernel(receiver, method, args, cx))
                    }
                    BaseType::Duration => {
                        pre.absorb_opt(lower_duration_kernel(receiver, method, args, cx))
                    }
                    BaseType::Instant => {
                        pre.absorb_opt(lower_instant_kernel(receiver, method, args, cx))
                    }
                    BaseType::Bytes => {
                        pre.absorb_opt(lower_bytes_kernel(receiver, method, args, cx))
                    }
                    BaseType::Bool => None,
                };
                if let Some(s) = lowered {
                    return pre.finish(s);
                }
            }
            _ => {}
        }
    }
    // Instance call: UFCS lowering with the receiver as first arg.
    let ns = cx
        .receiver_namespace(receiver)
        .unwrap_or_else(|| "/* unknown */".to_string());
    let recv = pre.lower(receiver, cx);
    let mut all = vec![recv];
    for a in args {
        all.push(pre.lower(a, cx));
    }
    pre.finish(format!("{ns}.{}({})", method.name, all.join(", ")))
}

/// v0.22b: the typed JSON codec (ADR 0045). `encode` dispatches to the
/// module-local `serialise_*` helpers + `JSON.stringify`; `decode[T]` to
/// `JSON.parse` + `deserialise_*`, mapping a parse failure to a `Malformed`
/// JsonError and a BoundaryError to the uniform `kind`/`path`/`message`
/// record (ADR 0047). Returns `None` when the receiver is not `Json` or the
/// shape does not match, so the dispatcher falls through.
fn lower_json_codec_call(
    e: &Expr,
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    cx: &mut LowerCtx,
) -> Option<Lowered> {
    let tys = cx.commons().tys();
    let mut pre = Pre::new();
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == JSON
        && args.len() == 1
    {
        if method.name == "encode"
            && let Some(arg_ty) = cx
                .commons()
                .expr_types
                .get(&args[0].id)
                .map(|te| &te.ty)
                .cloned()
            && let Some(tref) = ty_to_type_ref(arg_ty, tys)
        {
            // #917: a test-scaffold module's target/`uses` types export no codec
            // of their own — record the root so the module that spliced this
            // body in can generate its own `serialise_*`/`deserialise_*` closure,
            // the same way a workers cross-context caller generates one for a
            // consumed context's boundary types (#661). P6.28: pushes the `TyId`
            // already in hand, not a re-derived `TypeRef` — `tref` below still
            // exists for this call site's own, separate codec-rendering use.
            if cx.in_test_scaffold() {
                cx.runtime_use().note_json_codec_root(arg_ty);
            }
            let v = pre.lower(&args[0], cx);
            let ser = serialisation::serialise_expr(&tref, &v, cx.runtime_use());
            return Some(pre.finish(format!("JSON.stringify({ser})")));
        }
        if method.name == "decode"
            && let Some(Ty::Result(t, _)) = cx.commons().expr_ty(e.id).as_deref().cloned()
            && let Some(tref) = ty_to_type_ref(t, tys)
        {
            // #917: in a test-scaffold module `T` may be foreign (owned by the
            // suite target or one of its `uses`, never declared locally), so its
            // TS type positions reach through the module's namespace qualifier —
            // empty everywhere else, where this renders identically to `ts_ty`.
            let ts = if cx.in_test_scaffold() {
                let qual = cx.runtime_use().json_codec_qual();
                serialisation::ts_type_ref_qualified(&tref, &qual)
            } else {
                ts_ty(t, tys)
            };
            // P6.28: pushes the `TyId` already in hand, not a re-derived
            // `TypeRef` — `tref` above still exists for this call site's own,
            // separate codec-rendering use.
            if cx.in_test_scaffold() {
                cx.runtime_use().note_json_codec_root(t);
            }
            // #914: the wrapper below names `Result`, `JsonValue` and `JsonError`
            // in its own signature and body, whichever arm the inner deserialiser
            // takes — including the delegating ones, which record nothing. A
            // module that curates its import list (the test-scaffold modules)
            // otherwise emits all three unimported.
            cx.runtime_use().note_json_codec();
            let des = serialisation::deserialise_expr(&tref, "__j", "$", cx.runtime_use());
            let arg = pre.lower(&args[0], cx);
            return Some(pre.finish(format!(
                "((__s: string): Result<{ts}, JsonError> => {{ \
                 let __j: JsonValue; \
                 try {{ __j = JSON.parse(__s) as JsonValue; }} \
                 catch (__e) {{ return Err({{ kind: \"Malformed\", path: \"$\", message: String(__e) }}); }} \
                 const __r = {des}; \
                 if (__r.tag === \"Ok\") return Ok(__r.value as {ts}); \
                 const __be = __r.error; \
                 return Err({{ kind: __be.kind, \
                 path: (__be.kind === \"StructuralMismatch\" || __be.kind === \"RefinementViolation\") ? __be.path : \"$\", \
                 message: __be.kind === \"StructuralMismatch\" ? `expected ${{__be.expected}}, got ${{String(__be.actual)}}` : __be.kind === \"RefinementViolation\" ? __be.violation.message : __be.details }}); }})({arg})"
            )));
        }
    }
    let text: Option<String> = None;
    text.map(|expr| pre.finish(expr))
}

/// v0.6 cross-context service call: receiver is an alias or the dotted name
/// of a consumed context. In bundle mode, lower to
/// `deps.surface.<key>.<method>(args as <consumed_ns>.<T>)`; in workers mode
/// (v0.8), lower to
/// `callService(deps.env.<BINDING>, "<method>", serialise...(args), deserialise_<R>)`.
/// Returns `None` when the receiver is not a consumed-context prefix.
fn lower_cross_context_service_call(
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    cx: &mut LowerCtx,
) -> Option<Lowered> {
    let mut pre = Pre::new();
    let text: Option<String> =
        if let Some((consumed, key)) = cross_context_lowering_prefix(receiver, cx) {
            cx.note_cross_context_used();
            match cx.target() {
                BuildTarget::Bundle => {
                    let args_lowered: Vec<String> = args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            let lowered = pre.lower(a, cx);
                            param_cast(&consumed, cx.cross_context(), method, i, lowered)
                        })
                        .collect();
                    Some(format!(
                        "deps.surface.{key}.{}({})",
                        method.name,
                        args_lowered.join(", ")
                    ))
                }
                BuildTarget::Workers => {
                    let _ = key;
                    let call = pre.absorb(lower_workers_cross_context_call(
                        &consumed, method, args, cx,
                    ));
                    return Some(pre.finish(call));
                }
            }
        } else {
            None
        };
    text.map(|expr| pre.finish(expr))
}

fn lower_val(type_ref: &TypeRef, args: &[Expr], cx: &mut LowerCtx) -> Lowered {
    let tys = cx.commons().tys();
    let mut pre = Pre::new();
    // Resolve the mocked type straight from the AST node rather than the
    // checker's `expr_types` side-table — the static type table is always
    // populated, whereas a test body's per-expression types may not be visible
    // to the emitter.
    let ty = match bynk_check::checker::resolve_type_ref(type_ref, &cx.commons().types, tys) {
        Some(t) => t,
        None => return pre.finish("undefined /* mock: unresolved type */".to_string()),
    };
    // Refined literal pin. This `Val[T](lit)` path is test-only scaffolding, where
    // the branded type is an `any` value binding, not a type (ADR 0182) — so brand
    // via `unchecked_construct_test` (refined → `(lit as any)`), not a `(lit as T)`
    // that would fail to resolve `T`. Opaque never reaches here (`NamedKind::Refined`).
    let ty_node = tys.get(ty);
    if let (
        Some(arg),
        Ty::Named {
            name,
            kind: NamedKind::Refined(_),
            ..
        },
    ) = (args.first(), &*ty_node)
    {
        let raw = lower_const_literal_raw(arg).unwrap_or_else(|| pre.lower(arg, cx));
        return pre.finish(unchecked_construct_test(name, &raw, false));
    }
    // Bare mock (refined / opaque / sum / record).
    pre.finish(mock_value(ty, cx, MOCK_DEPTH))
}

/// When we encounter `lhs && rhs`, see if lhs is an `is` (possibly wrapped
/// in parens or nested `&&`) and if so collect the bindings to inject into
/// rhs. Returns `(binding_const_decls, lowered_lhs, lowered_rhs)` if
/// special handling is appropriate; otherwise returns None.
fn lower_and_with_is(
    lhs: &Expr,
    rhs: &Expr,
    cx: &mut LowerCtx,
) -> Option<(Vec<String>, Lowered, String, bool)> {
    // Probe structurally (no lowering) so a `&&` without an `is` falls through
    // to the caller's ordinary lowering untouched. This mirrors exactly the
    // shapes `gather_is_bindings_for_emit` walks (`&&` and parens), preserving
    // the original "Some iff lhs contains an `is`" behaviour.
    if !cond_contains_is(lhs) {
        return None;
    }
    // Lower lhs *before* gathering its bindings. Lowering lifts any complex
    // `is` receiver (e.g. a call) into a shared temp recorded on `cx`; the
    // binding gatherer below then references that temp via `is_receiver_text`
    // instead of re-emitting the receiver. For simple receivers nothing is
    // cached and the output is byte-identical to before.
    let lhs_lowered = lower_expr(lhs, cx);
    // #908: the gathered bindings are only in scope for `rhs` (the caller
    // wraps them together in one IIFE) — push a frame so a same-named `rhs`
    // read resolves to the binding rather than an outer `let` rename, and pop
    // once `rhs` is lowered so the registration doesn't leak past this `&&`.
    cx.shadow_scopes.push(HashMap::new());
    let mut bindings = Vec::new();
    let mut found = false;
    gather_is_bindings_for_emit(lhs, cx, &mut bindings, &mut found);
    let _ = found; // guaranteed true by the `cond_contains_is` guard above
    // #1/#3 review: `rhs`'s own hoisted statements used to be joined into one
    // string and spliced back in as if they were part of the expression text
    // (`bindings.is_empty()` — the common case, not an edge one — took the
    // caller's `format!("{lhs_expr} && {rhs_expr}")` path unedited, which is a
    // syntax error the moment `rhs` hoists anything: a `const` declaration
    // where JS expects an expression). `lower_expr` keeps them as real
    // statements instead, appended after the is-bindings — both must run, in
    // order, before the final `return rhs_expr` the caller wraps them in, so
    // one combined list is exactly what the caller's existing wrap already
    // expects.
    // T2.3 (R6.3): isolate the flag to just `rhs`'s own lowering, so a `?`
    // inside `lhs`'s `is`-receiver (already hoisted unconditionally into the
    // caller's own statement position, not this arrow — see `lower_bin_op`)
    // doesn't falsely report a propagating return coming from `bindings`.
    let saved_early_return = std::mem::take(&mut cx.emitted_early_return);
    let rhs_lowered = lower_expr(rhs, cx);
    let rhs_returns = cx.emitted_early_return;
    cx.emitted_early_return = saved_early_return || rhs_returns;
    bindings.extend(rhs_lowered.pre);
    cx.shadow_scopes.pop();
    Some((bindings, lhs_lowered, rhs_lowered.expr, rhs_returns))
}

/// Walk an expression collecting `const name = expr.field;` strings for
/// any `is`-pattern bindings on the truthy path. `found` indicates whether
/// at least one `is` was seen.
fn gather_is_bindings_for_emit(
    e: &Expr,
    cx: &mut LowerCtx,
    out: &mut Vec<String>,
    found: &mut bool,
) {
    let tys = cx.commons().tys();
    match &e.kind {
        ExprKind::Is { value, pattern } => {
            *found = true;
            let value_text = cx.is_receiver_text(value);
            let disc_ty = cx.commons().expr_types.get(&value.id).map(|te| te.ty);
            if let Pattern::Variant {
                variant, bindings, ..
            } = pattern.as_ref()
            {
                // v0.13: refinement narrowing re-binds the value's name to the
                // branded refined type, read from the forced receiver temp.
                if bindings.is_empty()
                    && cx.is_refined_is_check(value, &variant.name)
                    && let ExprKind::Ident(id) = &value.kind
                {
                    out.push(format!(
                        "const {name} = {value_text} as {refined};",
                        name = ts_ident(&id.name),
                        refined = variant.name,
                    ));
                    cx.declare_binder(&id.name);
                    return;
                }
                for (i, b) in bindings.iter().enumerate() {
                    // `is` emits only flat, depth-1 name bindings (ADR 0169 keeps
                    // nesting/guards match-only); `_` and nested patterns bind nothing.
                    let Pattern::Binding(name) = b.pattern() else {
                        continue;
                    };
                    match &b.kind {
                        PatternBindingKind::Named { field, .. } => {
                            out.push(format!(
                                "const {name} = {value}.{field};",
                                name = ts_ident(&name.name),
                                value = value_text,
                                field = field.name
                            ));
                        }
                        PatternBindingKind::Positional { .. } => {
                            let field = cx.positional_field_name(disc_ty, &variant.name, i, tys);
                            out.push(format!(
                                "const {name} = {value}.{field};",
                                name = ts_ident(&name.name),
                                value = value_text,
                                field = field
                            ));
                        }
                    }
                    cx.declare_binder(&name.name);
                }
            }
            // #474 §2.3.6: an or-pattern's shared names can live at different
            // structural paths per alternative (`Held`'s field 2 vs
            // `Confirmed`'s field 4), so — like the `match` if-chain's
            // `emit_pattern_bindings` — declare each name once with `let`,
            // then dispatch per alternative. Still depth-1 only (the same
            // existing `is` limitation as the `Variant` case above): an
            // alternative that isn't itself a flat `Variant` contributes no
            // bindings, only its tag test.
            if let Pattern::Or(alts, _) = pattern.as_ref() {
                let names = pattern.bound_names();
                if names.is_empty() {
                    return;
                }
                let decl: Vec<String> = names.iter().map(|id| ts_ident(&id.name)).collect();
                out.push(format!("let {};", decl.join(", ")));
                for id in &names {
                    cx.declare_binder(&id.name);
                }
                let last = alts.len() - 1;
                for (i, alt) in alts.iter().enumerate() {
                    let (tag, pairs) = match alt {
                        Pattern::Variant {
                            variant, bindings, ..
                        } => {
                            let mut pairs = Vec::new();
                            for (j, b) in bindings.iter().enumerate() {
                                let Pattern::Binding(name) = b.pattern() else {
                                    continue;
                                };
                                let field = match &b.kind {
                                    PatternBindingKind::Named { field, .. } => field.name.clone(),
                                    PatternBindingKind::Positional { .. } => {
                                        cx.positional_field_name(disc_ty, &variant.name, j, tys)
                                    }
                                };
                                pairs.push((ts_ident(&name.name), format!("{value_text}.{field}")));
                            }
                            (Some(variant.name.clone()), pairs)
                        }
                        _ => (None, Vec::new()),
                    };
                    if i == last {
                        out.push("} else {".to_string());
                    } else {
                        let cond = tag
                            .as_ref()
                            .map(|t| format!("{value_text}.tag === \"{t}\""))
                            .unwrap_or_else(|| "true".to_string());
                        let kw = if i == 0 { "if" } else { "} else if" };
                        out.push(format!("{kw} ({cond}) {{"));
                    }
                    for (name, path) in &pairs {
                        out.push(format!("  {name} = {path};"));
                    }
                }
                out.push("}".to_string());
            }
        }
        ExprKind::BinOp(BinOp::And, l, r) => {
            gather_is_bindings_for_emit(l, cx, out, found);
            gather_is_bindings_for_emit(r, cx, out, found);
        }
        ExprKind::Paren(inner) => gather_is_bindings_for_emit(inner, cx, out, found),
        _ => {}
    }
}

/// True when an `is` receiver is a simple, side-effect-free, repeatable
/// lvalue — an identifier or a field-access chain ending at one (optionally
/// parenthesised). Such receivers can be referenced textually as many times
/// as there are pattern bindings without re-evaluation. Anything else (calls,
/// matches, arithmetic, …) must be lifted to a temp before use; see
/// `LowerCtx::is_receiver_ref`.
pub(crate) fn is_simple_is_receiver(value: &Expr) -> bool {
    match &value.kind {
        ExprKind::Ident(_) => true,
        ExprKind::FieldAccess { receiver, .. } => is_simple_is_receiver(receiver),
        ExprKind::Paren(inner) => is_simple_is_receiver(inner),
        _ => false,
    }
}

/// Render a *simple* `is` receiver (see `is_simple_is_receiver`) as a textual
/// reference for binding lookups. Complex receivers never reach this function
/// — they are lifted to a temp and resolved via the span cache in
/// `LowerCtx::is_receiver_text` — so the final arm is a defensive backstop the
/// `no_unknown_placeholder_in_emitted_output` test also guards against.
pub(crate) fn value_text_for_is(value: &Expr) -> String {
    match &value.kind {
        ExprKind::Ident(id) => ts_ident(&id.name),
        ExprKind::FieldAccess { receiver, field } => {
            format!("{}.{}", value_text_for_is(receiver), field.name)
        }
        ExprKind::Paren(inner) => value_text_for_is(inner),
        _ => "(/* TODO: complex is-receiver */ )".to_string(),
    }
}

/// v0.150 (ADR 0174): peel a `traverseTry`/`parTraverseTry` call's checked type
/// `Effect[Result[List[U], E]]` to the `U` TS — the short-circuit collect's
/// output-array element annotation. Falls back to `any` if the shape is absent.
fn list_ok_elem_ts(call_ty: Option<TyId>, tys: &Arc<Types>) -> String {
    match call_ty.map(|t| tys.get(t)).as_deref() {
        Some(Ty::Effect(inner)) => match &*tys.get(*inner) {
            Ty::Result(ok, _) => match &*tys.get(*ok) {
                Ty::List(u) => ts_ty(*u, tys),
                _ => ts_ty(*ok, tys),
            },
            _ => ts_ty(*inner, tys),
        },
        _ => "any".to_string(),
    }
}

/// v0.20b: lower a built-in `List` kernel method. Returns None for a method
/// name the kernel doesn't own (the checker has already rejected it; this
/// keeps the dispatch defensive). All forms are pure expressions; `foldEff`
/// returns a Promise that the surrounding `<-` bind awaits.
fn lower_list_kernel(
    e: &Expr,
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    elem: TyId,
    cx: &mut LowerCtx,
) -> Option<Lowered> {
    let tys = cx.commons().tys();
    let mut pre = Pre::new();
    let elem_ts = ts_ty(elem, tys);
    let text: Option<String> = match (method.name.as_str(), args) {
        ("length", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("({recv}).length"))
        }
        ("get", [index]) => {
            let recv = pre.lower(receiver, cx);
            let idx = pre.lower(index, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[], __i: number) => __i >= 0 && __i < __xs.length ? Some(__xs[__i] as {elem_ts}) : None)({recv}, {idx})"
            ))
        }
        ("prepend", [head]) => {
            let head = pre.lower(head, cx);
            let recv = pre.lower(receiver, cx);
            Some(format!("[{head}, ...{recv}]"))
        }
        ("fold", [init, f]) => {
            // The call's checked type is the accumulator type.
            let acc_ts = cx
                .commons()
                .expr_types
                .get(&e.id)
                .map(|te| ts_ty(te.ty, tys))
                .unwrap_or_else(|| {
                    panic!(
                        "bynk internal error (finding #28): emitter has no recorded type for a \
                     `fold` call at {:?} — the checker should already have typed it",
                        e.span
                    )
                });
            let recv = pre.lower(receiver, cx);
            let init = pre.lower(init, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[], __acc: {acc_ts}, __f: (acc: {acc_ts}, x: {elem_ts}) => {acc_ts}) => {{ for (const __x of __xs) __acc = __f(__acc, __x); return __acc; }})({recv}, {init}, {f})"
            ))
        }
        (FOLD_EFF, [init, f]) => {
            // The call's checked type is `Effect[Acc]` — peel for the TS
            // accumulator annotation.
            let acc_ts = match cx.commons().expr_ty(e.id).as_deref() {
                Some(Ty::Effect(acc)) => ts_ty(*acc, tys),
                Some(_) => ts_ty(cx.commons().expr_types[&e.id].ty, tys),
                None => panic!(
                    "bynk internal error (finding #28): emitter has no recorded type for a \
                     `foldEff` call at {:?} — the checker should already have typed it",
                    e.span
                ),
            };
            let recv = pre.lower(receiver, cx);
            let init = pre.lower(init, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "(async (__xs: readonly {elem_ts}[], __acc: {acc_ts}, __f: (acc: {acc_ts}, x: {elem_ts}) => Promise<{acc_ts}>) => {{ for (const __x of __xs) __acc = await __f(__acc, __x); return __acc; }})({recv}, {init}, {f})"
            ))
        }
        // v0.146 (ADR 0170): `forEach` — run an effectful step per element in
        // order, awaiting each; yields `Promise<void>`. The eager `List`
        // analogue of the `Query.forEach` terminal, emitted inline.
        (FOR_EACH, [f]) => {
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "(async (__xs: readonly {elem_ts}[]) => {{ for (const __x of __xs) {{ await ({f})(__x); }} }})({recv})"
            ))
        }
        // v0.147 (ADR 0171): `parTraverse` — issue the effectful fn over every
        // element concurrently and await them together, so one slow element does
        // not head-of-line-block the rest. The eager `List` analogue of the
        // `Query.parTraverse` terminal, emitted inline.
        (PAR_TRAVERSE, [f]) => {
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "(async (__xs: readonly {elem_ts}[]) => {{ await Promise.all(__xs.map((__x: {elem_ts}) => ({f})(__x))); }})({recv})"
            ))
        }
        // v0.148 (ADR 0172): the collect-all iterators — every element's
        // `Result` outcome is gathered (an `Err` is a value, so nothing rejects).
        // `traverseAll` awaits each in order into a typed array; `parTraverseAll`
        // issues all at once and `Promise.all`s the resolved `Result`s. Both
        // yield `Promise<Result<…>[]>`.
        (TRAVERSE_ALL, [f]) => {
            // The call's checked type is `Effect[List[Result[B, E]]]` — peel to
            // the `Result[B, E]` TS for the output-array annotation.
            let res_ts = match cx.commons().expr_ty(e.id).as_deref() {
                Some(Ty::Effect(inner)) => match &*tys.get(*inner) {
                    Ty::List(el) => ts_ty(*el, tys),
                    _ => ts_ty(*inner, tys),
                },
                other => panic!(
                    "bynk internal error (finding #28): emitter expected `Effect[List[_]]` \
                     recorded for a `traverseAll` call at {:?}, but found {other:?} — the \
                     checker should already have typed it",
                    e.span
                ),
            };
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "(async (__xs: readonly {elem_ts}[]) => {{ const __out: {res_ts}[] = []; for (const __x of __xs) {{ __out.push(await ({f})(__x)); }} return __out; }})({recv})"
            ))
        }
        (PAR_TRAVERSE_ALL, [f]) => {
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "(async (__xs: readonly {elem_ts}[]) => await Promise.all(__xs.map((__x: {elem_ts}) => ({f})(__x))))({recv})"
            ))
        }
        // v0.150 (ADR 0174): the short-circuit collect iterators — stop at the
        // first `Err` and return it (`Effect[Result[List[U], E]]`). `traverseTry`
        // awaits each in order, bailing on the first `Err`; `parTraverseTry`
        // issues all at once, then scans the resolved `Result`s in input order.
        (TRAVERSE_TRY, [f]) => {
            let u_ts = list_ok_elem_ts(cx.commons().expr_types.get(&e.id).map(|te| te.ty), tys);
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "(async (__xs: readonly {elem_ts}[]) => {{ const __out: {u_ts}[] = []; for (const __x of __xs) {{ const __r = await ({f})(__x); if (__r.tag === \"Err\") {{ return Err(__r.error); }} __out.push(__r.value); }} return Ok(__out); }})({recv})"
            ))
        }
        (PAR_TRAVERSE_TRY, [f]) => {
            let u_ts = list_ok_elem_ts(cx.commons().expr_types.get(&e.id).map(|te| te.ty), tys);
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "(async (__xs: readonly {elem_ts}[]) => {{ const __rs = await Promise.all(__xs.map((__x: {elem_ts}) => ({f})(__x))); const __out: {u_ts}[] = []; for (const __r of __rs) {{ if (__r.tag === \"Err\") {{ return Err(__r.error); }} __out.push(__r.value); }} return Ok(__out); }})({recv})"
            ))
        }
        // v0.88 (ADR 0116): the eager builder/terminal vocabulary. Most lower
        // to native array methods; callbacks are wrapped in a single-arg arrow
        // so the array index/array extra args never reach a Bynk one-param fn.
        ("map", [f]) => {
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!("({recv}).map((__x: {elem_ts}) => ({f})(__x))"))
        }
        ("filter", [p]) => {
            let recv = pre.lower(receiver, cx);
            let p = pre.lower(p, cx);
            Some(format!("({recv}).filter((__x: {elem_ts}) => ({p})(__x))"))
        }
        ("flatMap", [f]) => {
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!("({recv}).flatMap((__x: {elem_ts}) => ({f})(__x))"))
        }
        ("take", [n]) => {
            let recv = pre.lower(receiver, cx);
            let n = pre.lower(n, cx);
            Some(format!("({recv}).slice(0, Math.max(0, {n}))"))
        }
        ("skip", [n]) => {
            let recv = pre.lower(receiver, cx);
            let n = pre.lower(n, cx);
            Some(format!("({recv}).slice(Math.max(0, {n}))"))
        }
        ("count", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("({recv}).length"))
        }
        ("any", [p]) => {
            let recv = pre.lower(receiver, cx);
            let p = pre.lower(p, cx);
            Some(format!("({recv}).some((__x: {elem_ts}) => ({p})(__x))"))
        }
        ("all", [p]) => {
            let recv = pre.lower(receiver, cx);
            let p = pre.lower(p, cx);
            Some(format!("({recv}).every((__x: {elem_ts}) => ({p})(__x))"))
        }
        // v0.119 (ADR 0155, DECISION C-a): `run.upTo(step)` — the driven history
        // strictly before `step`. Steps are distinct object instances in the run
        // array, so `indexOf` is reference identity. An IIFE avoids re-evaluating
        // the receiver.
        ("upTo", [step]) => {
            let recv = pre.lower(receiver, cx);
            let step = pre.lower(step, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[], __s: {elem_ts}) => __xs.slice(0, __xs.indexOf(__s)))({recv}, {step})"
            ))
        }
        ("first", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[]) => __xs.length > 0 ? Some(__xs[0]) : None)({recv})"
            ))
        }
        ("firstOrElse", [default]) => {
            let recv = pre.lower(receiver, cx);
            let default = pre.lower(default, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[], __d: {elem_ts}) => __xs.length > 0 ? __xs[0] : __d)({recv}, {default})"
            ))
        }
        // v0.88 (ADR 0116 D2/D3/D4): ordering + aggregates. The comparator
        // `<`/`>` works for the numeric- and string-erased orderable keys
        // alike, so no key-type branch is needed (except average's rounding).
        ("sortBy", [key]) => {
            let recv = pre.lower(receiver, cx);
            let key = pre.lower(key, cx);
            Some(format!(
                "[...{recv}].sort((__a: {elem_ts}, __b: {elem_ts}) => {{ const __ka = ({key})(__a), __kb = ({key})(__b); return __ka < __kb ? -1 : __ka > __kb ? 1 : 0; }})"
            ))
        }
        ("distinct", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("[...new Set({recv})]"))
        }
        ("distinctBy", [key]) => {
            let recv = pre.lower(receiver, cx);
            let key = pre.lower(key, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[]) => {{ const __seen = new Set(); const __out: {elem_ts}[] = []; for (const __x of __xs) {{ const __k = ({key})(__x); if (!__seen.has(__k)) {{ __seen.add(__k); __out.push(__x); }} }} return __out; }})({recv})"
            ))
        }
        ("sum", [key]) => {
            let recv = pre.lower(receiver, cx);
            let key = pre.lower(key, cx);
            Some(format!(
                "({recv}).reduce((__s: number, __x: {elem_ts}) => __s + ({key})(__x), 0)"
            ))
        }
        ("min" | "max", [key]) => {
            let cmp = if method.name == "min" { "<" } else { ">" };
            let recv = pre.lower(receiver, cx);
            let key = pre.lower(key, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[]) => {{ if (__xs.length === 0) return None; let __m = ({key})(__xs[0]); for (const __x of __xs) {{ const __k = ({key})(__x); if (__k {cmp} __m) __m = __k; }} return Some(__m); }})({recv})"
            ))
        }
        ("average", [key]) => {
            // D3: Duration averages round to integer millis; Int/Float -> Float.
            let round = matches!(
                cx.commons().expr_ty(e.id).as_deref(),
                Some(Ty::Option(inner)) if matches!(&*tys.get(*inner), Ty::Base(BaseType::Duration))
            );
            let mean = if round {
                "Math.round(__s / __xs.length)"
            } else {
                "__s / __xs.length"
            };
            let recv = pre.lower(receiver, cx);
            let key = pre.lower(key, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[]) => {{ if (__xs.length === 0) return None; let __s = 0; for (const __x of __xs) __s += ({key})(__x); return Some({mean}); }})({recv})"
            ))
        }
        // v0.94 (ADR 0116/0120): joins & grouping. Hash on a stringified key
        // (value-keyable, like the `@indexed` posting list), probe, and project
        // each result through `into` — there is no pair value. `joinOn`/`leftJoin`
        // build the hash from `other`'s key; `join` is a nested-loop predicate;
        // `groupBy` partitions in first-seen key order. Group/`into` receive the
        // **original** key (re-derived from a representative row), not the
        // stringified hash key.
        ("joinOn", [other, left, right, into]) => {
            let recv = pre.lower(receiver, cx);
            let u_ts = join_other_elem_ts(args, cx);
            let other = pre.lower(other, cx);
            let left = pre.lower(left, cx);
            let right = pre.lower(right, cx);
            let into = pre.lower(into, cx);
            Some(format!(
                "(() => {{ const __h: Record<string, {u_ts}[]> = {{}}; for (const __u of {other}) {{ const __k = String(({right})(__u)); (__h[__k] = __h[__k] ?? []).push(__u); }} return ({recv}).flatMap((__t: {elem_ts}) => {{ const __m = __h[String(({left})(__t))] ?? []; return __m.map((__u: {u_ts}) => ({into})(__t, __u)); }}); }})()"
            ))
        }
        ("leftJoin", [other, left, right, into]) => {
            let recv = pre.lower(receiver, cx);
            let u_ts = join_other_elem_ts(args, cx);
            let other = pre.lower(other, cx);
            let left = pre.lower(left, cx);
            let right = pre.lower(right, cx);
            let into = pre.lower(into, cx);
            Some(format!(
                "(() => {{ const __h: Record<string, {u_ts}[]> = {{}}; for (const __u of {other}) {{ const __k = String(({right})(__u)); (__h[__k] = __h[__k] ?? []).push(__u); }} return ({recv}).flatMap((__t: {elem_ts}) => {{ const __m = __h[String(({left})(__t))] ?? []; return __m.length > 0 ? __m.map((__u: {u_ts}) => ({into})(__t, Some(__u))) : [({into})(__t, None)]; }}); }})()"
            ))
        }
        ("join", [other, on, into]) => {
            let recv = pre.lower(receiver, cx);
            let u_ts = join_other_elem_ts(args, cx);
            let other = pre.lower(other, cx);
            let on = pre.lower(on, cx);
            let into = pre.lower(into, cx);
            Some(format!(
                "(() => {{ const __b: readonly {u_ts}[] = {other}; return ({recv}).flatMap((__t: {elem_ts}) => __b.filter((__u: {u_ts}) => ({on})(__t, __u)).map((__u: {u_ts}) => ({into})(__t, __u))); }})()"
            ))
        }
        ("groupBy", [key, into]) => {
            let recv = pre.lower(receiver, cx);
            let key = pre.lower(key, cx);
            let into = pre.lower(into, cx);
            Some(format!(
                "(() => {{ const __h: Record<string, {elem_ts}[]> = {{}}; const __order: string[] = []; for (const __t of {recv}) {{ const __k = String(({key})(__t)); if (!(__k in __h)) {{ __h[__k] = []; __order.push(__k); }} __h[__k].push(__t); }} return __order.map((__k) => {{ const __rows = __h[__k]; return ({into})(({key})(__rows[0]), __rows); }}); }})()"
            ))
        }
        _ => None,
    };
    text.map(|expr| pre.finish(expr))
}

/// v0.94 (ADR 0120): the TS element type of a join's `other` collection (its
/// `List`/`Query` element), for typing the hash-map buckets. Falls back to
/// `unknown` if the checker recorded no type (an already-diagnosed program).
fn join_other_elem_ts(args: &[Expr], cx: &LowerCtx) -> String {
    let tys = cx.commons().tys();
    match cx.commons().expr_ty(args[0].id).as_deref() {
        Some(Ty::List(u) | Ty::Query(u)) => ts_ty(*u, tys),
        _ => "unknown".to_string(),
    }
}

/// v0.100 (real-time track slice 0): lower a `Stream[T]` op over a `source`
/// async-iterable expression. **Builders** (`map`/`take`) wrap the source in a
/// new async generator — still lazy, still an `AsyncIterable`. The **terminal**
/// `collect` drains the source into an array (an `Effect`, awaited with `<-`).
/// Emitted inline, like the collection kernels, so non-stream files are
/// untouched. Callbacks are wrapped in a single-arg arrow so no extra iterator
/// argument reaches a one-param Bynk fn.
fn lower_stream_method(source: String, method: &Ident, a: &[String]) -> Option<String> {
    match (method.name.as_str(), a) {
        // -- builders (return an AsyncIterable) --
        ("map", [f]) => Some(format!(
            "(async function* (__s) {{ for await (const __e of __s) {{ yield ({f})(__e); }} }})({source})"
        )),
        ("take", [n]) => Some(format!(
            "(async function* (__s) {{ const __n = {n}; if (__n <= 0) {{ return; }} let __i = 0; for await (const __e of __s) {{ yield __e; if (++__i >= __n) {{ return; }} }} }})({source})"
        )),
        // -- terminal (drains to a list; Effect-typed) --
        ("collect", []) => Some(format!(
            "(async (__s) => {{ const __r = []; for await (const __e of __s) {{ __r.push(__e); }} return __r; }})({source})"
        )),
        _ => None,
    }
}

/// v0.91 (ADR 0119, query-algebra slice 2): lower a lazy storage-query op over a
/// `source` array expression. **Builders** wrap the source in a deferred thunk
/// `() => …[]` (so the terminal reads *staged* state when it runs —
/// read-your-writes, ADR 0109/0119 D7). **Terminals** read the source array and
/// produce the result; they are `Effect`-typed (awaited with `<-`), but the
/// staged map is in memory, so a synchronous expression suffices (`await` on a
/// non-promise is identity) — except `forEach`, which awaits its effectful fn.
/// The element type is inferred from the typed source (`Record`'s values), so no
/// `__x` annotations are needed. Callbacks are wrapped in a single-arg arrow so
/// the array index never reaches a one-param Bynk fn.
fn lower_query_method(
    source: String,
    method: &Ident,
    a: &[String],
    result_ty: Option<TyId>,
    tys: &Arc<Types>,
) -> Option<String> {
    let thunk = |body: String| format!("(() => {body})");
    Some(match (method.name.as_str(), a) {
        // -- builders → a deferred thunk over the narrowed source --
        ("filter", [p]) => thunk(format!("{source}.filter((__x) => ({p})(__x))")),
        ("map", [f]) => thunk(format!("{source}.map((__x) => ({f})(__x))")),
        // storage flatMap: the fn returns a `Query` (a thunk) — invoke each.
        ("flatMap", [f]) => thunk(format!("{source}.flatMap((__x) => ({f})(__x)())")),
        ("sortBy", [key]) => thunk(format!(
            "[...{source}].sort((__a, __b) => {{ const __ka = ({key})(__a), __kb = ({key})(__b); return __ka < __kb ? -1 : __ka > __kb ? 1 : 0; }})"
        )),
        ("take", [n]) => thunk(format!("{source}.slice(0, Math.max(0, {n}))")),
        ("skip", [n]) => thunk(format!("{source}.slice(Math.max(0, {n}))")),
        ("distinct", []) => thunk(format!("[...new Set({source})]")),
        ("distinctBy", [key]) => thunk(format!(
            "(() => {{ const __seen = new Set(); const __out: any[] = []; for (const __x of {source}) {{ const __k = ({key})(__x); if (!__seen.has(__k)) {{ __seen.add(__k); __out.push(__x); }} }} return __out; }})()"
        )),
        // v0.94 (ADR 0116/0120): joins & grouping over storage queries — lazy
        // builders. `other` is itself a `Query` thunk, invoked to materialise the
        // probed side; the result projects through `into` (no pair value). The
        // hash key is stringified (value-keyable); `groupBy`/`into` get the
        // original key, re-derived from a representative row.
        ("joinOn", [other, left, right, into]) => thunk(format!(
            "{{ const __h: Record<string, any[]> = {{}}; for (const __u of ({other})()) {{ const __k = String(({right})(__u)); (__h[__k] = __h[__k] ?? []).push(__u); }} return {source}.flatMap((__t) => {{ const __m = __h[String(({left})(__t))] ?? []; return __m.map((__u) => ({into})(__t, __u)); }}); }}"
        )),
        ("leftJoin", [other, left, right, into]) => thunk(format!(
            "{{ const __h: Record<string, any[]> = {{}}; for (const __u of ({other})()) {{ const __k = String(({right})(__u)); (__h[__k] = __h[__k] ?? []).push(__u); }} return {source}.flatMap((__t) => {{ const __m = __h[String(({left})(__t))] ?? []; return __m.length > 0 ? __m.map((__u) => ({into})(__t, Some(__u))) : [({into})(__t, None)]; }}); }}"
        )),
        ("join", [other, on, into]) => thunk(format!(
            "{{ const __b = ({other})(); return {source}.flatMap((__t) => __b.filter((__u) => ({on})(__t, __u)).map((__u) => ({into})(__t, __u))); }}"
        )),
        ("groupBy", [key, into]) => thunk(format!(
            "{{ const __h: Record<string, any[]> = {{}}; const __order: string[] = []; for (const __t of {source}) {{ const __k = String(({key})(__t)); if (!(__k in __h)) {{ __h[__k] = []; __order.push(__k); }} __h[__k].push(__t); }} return __order.map((__k) => {{ const __rows = __h[__k]; return ({into})(({key})(__rows[0]), __rows); }}); }}"
        )),
        // -- terminals → read the source array (awaited at the `<-`) --
        ("collect", []) => source,
        ("first", []) => format!(
            "(() => {{ const __a = {source}; return __a.length > 0 ? Some(__a[0]) : None; }})()"
        ),
        ("firstOrElse", [default]) => format!(
            "(() => {{ const __a = {source}; return __a.length > 0 ? __a[0] : ({default}); }})()"
        ),
        ("count", []) => format!("{source}.length"),
        ("fold", [init, f]) => format!(
            "(() => {{ let __acc = {init}; for (const __x of {source}) __acc = ({f})(__acc, __x); return __acc; }})()"
        ),
        ("any", [p]) => format!("{source}.some((__x) => ({p})(__x))"),
        ("all", [p]) => format!("{source}.every((__x) => ({p})(__x))"),
        ("sum", [key]) => format!("{source}.reduce((__s: number, __x) => __s + ({key})(__x), 0)"),
        ("min" | "max", [key]) => {
            let cmp = if method.name == "min" { "<" } else { ">" };
            format!(
                "(() => {{ const __a = {source}; if (__a.length === 0) return None; let __m = ({key})(__a[0]); for (const __x of __a) {{ const __k = ({key})(__x); if (__k {cmp} __m) __m = __k; }} return Some(__m); }})()"
            )
        }
        ("average", [key]) => {
            // Duration averages round to integer millis (checker result decides).
            let round = matches!(
                result_ty.map(|t| tys.get(t)).as_deref(),
                Some(Ty::Effect(inner))
                    if matches!(&*tys.get(*inner), Ty::Option(o)
                        if matches!(&*tys.get(*o), Ty::Base(BaseType::Duration)))
            );
            let mean = if round {
                "Math.round(__s / __a.length)"
            } else {
                "__s / __a.length"
            };
            format!(
                "(() => {{ const __a = {source}; if (__a.length === 0) return None; let __s = 0; for (const __x of __a) __s += ({key})(__x); return Some({mean}); }})()"
            )
        }
        ("forEach", [f]) => {
            format!("(async () => {{ for (const __x of {source}) {{ await ({f})(__x); }} }})()")
        }
        // v0.107 (slice 4): the parallel broadcast form — issue the effectful fn over
        // every element concurrently and await them together, so one slow element
        // does not head-of-line-block the rest.
        ("parTraverse", [f]) => {
            format!("(async () => {{ await Promise.all({source}.map((__x) => ({f})(__x))); }})()")
        }
        // v0.149 (ADR 0173): the collect-all terminals over a query/broadcast —
        // gather every `Result` outcome (an `Err` is a value, so nothing rejects).
        ("traverseAll", [f]) => {
            let res_ts = match result_ty.map(|t| tys.get(t)).as_deref() {
                Some(Ty::Effect(inner)) => match &*tys.get(*inner) {
                    Ty::List(el) => ts_ty(*el, tys),
                    _ => ts_ty(*inner, tys),
                },
                _ => "any".to_string(),
            };
            format!(
                "(async () => {{ const __out: {res_ts}[] = []; for (const __x of {source}) {{ __out.push(await ({f})(__x)); }} return __out; }})()"
            )
        }
        ("parTraverseAll", [f]) => {
            format!("(async () => await Promise.all({source}.map((__x) => ({f})(__x))))()")
        }
        // v0.150 (ADR 0174): the short-circuit collect terminals — stop at the
        // first `Err`, returning `Result[List[U], E]`.
        ("traverseTry", [f]) => {
            let u_ts = list_ok_elem_ts(result_ty, tys);
            format!(
                "(async () => {{ const __out: {u_ts}[] = []; for (const __x of {source}) {{ const __r = await ({f})(__x); if (__r.tag === \"Err\") {{ return Err(__r.error); }} __out.push(__r.value); }} return Ok(__out); }})()"
            )
        }
        ("parTraverseTry", [f]) => {
            let u_ts = list_ok_elem_ts(result_ty, tys);
            format!(
                "(async () => {{ const __rs = await Promise.all({source}.map((__x) => ({f})(__x))); const __out: {u_ts}[] = []; for (const __r of __rs) {{ if (__r.tag === \"Err\") {{ return Err(__r.error); }} __out.push(__r.value); }} return Ok(__out); }})()"
            )
        }
        _ => return None,
    })
}

// ---- v0.93 (ADR 0118): `@indexed` secondary-index emission ----------------
//
// For a `store Map[K, V] @indexed(by: f)` field `m`, a sibling state record
// `m__idx_f: Record<string, string[]>` maps a (stringified) field value to the
// primary keys whose value carries it. The mutators maintain it inside the same
// staged commit; an equality `filter` on `f` reads it instead of scanning.

/// Fragment that *removes* `pk` from the posting-list of every indexed field of
/// the value bound to `val_local` (used before overwriting/deleting an entry).
fn idx_unindex(var: &str, map: &str, fields: &[String], val_local: &str, pk: &str) -> String {
    fields
        .iter()
        .map(|f| {
            let idx = format!("{var}.{map}__idx_{f}");
            format!(
                "{{ const __ik = String(({val_local}).{f}); const __ia = {idx}[__ik]; if (__ia) {{ const __ii = __ia.indexOf({pk}); if (__ii >= 0) __ia.splice(__ii, 1); if (__ia.length === 0) delete {idx}[__ik]; }} }}"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Fragment that *adds* `pk` to the posting-list of every indexed field of the
/// value bound to `val_local` (used after writing an entry).
fn idx_reindex(var: &str, map: &str, fields: &[String], val_local: &str, pk: &str) -> String {
    fields
        .iter()
        .map(|f| {
            let idx = format!("{var}.{map}__idx_{f}");
            format!(
                "{{ const __ik = String(({val_local}).{f}); ({idx}[__ik] = {idx}[__ik] ?? []).push({pk}); }}"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `put` with index maintenance: drop the prior value's postings (if any), write,
/// then post the new value. Last-write-wins re-indexing.
fn idx_map_put(m: &str, var: &str, map: &str, fields: &[String], a: &[String]) -> String {
    let un = idx_unindex(var, map, fields, "__o", "__k");
    let re = idx_reindex(var, map, fields, "__v", "__k");
    format!(
        "(() => {{ const __k = String({k}); const __v = {v}; const __o = {m}[__k]; if (__o !== undefined) {{ {un} }} {m}[__k] = __v; {re} return undefined; }})()",
        k = a[0],
        v = a[1],
    )
}

/// `remove` with index maintenance: drop the value's postings, then delete.
fn idx_map_remove(m: &str, var: &str, map: &str, fields: &[String], a: &[String]) -> String {
    let un = idx_unindex(var, map, fields, "__o", "__k");
    format!(
        "(() => {{ const __k = String({k}); const __o = {m}[__k]; if (__o !== undefined) {{ {un} delete {m}[__k]; }} return undefined; }})()",
        k = a[0],
    )
}

/// `update` with index maintenance: the key must exist (else a fault); re-index
/// from old value to new.
fn idx_map_update(m: &str, var: &str, map: &str, fields: &[String], a: &[String]) -> String {
    let un = idx_unindex(var, map, fields, "__o", "__k");
    let re = idx_reindex(var, map, fields, "__v", "__k");
    format!(
        "(() => {{ const __k = String({k}); if (!(__k in {m})) {{ throw new Error(\"Map.update: key absent\"); }} const __o = {m}[__k]; {un} const __v = ({f})(__o); {m}[__k] = __v; {re} return undefined; }})()",
        k = a[0],
        f = a[1],
    )
}

/// `upsert` with index maintenance: re-index from the prior value (if present)
/// to the computed new value.
fn idx_map_upsert(m: &str, var: &str, map: &str, fields: &[String], a: &[String]) -> String {
    let un = idx_unindex(var, map, fields, "__o", "__k");
    let re = idx_reindex(var, map, fields, "__v", "__k");
    format!(
        "(() => {{ const __k = String({k}); const __e = __k in {m}; const __o = __e ? {m}[__k] : undefined; if (__e) {{ {un} }} const __v = ({f})(__e ? __o : ({d})); {m}[__k] = __v; {re} return undefined; }})()",
        k = a[0],
        d = a[1],
        f = a[2],
    )
}

/// If `lam` is `(p) => p.<field> == <value>` (either order) where `<field>` is
/// indexed and `<value>` does not mention `p`, lower it to a posting-list lookup
/// thunk `() => idx[v].map(pk => map[pk])`. Otherwise `None` (fall back to scan).
fn route_indexed_filter(
    m: &str,
    var: &str,
    map: &str,
    fields: &[String],
    lam: &LambdaExpr,
    cx: &mut LowerCtx,
) -> Option<Lowered> {
    let mut pre = Pre::new();
    if fields.is_empty() {
        return None;
    }
    let [param] = lam.params.as_slice() else {
        return None;
    };
    let pname = param.name.name.as_str();
    let ExprKind::BinOp(BinOp::Eq, lhs, rhs) = &lam.body.kind else {
        return None;
    };
    // One side must be `p.<field>`; the other is the (param-independent) value.
    let (field, value) = match as_param_field(pname, lhs) {
        Some(f) => (f, rhs.as_ref()),
        None => (as_param_field(pname, rhs)?, lhs.as_ref()),
    };
    if !fields.iter().any(|f| f == field) || !param_independent(value, pname) {
        return None;
    }
    let v = pre.lower(value, cx);
    let text: Option<String> = Some(format!(
        "(() => ({var}.{map}__idx_{field}[String({v})] ?? []).map((__pk) => {m}[__pk]))"
    ));
    text.map(|expr| pre.finish(expr))
}

/// `e` as `<param>.<field>` → the field name; else `None`.
fn as_param_field<'e>(pname: &str, e: &'e Expr) -> Option<&'e str> {
    if let ExprKind::FieldAccess { receiver, field } = &e.kind
        && let ExprKind::Ident(r) = &receiver.kind
        && r.name == pname
    {
        Some(field.name.as_str())
    } else {
        None
    }
}

/// Whether `e` provably does not reference the lambda parameter `pname` — only
/// then is it safe to hoist it out of the per-row predicate into one lookup key.
/// Conservative: unrecognised shapes return `false` (no routing).
fn param_independent(e: &Expr, pname: &str) -> bool {
    match &e.kind {
        ExprKind::IntLit { .. }
        | ExprKind::FloatLit { .. }
        | ExprKind::StrLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::DurationLit { .. } => true,
        ExprKind::Ident(id) => id.name != pname,
        ExprKind::FieldAccess { receiver, .. } => param_independent(receiver, pname),
        _ => false,
    }
}

/// v0.21/v0.22a: lower a built-in numeric kernel method. `toFloat` is the
/// identity at runtime (the Int/Float distinction is erased); everything
/// else maps onto `Math.*` / `Number.*`.
fn lower_numeric_kernel(
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    cx: &mut LowerCtx,
) -> Option<Lowered> {
    let mut pre = Pre::new();
    let text: Option<String> = match (method.name.as_str(), args) {
        ("toFloat", []) => Some(pre.lower(receiver, cx)),
        ("round" | "floor" | "ceil" | "abs", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("Math.{}({recv})", method.name))
        }
        ("truncate", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("Math.trunc({recv})"))
        }
        ("min" | "max", [other]) => {
            let recv = pre.lower(receiver, cx);
            let other = pre.lower(other, cx);
            Some(format!("Math.{}({recv}, {other})", method.name))
        }
        ("clamp", [lo, hi]) => {
            let recv = pre.lower(receiver, cx);
            let lo = pre.lower(lo, cx);
            let hi = pre.lower(hi, cx);
            Some(format!("Math.min(Math.max({recv}, {lo}), {hi})"))
        }
        ("isNaN" | "isFinite", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("Number.{}({recv})", method.name))
        }
        // v0.42 (ADR 0074): host number→string — `String(n)` is ECMAScript's
        // Number::toString (shortest round-trip; `1e21`/`Infinity`/`NaN` as the
        // host renders them). The normative contract is the platform's.
        ("toString", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("String({recv})"))
        }
        _ => None,
    };
    text.map(|expr| pre.finish(expr))
}

/// v0.86 (ADR 0112): lower a `Duration` kernel method. `toMillis` is the
/// identity (a `Duration` lowers to its milliseconds); `toString` renders it.
fn lower_duration_kernel(
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    cx: &mut LowerCtx,
) -> Option<Lowered> {
    let mut pre = Pre::new();
    let text: Option<String> = match (method.name.as_str(), args) {
        ("toMillis", []) => Some(pre.lower(receiver, cx)),
        ("toString", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("String({recv})"))
        }
        _ => None,
    };
    text.map(|expr| pre.finish(expr))
}

/// v0.90 (ADR 0114): lower an `Instant` kernel method. `toEpochMillis` is the
/// identity (an `Instant` lowers to its epoch milliseconds); `toString` renders
/// it.
fn lower_instant_kernel(
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    cx: &mut LowerCtx,
) -> Option<Lowered> {
    let mut pre = Pre::new();
    let text: Option<String> = match (method.name.as_str(), args) {
        ("toEpochMillis", []) => Some(pre.lower(receiver, cx)),
        ("toString", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("String({recv})"))
        }
        _ => None,
    };
    text.map(|expr| pre.finish(expr))
}

/// v0.110 (ADR 0142 D3/D4): lower a `Bytes` kernel method. `length` is the
/// `Uint8Array.length` (octet count, not any string length); `toBase64` is a
/// total encode; `decodeUtf8` is a guarded fatal decode returning `Option`
/// (`None` on an invalid UTF-8 sequence).
fn lower_bytes_kernel(
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    cx: &mut LowerCtx,
) -> Option<Lowered> {
    let mut pre = Pre::new();
    let text: Option<String> = match (method.name.as_str(), args) {
        ("length", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("({recv}).length"))
        }
        ("toBase64", []) => {
            let recv = pre.lower(receiver, cx);
            cx.note_bytes();
            Some(format!("__bynkBytesToBase64({recv})"))
        }
        ("decodeUtf8", []) => {
            let recv = pre.lower(receiver, cx);
            cx.note_bytes();
            Some(format!("__bynkBytesDecodeUtf8({recv})"))
        }
        _ => None,
    };
    text.map(|expr| pre.finish(expr))
}

/// v0.22a: lower a built-in `String` kernel method (ADR 0046). Pinned
/// semantics: `replace` is replace-**all** (`replaceAll`); `chars()` is
/// code **points** (`[...s]`), not code units; `slice` clamps negative
/// indices to `0` (no TS wrap-around); `indexOf` turns `-1` into `None`.
fn lower_string_kernel(
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    cx: &mut LowerCtx,
) -> Option<Lowered> {
    let mut pre = Pre::new();
    let text: Option<String> = match (method.name.as_str(), args) {
        ("length", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("({recv}).length"))
        }
        ("trim", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("{recv}.trim()"))
        }
        ("toUpper", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("{recv}.toUpperCase()"))
        }
        ("toLower", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("{recv}.toLowerCase()"))
        }
        ("chars", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("[...{recv}]"))
        }
        ("split", [sep]) => {
            let recv = pre.lower(receiver, cx);
            let sep = pre.lower(sep, cx);
            Some(format!("{recv}.split({sep})"))
        }
        ("contains", [sub]) => {
            let recv = pre.lower(receiver, cx);
            let sub = pre.lower(sub, cx);
            Some(format!("{recv}.includes({sub})"))
        }
        ("startsWith" | "endsWith", [sub]) => {
            let recv = pre.lower(receiver, cx);
            let sub = pre.lower(sub, cx);
            Some(format!("{recv}.{}({sub})", method.name))
        }
        ("concat", [other]) => {
            let recv = pre.lower(receiver, cx);
            let other = pre.lower(other, cx);
            Some(format!("{recv}.concat({other})"))
        }
        ("replace", [from, to]) => {
            let recv = pre.lower(receiver, cx);
            let from = pre.lower(from, cx);
            let to = pre.lower(to, cx);
            Some(format!("{recv}.replaceAll({from}, {to})"))
        }
        ("slice", [lo, hi]) => {
            let recv = pre.lower(receiver, cx);
            let lo = pre.lower(lo, cx);
            let hi = pre.lower(hi, cx);
            Some(format!(
                "{recv}.slice(Math.max(0, {lo}), Math.max(0, {hi}))"
            ))
        }
        ("indexOf", [sub]) => {
            let recv = pre.lower(receiver, cx);
            let sub = pre.lower(sub, cx);
            Some(format!(
                "((__i: number) => __i < 0 ? None : Some(__i))({recv}.indexOf({sub}))"
            ))
        }
        _ => None,
    };
    text.map(|expr| pre.finish(expr))
}

/// v0.22a: lower a built-in `Option[T]` kernel method (ADR 0048). Typed
/// IIFEs in the v0.20b posture — no runtime imports beyond the
/// `Some`/`None`/`Ok`/`Err` constructors every module already has.
#[allow(clippy::too_many_arguments)]
fn lower_option_kernel(
    e: &Expr,
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    inner: TyId,
    cx: &mut LowerCtx,
) -> Option<Lowered> {
    let tys = cx.commons().tys();
    let mut pre = Pre::new();
    let t = ts_ty(inner, tys);
    let text: Option<String> = match (method.name.as_str(), args) {
        ("map", [f]) => {
            // The call's checked type is `Option[B]` — peel for B.
            let b = match cx.commons().expr_ty(e.id).as_deref() {
                Some(Ty::Option(b)) => ts_ty(*b, tys),
                other => panic!(
                    "bynk internal error (finding #28): emitter expected `Option[_]` recorded \
                     for an `Option.map` call at {:?}, but found {other:?} — the checker \
                     should already have typed it",
                    e.span
                ),
            };
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "((__o: Option<{t}>, __f: (x: {t}) => {b}) => __o.tag === \"Some\" ? Some(__f(__o.value)) : None)({recv}, {f})"
            ))
        }
        ("andThen", [f]) => {
            let b = match cx.commons().expr_ty(e.id).as_deref() {
                Some(Ty::Option(b)) => ts_ty(*b, tys),
                other => panic!(
                    "bynk internal error (finding #28): emitter expected `Option[_]` recorded \
                     for an `Option.andThen` call at {:?}, but found {other:?} — the checker \
                     should already have typed it",
                    e.span
                ),
            };
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "((__o: Option<{t}>, __f: (x: {t}) => Option<{b}>) => __o.tag === \"Some\" ? __f(__o.value) : None)({recv}, {f})"
            ))
        }
        ("getOrElse", [fallback]) => {
            let recv = pre.lower(receiver, cx);
            let fallback = pre.lower(fallback, cx);
            Some(format!(
                "((__o: Option<{t}>, __d: {t}) => __o.tag === \"Some\" ? __o.value : __d)({recv}, {fallback})"
            ))
        }
        ("isSome", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("({recv}.tag === \"Some\")"))
        }
        ("okOr", [error]) => {
            // The call's checked type is `Result[T, E]` — peel for E.
            let err = match cx.commons().expr_ty(e.id).as_deref() {
                Some(Ty::Result(_, err)) => ts_ty(*err, tys),
                other => panic!(
                    "bynk internal error (finding #28): emitter expected `Result[_, _]` \
                     recorded for an `Option.okOr` call at {:?}, but found {other:?} — the \
                     checker should already have typed it",
                    e.span
                ),
            };
            let recv = pre.lower(receiver, cx);
            let error = pre.lower(error, cx);
            Some(format!(
                "((__o: Option<{t}>, __e: {err}) => __o.tag === \"Some\" ? Ok(__o.value) : Err(__e))({recv}, {error})"
            ))
        }
        _ => None,
    };
    text.map(|expr| pre.finish(expr))
}

/// v0.22a: lower a built-in `Result[T, E]` kernel method (ADR 0048). The
/// miss branches return the narrowed receiver — TS's discriminated-union
/// narrowing makes the `Err` arm assignable to `Result<B, E>` directly.
#[allow(clippy::too_many_arguments)]
fn lower_result_kernel(
    e: &Expr,
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    ok: TyId,
    err: TyId,
    cx: &mut LowerCtx,
) -> Option<Lowered> {
    let tys = cx.commons().tys();
    let mut pre = Pre::new();
    let t = ts_ty(ok, tys);
    let et = ts_ty(err, tys);
    let text: Option<String> = match (method.name.as_str(), args) {
        ("map", [f]) => {
            // The call's checked type is `Result[B, E]` — peel for B.
            let b = match cx.commons().expr_ty(e.id).as_deref() {
                Some(Ty::Result(b, _)) => ts_ty(*b, tys),
                other => panic!(
                    "bynk internal error (finding #28): emitter expected `Result[_, _]` \
                     recorded for a `Result.map` call at {:?}, but found {other:?} — the \
                     checker should already have typed it",
                    e.span
                ),
            };
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "((__r: Result<{t}, {et}>, __f: (x: {t}) => {b}) => __r.tag === \"Ok\" ? Ok(__f(__r.value)) : __r)({recv}, {f})"
            ))
        }
        ("andThen", [f]) => {
            let b = match cx.commons().expr_ty(e.id).as_deref() {
                Some(Ty::Result(b, _)) => ts_ty(*b, tys),
                other => panic!(
                    "bynk internal error (finding #28): emitter expected `Result[_, _]` \
                     recorded for a `Result.andThen` call at {:?}, but found {other:?} — the \
                     checker should already have typed it",
                    e.span
                ),
            };
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "((__r: Result<{t}, {et}>, __f: (x: {t}) => Result<{b}, {et}>) => __r.tag === \"Ok\" ? __f(__r.value) : __r)({recv}, {f})"
            ))
        }
        ("mapErr", [f]) => {
            // The call's checked type is `Result[T, F]` — peel for F.
            let fts = match cx.commons().expr_ty(e.id).as_deref() {
                Some(Ty::Result(_, f)) => ts_ty(*f, tys),
                other => panic!(
                    "bynk internal error (finding #28): emitter expected `Result[_, _]` \
                     recorded for a `Result.mapErr` call at {:?}, but found {other:?} — the \
                     checker should already have typed it",
                    e.span
                ),
            };
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "((__r: Result<{t}, {et}>, __f: (e: {et}) => {fts}) => __r.tag === \"Err\" ? Err(__f(__r.error)) : __r)({recv}, {f})"
            ))
        }
        ("getOrElse", [fallback]) => {
            let recv = pre.lower(receiver, cx);
            let fallback = pre.lower(fallback, cx);
            Some(format!(
                "((__r: Result<{t}, {et}>, __d: {t}) => __r.tag === \"Ok\" ? __r.value : __d)({recv}, {fallback})"
            ))
        }
        ("isOk", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("({recv}.tag === \"Ok\")"))
        }
        _ => None,
    };
    text.map(|expr| pre.finish(expr))
}

/// §2.8.3: lower an `Effect[Result[T, E]]` combinator. `Effect[T]` is
/// `Promise<T>` at runtime, so each method is an `async` IIFE that `await`s the
/// receiver Promise and rebuilds the transformed `Result` — the exact
/// desugaring the spec gives (`mapOk` ≡ `map(r => r.map(f))`, etc.), inlined so
/// no runtime helper is needed. `ok`/`err` are the receiver's peeled `Result`
/// parameters; the call's checked type supplies the transformed parameter for
/// the TS annotation.
#[allow(clippy::too_many_arguments)]
fn lower_effect_result_kernel(
    e: &Expr,
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    ok: TyId,
    err: TyId,
    cx: &mut LowerCtx,
) -> Option<Lowered> {
    let tys = cx.commons().tys();
    let mut pre = Pre::new();
    let t = ts_ty(ok, tys);
    let et = ts_ty(err, tys);
    // The call's checked type is `Effect[Result[a, b]]`; peel `(a, b)` for the
    // transformed parameter's TS type.
    let (a_ts, b_ts) = match cx.commons().expr_ty(e.id).as_deref() {
        Some(Ty::Effect(inner)) => match &*tys.get(*inner) {
            Ty::Result(a, b) => (ts_ty(*a, tys), ts_ty(*b, tys)),
            other => panic!(
                "bynk internal error (finding #28): emitter expected `Effect[Result[_, _]]` \
                 recorded for an effectful `Result` combinator call at {:?}, but found \
                 `Effect[{other:?}]` — the checker should already have typed it",
                e.span
            ),
        },
        other => panic!(
            "bynk internal error (finding #28): emitter expected `Effect[Result[_, _]]` \
             recorded for an effectful `Result` combinator call at {:?}, but found {other:?} \
             — the checker should already have typed it",
            e.span
        ),
    };
    let text: Option<String> = match (method.name.as_str(), args) {
        ("mapOk", [f]) => {
            // result `Effect[Result[U, E]]` — `a_ts` is `U`.
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "(async (__e: Promise<Result<{t}, {et}>>, __f: (x: {t}) => {a_ts}) => {{ const __r = await __e; return __r.tag === \"Ok\" ? Ok(__f(__r.value)) : __r; }})({recv}, {f})"
            ))
        }
        ("mapErr", [f]) => {
            // result `Effect[Result[T, F]]` — `b_ts` is `F`.
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "(async (__e: Promise<Result<{t}, {et}>>, __f: (e: {et}) => {b_ts}) => {{ const __r = await __e; return __r.tag === \"Err\" ? Err(__f(__r.error)) : __r; }})({recv}, {f})"
            ))
        }
        ("flatMapOk", [f]) => {
            // `f: T -> Effect[Result[U, E]]`; result `Effect[Result[U, E]]`.
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "(async (__e: Promise<Result<{t}, {et}>>, __f: (x: {t}) => Promise<Result<{a_ts}, {et}>>) => {{ const __r = await __e; return __r.tag === \"Ok\" ? await __f(__r.value) : __r; }})({recv}, {f})"
            ))
        }
        ("flatMapErr", [f]) => {
            // `f: E -> Effect[Result[T, F]]`; result `Effect[Result[T, F]]`.
            let recv = pre.lower(receiver, cx);
            let f = pre.lower(f, cx);
            Some(format!(
                "(async (__e: Promise<Result<{t}, {et}>>, __f: (e: {et}) => Promise<Result<{t}, {b_ts}>>) => {{ const __r = await __e; return __r.tag === \"Err\" ? await __f(__r.error) : __r; }})({recv}, {f})"
            ))
        }
        _ => None,
    };
    text.map(|expr| pre.finish(expr))
}

/// v0.20b: lower a built-in `Map` kernel method. `insert` copies — the
/// emitted `ReadonlyMap` is never mutated in place; updating an existing key
/// keeps its insertion position (JS `Map` semantics, normative in §7).
fn lower_map_kernel(
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    key: TyId,
    val: TyId,
    cx: &mut LowerCtx,
) -> Option<Lowered> {
    let tys = cx.commons().tys();
    let mut pre = Pre::new();
    let key_ts = ts_ty(key, tys);
    let val_ts = ts_ty(val, tys);
    let text: Option<String> = match (method.name.as_str(), args) {
        ("length", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("({recv}).size"))
        }
        ("keys", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("[...({recv}).keys()]"))
        }
        // v0.149 (ADR 0173): the values in key order — the `keys()` sibling over
        // the in-memory `ReadonlyMap`.
        ("values", []) => {
            let recv = pre.lower(receiver, cx);
            Some(format!("[...({recv}).values()]"))
        }
        ("get", [k]) => {
            let recv = pre.lower(receiver, cx);
            let k = pre.lower(k, cx);
            Some(format!(
                "((__m: ReadonlyMap<{key_ts}, {val_ts}>, __k: {key_ts}) => __m.has(__k) ? Some(__m.get(__k) as {val_ts}) : None)({recv}, {k})"
            ))
        }
        ("insert", [k, v]) => {
            let recv = pre.lower(receiver, cx);
            let k = pre.lower(k, cx);
            let v = pre.lower(v, cx);
            Some(format!("new Map({recv}).set({k}, {v})"))
        }
        _ => None,
    };
    text.map(|expr| pre.finish(expr))
}

fn lower_if(
    e: &Expr,
    cond: &Expr,
    then_block: &Block,
    else_block: &Block,
    cx: &mut LowerCtx,
) -> Lowered {
    let mut pre = Pre::new();
    let slot_ty = checked_ty_ts(e, cx);
    let cond_expr = pre.lower(cond, cx);
    // If the cond contains `is`-bindings, the then-branch needs a place
    // for the `const name = receiver.field;` declarations — a ternary
    // has no such place. Force the IIFE form. (`ternary_shaped`.)
    if ternary_shaped(cond, then_block, else_block, cx) {
        // T2.1 (R6.2): lower each branch's tail once, then ask the branches
        // themselves whether they hoisted, rather than asking `simple_expr` to
        // predict it.
        //
        // #1029 review: `return_ty` is deliberately **not** cleared here, unlike
        // the IIFE path below. That path clears it because a `return` in its
        // arms exits the arrow rather than the enclosing function, so an
        // embedding `?` must behave like a plain one (ADR 0178). This path
        // inverts that premise: when a branch hoists, `hoist_if_as_statement`
        // puts the statements in the *caller's* statement position, so the `?`
        // expansion's `return` does exit the enclosing function and the declared
        // embedding must still apply. Clearing it emitted a bare `return __rN;`
        // — an `Err` of the operand's error type returned from a function
        // declared with the embedding sum's.
        //
        // Preserving it cannot move the no-hoist branch: `return_ty` is read
        // only by `embed_conversion`, only from the `?` expansion, and a `?`
        // expansion always pushes to `pre` — so where `pre` is empty, nothing
        // read it.
        let then_tail = lower_expr(&then_block.tail, cx);
        let else_tail = lower_expr(&else_block.tail, cx);
        if then_tail.pre.is_empty() && else_tail.pre.is_empty() {
            return pre.finish(format!(
                "({cond_expr} ? {t} : {e})",
                t = then_tail.expr,
                e = else_tail.expr
            ));
        }
        // A branch hoisted. Before T2.1 there was nowhere to put its statements
        // from here — a value position had no statement slot — and the only
        // shape available was the arrow below, which swallows a hoisted `?`'s
        // early return. `lower_if` now returns its own `pre`, so the `if` hoists
        // as a real statement in the caller's statement position and the
        // `return` keeps exiting the enclosing function. See
        // `hoist_if_as_statement`.
        let value = hoist_if_as_statement(&mut pre, cond_expr, then_tail, else_tail, slot_ty, cx);
        pre.finish(value)
    } else {
        // T2.2 (R6.4): isolate the effectfulness flag to just this IIFE's own
        // body. Unlike `lower_match_as_iife`'s `disc_expr` (spliced *outside*
        // the returned arrow, at the call site), `cond_expr`'s *text* is
        // spliced inside this arrow's own `if (...)` below — only its hoisted
        // statements landed in `pre` above. A bare `await` in `cond_expr`'s
        // text would need to feed `needs_async`, not `saved_await`; the
        // debug_assert after the arrow closes documents that this can't
        // happen today (a literal `await` comes only from `EffectLet`/`Do`,
        // both hoisted to `pre`, or from a nested `finish_async_iife` wrap,
        // which would require a value-position `match`/`if` needing the async
        // wrap to itself type as `Bool` — ruled out by the checker's
        // Effect-typing) rather than silently trusting it.
        let saved_await = std::mem::take(&mut cx.emitted_await);
        debug_assert!(
            !cond_expr.contains("await "),
            "cond text lands inside this arrow; its awaits must feed `needs_async`, not `saved_await`: {cond_expr}"
        );
        let mut iife = String::new();
        iife.push_str("(() => {\n");
        iife.push_str("    if (");
        iife.push_str(&cond_expr);
        iife.push_str(") {\n");
        // Inject is-binding declarations on the truthy side. #908: scoped to
        // the then-branch only — pushed here, popped once it's emitted —
        // so a read inside resolves to the binding rather than an outer
        // `let` rename, and the registration doesn't leak into the else
        // branch or beyond.
        cx.shadow_scopes.push(HashMap::new());
        let mut is_bindings = Vec::new();
        let mut found = false;
        gather_is_bindings_for_emit(cond, cx, &mut is_bindings, &mut found);
        for b in &is_bindings {
            for _ in 0..(INDENT_STEP * 3) {
                iife.push(' ');
            }
            iife.push_str(b);
            iife.push('\n');
        }
        // v0.154 (ADR 0178): a value-position `if` lowers to an IIFE — a `return`
        // in its arms exits the arrow, not the enclosing function — so clear the
        // enclosing `return_ty`: an embedding `?` here behaves exactly like a
        // plain `?` (no function-level wrap), never inheriting the outer type.
        let saved = cx.return_ty.take();
        // #4 review: `iife` is a local buffer spliced into the caller's
        // output later, at an offset `record_span` has no way to learn —
        // see `without_source_map`.
        cx.without_source_map(|cx| {
            emit_block_as_function_body(&mut iife, then_block, cx, INDENT_STEP * 3, false)
        });
        cx.shadow_scopes.pop();
        for _ in 0..(INDENT_STEP * 2) {
            iife.push(' ');
        }
        iife.push_str("} else {\n");
        cx.without_source_map(|cx| {
            emit_block_as_function_body(&mut iife, else_block, cx, INDENT_STEP * 3, false)
        });
        cx.return_ty = saved;
        for _ in 0..(INDENT_STEP * 2) {
            iife.push(' ');
        }
        iife.push_str("}\n");
        for _ in 0..INDENT_STEP {
            iife.push(' ');
        }
        iife.push_str("})()");
        // #2 review: unlike `lower_match_as_iife`'s IIFE, this one used to never
        // consult the async-wrap decision at all — confirmed live: `let r = if
        // c { let y <- fetch(); y + 1 } else { 0 }` emitted a bare `await`
        // inside this arrow with no `async` keyword, a hard JS syntax error
        // ("Unexpected reserved word"), not merely a missed optimisation.
        let needs_async = cx.emitted_await;
        cx.emitted_await = saved_await || needs_async;
        pre.finish(finish_async_iife(iife, needs_async))
    }
}

/// True if `e` contains an `is` test reachable through `&&` and parentheses —
/// matching exactly the shapes `gather_is_bindings_for_emit` walks (note: it
/// does *not* descend into `||`). Used by `lower_and_with_is` to decide whether
/// the `is`-binding flow applies before doing any lowering.
fn cond_contains_is(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Is { .. } => true,
        ExprKind::BinOp(BinOp::And, l, r) => cond_contains_is(l) || cond_contains_is(r),
        ExprKind::Paren(inner) => cond_contains_is(inner),
        _ => false,
    }
}

/// True if the expression contains an `is` test with at least one
/// non-wildcard binding. Walks through `&&`, `||`, and parens.
fn cond_has_is_bindings(e: &Expr, cx: &LowerCtx) -> bool {
    match &e.kind {
        ExprKind::Is { value, pattern } => pattern_is_has_bindings(pattern, value, cx),
        ExprKind::BinOp(BinOp::And, l, r) | ExprKind::BinOp(BinOp::Or, l, r) => {
            cond_has_is_bindings(l, cx) || cond_has_is_bindings(r, cx)
        }
        ExprKind::Paren(inner) => cond_has_is_bindings(inner, cx),
        _ => false,
    }
}

/// The per-pattern-kind check `cond_has_is_bindings` delegates to (#474: split
/// out so an or-pattern can recurse into its alternatives).
fn pattern_is_has_bindings(pattern: &Pattern, value: &Expr, cx: &LowerCtx) -> bool {
    match pattern {
        Pattern::Variant {
            variant, bindings, ..
        } => {
            bindings.iter().any(|b| !b.is_wildcard())
                // v0.13: a refined `is`-narrowing introduces a (shadow) binding
                // even though the pattern carries none.
                || (bindings.is_empty() && cx.is_refined_is_check(value, &variant.name))
        }
        // #474: Rule 1 guarantees every alternative binds the same names, so
        // any one alternative having bindings means the whole pattern does.
        Pattern::Or(alts, _) => alts
            .iter()
            .any(|alt| pattern_is_has_bindings(alt, value, cx)),
        _ => false,
    }
}

fn emit_if_tail(
    out: &mut String,
    cond: &Expr,
    then_block: &Block,
    else_block: &Block,
    cx: &mut LowerCtx,
    indent: usize,
    async_tail: bool,
) {
    let mut pre = Pre::new();
    let cond_expr = pre.lower(cond, cx);
    for s in pre.stmts() {
        write_line(out, indent, s);
    }
    write_line(out, indent, &format!("if ({cond_expr}) {{"));
    // is-binding declarations on the truthy path. #908: scoped to the
    // then-branch only (see the IIFE sibling above for the same reasoning).
    cx.shadow_scopes.push(HashMap::new());
    let mut is_bindings = Vec::new();
    let mut found = false;
    gather_is_bindings_for_emit(cond, cx, &mut is_bindings, &mut found);
    for b in &is_bindings {
        write_line(out, indent + INDENT_STEP, b);
    }
    emit_block_as_function_body(out, then_block, cx, indent + INDENT_STEP, async_tail);
    cx.shadow_scopes.pop();
    write_line(out, indent, "} else {");
    emit_block_as_function_body(out, else_block, cx, indent + INDENT_STEP, async_tail);
    write_line(out, indent, "}");
}

fn lower_ident(e: &Expr, id: &Ident, cx: &mut LowerCtx) -> String {
    // v0.116: inside a `transition` predicate, the contextual `old`/`new` idents
    // are the pre-/post-commit state records, lowered to their JS names (`new` is
    // reserved, so both are renamed). Field access `old.<f>` reads off the record.
    // Checked first so `old`/`new` never collide with the heuristics below.
    if let Some((old_var, new_var)) = cx.transition_states() {
        if id.name == "old" {
            return old_var.to_string();
        }
        if id.name == "new" {
            return new_var.to_string();
        }
    }
    // v0.80: inside an invariant predicate, a bare ident naming a state field
    // reads it off the proposed-state value (`s.<field>`). Checked first so a
    // field never collides with the variant-constructor heuristics below.
    if let Some((var, fields)) = cx.invariant_state()
        && fields.contains(&id.name)
    {
        return format!("{var}.{}", id.name);
    }
    // v0.81: inside a `store`-agent handler, a bare ident naming a `Cell` field
    // reads it off the mutable working state (`__state.<cell>`), so a read after
    // a `:=` write in the same handler sees the written value (read-your-writes).
    if let Some((var, cells)) = cx.agent_store_cells()
        && cells.contains(&id.name)
    {
        return format!("{var}.{}", id.name);
    }
    // v0.104/v0.105 (real-time track slice 3b): a bare held-`Map` ident used as a
    // value is a lazy `Query` over its **resolved** connections — the persisted
    // `Record<K, connId>` mapped through `resolveConnection`, keeping the present
    // ones. Checked before the persisted-`Map` branch (held maps are excluded from
    // `agent_store_maps`).
    if !cx.is_local(&id.name)
        && let Some(f_ts) = cx.agent_held_map_frame(&id.name)
    {
        let var = cx.agent_store_var();
        return format!(
            "(() => Object.values({var}.{name}).flatMap((__cid) => {{ const __c = resolveConnection<{f_ts}>(this.state, __cid); return __c.tag === \"Some\" ? [__c.value] : []; }}))",
            name = id.name
        );
    }
    // v0.94 (ADR 0120): a bare `store Map` ident used as a **value** — not a
    // method receiver (those are handled in the method dispatch) — is a lazy
    // `Query` over the whole map, e.g. the `other` side of a join. It lowers to
    // the same deferred thunk a query builder yields: `() => Object.values(map)`.
    if !cx.is_local(&id.name) && cx.is_agent_store_map(&id.name) {
        let var = cx.agent_store_var();
        return format!("(() => Object.values({var}.{}))", id.name);
    }
    // v0.95 (ADR 0121): a bare `store Log` ident used as a value is a lazy
    // `Query` over its entry values — `() => log.map((__e) => __e.v)`.
    if !cx.is_local(&id.name) && cx.is_agent_store_log(&id.name) {
        let var = cx.agent_store_var();
        return format!("(() => {var}.{}.map((__e) => __e.v))", id.name);
    }
    // v0.9: a nullary HttpResult variant (whose checker type is
    // `HttpResult[_]`) constructs an HttpResult.<Variant>.
    //
    // P6.21/P6.23 (review of #1251): reads `Callee::Intrinsic` instead of
    // the `Ty::HttpResult`/`Ty::QueueResult` type check plus a bare
    // `http_variant`/`queue_variant` keyword lookup — consistency with
    // every other static-constructor branch in this module, not a new
    // defect closure: the resolved-type guard here was already immune to
    // name-matching (a shadowing local never types as `Ty::HttpResult`).
    if matches!(
        cx.commons().callee(e.id),
        Some(Callee::Intrinsic { ns, .. }) if *ns == HTTP_RESULT
    ) {
        return format!("HttpResult.{}", id.name);
    }
    // v0.44: a nullary QueueResult variant (`Ack`) constructs `QueueResult.Ack`.
    if matches!(
        cx.commons().callee(e.id),
        Some(Callee::Intrinsic { ns, .. }) if *ns == QUEUE_RESULT
    ) {
        return format!("QueueResult.{}", id.name);
    }
    // A bare ident whose name matches a declared variant of a sum
    // type (and whose checker type is that sum) is a nullary
    // variant constructor reference. Qualify it as `Type.Variant`.
    // Otherwise (locals, params, `self`) emit the identifier as-is.
    if let Some(Ty::Named {
        kind: NamedKind::Sum,
        name: type_name,
        ..
    }) = cx.commons().expr_ty(e.id).as_deref()
        && let Some(decl) = cx.commons().types.get(type_name)
        && let TypeBody::Sum(s) = &decl.body
        && s.variants.iter().any(|v| v.name.name == id.name)
    {
        return format!("{}.{}", type_name, id.name);
    }
    // v0.52: the multi-actor sum binder is not a runtime local — the resolved
    // actor is threaded through `deps.who` at the boundary wrapper, so the
    // binder ident lowers to it (the tagged union the body `match`es).
    if cx.actor_sum_binder() == Some(id.name.as_str())
        && matches!(cx.commons().expr_ty(e.id).as_deref(), Some(Ty::ActorSum(_)))
    {
        return "deps.who".to_string();
    }
    // #908: a re-`let` of this name (in this or an enclosing block) renamed
    // its emitted binding — resolve to it rather than the natural
    // (now-shadowed) `ts_ident` name.
    cx.resolved_local_name(&id.name)
        .unwrap_or_else(|| ts_ident(&id.name))
}

fn lower_call(e: &Expr, name: &Ident, args: &[Expr], cx: &mut LowerCtx) -> Lowered {
    let mut pre = Pre::new();
    // Bare variant constructor with payload → qualify.
    let args_lowered: Vec<String> = args.iter().map(|a| pre.lower(a, cx)).collect();
    // v0.9: HttpResult variant call.
    //
    // P6.21/P6.23 (review of #1251): reads `Callee::Intrinsic` instead of
    // the `Ty::HttpResult`/`Ty::QueueResult` type check plus a bare
    // keyword lookup — consistency with the bare-`Ident` sibling in
    // `lower_ident`, not a new defect closure.
    if matches!(
        cx.commons().callee(e.id),
        Some(Callee::Intrinsic { ns, .. }) if *ns == HTTP_RESULT
    ) {
        return pre.finish(format!(
            "HttpResult.{}({})",
            name.name,
            args_lowered.join(", ")
        ));
    }
    // v0.44: a QueueResult variant call (`Retry(reason)`) → `QueueResult.Retry(...)`.
    if matches!(
        cx.commons().callee(e.id),
        Some(Callee::Intrinsic { ns, .. }) if *ns == QUEUE_RESULT
    ) {
        return pre.finish(format!(
            "QueueResult.{}({})",
            name.name,
            args_lowered.join(", ")
        ));
    }
    // P6.21 (real P6.2 cutover, partial — #1143/#1140's `Callee` classification
    // read back instead of re-derived): agent instantiation `AgentName(key)`
    // lowers to the generated `__makeAgentName(key)` factory, which obtains
    // the instance for this key (lookup-or-create against the registry in
    // bundle mode, or a typed DO proxy in workers mode). Skipped when this
    // Call is the receiver of a MethodCall — that path folds construction
    // and the method invocation together. Previously guarded by
    // `cx.local_agents.contains(&name.name)`, a name-matched re-derivation
    // of what the checker's own `Callee::AgentInit` already resolved once —
    // the same defect class `block_writes_state`'s receiver-name matching
    // carried before #1196 converted it to read `Callee::Store` directly.
    if matches!(cx.commons().callee(e.id), Some(Callee::AgentInit(_))) {
        let key_expr = args_lowered.first().unwrap_or_else(|| {
            panic!(
                "bynk internal error (ADR 0334): Callee::AgentInit records agent construction \
                 `{}(...)` with {} args, but every AgentDecl declares exactly one key field — \
                 the checker already accepted this call",
                name.name,
                args_lowered.len()
            )
        });
        return pre.finish(cx.agent_construct(&name.name, key_expr));
    }
    // P6.21: sum-variant construction is `Callee::Ctor` — reads the sum
    // type's own name and the variant's own tag straight off the checker's
    // classification instead of `call_is_sum_variant`'s own name-matched
    // `sum_name`/`call_name` string comparison against the call's checked
    // type (the same re-derivation this whole `Callee` precedent, P6.0,
    // exists to close).
    if let Some(Callee::Ctor { sum, tag }) = cx.commons().callee(e.id) {
        return pre.finish(format!(
            "{}.{}({})",
            sum.name.name,
            tag,
            args_lowered.join(", ")
        ));
    }
    // #527: a commons-imported fn speaks the *unbranded* commons types, but
    // this context rebrands them (`Event & { __ctxBrand }`); assert the call
    // back into the local namespace so branded positions accept it. The brand
    // is phantom — the value is identical. Pure fns only: an effectful call
    // is awaited by its caller, and the assertion would need to target the
    // Promise, not the value.
    if cx.commons_imported_fns().contains(&name.name)
        && let Some(f) = cx.commons().fns.get(&name.name)
        && !matches!(f.return_type, TypeRef::Effect(..))
        && typeref_mentions_any(&f.return_type, cx.rebranded_types())
    {
        return pre.finish(format!(
            "({}({}) as {})",
            ts_ident(&name.name),
            args_lowered.join(", "),
            ts_type_ref(&f.return_type)
        ));
    }
    pre.finish(format!(
        "{}({})",
        ts_ident(&name.name),
        args_lowered.join(", ")
    ))
}

/// True when `r` references any of `names` (recursing through the compound
/// constructors).
fn typeref_mentions_any(r: &TypeRef, names: &HashSet<String>) -> bool {
    match r {
        TypeRef::Named(id) => names.contains(&id.name),
        TypeRef::Result(a, b, _) | TypeRef::Map(a, b, _) => {
            typeref_mentions_any(a, names) || typeref_mentions_any(b, names)
        }
        TypeRef::Option(t, _)
        | TypeRef::Effect(t, _)
        | TypeRef::HttpResult(t, _)
        | TypeRef::List(t, _)
        | TypeRef::Query(t, _)
        | TypeRef::Stream(t, _)
        | TypeRef::Connection(t, _)
        | TypeRef::History(t, _) => typeref_mentions_any(t, names),
        TypeRef::Fn(params, ret, _) => {
            params.iter().any(|p| typeref_mentions_any(p, names))
                || typeref_mentions_any(ret, names)
        }
        // v0.157 (ADR 0183): a `Name[Arg, …]` mentions the generic type's name
        // or any name inside its arguments.
        TypeRef::App { name, args, .. } => {
            names.contains(&name.name) || args.iter().any(|a| typeref_mentions_any(a, names))
        }
        TypeRef::Base(..)
        | TypeRef::QueueResult(_)
        | TypeRef::ValidationError(_)
        | TypeRef::JsonError(_)
        | TypeRef::Unit(_) => false,
    }
}

fn lower_bin_op(op: BinOp, lhs: &Expr, rhs: &Expr, cx: &mut LowerCtx) -> Lowered {
    let tys = cx.commons().tys();
    let mut pre = Pre::new();
    // For `&&` we need to lower `is` bindings into the rhs scope.
    // We handle that here by collecting bindings from lhs, emitting
    // them as `const` declarations before evaluating rhs — but
    // `&&` short-circuits, so simply emitting them inline is wrong.
    // We compile `lhs && (...is binding flow...) rhs` to a function
    // expression: `(lhs && ((bindings) => rhs)())`. Simpler: rely
    // on TypeScript's narrowing for the value-from-is part of
    // `is` patterns. For now, for the special pattern `x is Ok(n)`
    // we lower the rhs assuming the binding `n = x.value` was
    // captured. We use a parenthesised IIFE to scope the binding.
    if op == BinOp::And
        && let Some((bindings, lhs_lowered, rhs_expr, rhs_returns)) =
            lower_and_with_is(lhs, rhs, cx)
    {
        let lhs_expr = pre.absorb(lhs_lowered);
        if bindings.is_empty() {
            return pre.finish(format!("{lhs_expr} && {rhs_expr}"));
        }
        // T2.3 (R6.3): `bindings` includes rhs's own hoisted statements
        // (`lower_and_with_is`'s doc comment above), so if one of those is a
        // `?`'s propagating early return, the arrow-IIFE below would capture
        // it instead of letting it exit the enclosing function — reuse
        // `hoist_if_as_statement` (built for T2.1's `if`-hoisting) to hoist
        // this `&&` as a real `if` statement instead.
        //
        // #1044 review: `hoist_if_as_statement`'s bare `slot` return is right
        // for the general path (no narrowing at stake there — see below), but
        // wrong here. `lhs_expr` is the very `is`-check `lower_if`'s own
        // `gather_is_bindings_for_emit` relies on being *textually present* in
        // the condition it hands to `tsc` — that's what lets `o` narrow to
        // `Hit` for a hoisted `const score = o.score;` in the caller's
        // then-block. Dropping `lhs_expr` for a disconnected `let __rN:
        // boolean` (assigned across an `if`/`else`, not a `tsc`-4.4+ aliased
        // `const`) severs that link and turns a silent `?`-escape miscompile
        // into a hard `tsc --strict` failure on the same input. Re-including
        // `lhs_expr` in the returned text — safe to re-evaluate, since it is
        // always a pure tag/field read on an already-hoisted receiver, never
        // the original (possibly effectful) `is` subject — keeps the
        // conjunction's left side exactly where a caller's narrowing expects
        // to find it; `slot` only stands in for rhs's value, gated by the
        // same `if` that already guards evaluating it.
        if rhs_returns {
            let slot = hoist_if_as_statement(
                &mut pre,
                lhs_expr.clone(),
                Lowered {
                    pre: bindings,
                    expr: rhs_expr,
                },
                Lowered::bare("false"),
                Some("boolean".to_string()),
                cx,
            );
            return pre.finish(format!("{lhs_expr} && {slot}"));
        }
        // Emit:  lhs && (() => { const n = ...; return rhs; })()
        let mut wrap = String::new();
        wrap.push_str(&lhs_expr);
        wrap.push_str(" && ((() => { ");
        for b in &bindings {
            wrap.push_str(b);
            wrap.push(' ');
        }
        wrap.push_str(&format!("return {rhs_expr}; }})())"));
        return pre.finish(wrap);
    }
    // v0.80: `P implies Q` lowers to `(!(P) || Q)`. As with `&&`, an `is` test in
    // the antecedent binds into the consequent (the consequent is only reached
    // when the antecedent holds), so reuse the same is-binding IIFE flow.
    if op == BinOp::Implies
        && let Some((bindings, lhs_lowered, rhs_expr, rhs_returns)) =
            lower_and_with_is(lhs, rhs, cx)
    {
        let lhs_expr = pre.absorb(lhs_lowered);
        if bindings.is_empty() {
            return pre.finish(format!("(!({lhs_expr}) || {rhs_expr})"));
        }
        // #1044 review: same fix as the `And` arm above — `lhs_expr` (the
        // antecedent's `is`-check) must stay in the returned text for a
        // caller's narrowing to hold, so gate `slot`'s computation on
        // `lhs_expr` directly (rhs is only reached when the antecedent
        // holds, same guard as `&&`) rather than folding the negation into
        // the hoisted `if`'s own condition.
        if rhs_returns {
            let slot = hoist_if_as_statement(
                &mut pre,
                lhs_expr.clone(),
                Lowered {
                    pre: bindings,
                    expr: rhs_expr,
                },
                Lowered::bare("true"),
                Some("boolean".to_string()),
                cx,
            );
            return pre.finish(format!("(!({lhs_expr}) || {slot})"));
        }
        let mut wrap = String::new();
        wrap.push_str(&format!("(!({lhs_expr}) || ((() => {{ "));
        for b in &bindings {
            wrap.push_str(b);
            wrap.push(' ');
        }
        wrap.push_str(&format!("return {rhs_expr}; }})()))"));
        return pre.finish(wrap);
    }
    let l = pre.lower(lhs, cx);
    // #1/#3 review: `&&`/`||`/`implies` short-circuit — `design/bynk-type-
    // system.md:1671-1678` calls this "both a performance property and a
    // safety property … developers can rely on." Lowering rhs into the same
    // `pre` as lhs — the previous behaviour for every case that didn't go
    // through `lower_and_with_is` above (i.e. `||` always, and `&&`/`implies`
    // whenever the antecedent contains no `is`) — hoisted rhs's
    // pre-statements unconditionally, running them even when lhs's value
    // means rhs is never evaluated. Only inline `r.expr` bare when rhs
    // hoists nothing; otherwise wrap it in an IIFE so the hoist runs only
    // when the operator actually reaches rhs.
    //
    // T2.3 (R6.3): if rhs's hoist itself contains a `?`'s propagating early
    // return (`if (__rN.tag === "Err") return __rN;`), a plain
    // `(() => { ...; return expr; })()` wrap would capture that `return`
    // instead of letting it exit the enclosing function — the same class of
    // miscompile `lower_match_as_iife` had. `LowerCtx::emitted_early_return`
    // says whether rhs's own lowering hit that case; when it did, hoist this
    // operator as a real `if` statement via `hoist_if_as_statement` (built for
    // T2.1's `if`-hoisting) instead of an expression-position IIFE — the
    // `return` then keeps exiting the enclosing function, and still only runs
    // when the operator actually reaches rhs. An ordinary hoist (no `?`
    // involved) keeps the cheaper arrow form; nothing inside it needs to
    // escape past the arrow, and this is the shape the `947_short_circuit_rhs_hoist`
    // fixture pins.
    if matches!(op, BinOp::And | BinOp::Or | BinOp::Implies) {
        let saved_early_return = std::mem::take(&mut cx.emitted_early_return);
        let r = lower_expr(rhs, cx);
        let rhs_returns = cx.emitted_early_return;
        cx.emitted_early_return = saved_early_return || rhs_returns;
        if !r.pre.is_empty() && rhs_returns {
            let value = match op {
                BinOp::And => hoist_if_as_statement(
                    &mut pre,
                    l,
                    r,
                    Lowered::bare("false"),
                    Some("boolean".to_string()),
                    cx,
                ),
                BinOp::Or => hoist_if_as_statement(
                    &mut pre,
                    l,
                    Lowered::bare("true"),
                    r,
                    Some("boolean".to_string()),
                    cx,
                ),
                BinOp::Implies => hoist_if_as_statement(
                    &mut pre,
                    format!("!({l})"),
                    Lowered::bare("true"),
                    r,
                    Some("boolean".to_string()),
                    cx,
                ),
                _ => unreachable!("guarded by the outer `matches!`"),
            };
            return pre.finish(value);
        }
        let rhs_text = if r.pre.is_empty() {
            r.expr
        } else {
            let mut s = String::from("(() => { ");
            for p in &r.pre {
                s.push_str(p);
                s.push(' ');
            }
            s.push_str(&format!("return {}; }})()", r.expr));
            s
        };
        return pre.finish(match op {
            BinOp::And => format!("{l} && {rhs_text}"),
            BinOp::Or => format!("{l} || {rhs_text}"),
            // `P implies Q` ≡ `!P || Q` (no `is` bindings in the antecedent —
            // that case returned above via `lower_and_with_is`).
            BinOp::Implies => format!("(!({l}) || {rhs_text})"),
            _ => unreachable!("guarded by the outer `matches!`"),
        });
    }
    let r = pre.lower(rhs, cx);
    let text = if op == BinOp::Div {
        // v0.21: division is operand-typed (ADR 0042) — `Float`
        // true-divides; `Int` keeps truncating. The checker rejects
        // mixed operands, so the left operand decides; a missing
        // type entry falls back to the `Int` (truncating) lowering.
        let lhs_is_float = cx
            .commons()
            .expr_types
            .get(&lhs.id)
            .and_then(|te| te.ty.base(tys))
            == Some(BaseType::Float);
        if lhs_is_float {
            format!("{l} / {r}")
        } else {
            format!("Math.trunc({l} / {r})")
        }
    } else if matches!(op, BinOp::Eq | BinOp::NotEq)
        && cx
            .commons()
            .expr_types
            .get(&lhs.id)
            .and_then(|te| te.ty.base(tys))
            == Some(BaseType::Bytes)
    {
        // v0.110 (ADR 0142 D4): `Bytes` is the one base type whose `==` is not
        // host `===`. It erases to `Uint8Array`, so `===` is reference equality
        // (`Bytes.fromUtf8("a") === Bytes.fromUtf8("a")` is `false`). Equality
        // must compare by content — operand-typed dispatch, exactly like `Div`.
        // The checker rejects mixed operands, so the left operand decides.
        cx.note_bytes();
        let eq = format!("__bynkBytesEqual({l}, {r})");
        if op == BinOp::Eq {
            eq
        } else {
            format!("!{eq}")
        }
    } else {
        format!("{l} {} {r}", ts_binop(op))
    };
    pre.finish(text)
}

fn lower_constructor_call(
    type_name: &Ident,
    method: &Ident,
    args: &[Expr],
    cx: &mut LowerCtx,
) -> Lowered {
    let mut pre = Pre::new();
    let args: Vec<String> = args.iter().map(|a| pre.lower(a, cx)).collect();
    let text = // Nullary variant qualified construction: `T.V` (no parens) at the
    // source level wouldn't reach here, so `T.V()` always means call.
    format!("{}.{}({})", type_name.name, method.name, args.join(", "));
    pre.finish(text)
}

fn lower_record_construction(
    type_name: &Ident,
    fields: &[FieldInit],
    cx: &mut LowerCtx,
) -> Lowered {
    let mut pre = Pre::new();
    let mut parts = Vec::new();
    for f in fields {
        match &f.value {
            Some(v) => {
                let val = pre.lower(v, cx);
                parts.push(format!("{}: {}", f.name.name, val));
            }
            None => {
                // Shorthand `{ x }` references the binding `x`; if the binding
                // was renamed (a reserved word), expand to `x: __id_x` — the
                // key is wire format and must keep the source spelling.
                let v = ts_ident(&f.name.name);
                if v == f.name.name {
                    parts.push(f.name.name.clone());
                } else {
                    parts.push(format!("{}: {v}", f.name.name));
                }
            }
        }
    }
    let _ = type_name;
    pre.finish(format!("{{ {} }}", parts.join(", ")))
}

fn lower_field_access(e: &Expr, receiver: &Expr, field: &Ident, cx: &mut LowerCtx) -> Lowered {
    let tys = cx.commons().tys();
    let mut pre = Pre::new();
    // v0.158 (ADR 0184): `<map>.entries` / `.keys` / `.values` on a `store
    // Map[K, V]` field — the key-exposing lazy queries. Like every store-map
    // query, they lower to a deferred thunk over the working record. `.values`
    // scans `Object.values` (the existing value lift); `.keys` scans
    // `Object.keys`; `.entries` zips both into `{ key, value }` records
    // (`MapEntry[K, V]`). A persisted key is a JS object key (a string), so an
    // `Int`-typed key is decoded back with `Number(...)`; a `String`-typed key
    // is already correct.
    if let ExprKind::Ident(id) = &receiver.kind
        && cx.is_agent_store_map(&id.name)
        && !cx.is_agent_held_map(&id.name)
        && matches!(
            field.name.as_str(),
            map_query::ENTRIES | map_query::KEYS | map_query::VALUES
        )
    {
        let var = cx.agent_store_var().to_string();
        let m = format!("{var}.{}", id.name);
        return pre.finish(match field.name.as_str() {
            map_query::VALUES => format!("(() => Object.values({m}))"),
            map_query::KEYS => {
                let decoded = decode_map_key(map_key_ty(e, cx), "__k", tys);
                if decoded == "__k" {
                    format!("(() => Object.keys({m}))")
                } else {
                    format!("(() => Object.keys({m}).map((__k) => {decoded}))")
                }
            }
            // entries
            _ => {
                let decoded = decode_map_key(map_key_ty(e, cx), "__k", tys);
                format!(
                    "(() => Object.entries({m}).map(([__k, __v]) => ({{ key: {decoded}, value: __v }})))"
                )
            }
        });
    }
    // v0.9: `HttpResult.Variant` (nullary).
    //
    // P6.21/P6.23 (review of #1251): reads `Callee::Intrinsic` instead of
    // `id.name == HTTP_RESULT` — see the `MethodCall` sibling above (this
    // module's own qualified-with-args `HttpResult.Variant(args)` branch)
    // for the full reasoning.
    if let ExprKind::Ident(_) = &receiver.kind
        && matches!(
            cx.commons().callee(e.id),
            Some(Callee::Intrinsic { ns, .. }) if *ns == HTTP_RESULT
        )
    {
        return pre.finish(format!("HttpResult.{}", field.name));
    }
    // Agent-handler `self.<key>` rewrite.
    if cx.in_agent_handler()
        && let ExprKind::Ident(id) = &receiver.kind
        && id.name == "self"
        && let Some(k) = cx.agent_key_field()
        && field.name == *k
    {
        return pre.finish(format!("(this.state.id.toString() as {})", k));
    }
    // v0.45: `<binder>.identity` on a verified actor binding. The binder is not
    // a runtime value; the identity is minted at the verification seam. For the
    // zero-crypto schemes the sealed identity carries no payload, so it lowers
    // to the unit value (`undefined`). Authenticated identities (Bearer/
    // Signature) and the calling-context value arrive with their later slices.
    //
    // v0.47: a Bearer handler's identity is minted at the verification seam and
    // threaded through `deps`, so `<binder>.identity` reads `deps.identity`.
    // Other (unit) identities — `None`/`Internal` actors — carry no payload and
    // stay the unit value `undefined`.
    if field.name == "identity"
        && matches!(
            cx.commons().expr_ty(receiver.id).as_deref(),
            Some(Ty::Actor(_))
        )
    {
        if let (Some(binder), ExprKind::Ident(id)) = (cx.deps_identity_binder(), &receiver.kind)
            && id.name == binder
        {
            return pre.finish("deps.identity".to_string());
        }
        return pre.finish("undefined".to_string());
    }
    let r = pre.lower(receiver, cx);
    // `.raw` on an opaque value compiles to a TypeScript type
    // assertion back to the base type. The checker has already
    // verified that the receiver is opaque and the call site is
    // inside the defining commons.
    if field.name == RAW
        && let Some(Ty::Named {
            kind: NamedKind::Opaque(base),
            ..
        }) = cx.commons().expr_ty(receiver.id).as_deref()
    {
        return pre.finish(format!("({r} as {})", ts_base(*base)));
    }
    pre.finish(format!("{r}.{}", field.name))
}

/// v0.158 (ADR 0184): the key type `K` of a map `.entries`/`.keys` query, read
/// off the field-access result type — `Query[MapEntry[K, V]]` for `.entries`,
/// `Query[K]` for `.keys`. Used to decode persisted string object-keys back to
/// `K`. `None` only when the accessor's type was not recorded — which the
/// checker never leaves unset for a well-typed `.entries`/`.keys` (it stamps the
/// result type at `e.span`), so a `None` here is a checker/emitter invariant
/// break, not a runtime input. The `debug_assert!` in [`decode_map_key`] pins
/// that: the fallback identity decode must never be reached for an `Int` key,
/// which would silently emit a string where a `number` is expected.
fn map_key_ty(e: &Expr, cx: &LowerCtx) -> Option<TyId> {
    let tys = cx.commons().tys();
    match &*tys.get(cx.commons().expr_types.get(&e.id).map(|te| te.ty)?) {
        Ty::Query(inner) => match &*tys.get(*inner) {
            Ty::Named { name, args, .. } if name == MAP_ENTRY && !args.is_empty() => Some(args[0]),
            _ => Some(*inner),
        },
        _ => None,
    }
}

/// v0.158 (ADR 0184): decode a persisted map key (a JS object key, always a
/// string) back to its bynk type. Value-keyable keys are `Int`/`String` and
/// refinements/opaques over them (ADR 0110 D5); an `Int`-based key erases to
/// `number`, so it is parsed with `Number(...)`, while a `String`-based key is
/// already correct and passes through unchanged.
fn decode_map_key(k: Option<TyId>, raw: &str, tys: &Arc<Types>) -> String {
    // The key type is always recorded for a well-typed `.entries`/`.keys` — a
    // `None` means the checker failed to stamp it, and the identity fallback
    // below would silently emit a string for an `Int` key. Pin the invariant in
    // debug builds so it can never become load-bearing by accident.
    debug_assert!(
        k.is_some(),
        "map key type unrecorded — `.entries`/`.keys` key decode would fall back to identity",
    );
    let base = match k.map(|t| tys.get(t)).as_deref() {
        Some(Ty::Base(b)) => Some(*b),
        Some(Ty::Named {
            kind: NamedKind::Refined(b) | NamedKind::Opaque(b),
            ..
        }) => Some(*b),
        _ => None,
    };
    match base {
        Some(BaseType::Int) => format!("Number({raw})"),
        Some(BaseType::String) => raw.to_string(),
        // #70/#71 review: `resolver.rs`'s `check_map_key_keyable` and
        // `validate.rs`'s `@indexed`-field check both gate value-keyability
        // (ADR 0110 D5) on exactly `String`/`Int` — the same pair this match
        // now names explicitly. If either of those independent copies ever
        // widens the accepted set without this one being taught the new
        // type's wire shape, silently falling through to bare `raw` would
        // *miscompile* rather than reject — e.g. a `Bool` key needs
        // `raw === "true"`, not the raw string. Fail loudly instead.
        Some(other) => unreachable!(
            "map key base type {other:?} is not value-keyable (ADR 0110 D5) — \
             decode_map_key has no wire-decode case for it"
        ),
        None => raw.to_string(),
    }
}

#[cfg(test)]
mod decode_map_key_tests {
    use super::*;

    /// One table for this module's fixtures — `Types` is `Send + Sync`
    /// (T3.6b), so a `LazyLock` static serves every test without threading.
    static TYS: std::sync::LazyLock<Arc<Types>> =
        std::sync::LazyLock::new(|| Arc::new(Types::new()));

    #[test]
    fn int_key_is_parsed_with_number() {
        let int = TYS.intern(Ty::Base(BaseType::Int));
        assert_eq!(decode_map_key(Some(int), "k", &TYS), "Number(k)");
    }

    #[test]
    fn string_key_passes_through() {
        let string = TYS.intern(Ty::Base(BaseType::String));
        assert_eq!(decode_map_key(Some(string), "k", &TYS), "k");
    }

    #[test]
    fn refined_int_key_is_parsed_with_number() {
        let refined = TYS.intern(Ty::Named {
            name: "UserId".to_string(),
            kind: NamedKind::Refined(BaseType::Int),
            args: Vec::new(),
        });
        assert_eq!(decode_map_key(Some(refined), "k", &TYS), "Number(k)");
    }

    #[test]
    fn opaque_string_key_passes_through() {
        let opaque = TYS.intern(Ty::Named {
            name: "Slug".to_string(),
            kind: NamedKind::Opaque(BaseType::String),
            args: Vec::new(),
        });
        assert_eq!(decode_map_key(Some(opaque), "k", &TYS), "k");
    }

    // #70/#71 review: a non-`String`/`Int` base reaching here means the
    // value-keyable set widened elsewhere (`resolver.rs`/`validate.rs`)
    // without teaching this decode the new type's wire shape — it must fail
    // loudly, not silently emit a wrong pass-through.
    #[test]
    #[should_panic(expected = "is not value-keyable")]
    fn a_widened_non_string_int_base_panics_instead_of_silently_passing_through() {
        let b = TYS.intern(Ty::Base(BaseType::Bool));
        decode_map_key(Some(b), "k", &TYS);
    }
}

fn lower_lambda(e: &Expr, lambda: &LambdaExpr, cx: &mut LowerCtx) -> String {
    let tys = cx.commons().tys();
    let is_async = matches!(
        cx.commons().expr_ty(e.id).as_deref(),
        Some(bynk_check::checker::Ty::Fn { ret, .. }) if ret.is_effect(tys)
    );
    let prefix = if is_async { "async " } else { "" };
    let params: Vec<String> = lambda
        .params
        .iter()
        .map(|p| match &p.type_ref {
            Some(tr) => format!("{}: {}", ts_ident(&p.name.name), ts_type_ref(tr)),
            None => ts_ident(&p.name.name),
        })
        .collect();
    let params = params.join(", ");
    // #908: a param's name is the lambda body's own binding — push a frame and
    // register each so a body read resolves to the param rather than an
    // outer `let` rename, popped once the body is lowered so the
    // registration doesn't leak past the lambda.
    cx.shadow_scopes.push(HashMap::new());
    for p in &lambda.params {
        cx.declare_binder(&p.name.name);
    }
    // v0.154 (ADR 0178): a lambda is its own return scope — a `return` in its
    // body (from a `?`) exits the arrow, not the enclosing function. Rebind
    // `return_ty` to the lambda's own return (its `Fn` type's `ret`) so an
    // embedding `?` anywhere in the body — block-bodied or expression-bodied —
    // uses that, not the outer one; restore after either form.
    let lam_ret = match cx.commons().expr_ty(e.id).as_deref() {
        Some(bynk_check::checker::Ty::Fn { ret, .. }) => Some(*ret),
        _ => None,
    };
    let saved = cx.return_ty.take();
    cx.return_ty = lam_ret;
    let result = match &lambda.body.kind {
        ExprKind::Block(b) => {
            let mut out = format!("{prefix}({params}) => {{\n");
            // #4 review: `out` is a local buffer spliced into the caller's
            // output later — see `without_source_map`.
            cx.without_source_map(|cx| {
                emit_block_as_function_body(&mut out, b, cx, INDENT_STEP * 2, is_async)
            });
            for _ in 0..INDENT_STEP {
                out.push(' ');
            }
            out.push('}');
            out
        }
        _ => {
            let lowered = lower_expr(&lambda.body, cx);
            let body_stmts = lowered.pre;
            let body = lowered.expr;
            if body_stmts.is_empty() {
                // An object-literal body (a record construction/spread) must be
                // parenthesised, or `(x) => { … }` reads as a block, not an object.
                let body = if body.trim_start().starts_with('{') {
                    format!("({body})")
                } else {
                    body
                };
                format!("{prefix}({params}) => {body}")
            } else {
                let mut out = format!("{prefix}({params}) => {{\n");
                for s in &body_stmts {
                    out.push_str(s);
                    out.push('\n');
                }
                out.push_str(&format!("  return {body};\n}}"));
                out
            }
        }
    };
    cx.return_ty = saved;
    cx.shadow_scopes.pop();
    result
}

fn lower_record_spread(base: &Expr, overrides: &[FieldInit], cx: &mut LowerCtx) -> Lowered {
    let mut pre = Pre::new();
    let base_expr = pre.lower(base, cx);
    let mut parts = vec![format!("...{base_expr}")];
    for f in overrides {
        match &f.value {
            Some(v) => {
                let val = pre.lower(v, cx);
                parts.push(format!("{}: {}", f.name.name, val));
            }
            None => {
                // Shorthand `{ x }` references the binding `x`; if the binding
                // was renamed (a reserved word), expand to `x: __id_x` — the
                // key is wire format and must keep the source spelling.
                let v = ts_ident(&f.name.name);
                if v == f.name.name {
                    parts.push(f.name.name.clone());
                } else {
                    parts.push(format!("{}: {v}", f.name.name));
                }
            }
        }
    }
    pre.finish(format!("{{ {} }}", parts.join(", ")))
}

fn lower_block_as_expr(b: &Block, cx: &mut LowerCtx) -> String {
    let mut iife = String::new();
    iife.push_str("(() => {\n");
    // IIFE is a synchronous arrow function; the surrounding expression context
    // expects a concrete value, so `Effect.pure(...)` must still wrap as
    // `Promise.resolve(...)`.
    // v0.154 (ADR 0178): a `return` here exits the arrow, not the function, so
    // clear `return_ty` — an embedding `?` behaves like a plain `?`.
    let saved = cx.return_ty.take();
    // #4 review: `iife` is a local buffer spliced into the caller's output
    // later — see `without_source_map`.
    cx.without_source_map(|cx| {
        emit_block_as_function_body(&mut iife, b, cx, INDENT_STEP * 2, false)
    });
    cx.return_ty = saved;
    for _ in 0..INDENT_STEP {
        iife.push(' ');
    }
    iife.push_str("})()");
    iife
}

/// #1 review: the discriminant's hoisted statements (`pre`) are the
/// caller's own — pushed straight into whatever scope is lowering this
/// `match` expression, exactly like every other `ExprKind` arm in
/// `lower_expr_into`. This used to lower the discriminant into a *local*,
/// throwaway vector and — when non-empty — splice it into a freshly
/// synthesised wrapper arrow via `(() => { ...stmts; return inner; })()`. A
/// `?` in the discriminant hoists `if (__rN.tag === "Err") return __rN;`; a
/// `return` inside that wrapper arrow exits the wrapper, not the enclosing
/// function, so the early return's `Err` silently became the whole match
/// expression's *value* instead of propagating out — a miscompile with no
/// diagnostic and no assertion, unlike its two ternary-path siblings (see
/// `lower_if`/`lower_tail_expr`, which at least `debug_assert!`ed the
/// analogous case before this same class of fix).
fn lower_match_as_iife(discriminant: &Expr, arms: &[MatchArm], cx: &mut LowerCtx) -> Lowered {
    let mut pre = Pre::new();
    let disc_ty = cx
        .commons()
        .expr_types
        .get(&discriminant.id)
        .map(|te| te.ty);
    let disc = pre.lower(discriminant, cx);
    // v0.154 (ADR 0178): a value-position `match` lowers to an IIFE, so a
    // `return` in an arm exits the arrow, not the function — clear `return_ty`
    // so an embedding `?` behaves like a plain `?` here (no function-level wrap).
    let saved = cx.return_ty.take();
    // T2.2 (R6.4): isolate the effectfulness flag to just this IIFE's own
    // body — `disc` was already lowered above, so any await it needed landed
    // in `pre`, not here. Restore as `saved_await || needs_async` so an
    // `await (async …)()` this call produces (if it does) still marks the
    // *caller's* scope — the propagation the old text scan got for free from
    // the substring surviving in the returned string.
    let saved_await = std::mem::take(&mut cx.emitted_await);
    // #4 review: `build_match_iife` accumulates into its own local buffer,
    // spliced into the caller's output later — see `without_source_map`.
    let built = cx.without_source_map(|cx| build_match_iife(&disc, &disc_ty, arms, cx));
    let needs_async = cx.emitted_await;
    cx.emitted_await = saved_await || needs_async;
    let inner_iife = finish_async_iife(built, needs_async);
    cx.return_ty = saved;
    pre.finish(inner_iife)
}

/// v0.9.2: a match/if lowered to an IIFE in *expression* position may have arms
/// that `await` (an effectful `let x <- …` or `do …`). A synchronous arrow can't
/// host `await`, so when `needs_async` says the body just constructed emitted one
/// (T2.2, R6.4: a flag set at the two statement sites that emit a literal
/// `await`, read by the caller — see `LowerCtx::emitted_await` — not a scan of
/// `iife` for the substring `"await "`), make the outermost arrow `async` and
/// `await` its call. A nested match/if that already did this leaves its own
/// `await (async …)()` in the body text *and* restores the flag on `cx` before
/// returning, so the caller's own read-and-reset still sees it and the
/// transform still propagates outward.
fn finish_async_iife(iife: String, needs_async: bool) -> String {
    if !needs_async {
        return iife;
    }
    let async_iife = if let Some(rest) = iife.strip_prefix("((__d) =>") {
        format!("(async (__d) =>{rest}")
    } else if let Some(rest) = iife.strip_prefix("(() => {") {
        format!("(async () => {{{rest}")
    } else {
        // Defensive: unreachable given the two current callers, which always
        // build one of the two headers above. `needs_async` is authoritative
        // that `iife` holds a literal `await`, so falling through here emits a
        // bare `await` in a synchronous arrow — a hard `SyntaxError`, not a
        // missed optimisation. Fail here instead of at `tsc`.
        debug_assert!(false, "finish_async_iife: unrecognised IIFE header: {iife}");
        return iife;
    };
    format!("await {async_iife}")
}

fn build_match_iife(
    disc_expr: &str,
    disc_ty: &Option<TyId>,
    arms: &[MatchArm],
    cx: &mut LowerCtx,
) -> String {
    let tys = cx.commons().tys();
    let mut out = String::new();
    out.push_str("((__d) => {\n");
    // ADR 0169: nested/guarded matches lower to an if-chain (which emits its own
    // per-arm bodies and trailing `throw`); flat/unguarded matches keep the switch.
    if match_needs_if_chain(arms) {
        emit_match_if_chain(&mut out, "__d", disc_ty, arms, cx, INDENT_STEP * 2, false);
    } else {
        for _ in 0..(INDENT_STEP * 2) {
            out.push(' ');
        }
        // v0.130: literal-kind matches switch on the value; variant-kind on `.tag`.
        let scrutinee = if is_literal_match(disc_ty, tys) {
            "__d"
        } else {
            "__d.tag"
        };
        out.push_str(&format!("switch ({scrutinee}) {{\n"));
        for arm in arms {
            // IIFE form (non-tail match expression): `Effect.pure(...)` must keep
            // its `Promise.resolve` wrapper because the IIFE is a synchronous arrow.
            emit_match_case(&mut out, "__d", disc_ty, arm, cx, INDENT_STEP * 3, false);
        }
        for _ in 0..(INDENT_STEP * 2) {
            out.push(' ');
        }
        out.push_str("}\n");
        for _ in 0..(INDENT_STEP * 2) {
            out.push(' ');
        }
        out.push_str("throw new Error(\"non-exhaustive match\");\n");
    }
    for _ in 0..INDENT_STEP {
        out.push(' ');
    }
    out.push_str(&format!("}})({disc_expr})"));
    out
}

/// v0.130: whether a `match` is literal-kind — its scrutinee is a primitive
/// `Int`/`String`/`Bool` (or a refinement over one), so arms are literal
/// patterns and the emitter switches on the *value* itself, not on a `.tag`
/// discriminant. Mirrors the checker's `literal_base_of` classification.
fn is_literal_match(disc_ty: &Option<TyId>, tys: &Arc<Types>) -> bool {
    match disc_ty.map(|t| tys.get(t)).as_deref() {
        Some(Ty::Base(b)) => matches!(b, BaseType::Int | BaseType::String | BaseType::Bool),
        Some(Ty::Named {
            kind: NamedKind::Refined(b),
            ..
        }) => matches!(b, BaseType::Int | BaseType::String | BaseType::Bool),
        _ => false,
    }
}

/// v0.130: render a literal pattern's value as a JS `switch` case label.
fn literal_case_label(value: &LiteralValue) -> String {
    match value {
        LiteralValue::Int(n) => n.to_string(),
        LiteralValue::Str(s) => format!("\"{}\"", escape_ts_string(s)),
        LiteralValue::Bool(b) => b.to_string(),
    }
}

/// #472: the `Int`/`String`/`Bool` base of a literal-kind type, or `None` for
/// anything else. Mirrors the checker's `literal_base_of` (and the
/// per-variant logic in [`is_literal_match`]) — used to feed
/// [`refined_check_as_bool`] the base a `_ where predicate` pattern's
/// scrutinee actually has.
fn literal_base_of_ty(ty: TyId, tys: &Arc<Types>) -> Option<BaseType> {
    let base = match &*tys.get(ty) {
        Ty::Base(b) => *b,
        Ty::Named {
            kind: NamedKind::Refined(b),
            ..
        } => *b,
        _ => return None,
    };
    matches!(base, BaseType::Int | BaseType::String | BaseType::Bool).then_some(base)
}

/// Whether an expression lowers to a stable reference TypeScript can narrow
/// across a `switch` (a variable or a property path rooted in one). Calls and
/// other computed expressions are not narrowable and must be bound to a temp.
fn is_narrowable_path(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Ident(_) => true,
        ExprKind::FieldAccess { receiver, .. } => is_narrowable_path(receiver),
        ExprKind::Paren(inner) => is_narrowable_path(inner),
        _ => false,
    }
}

fn emit_match_tail(
    out: &mut String,
    discriminant: &Expr,
    arms: &[MatchArm],
    cx: &mut LowerCtx,
    indent: usize,
    async_tail: bool,
) {
    let tys = cx.commons().tys();
    // Anchor the discriminant lowering + `switch (…) {` header to the match's
    // scrutinee span (slice 1); each arm re-anchors to its own span below.
    cx.record_span(out.len(), discriminant.span);
    let mut pre = Pre::new();
    let mut disc = pre.lower(discriminant, cx);
    let disc_ty = cx
        .commons()
        .expr_types
        .get(&discriminant.id)
        .map(|te| te.ty);
    for s in pre.stmts() {
        write_line(out, indent, s);
    }
    // v0.9.2: a statement-position `switch` narrows the scrutinee only when it
    // is a stable reference (a variable or property path). A call discriminant
    // such as `ShortCode.of(raw)` is re-evaluated per arm and TypeScript cannot
    // narrow it (and re-evaluation could repeat side effects), so bind it to a
    // fresh temp once and switch on that.
    if !is_narrowable_path(discriminant) {
        let tmp = cx.fresh();
        write_line(out, indent, &format!("const {tmp} = {disc};"));
        disc = tmp;
    }
    // ADR 0169: nested/guarded matches lower to an if-chain; flat/unguarded
    // matches keep the `switch`.
    if match_needs_if_chain(arms) {
        emit_match_if_chain(out, &disc, &disc_ty, arms, cx, indent, async_tail);
        return;
    }
    // v0.130: literal-kind matches switch on the value; variant-kind on `.tag`.
    let scrutinee = if is_literal_match(&disc_ty, tys) {
        disc.clone()
    } else {
        format!("{disc}.tag")
    };
    write_line(out, indent, &format!("switch ({scrutinee}) {{"));
    for arm in arms {
        emit_match_case(
            out,
            &disc,
            &disc_ty,
            arm,
            cx,
            indent + INDENT_STEP,
            async_tail,
        );
    }
    write_line(out, indent, "}");
    write_line(out, indent, "throw new Error(\"non-exhaustive match\");");
}

fn emit_match_case(
    out: &mut String,
    disc_var: &str,
    disc_ty: &Option<TyId>,
    arm: &MatchArm,
    cx: &mut LowerCtx,
    indent: usize,
    async_tail: bool,
) {
    let tys = cx.commons().tys();
    // Anchor this arm's `case`/binding/`return` lines to the arm's source span
    // (slice 1, ADR 0103 D2) — so stepping a `match` walks arm-to-arm.
    cx.record_span(out.len(), arm.span);
    // #908: this arm's payload/catch-all bindings live in the `case`'s own JS
    // block, so a read inside the body must resolve to them rather than an
    // outer `let` rename — push a frame and register each below, popped once
    // the whole arm (bindings + body) is emitted.
    cx.shadow_scopes.push(HashMap::new());
    match &arm.pattern {
        Pattern::Wildcard(_) => {
            write_line(out, indent, "default: {");
            emit_match_body(out, &arm.body, cx, indent + INDENT_STEP, async_tail);
            write_line(out, indent, "}");
        }
        // A bare name binding is a catch-all that binds the scrutinee (ADR 0169).
        Pattern::Binding(id) => {
            write_line(out, indent, "default: {");
            write_line(
                out,
                indent + INDENT_STEP,
                &format!("const {} = {disc_var};", ts_ident(&id.name)),
            );
            cx.declare_binder(&id.name);
            emit_match_body(out, &arm.body, cx, indent + INDENT_STEP, async_tail);
            write_line(out, indent, "}");
        }
        // v0.130: a literal pattern lowers to `case <literal>:`. JS `switch`
        // compares with `===`, matching the value semantics we want for
        // `Int`/`String`/`Bool`.
        Pattern::Literal { value, .. } => {
            write_line(
                out,
                indent,
                &format!("case {}: {{", literal_case_label(value)),
            );
            emit_match_body(out, &arm.body, cx, indent + INDENT_STEP, async_tail);
            write_line(out, indent, "}");
        }
        // #472: a refined arm always trips `match_needs_if_chain`, so the
        // switch path (this function) never sees one in practice — a
        // predicate has no `case`-label lowering, unlike a literal or a tag.
        Pattern::Refined { .. } => {
            unreachable!("a Pattern::Refined arm always routes through emit_match_if_chain")
        }
        Pattern::Variant {
            variant, bindings, ..
        } => {
            write_line(
                out,
                indent,
                &format!("case \"{tag}\": {{", tag = variant.name),
            );
            // The switch path only handles flat, unguarded arms (nested/guarded
            // matches take the if-chain path), so each payload binding is an
            // irrefutable name or `_`.
            for (i, b) in bindings.iter().enumerate() {
                let Pattern::Binding(name) = b.pattern() else {
                    continue;
                };
                let field = match &b.kind {
                    PatternBindingKind::Named { field, .. } => field.name.clone(),
                    PatternBindingKind::Positional { .. } => {
                        cx.positional_field_name(*disc_ty, &variant.name, i, tys)
                    }
                };
                let local = ts_ident(&name.name);
                write_line(
                    out,
                    indent + INDENT_STEP,
                    &format!("const {local} = {disc_var}.{field};"),
                );
                cx.declare_binder(&name.name);
            }
            emit_match_body(out, &arm.body, cx, indent + INDENT_STEP, async_tail);
            write_line(out, indent, "}");
        }
        // #474: a bindingless or-pattern (guaranteed by `match_needs_if_chain`
        // routing — a bound/guarded one takes the if-chain path instead) lowers
        // to one fall-through `case` label per alternative, sharing one body —
        // *unless* an alternative is `_`/a bare binding (#825 review): that
        // alone already matches everything (`Pending | _` is exactly as
        // irrefutable as a bare `_`, per `Pattern::is_irrefutable`), and JS
        // `switch` has no `case _:` form, only `default:`. Emitting case
        // labels for the other (now-redundant) alternatives and dropping the
        // wildcard produced malformed, `tsc`-rejected output.
        Pattern::Or(alts, _) if arm.pattern.is_irrefutable() => {
            write_line(out, indent, "default: {");
            emit_match_body(out, &arm.body, cx, indent + INDENT_STEP, async_tail);
            write_line(out, indent, "}");
        }
        Pattern::Or(alts, _) => {
            let last = alts.len() - 1;
            for (i, alt) in alts.iter().enumerate() {
                let label = match alt {
                    Pattern::Literal { value, .. } => literal_case_label(value),
                    Pattern::Variant { variant, .. } => format!("\"{}\"", variant.name),
                    Pattern::Wildcard(_) | Pattern::Binding(_) => unreachable!(
                        "pattern.is_irrefutable() is false, so no alternative is a wildcard or binding"
                    ),
                    Pattern::Or(..) => unreachable!("an Or's alternatives are always leaves"),
                    // #472/#474: the parser only ever wraps a *whole*,
                    // already-folded `|`-chain in `Refined` (never the other
                    // way — see `parse_pattern_where_suffix`), so an `Or`'s
                    // alternatives (built from `parse_pattern_base`) can never
                    // themselves be `Refined`.
                    Pattern::Refined { .. } => {
                        unreachable!("an Or's alternatives are never a Refined pattern")
                    }
                };
                if i == last {
                    write_line(out, indent, &format!("case {label}: {{"));
                } else {
                    write_line(out, indent, &format!("case {label}:"));
                }
            }
            emit_match_body(out, &arm.body, cx, indent + INDENT_STEP, async_tail);
            write_line(out, indent, "}");
        }
    }
    cx.shadow_scopes.pop();
}

fn emit_match_body(
    out: &mut String,
    body: &MatchBody,
    cx: &mut LowerCtx,
    indent: usize,
    async_tail: bool,
) {
    match body {
        MatchBody::Expr(e) => {
            let mut pre = Pre::new();
            let v = pre.absorb(lower_tail_expr(e, cx, async_tail));
            for s in pre.stmts() {
                write_line(out, indent, s);
            }
            write_line(out, indent, &format!("return {v};"));
        }
        MatchBody::Block(b) => emit_block_as_function_body(out, b, cx, indent, async_tail),
    }
}

/// A match needs the if/else-if lowering (ADR 0169) when any arm carries a guard
/// or a refutable nested payload pattern — a JS `switch` on `.tag` can express
/// neither. Flat, unguarded matches keep the `switch` (zero churn to existing
/// output).
///
/// `pub(crate)` since P6.5 (#1159, Decision B) — `bynk-emit::ir::lower`
/// reuses this pure predicate verbatim to decide `MatchForm`, rather than
/// re-deriving an equivalent one over `IrPat`'s own shape, so the string
/// emitter's own if-chain-vs-switch choice and the IR's own recorded `form`
/// can never silently disagree.
pub(crate) fn match_needs_if_chain(arms: &[MatchArm]) -> bool {
    arms.iter().any(|a| {
        a.guard.is_some()
            || pattern_has_nested_test(&a.pattern)
            || matches!(a.pattern, Pattern::Refined { .. })
    })
}

/// True when `pat` carries a payload sub-pattern that is itself refutable (a
/// nested variant/literal) — i.e. it cannot be tested by a single `.tag` switch.
fn pattern_has_nested_test(pat: &Pattern) -> bool {
    match pat {
        Pattern::Variant { bindings, .. } => bindings.iter().any(|b| {
            let sp = b.pattern();
            !sp.is_irrefutable() || pattern_has_nested_test(sp)
        }),
        // #472: a refined pattern is never a top-level irrefutable/nested
        // payload today (the parser admits only `_` as `inner`, and only at a
        // match arm's top level), but keep this exhaustive and correct for
        // when nesting is admitted. (An `Or`'s alternatives can never
        // themselves be `Refined` — #472/#474's merged parser design only
        // ever wraps a *whole*, already-folded `|`-chain in `Refined`, never
        // the other way around — so this arm only matters nested under a
        // payload, e.g. a hypothetical `Some(_ where P)`.)
        Pattern::Refined { inner, .. } => !inner.is_irrefutable() || pattern_has_nested_test(inner),
        // #474: a bindingless, non-nested or-pattern (`1 | 2 | 3`,
        // `Pending | Cancelled(_, _)`) stays on the flat switch — it lowers to
        // fall-through `case` labels sharing one body. One with bindings or a
        // nested refutable payload needs the if-chain (a `switch` can't bind
        // different alternatives' fields to the same name).
        Pattern::Or(alts, _) => alts
            .iter()
            .any(|p| !p.bound_names().is_empty() || pattern_has_nested_test(p)),
        _ => false,
    }
}

/// Emit the boolean tests that must hold for `pattern` to match the value at
/// runtime `path` (static type `path_ty`). Irrefutable patterns add nothing;
/// nested variant/literal payloads recurse into `path.<field>`.
fn pattern_match_tests(
    path: &str,
    path_ty: Option<TyId>,
    pattern: &Pattern,
    cx: &LowerCtx,
    tests: &mut Vec<String>,
) {
    let tys = cx.commons().tys();
    match pattern {
        Pattern::Wildcard(_) | Pattern::Binding(_) => {}
        Pattern::Literal { value, .. } => {
            tests.push(format!("{path} === {}", literal_case_label(value)));
        }
        // #472: `p where predicate` — the inner pattern's own tests, then the
        // predicate as a boolean guard over the runtime value at `path`,
        // reusing `refined_check_as_bool` verbatim (the same helper `value is
        // RefinedType` lowers through).
        Pattern::Refined {
            inner, predicate, ..
        } => {
            pattern_match_tests(path, path_ty, inner, cx, tests);
            // The checker (`check_pattern`) already rejected any scrutinee
            // that isn't literal-kind before this point, so `path_ty` is
            // always `Int` or `String` here for a well-typed program — a
            // silent fallback would mask a real bug (wrong predicate codegen
            // against the wrong base) rather than surfacing it loudly, the
            // same posture as `emit_match_case`'s `Pattern::Refined` arm.
            let base = path_ty
                .and_then(|t| literal_base_of_ty(t, tys))
                .unwrap_or_else(|| {
                    panic!(
                        "a refined pattern's scrutinee must be literal-kind (Int/String), got {:?}",
                        path_ty
                    )
                });
            tests.push(refined_check_as_bool(path, base, Some(predicate)));
        }
        Pattern::Variant {
            variant, bindings, ..
        } => {
            tests.push(format!("{path}.tag === \"{}\"", variant.name));
            for (i, b) in bindings.iter().enumerate() {
                let sp = b.pattern();
                if sp.is_irrefutable() {
                    continue;
                }
                let field = match &b.kind {
                    PatternBindingKind::Named { field, .. } => field.name.clone(),
                    PatternBindingKind::Positional { .. } => {
                        cx.positional_field_name(path_ty, &variant.name, i, tys)
                    }
                };
                let field_ty = cx.payload_field_ty(path_ty, &variant.name, i, tys);
                pattern_match_tests(&format!("{path}.{field}"), field_ty, sp, cx, tests);
            }
        }
        // #474: an or-pattern matches `path` when any alternative does — AND
        // each alternative's own tests, then OR the alternatives together, and
        // push the whole disjunction as a single combined test (so a caller
        // AND-joining `tests` composes it correctly with sibling tests, e.g. a
        // nested `Some(1 | 2)`'s outer `.tag === "Some"` test).
        Pattern::Or(alts, _) => {
            let terms: Vec<String> = alts
                .iter()
                .map(|alt| {
                    let mut t = Vec::new();
                    pattern_match_tests(path, path_ty, alt, cx, &mut t);
                    if t.is_empty() {
                        "true".to_string()
                    } else {
                        t.join(" && ")
                    }
                })
                .collect();
            tests.push(format!("({})", terms.join(" || ")));
        }
    }
}

/// #1187's slice 5 (the `Service` emitter cutover): the JS boolean guard for
/// a `from Events(E { field: value, .. })` subscription filter, AND-joining
/// one test per listed field — reads [`crate::ir::EventPatternIr`] (already
/// resolved by [`crate::ir::lower::lower_protocol_ir`]) rather than the raw
/// AST `EventPattern`; [`EventPatternValueIr`]'s own doc comment already
/// named this function as its sole intended consumer, never wired up until
/// now (the AST-driven original this replaces produced byte-identical guard
/// text: `EventPatternValueIr::Const` only ever holds the same closed
/// `Int`/`Str`/`Bool` set `literal_case_label` covers, and `Variant { tag }`
/// already carries exactly the bare, unqualified tag the AST version's
/// `variant.name` destructured down to). No `Ty`/`LowerCtx` needed:
/// `check_event_pattern` (`bynk-check/src/project_model.rs`) has already
/// proven every field exists and every value type-checks, so this is total
/// and cannot panic. Returns `None` for a pattern-less subscription (nothing
/// to guard).
pub(crate) fn event_pattern_guard_ir(
    path: &str,
    pattern: Option<&EventPatternIr>,
) -> Option<String> {
    let pattern = pattern?;
    let tests: Vec<String> = pattern
        .fields
        .iter()
        .map(|(name, value)| match value {
            EventPatternValueIr::Const(ConstVal::Int(n)) => format!("{path}.{name} === {n}"),
            EventPatternValueIr::Const(ConstVal::Str(s)) => {
                format!("{path}.{name} === \"{}\"", escape_ts_string(s))
            }
            EventPatternValueIr::Const(ConstVal::Bool(b)) => format!("{path}.{name} === {b}"),
            EventPatternValueIr::Const(
                ConstVal::Float(_) | ConstVal::DurationMillis(_) | ConstVal::Unit,
            ) => {
                unreachable!(
                    "EventPatternValueIr::Const only ever holds Int/Str/Bool (its own doc \
                     comment) — the checker's own check_event_pattern already restricted a \
                     from Events(...) filter to that closed set before this pattern could exist"
                )
            }
            EventPatternValueIr::Variant { tag } => format!("{path}.{name}.tag === \"{tag}\""),
        })
        .collect();
    Some(tests.join(" && "))
}

#[cfg(test)]
mod event_pattern_guard_ir_tests {
    use super::*;

    /// Review of #1198: the byte-identity evidence against the deleted
    /// AST-driven `event_pattern_guard` was entirely fixture-level. This
    /// pins the guard text format directly, over a hand-built
    /// `EventPatternIr` covering all four cases `EventPatternValueIr` can
    /// hold (`Int`/`Str`/`Bool`/`Variant`).
    #[test]
    fn covers_every_value_shape() {
        let pattern = EventPatternIr {
            fields: vec![
                (
                    "n".to_string(),
                    EventPatternValueIr::Const(ConstVal::Int(3)),
                ),
                (
                    "s".to_string(),
                    EventPatternValueIr::Const(ConstVal::Str("a\"b".to_string())),
                ),
                (
                    "b".to_string(),
                    EventPatternValueIr::Const(ConstVal::Bool(true)),
                ),
                (
                    "status".to_string(),
                    EventPatternValueIr::Variant {
                        tag: "Active".to_string(),
                    },
                ),
            ],
        };
        assert_eq!(
            event_pattern_guard_ir("e", Some(&pattern)),
            Some(
                "e.n === 3 && e.s === \"a\\\"b\" && e.b === true && e.status.tag === \"Active\""
                    .to_string()
            )
        );
        assert_eq!(
            event_pattern_guard_ir("e", None),
            None,
            "a pattern-less subscription has nothing to guard"
        );
    }
}

/// Emit `const` declarations binding the names in `pattern` from runtime `path`,
/// recursing through nested payloads (ADR 0169).
fn emit_pattern_bindings(
    out: &mut String,
    indent: usize,
    path: &str,
    path_ty: Option<TyId>,
    pattern: &Pattern,
    cx: &mut LowerCtx,
) {
    let tys = cx.commons().tys();
    match pattern {
        Pattern::Wildcard(_) | Pattern::Literal { .. } => {}
        Pattern::Binding(id) => {
            write_line(
                out,
                indent,
                &format!("const {} = {path};", ts_ident(&id.name)),
            );
            cx.declare_binder(&id.name);
        }
        // #472: a refined pattern binds only what its inner pattern binds
        // (`_ where P` binds nothing today).
        Pattern::Refined { inner, .. } => {
            emit_pattern_bindings(out, indent, path, path_ty, inner, cx);
        }
        Pattern::Variant {
            variant, bindings, ..
        } => {
            for (i, b) in bindings.iter().enumerate() {
                let field = match &b.kind {
                    PatternBindingKind::Named { field, .. } => field.name.clone(),
                    PatternBindingKind::Positional { .. } => {
                        cx.positional_field_name(path_ty, &variant.name, i, tys)
                    }
                };
                let field_ty = cx.payload_field_ty(path_ty, &variant.name, i, tys);
                emit_pattern_bindings(
                    out,
                    indent,
                    &format!("{path}.{field}"),
                    field_ty,
                    b.pattern(),
                    cx,
                );
            }
        }
        // #474: different alternatives can bind the same name at different
        // structural paths (`Held`'s field 2 vs `Confirmed`'s field 4), so a
        // single `const` can't express it. Declare each shared name once with
        // `let`, then dispatch per alternative — only the (small) binding
        // resolution is duplicated per alternative, not the arm body that
        // follows. Alternatives are mutually exclusive at runtime (a value has
        // exactly one tag, or a literal one value), so a plain if/else-if/else
        // chain is exhaustive over them once the caller's own combined test
        // (built by `pattern_match_tests`) has already passed.
        Pattern::Or(alts, _) => {
            let names = pattern.bound_names();
            if names.is_empty() {
                return;
            }
            let decl: Vec<String> = names.iter().map(|id| ts_ident(&id.name)).collect();
            write_line(out, indent, &format!("let {};", decl.join(", ")));
            for id in &names {
                cx.declare_binder(&id.name);
            }
            let last = alts.len() - 1;
            for (i, alt) in alts.iter().enumerate() {
                if i == last {
                    write_line(out, indent, "} else {");
                } else {
                    let mut t = Vec::new();
                    pattern_match_tests(path, path_ty, alt, cx, &mut t);
                    let cond = if t.is_empty() {
                        "true".to_string()
                    } else {
                        t.join(" && ")
                    };
                    let kw = if i == 0 { "if" } else { "} else if" };
                    write_line(out, indent, &format!("{kw} ({cond}) {{"));
                }
                let mut pairs = Vec::new();
                pattern_binding_paths(path, path_ty, alt, cx, &mut pairs);
                for (name, p) in &pairs {
                    write_line(
                        out,
                        indent + INDENT_STEP,
                        &format!("{} = {p};", ts_ident(name)),
                    );
                }
            }
            write_line(out, indent, "}");
        }
    }
}

/// Collect `(name, runtime-path)` pairs for the names `pattern` binds from
/// `path`, recursing through nested payloads — the same traversal as
/// `emit_pattern_bindings`, but returning the resolved paths instead of
/// writing `const` declarations, so an or-pattern's per-alternative dispatch
/// (above) can assign a shared `let` from whichever alternative matched.
fn pattern_binding_paths(
    path: &str,
    path_ty: Option<TyId>,
    pattern: &Pattern,
    cx: &LowerCtx,
    out: &mut Vec<(String, String)>,
) {
    let tys = cx.commons().tys();
    match pattern {
        Pattern::Wildcard(_) | Pattern::Literal { .. } => {}
        Pattern::Binding(id) => out.push((id.name.clone(), path.to_string())),
        Pattern::Variant {
            variant, bindings, ..
        } => {
            for (i, b) in bindings.iter().enumerate() {
                let field = match &b.kind {
                    PatternBindingKind::Named { field, .. } => field.name.clone(),
                    PatternBindingKind::Positional { .. } => {
                        cx.positional_field_name(path_ty, &variant.name, i, tys)
                    }
                };
                let field_ty = cx.payload_field_ty(path_ty, &variant.name, i, tys);
                pattern_binding_paths(&format!("{path}.{field}"), field_ty, b.pattern(), cx, out);
            }
        }
        // Never reached from the parser (an `Or`'s alternatives are always
        // leaves), but recurse into the first alternative defensively rather
        // than panic, mirroring `gather_pattern_bindings`'s checker-side choice.
        Pattern::Or(alts, _) => {
            if let Some(first) = alts.first() {
                pattern_binding_paths(path, path_ty, first, cx, out);
            }
        }
        // #472: binds only what its inner pattern binds (`_ where P` binds
        // nothing today) — same recursion as `emit_pattern_bindings`'s
        // `Refined` arm.
        Pattern::Refined { inner, .. } => pattern_binding_paths(path, path_ty, inner, cx, out),
    }
}

/// Lower a nested/guarded match to a sequence of independent `if` blocks
/// (ADR 0169, DECISION E). Each arm tests its structural pattern; on a match it
/// binds names, then (if guarded) tests the guard, then runs its body — whose
/// tail `return` short-circuits the remaining arms. A guard failing falls
/// through to the next arm, which a `switch` cannot express. Per-arm span
/// anchoring is preserved (ADR 0103).
fn emit_match_if_chain(
    out: &mut String,
    disc_var: &str,
    disc_ty: &Option<TyId>,
    arms: &[MatchArm],
    cx: &mut LowerCtx,
    indent: usize,
    async_tail: bool,
) {
    // An unguarded irrefutable arm is a catch-all: its body always returns, so
    // the trailing non-exhaustive `throw` would be unreachable (tsc rejects it).
    let has_catchall = arms
        .iter()
        .any(|a| a.guard.is_none() && a.pattern.is_irrefutable());
    for arm in arms {
        cx.record_span(out.len(), arm.span);
        // #908: this arm's bindings must resolve within its own body rather
        // than falling through to an outer `let` rename — see the identical
        // reasoning in `emit_match_case`. Pushed/popped once per arm
        // regardless of `has_tests` below, since the concern is which
        // binding a read resolves to, not whether the arm also happens to
        // emit a wrapping JS block.
        cx.shadow_scopes.push(HashMap::new());
        let mut tests = Vec::new();
        pattern_match_tests(disc_var, *disc_ty, &arm.pattern, cx, &mut tests);
        let has_tests = !tests.is_empty();
        if has_tests {
            write_line(out, indent, &format!("if ({}) {{", tests.join(" && ")));
        }
        let body_indent = if has_tests {
            indent + INDENT_STEP
        } else {
            indent
        };
        emit_pattern_bindings(out, body_indent, disc_var, *disc_ty, &arm.pattern, cx);
        if let Some(guard) = &arm.guard {
            let guard_lowered = lower_expr(guard, cx);
            let gv = guard_lowered.expr;
            for s in &guard_lowered.pre {
                write_line(out, body_indent, s);
            }
            write_line(out, body_indent, &format!("if ({gv}) {{"));
            emit_match_body(out, &arm.body, cx, body_indent + INDENT_STEP, async_tail);
            write_line(out, body_indent, "}");
        } else {
            emit_match_body(out, &arm.body, cx, body_indent, async_tail);
        }
        if has_tests {
            write_line(out, indent, "}");
        }
        cx.shadow_scopes.pop();
    }
    if !has_catchall {
        write_line(out, indent, "throw new Error(\"non-exhaustive match\");");
    }
}

fn lower_is(value: &Expr, pattern: &Pattern, cx: &mut LowerCtx) -> Lowered {
    let mut pre = Pre::new();
    // v0.13: refinement check — `value is RefinedType` lowers to the refined
    // type's predicates as a boolean expression. The receiver is forced to a
    // temp so the narrowing binding (`const n = <temp> as Quantity`) can shadow
    // the value's name without a TDZ.
    if let Pattern::Variant {
        variant, bindings, ..
    } = pattern
        && bindings.is_empty()
        && cx.is_refined_is_check(value, &variant.name)
        && let Some(TypeBody::Refined {
            base, refinement, ..
        }) = cx
            .commons()
            .types
            .get(&variant.name)
            .map(|d| d.body.clone())
    {
        let recv = pre.absorb(cx.is_receiver_ref_forced(value));
        return pre.finish(refined_check_as_bool(&recv, base, refinement.as_ref()));
    }
    let v = pre.absorb(cx.is_receiver_ref(value));
    let text = match pattern {
        Pattern::Wildcard(_) => "true".to_string(),
        // A bare name binding after `is` matches anything over a sum (ADR 0169).
        Pattern::Binding(_) => "true".to_string(),
        // v0.130 (DECISION F): the checker rejects a literal on the RHS of `is`,
        // so this is unreachable for a well-typed program; lower it to the
        // value-equality it would mean, defensively.
        Pattern::Literal { value, .. } => {
            format!("{v} === {}", literal_case_label(value))
        }
        // #472: the checker rejects a refined pattern on the RHS of `is`
        // (D5), so this is unreachable for a well-typed program; lower it
        // defensively via the same `pattern_match_tests` a `match` arm uses
        // (which already handles `Pattern::Refined` — inner tests plus the
        // predicate via `refined_check_as_bool`).
        Pattern::Refined { .. } => {
            let scrut_ty = cx.commons().expr_types.get(&value.id).map(|te| te.ty);
            let mut tests = Vec::new();
            pattern_match_tests(&v, scrut_ty, pattern, cx, &mut tests);
            if tests.is_empty() {
                "true".to_string()
            } else {
                tests.join(" && ")
            }
        }
        // #705: a variant pattern lowers to its full structural test — the outer
        // tag *and* any nested refutable payload patterns, so
        // `r is Rejected(RefinementViolation(_))` checks both levels (the same
        // tests a `match` arm emits, via `pattern_match_tests`). A plain
        // `is Ok(_)` / `is Rejected(_)` still yields a single `.tag` test (the
        // `_` payload is irrefutable). For a loose scrutinee — the `Wire`-call
        // outcome — the single-field `"value"` fallback matches the runtime
        // `{ tag, value }` shape, so the nested test lands without a static type.
        Pattern::Variant { .. } => {
            let scrut_ty = cx.commons().expr_types.get(&value.id).map(|te| te.ty);
            let mut tests = Vec::new();
            // For a loose scrutinee, a *multi*-payload nested pattern would map
            // every positional field to the single-field `"value"` fallback; that
            // is unreachable today (the only loose variant sums — the `Wire`
            // outcome's rejection kinds — are single-field) and is shared with
            // `match`'s lowering. Precise for any typed scrutinee.
            pattern_match_tests(&v, scrut_ty, pattern, cx, &mut tests);
            // `pattern_match_tests` always pushes the outer `.tag` test for a
            // variant pattern, so a variant `is` is never vacuously `true` — a
            // silently-passing `expect` is exactly the failure this reuse removes.
            debug_assert!(
                !tests.is_empty(),
                "a variant `is` pattern must emit at least its outer tag test"
            );
            if tests.is_empty() {
                "true".to_string()
            } else {
                tests.join(" && ")
            }
        }
        // #474 §2.3.6: an or-pattern is legal after `is`. `pattern_match_tests`
        // already knows how to OR its alternatives together (pushing one
        // combined `(...) || (...)` term), so this is a thin wrapper.
        Pattern::Or(..) => {
            let scrut_ty = cx.commons().expr_types.get(&value.id).map(|te| te.ty);
            let mut tests = Vec::new();
            pattern_match_tests(&v, scrut_ty, pattern, cx, &mut tests);
            tests.join(" && ")
        }
    };
    pre.finish(text)
}

/// v0.13: render a refined type's predicates as a single boolean expression over
/// `recv`, for `value is RefinedType`. Mirrors `emit_pred_check`'s per-predicate
/// logic but as `&&`-joined terms instead of `Result`-returning statements.
fn refined_check_as_bool(recv: &str, base: BaseType, refinement: Option<&Refinement>) -> String {
    let mut terms: Vec<String> = Vec::new();
    if base == BaseType::Int {
        terms.push(format!("Number.isInteger({recv})"));
    }
    // v0.21: validated `Float` values are finite (ADR 0040).
    if base == BaseType::Float {
        terms.push(format!("Number.isFinite({recv})"));
    }
    if let Some(r) = refinement {
        for p in &r.predicates {
            terms.push(match &p.kind {
                PredKind::NonNegative => format!("{recv} >= 0"),
                PredKind::Positive => format!("{recv} > 0"),
                PredKind::InRange(a, b) => {
                    format!("({recv} >= {} && {recv} <= {})", a.value, b.value)
                }
                PredKind::InRangeF(a, b) => {
                    format!("({recv} >= {} && {recv} <= {})", a.lexeme, b.lexeme)
                }
                PredKind::NonEmpty => format!("{recv}.length > 0"),
                PredKind::MinLength(n) => format!("{recv}.length >= {n}"),
                PredKind::MaxLength(n) => format!("{recv}.length <= {n}"),
                PredKind::Length(n) => format!("{recv}.length === {n}"),
                PredKind::Matches(pat) => {
                    let escaped = escape_ts_string(pat);
                    format!("new RegExp(\"^(?:\" + \"{escaped}\" + \")$\").test({recv})")
                }
            });
        }
    }
    if terms.is_empty() {
        "true".to_string()
    } else {
        format!("({})", terms.join(" && "))
    }
}

#[cfg(test)]
mod idempotency_scoping_tests {
    use super::*;
    use bynk_check::checker::TypedCommons;
    use bynk_syntax::ast::{Commons, CommonsForm, QualifiedName};

    fn empty_commons() -> TypedCommons {
        TypedCommons {
            commons: Commons {
                name: QualifiedName {
                    parts: Vec::new(),
                    span: Default::default(),
                },
                items: Vec::new(),
                uses: Vec::new(),
                documentation: None,
                form: CommonsForm::Fragment,
                span: Default::default(),
                trivia: Default::default(),
                trailing_comments: Vec::new(),
            },
            types: HashMap::new(),
            fns: HashMap::new(),
            methods: HashMap::new(),
            expr_types: HashMap::new(),
            callees: HashMap::new(),
            warnings: Vec::new(),
            ty_intern: Arc::new(bynk_check::checker::Types::new()),
            actor_bindings: HashMap::new(),
        }
    }

    /// #934 (Decision C): a first-party `Idempotency.dedup`/`remember` call
    /// reaching `scope_idempotency_key` with no `handler_scope` set means some
    /// emission site populated `capabilities` without also wiring the scope —
    /// a compiler bug. This must fail loudly, not silently ship an unscoped
    /// key; nothing in the fixture suite exercises a missing scope (every real
    /// site sets it), so this is the only proof the guard actually fires.
    #[test]
    #[should_panic(expected = "handler_scope")]
    fn missing_handler_scope_panics() {
        let commons = empty_commons();
        let cross_context = bynk_check::resolver::CrossContextInfo::default();
        let runtime_use = RuntimeUse::default();
        let cx = LowerCtx::new(
            ModuleCtx::new(&commons, &cross_context, &runtime_use),
            BodyMode::Method,
        );
        let mut args = vec!["orderId".to_string()];
        scope_idempotency_key(true, "dedup", &mut args, &cx);
    }

    /// The mirror case: a real scope present, the call proceeds and the key
    /// is prefixed — confirms the panic in the test above is really guarding
    /// the `None` case and not firing unconditionally.
    #[test]
    fn present_handler_scope_prefixes_the_key() {
        let commons = empty_commons();
        let cross_context = bynk_check::resolver::CrossContextInfo::default();
        let runtime_use = RuntimeUse::default();
        let cx = LowerCtx::new(
            ModuleCtx::new(&commons, &cross_context, &runtime_use),
            BodyMode::ServiceHandler {
                handler: HandlerShared {
                    handler_scope: Some("shop.reserve.ordering.call".to_string()),
                    ..HandlerShared::default()
                },
                deps_identity_binder: None,
                actor_sum_binder: None,
            },
        );
        let mut args = vec!["orderId".to_string()];
        scope_idempotency_key(true, "dedup", &mut args, &cx);
        assert_eq!(args[0], "`shop.reserve.ordering.call::${orderId}`");
    }

    /// A capability merely named `"Idempotency"` from some other adapter or
    /// context (not first-party `bynk`) must pass through untouched even with
    /// no scope set — `is_first_party: false` short-circuits before the
    /// `handler_scope` check, so this must NOT panic.
    #[test]
    fn non_first_party_capability_is_not_scoped() {
        let commons = empty_commons();
        let cross_context = bynk_check::resolver::CrossContextInfo::default();
        let runtime_use = RuntimeUse::default();
        let cx = LowerCtx::new(
            ModuleCtx::new(&commons, &cross_context, &runtime_use),
            BodyMode::Method,
        );
        let mut args = vec!["orderId".to_string()];
        scope_idempotency_key(false, "dedup", &mut args, &cx);
        assert_eq!(args[0], "orderId");
    }
}

/// T2.2 (#1018, R6.4): `maybe_async_iife`'s `if !iife.contains("await ")`
/// replaced by `LowerCtx::emitted_await`, a flag set at the two statement
/// sites that emit a literal `await` (`EffectLet`, `Do`) and read-and-reset
/// around a value-position `match`/`if` IIFE's own body construction — see
/// `finish_async_iife`. Same precedent as `conditional_runtime_import_tests`
/// (`body.contains("__bynkBytes")` → `RuntimeUse`): the scan and the flag
/// agree on every case except where the scan's substring match wasn't really
/// about *this* scope's own effectfulness.
#[cfg(test)]
mod async_iife_effectfulness_tests {
    use crate::testkit::emit_source;

    /// The over-match this slice closes: a `Query`/broadcast iterator terminal
    /// (`forEach`) lowers to a self-contained, always-`async () => {...}` IIFE
    /// — a plain `Effect`-typed *value*, not something this arm itself awaits.
    /// The old scan saw the literal `"await "` inside that embedded string and
    /// wrongly wrapped the enclosing `match` arrow as `async` too, producing a
    /// spurious `await await (async (__d) => …)()`. Neither arm needs the
    /// switch's own arrow to be async, so it must stay a plain `(__d) => {…}`,
    /// invoked with a single `await` from the enclosing `let r <- …`.
    #[test]
    fn match_arm_iterator_terminal_does_not_force_the_switch_arrow_async() {
        let ts = emit_source(
            "commons t\n\n\
             fn noop(n: Int) -> Effect[()] {\n  Effect.pure(())\n}\n\n\
             fn run(names: List[Int], flag: Bool) -> Effect[()] {\n  \
             let r <- match flag {\n    \
             true => names.forEach(noop)\n    \
             false => Effect.pure(())\n  \
             }\n  r\n}\n",
        );
        assert!(
            ts.contains("const r = await ((__d) => {"),
            "the switch arrow must stay synchronous, awaited once by the `let <-`: {ts}"
        );
        assert!(!ts.contains("await await"), "{ts}");
    }

    /// The same over-match, reached through the `if`-IIFE path (an `is`-binding
    /// forces the IIFE form rather than a ternary) instead of `match`.
    #[test]
    fn if_arm_iterator_terminal_does_not_force_the_iife_arrow_async() {
        let ts = emit_source(
            "commons t\n\n\
             fn noop(n: Int) -> Effect[()] {\n  Effect.pure(())\n}\n\n\
             fn run(o: Option[Int], names: List[Int]) -> Effect[()] {\n  \
             let r <- if o is Some(n) {\n    \
             names.forEach(noop)\n  \
             } else {\n    \
             Effect.pure(())\n  \
             }\n  r\n}\n",
        );
        assert!(
            ts.contains("const r = await (() => {"),
            "the if-IIFE arrow must stay synchronous, awaited once by the `let <-`: {ts}"
        );
        assert!(!ts.contains("await await"), "{ts}");
    }

    /// Non-regression: the defect `#2 review` fixed in `8068c0db` (a genuinely
    /// effectful `is`-binding `if` in value position) must still emit a real
    /// `async` IIFE — this is not an over-match, so it keeps needing the wrap
    /// that closed the original hard `SyntaxError`.
    #[test]
    fn if_arm_genuine_effect_still_forces_the_iife_arrow_async() {
        let ts = emit_source(
            "commons t\n\n\
             fn fetch(n: Int) -> Effect[Int] {\n  Effect.pure(n)\n}\n\n\
             fn run(o: Option[Int], c: Bool) -> Effect[Int] {\n  \
             let r <- if o is Some(n) && c {\n    \
             let y <- fetch(n)\n    \
             Effect.pure(y + 1)\n  \
             } else {\n    \
             Effect.pure(0)\n  \
             }\n  r\n}\n",
        );
        assert!(
            ts.contains("async () => {") && ts.contains("await fetch(n)"),
            "a real await inside the IIFE must still force the async wrap: {ts}"
        );
    }

    /// Non-regression: a genuinely effectful *nested* `match` (embedded
    /// directly as an outer arm's value, not via an intermediate `let <-`)
    /// must still propagate its own async-wrap need to the enclosing switch
    /// arrow — the flag's `saved || needs_async` restore is what replaces the
    /// old scan's implicit propagation (the substring surviving in the
    /// returned string).
    #[test]
    fn nested_match_effectfulness_propagates_to_the_outer_switch_arrow() {
        let ts = emit_source(
            "commons t\n\n\
             fn fetch(n: Int) -> Effect[Int] {\n  Effect.pure(n)\n}\n\n\
             fn run(a: Bool, b: Bool) -> Effect[Int] {\n  \
             let r <- match a {\n    \
             true => match b {\n      \
             true => {\n        let x <- fetch(1)\n        Effect.pure(x + 1)\n      }\n      \
             false => Effect.pure(2)\n    \
             }\n    \
             false => Effect.pure(3)\n  \
             }\n  r\n}\n",
        );
        assert!(
            ts.contains("const r = await await (async (__d) => {"),
            "the outer switch arrow must also become async: {ts}"
        );
    }

    /// Non-regression, mirroring the `EffectLet` case above for the *other*
    /// write site: an arm whose only effect is a `do` (no binder) must still
    /// force its switch arrow async. The old scan was blind to which
    /// statement produced the `await`, so `Do` came along for free; the flag
    /// has to name it explicitly at its own site (`emit_statement`'s
    /// `Statement::Do` arm) and nothing else pins that it does.
    #[test]
    fn match_arm_do_only_effect_still_forces_the_switch_arrow_async() {
        let ts = emit_source(
            "commons t\n\n\
             fn noop(n: Int) -> Effect[()] {\n  Effect.pure(())\n}\n\n\
             fn run(flag: Bool) -> Effect[()] {\n  \
             let r <- match flag {\n    \
             true => {\n      do noop(1)\n      Effect.pure(())\n    }\n    \
             false => Effect.pure(())\n  \
             }\n  r\n}\n",
        );
        assert!(
            ts.contains("const r = await await (async (__d) => {"),
            "a do-only effectful arm must also force the switch arrow async: {ts}"
        );
    }
}
