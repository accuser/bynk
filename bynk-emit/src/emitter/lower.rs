//! Expression and statement lowering — the `LowerCtx`-driven engine that
//! turns Bynk expressions, statements, matches, and mocks into TypeScript
//! source. Split out of `emitter.rs` (ADR 0060); `LowerCtx` and the `ts_*`
//! type renderers stay in the parent and are reached via `use super::*`.

use std::collections::HashSet;

use bynk_check::checker::{NamedKind, Ty, TypedCommons};

use super::*;

/// Lower a block to a sequence of TypeScript statements suitable for use as
/// an async function body. Used by v0.7 mock-operation emission.
pub fn lower_block_to_async_body(
    block: &Block,
    return_type: &TypeRef,
    typed: &mut TypedCommons,
    cross_context: &bynk_check::resolver::CrossContextInfo,
) -> (String, SourceMapBuilder) {
    let mut out = String::new();
    // v0.70: a sub-builder records body checkpoints relative to this local buffer;
    // the caller merges it into the module map at the splice offset.
    let smb = RefCell::new(SourceMapBuilder::new());
    {
        let mut cx = LowerCtx::new(typed, cross_context).with_source_map(Some(&smb));
        let async_tail = is_effectful_return(return_type);
        emit_block_as_function_body(&mut out, block, &mut cx, 0, async_tail);
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
    test_agents: HashSet<String>,
    source: &str,
    rel_path: &str,
) -> (String, SourceMapBuilder) {
    let mut out = String::new();
    let smb = RefCell::new(SourceMapBuilder::new());
    {
        let mut cx = LowerCtx::new(typed, cross_context).with_source_map(Some(&smb));
        cx.test_services = test_services;
        cx.local_agents = test_agents.clone();
        cx.test_agents = test_agents;
        // v0.117: observations and `trace` in the body read the per-case recorded
        // trace object declared by the runner scaffold.
        cx.observation_trace = Some("__obs".to_string());
        cx.assert_loc = Some(crate::emitter::AssertLoc {
            source: source.to_string(),
            rel_path: rel_path.to_string(),
        });
        for stmt in &block.statements {
            emit_statement(&mut out, stmt, &mut cx, 0);
        }
        // Evaluate the tail expression but discard its value; assertions inside
        // it still take effect via thrown AssertionErrors.
        cx.record_span(out.len(), block.tail.span);
        let mut stmts = Vec::new();
        let tail = lower_expr(&block.tail, &mut stmts, &mut cx);
        for s in &stmts {
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
pub fn lower_integration_case_body(
    block: &Block,
    typed: &mut TypedCommons,
    cross_context: &bynk_check::resolver::CrossContextInfo,
    source: &str,
    rel_path: &str,
) -> (String, SourceMapBuilder) {
    let mut out = String::new();
    let smb = RefCell::new(SourceMapBuilder::new());
    {
        let mut cx = LowerCtx::new(typed, cross_context).with_source_map(Some(&smb));
        cx.target = BuildTarget::Workers;
        cx.assert_loc = Some(crate::emitter::AssertLoc {
            source: source.to_string(),
            rel_path: rel_path.to_string(),
        });
        for stmt in &block.statements {
            emit_statement(&mut out, stmt, &mut cx, 0);
        }
        cx.record_span(out.len(), block.tail.span);
        let mut stmts = Vec::new();
        let tail = lower_expr(&block.tail, &mut stmts, &mut cx);
        for s in &stmts {
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
        } if !both_simple(then_block, else_block) || cond_has_is_bindings(cond, cx) => {
            emit_if_tail(out, cond, then_block, else_block, cx, indent, async_tail);
        }
        _ => {
            let mut stmts = Vec::new();
            let tail = lower_tail_expr(&block.tail, &mut stmts, cx, async_tail);
            for s in &stmts {
                write_line(out, indent, s);
            }
            write_line(out, indent, &format!("return {tail};"));
        }
    }
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
/// In non-async-tail position, defer to `lower_expr` unchanged.
fn lower_tail_expr(
    e: &Expr,
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
    async_tail: bool,
) -> String {
    if !async_tail {
        return lower_expr(e, stmts, cx);
    }
    match &e.kind {
        ExprKind::EffectPure(inner) => lower_expr(inner, stmts, cx),
        ExprKind::Paren(inner) => lower_tail_expr(inner, stmts, cx, true),
        ExprKind::Block(b) if b.statements.is_empty() => lower_tail_expr(&b.tail, stmts, cx, true),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } if both_simple(then_block, else_block) && !cond_has_is_bindings(cond, cx) => {
            let cond_expr = lower_expr(cond, stmts, cx);
            let mut tstmts = Vec::new();
            let testr = lower_tail_expr(&then_block.tail, &mut tstmts, cx, true);
            debug_assert!(tstmts.is_empty());
            let mut estmts = Vec::new();
            let eestr = lower_tail_expr(&else_block.tail, &mut estmts, cx, true);
            debug_assert!(estmts.is_empty());
            format!("({cond_expr} ? {testr} : {eestr})")
        }
        _ => lower_expr(e, stmts, cx),
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
            let mut stmts = Vec::new();
            let value = lower_expr(&l.value, &mut stmts, cx);
            for s in &stmts {
                write_line(out, indent, s);
            }
            let bind_name = if l.name.name == "_" {
                // Emit a unique throwaway local; TS allows `const _ = ...` only
                // once per scope, so use a fresh name to be safe.
                cx.fresh()
            } else {
                ts_ident(&l.name.name)
            };
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
            let mut stmts = Vec::new();
            let value = lower_expr(&l.value, &mut stmts, cx);
            for s in &stmts {
                write_line(out, indent, s);
            }
            let bind_name = if l.name.name == "_" {
                cx.fresh()
            } else {
                ts_ident(&l.name.name)
            };
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
            let mut stmts = Vec::new();
            let cond = lower_expr(&a.value, &mut stmts, cx);
            // Structural expected-vs-actual for a top-level comparison. The
            // predicate is pure (ADR 0144), so re-evaluating the operands for the
            // failure message is observationally identical to the condition.
            let detail = if let ExprKind::BinOp(op, l, r) = &a.value.kind
                && let Some(sym) = comparison_op_symbol(*op)
            {
                let lv = lower_expr(l, &mut stmts, cx);
                let rv = lower_expr(r, &mut stmts, cx);
                format!(
                    "\"expect {src}\\n  expected: {src}\\n  actual:   \" + __bynkShow(({lv})) + \" {sym} \" + __bynkShow(({rv}))"
                )
            } else {
                format!("\"expect {src}\"")
            };
            for s in &stmts {
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
            let mut stmts = Vec::new();
            let value = lower_expr(&s.value, &mut stmts, cx);
            for st in &stmts {
                write_line(out, indent, st);
            }
            write_line(
                out,
                indent,
                &format!("{deps}.__exec.waitUntil({value});", deps = cx.cap_deps_expr),
            );
        }
        Statement::Do(d) => {
            // v0.146 (ADR 0170): `do expr` → `await expr;`. The binder-free
            // `let _ <- expr` for a unit effect — the effect runs and joins the
            // handler, its `()` result discarded (no `const`).
            let mut stmts = Vec::new();
            let value = lower_expr(&d.value, &mut stmts, cx);
            for st in &stmts {
                write_line(out, indent, st);
            }
            write_line(out, indent, &format!("await {value};"));
        }
        Statement::Assign(a) => {
            // v0.81 (storage track, ADR 0109): `cell := expr` writes the mutable
            // working state in place (`__state.cell = <expr>`). It is staged in
            // memory — read-your-writes within the handler — and flushed once at
            // handler end via `commitState` (which runs the invariant gate before
            // the durable write). A fault before that flush persists nothing.
            let mut stmts = Vec::new();
            let value = lower_expr(&a.value, &mut stmts, cx);
            for st in &stmts {
                write_line(out, indent, st);
            }
            let lhs = match &cx.agent_store_state {
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
    match &cx.assert_loc {
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
    match &cx.assert_loc {
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

/// True when a bare `Call` whose result type is the sum `sum_name` is actually a
/// variant constructor of that sum (e.g. `Won(prize)` for `Outcome`), as opposed
/// to an ordinary function that merely *returns* the sum (e.g.
/// `classify(n) -> Outcome`). Only the former is qualified to `Sum.Variant(...)`.
fn call_is_sum_variant(cx: &LowerCtx, sum_name: &str, call_name: &str) -> bool {
    if let Some(decl) = cx.commons.types.get(sum_name)
        && let TypeBody::Sum(s) = &decl.body
    {
        s.variants.iter().any(|v| v.name.name == call_name)
    } else {
        false
    }
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
fn lower_interp_str(parts: &[InterpPart], stmts: &mut Vec<String>, cx: &mut LowerCtx) -> String {
    let mut out = String::from("`");
    for part in parts {
        match part {
            InterpPart::Chunk(text) => out.push_str(&escape_ts_template(text)),
            InterpPart::Hole(hole) => {
                let lowered = lower_expr(hole, stmts, cx);
                out.push_str(&format!("${{String({lowered})}}"));
            }
        }
    }
    out.push('`');
    out
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

pub(crate) fn lower_expr(e: &Expr, stmts: &mut Vec<String>, cx: &mut LowerCtx) -> String {
    // v0.9.4: a literal the checker admitted as a refined type (expected-type-
    // directed construction) is emitted through the unchecked `unsafe`
    // constructor — the refinement was already verified at compile time, so
    // there is no runtime check and no `Result`.
    if let Some(Ty::Named {
        name,
        kind: NamedKind::Refined(_),
    }) = cx.commons.expr_types.get(&e.span)
        && let Some(raw) = lower_const_literal_raw(e)
    {
        return format!("{name}.unsafe({raw})");
    }
    match &e.kind {
        ExprKind::IntLit { value: n, .. } => n.to_string(),
        // v0.21: the stored lexeme verbatim — `1e10` must not normalise.
        ExprKind::FloatLit { lexeme, .. } => lexeme.clone(),
        // v0.86 (ADR 0112): a `Duration` literal lowers to its constant
        // milliseconds (the value `Duration` erases to).
        ExprKind::DurationLit { millis, .. } => millis.to_string(),
        ExprKind::StrLit(s) => format!("\"{}\"", escape_ts_string(s)),
        // v0.43 (ADR 0075): an interpolated string lowers to a TS template
        // literal — chunks as escaped literal text, holes as `${String(…)}`.
        ExprKind::InterpStr(parts) => lower_interp_str(parts, stmts, cx),
        ExprKind::BoolLit(b) => b.to_string(),
        // v0.20b: a list literal lowers to a TS array literal; `readonly` is
        // a type-level property and the checker owns the element typing.
        ExprKind::ListLit(elems) => {
            let lowered: Vec<String> = elems.iter().map(|el| lower_expr(el, stmts, cx)).collect();
            format!("[{}]", lowered.join(", "))
        }
        ExprKind::Ident(id) => lower_ident(e, id, cx),
        ExprKind::Call { name, args, .. } => lower_call(e, name, args, stmts, cx),
        ExprKind::UnaryOp(op, inner) => {
            let inner = lower_expr(inner, stmts, cx);
            let sym = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            };
            format!("{sym}{inner}")
        }
        ExprKind::BinOp(op, lhs, rhs) => lower_bin_op(*op, lhs, rhs, stmts, cx),
        ExprKind::Paren(inner) => {
            let s = lower_expr(inner, stmts, cx);
            format!("({s})")
        }
        ExprKind::Ok(inner) => {
            let s = lower_expr(inner, stmts, cx);
            // v0.9: `Ok` is overloaded — use the checker's recorded type to
            // decide between `Result.Ok` and `HttpResult.Ok`.
            if matches!(cx.commons.expr_types.get(&e.span), Some(Ty::HttpResult(_))) {
                format!("HttpResult.Ok({s})")
            } else {
                format!("Ok({s})")
            }
        }
        ExprKind::Err(inner) => {
            let s = lower_expr(inner, stmts, cx);
            format!("Err({s})")
        }
        ExprKind::Some(inner) => {
            let s = lower_expr(inner, stmts, cx);
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
            let operand_ty = cx.commons.expr_types.get(&inner.span);
            debug_assert!(
                matches!(operand_ty, Some(Ty::Option(_) | Ty::Result(_, _))),
                "`?` operand has no `Option`/`Result` checked type at {:?}: {operand_ty:?}",
                inner.span,
            );
            let is_option = matches!(operand_ty, Some(Ty::Option(_)));
            let inner_expr = lower_expr(inner, stmts, cx);
            let tmp = cx.fresh();
            stmts.push(format!("const {tmp} = {inner_expr};"));
            if is_option {
                stmts.push(format!(
                    "if ({tmp}.tag === \"None\") return HttpResult.NotFound;"
                ));
            } else {
                stmts.push(format!("if ({tmp}.tag === \"Err\") return {tmp};"));
            }
            format!("{tmp}.value")
        }
        ExprKind::ConstructorCall {
            type_name,
            method,
            args,
        } => lower_constructor_call(type_name, method, args, stmts, cx),
        ExprKind::RecordConstruction { type_name, fields } => {
            lower_record_construction(type_name, fields, stmts, cx)
        }
        ExprKind::FieldAccess { receiver, field } => lower_field_access(receiver, field, stmts, cx),
        ExprKind::MethodCall {
            receiver,
            method,
            args,
            ..
        } => lower_method_call(e, receiver, method, args, stmts, cx),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => lower_if(cond, then_block, else_block, stmts, cx),
        // v0.20a: a lambda lowers to a TS arrow; `async` iff its checked type
        // is an effectful function. Expression bodies that need hoisted
        // statements (match-as-IIFE etc.) keep them local to the arrow.
        ExprKind::Lambda(lambda) => lower_lambda(e, lambda, cx),
        ExprKind::Block(b) => lower_block_as_expr(b, cx),
        ExprKind::Match { discriminant, arms } => lower_match_as_iife(discriminant, arms, cx),
        ExprKind::Is { value, pattern } => lower_is(value, pattern, stmts, cx),
        ExprKind::UnitLit => "undefined".to_string(),
        ExprKind::EffectPure(inner) => {
            let inner_expr = lower_expr(inner, stmts, cx);
            format!("Promise.resolve({inner_expr})")
        }
        ExprKind::RecordSpread {
            type_name: _,
            base,
            overrides,
        } => lower_record_spread(base, overrides, stmts, cx),
        ExprKind::Expect(inner) => {
            // v0.9.1: expect as an expression (v0.112, renamed from `assert`).
            // Emit a runtime helper call that returns void (i.e., evaluates to
            // `undefined` at runtime and is treated as the unit value `()` in Bynk
            // terms). The expression form reports the predicate source only — the
            // statement form carries the structural expected-vs-actual report.
            let value = lower_expr(inner, stmts, cx);
            let span_start = inner.span.start;
            let span_end = inner.span.end;
            let location = expect_location(cx, span_start);
            let src = expect_source_text(cx, inner.span);
            format!(
                "__bynkExpect(({value}), \"{location}\", {span_start}, {span_end}, \"expect {src}\")"
            )
        }
        ExprKind::Val { type_ref, args } => lower_val(type_ref, args, stmts, cx),
        ExprKind::Observation(o) => lower_observation(o, cx),
        ExprKind::Trace { cap, op } => {
            // `trace(Cap.op)` → the recorded calls mapped to per-call records
            // whose fields are the operation's parameters (positionally).
            let obs = cx
                .observation_trace
                .clone()
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
    }
}

/// The parameter names of a capability operation, looked up from the capability
/// declarations in scope (v0.117). Used to destructure a recorded call's
/// arguments for a `with` predicate and to build `trace(Cap.op)` records.
fn cap_op_param_names(cx: &LowerCtx, cap: &str, op: &str) -> Vec<String> {
    for item in &cx.commons.commons.items {
        if let bynk_syntax::ast::CommonsItem::Capability(c) = item
            && c.name.name == cap
            && let Some(o) = c.ops.iter().find(|o| o.name.name == op)
        {
            return o.params.iter().map(|p| p.name.name.clone()).collect();
        }
    }
    Vec::new()
}

/// Lower an observation (v0.117) to a `Bool` JavaScript expression over the
/// recorded trace object. `total`/`matching` counts drive the sugar; `before`
/// compares call order.
fn lower_observation(o: &ObservationExpr, cx: &mut LowerCtx) -> String {
    let obs = cx
        .observation_trace
        .clone()
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
                    let mut pre = Vec::new();
                    let n = lower_expr(c, &mut pre, cx);
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
                let mut pre = Vec::new();
                let pred = lower_expr(p, &mut pre, cx);
                let pre_src = pre.join(" ");
                let matching = format!(
                    "{calls}.filter((__c: any) => {{ {destructure}{pre_src}return ({pred}); }}).length"
                );
                match count {
                    None => format!("(({matching}) >= 1)"),
                    Some(c) => {
                        let mut pre2 = Vec::new();
                        let n = lower_expr(c, &mut pre2, cx);
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
fn mock_value(ty: &Ty, cx: &LowerCtx, depth: u32) -> String {
    if depth == 0 {
        return "undefined".to_string();
    }
    match ty {
        Ty::Base(BaseType::Int) => "0".to_string(),
        Ty::Base(BaseType::String) => "\"\"".to_string(),
        Ty::Base(BaseType::Bool) => "true".to_string(),
        Ty::Base(BaseType::Float) => "0".to_string(),
        Ty::Named { name, .. } => {
            let Some(decl) = cx.commons.types.get(name) else {
                return "undefined".to_string();
            };
            match &decl.body {
                TypeBody::Refined { .. } => {
                    let d = refined_default(decl).unwrap_or_else(|| "0".to_string());
                    format!("{name}.unsafe({d})")
                }
                TypeBody::Opaque { base, .. } => {
                    format!("{name}.unsafe({})", base_default_ts(*base))
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
                                    &cx.commons.types,
                                )
                                .map(|t| mock_value(&t, cx, depth - 1))
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
                                &cx.commons.types,
                            )
                            .map(|t| mock_value(&t, cx, depth - 1))
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
fn lower_method_call(
    e: &Expr,
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> String {
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
    if let ExprKind::Ident(id) = &receiver.kind
        && let Some(f_ts) = cx.agent_held_maps.get(&id.name).cloned()
    {
        let var = cx
            .agent_store_state
            .as_ref()
            .map(|(v, _)| v.clone())
            .unwrap_or_else(|| "__state".to_string());
        let m = format!("{var}.{}", id.name);
        let a: Vec<String> = args.iter().map(|x| lower_expr(x, stmts, cx)).collect();
        return match method.name.as_str() {
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
            _ => lower_query_method(
                format!(
                    "Object.values({m}).flatMap((__cid) => {{ const __c = resolveConnection<{f_ts}>(this.state, __cid); return __c.tag === \"Some\" ? [__c.value] : []; }})"
                ),
                method,
                &a,
                cx.commons.expr_types.get(&e.span),
            )
            .unwrap_or_else(|| {
                format!("(/* unsupported held Map op {} */ undefined)", method.name)
            }),
        };
    }
    // v0.82 (ADR 0110): a storage-`Map` operation — `<map>.<op>(…)` on a `store
    // Map[K, V]` field. Lowers to an entry op over `__state.<map>` (a
    // `Record<string, V>`): mutating the working record (`put`/`remove`/`update`/
    // `upsert`) or reading it (`get`/`contains`/`size`). The surface op is
    // `Effect`-typed and awaited with `<-`, but the working-record mutation is
    // synchronous (the durable write is the single end-of-handler flush), so an
    // awaited expression suffices. `update` on an absent key throws (a fault →
    // nothing commits).
    if let ExprKind::Ident(id) = &receiver.kind
        && cx.agent_store_maps.contains(&id.name)
    {
        let var = cx
            .agent_store_state
            .as_ref()
            .map(|(v, _)| v.clone())
            .unwrap_or_else(|| "__state".to_string());
        let m = format!("{var}.{}", id.name);
        // v0.93 (ADR 0118): the value-record fields this map is `@indexed(by:)` on.
        let idx_fields: Vec<String> = cx
            .agent_store_indexes
            .get(&id.name)
            .cloned()
            .unwrap_or_default();
        // Route an equality `filter` on an indexed field to a posting-list lookup
        // (`<map>__idx_<f>[v]`) instead of a full `Object.values` scan. The lookup
        // reads the staged map, so it stays read-your-writes (the index is
        // maintained in the same commit).
        if method.name == "filter"
            && let [arg] = args
            && let ExprKind::Lambda(lam) = &arg.kind
            && let Some(routed) =
                route_indexed_filter(&m, &var, &id.name, &idx_fields, lam, stmts, cx)
        {
            return routed;
        }
        let a: Vec<String> = args.iter().map(|x| lower_expr(x, stmts, cx)).collect();
        return match method.name.as_str() {
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
            _ => lower_query_method(
                format!("Object.values({m})"),
                method,
                &a,
                cx.commons.expr_types.get(&e.span),
            )
            .unwrap_or_else(|| format!("(/* unsupported Map op {} */ undefined)", method.name)),
        };
    }
    // v0.83: a storage-`Set` operation — `<set>.<op>(…)` on a `store Set[T]` field.
    // Lowers to an entry op over `__state.<set>` (a `Record<string, boolean>`):
    // `add`/`remove` mutate the working record, `contains`/`size` read it.
    if let ExprKind::Ident(id) = &receiver.kind
        && cx.agent_store_sets.contains(&id.name)
    {
        let var = cx
            .agent_store_state
            .as_ref()
            .map(|(v, _)| v.clone())
            .unwrap_or_else(|| "__state".to_string());
        let s = format!("{var}.{}", id.name);
        let a: Vec<String> = args.iter().map(|x| lower_expr(x, stmts, cx)).collect();
        return match method.name.as_str() {
            "add" => format!("(({s}[{}] = true), undefined)", a[0]),
            "remove" => format!("((delete {s}[{}]), undefined)", a[0]),
            "contains" => format!("(({}) in {s})", a[0]),
            "size" => format!("Object.keys({s}).length"),
            other => format!("(/* unsupported Set op {other} */ undefined)"),
        };
    }
    // v0.87 (ADR 0113): a storage-`Cache` operation — `<cache>.<op>(…)` on a
    // `store Cache[K, V]` field. Lowers to an entry op over `__state.<cache>` (a
    // `Record<string, { v, exp }>`) that applies TTL expiry against the injected
    // `Clock`: every op but `remove` reads `now()` (an awaited `Effect`), so the
    // op is an async IIFE. `put`/`update`/`upsert` stamp `exp = now() + ttl`;
    // `get`/`contains`/`size` treat an entry past `exp` as absent.
    if let ExprKind::Ident(id) = &receiver.kind
        && let Some(ttl) = cx.agent_store_caches.get(&id.name).copied()
    {
        let var = cx
            .agent_store_state
            .as_ref()
            .map(|(v, _)| v.clone())
            .unwrap_or_else(|| "__state".to_string());
        let c = format!("{var}.{}", id.name);
        let now = format!("await {}.Clock.now()", cx.cap_deps_expr);
        let a: Vec<String> = args.iter().map(|x| lower_expr(x, stmts, cx)).collect();
        return match method.name.as_str() {
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
        };
    }
    // v0.95 (ADR 0121): a storage-`Log` operation — `<log>.<op>(…)` on a
    // `store Log[T]` field (an array of `{ t, v }`). `append` stamps the clock
    // and pushes (pruning past `@retain`); the time-window roots and the general
    // query vocabulary lower to a lazy pipeline over the entry values.
    if let ExprKind::Ident(id) = &receiver.kind
        && let Some(retain) = cx.agent_store_logs.get(&id.name).copied()
    {
        let var = cx
            .agent_store_state
            .as_ref()
            .map(|(v, _)| v.clone())
            .unwrap_or_else(|| "__state".to_string());
        let g = format!("{var}.{}", id.name);
        let a: Vec<String> = args.iter().map(|x| lower_expr(x, stmts, cx)).collect();
        if method.name == "append" {
            let now = format!("await {}.Clock.now()", cx.cap_deps_expr);
            let prune = match retain {
                Some(ms) => format!(
                    " for (let __i = {g}.length - 1; __i >= 0; __i--) {{ if ({g}[__i].t < __now - {ms}) {g}.splice(__i, 1); }}"
                ),
                None => String::new(),
            };
            return format!(
                "(async () => {{ const __now = {now}; {g}.push({{ t: __now, v: {0} }});{prune} return undefined; }})()",
                a[0]
            );
        }
        // The values array feeding the general query pipeline.
        let values = format!("{g}.map((__e) => __e.v)");
        let thunk = |body: String| format!("(() => {body})");
        return match method.name.as_str() {
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
            _ => lower_query_method(values, method, &a, cx.commons.expr_types.get(&e.span))
                .unwrap_or_else(|| format!("(/* unsupported Log op {} */ undefined)", method.name)),
        };
    }
    // v0.98 (ADR 0125): a storage-`Cell` operation — `<cell>.update(f)` on a
    // `store Cell[T]` field. Lowers to a staged read-modify-write over
    // `__state.<cell>` (`Map.update`'s lowering minus the key-absent guard — a
    // cell is always present, so there is no fault path). The mutation is
    // synchronous against the working state (read-your-writes); the single
    // end-of-handler `commitState` flush runs the invariant gate before the
    // durable write, exactly as `:=` does.
    if let ExprKind::Ident(id) = &receiver.kind
        && cx
            .agent_store_state
            .as_ref()
            .is_some_and(|(_, cells)| cells.contains(&id.name))
    {
        let var = cx
            .agent_store_state
            .as_ref()
            .map(|(v, _)| v.clone())
            .unwrap_or_else(|| "__state".to_string());
        let n = format!("{var}.{}", id.name);
        let a: Vec<String> = args.iter().map(|x| lower_expr(x, stmts, cx)).collect();
        return match method.name.as_str() {
            "update" => format!("(() => {{ {n} = ({0})({n}); return undefined; }})()", a[0]),
            other => format!("(/* unsupported Cell op {other} */ undefined)"),
        };
    }
    // v0.9: explicit `HttpResult.Variant(args)` construction. The
    // checker has already recorded the expression's type — emit it
    // directly through the runtime's HttpResult namespace.
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == HTTP_RESULT
        && http_variant(&method.name).is_some()
    {
        let args_lowered: Vec<String> = args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
        return format!("HttpResult.{}({})", method.name, args_lowered.join(", "));
    }
    // v0.20b: built-in collection statics — `List.empty()` /
    // `Map.empty()`. The checker recorded the instantiated type;
    // emit it explicitly so the TS value doesn't infer as `never[]`
    // / `Map<unknown, unknown>` outside contextually-typed positions.
    if let ExprKind::Ident(id) = &receiver.kind
        && (id.name == LIST || id.name == MAP)
        && method.name == "empty"
        && args.is_empty()
        && !cx.commons.types.contains_key(&id.name)
    {
        match cx.commons.expr_types.get(&e.span) {
            Some(Ty::List(t)) => return format!("([] as readonly {}[])", ts_ty(t)),
            Some(Ty::Map(k, v)) => {
                return format!("new Map<{}, {}>()", ts_ty(k), ts_ty(v));
            }
            _ => {}
        }
    }
    // v0.22b: the typed JSON codec (ADR 0045).
    if let Some(s) = lower_json_codec_call(e, receiver, method, args, stmts, cx) {
        return s;
    }
    // v0.22a: the numeric parse statics — `Int.parse(s)` /
    // `Float.parse(s)` (ADR 0048). Full-string parse via `Number(…)`
    // (which, unlike `parseFloat`, rejects trailing garbage); an
    // empty/whitespace-only string would coerce to `0`, so it is
    // rejected first. `Int` requires a safe integer (the honest
    // runtime "overflow → None"); `Float` requires finite (the 0040
    // posture).
    if let ExprKind::Ident(id) = &receiver.kind
        && (id.name == INT || id.name == FLOAT)
        && method.name == "parse"
        && args.len() == 1
    {
        let s = lower_expr(&args[0], stmts, cx);
        let guard = if id.name == INT {
            "Number.isSafeInteger(__n)"
        } else {
            "Number.isFinite(__n)"
        };
        return format!(
            "((__s: string) => {{ const __n = __s.trim() === \"\" ? Number.NaN : Number(__s); return {guard} ? Some(__n) : None; }})({s})"
        );
    }
    // v0.86 (ADR 0112): `Duration.millis(n)` — the runtime `Int`→`Duration`
    // constructor. A `Duration` lowers to its milliseconds, so this is the
    // identity on the argument.
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == DURATION
        && method.name == "millis"
        && args.len() == 1
    {
        return lower_expr(&args[0], stmts, cx);
    }
    // v0.90 (ADR 0114): `Instant.fromEpochMillis(n)` — an `Instant` lowers to
    // its epoch milliseconds, so this is the identity on the argument.
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == INSTANT
        && method.name == "fromEpochMillis"
        && args.len() == 1
    {
        return lower_expr(&args[0], stmts, cx);
    }
    // v0.110 (ADR 0142 D2): the `Bytes` static constructors. `fromUtf8` is the
    // UTF-8 encoding of a string (total); `fromBase64` is a guarded base64
    // decode returning `Option` (`None` on invalid base64); `empty` is the
    // zero octet sequence.
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == BYTES
    {
        match (method.name.as_str(), args.len()) {
            ("fromUtf8", 1) => {
                let s = lower_expr(&args[0], stmts, cx);
                return format!("new TextEncoder().encode({s})");
            }
            ("fromBase64", 1) => {
                let s = lower_expr(&args[0], stmts, cx);
                return format!("__bynkBytesFromBase64({s})");
            }
            ("empty", 0) => {
                return "new Uint8Array()".to_string();
            }
            _ => {}
        }
    }
    // v0.100: `Stream.of(xs)` — the deterministic in-memory source. A `Stream`
    // lowers to a host async iterable; `of` wraps a list as an async generator.
    // Emitted inline (no runtime import), like the collection kernels.
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == STREAM
        && method.name == "of"
        && args.len() == 1
    {
        let xs = lower_expr(&args[0], stmts, cx);
        return format!("(async function* () {{ for (const __e of {xs}) {{ yield __e; }} }})()");
    }
    // v0.15 cross-context capability call: `B.Cap.op(args)` /
    // `Alias.Cap.op(args)`. The provider is instantiated locally in
    // the composition root, so this lowers to an in-process
    // `<deps>.<Cap>.op(args)` exactly like a local capability call —
    // the consumed-context prefix is resolved away.
    if let Some(chain) = flatten_emit_ident_chain(receiver)
        && let Some((_consumed, cap)) = cx.cross_context.resolve_cross_capability(&chain)
    {
        let args_lowered: Vec<String> = args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
        return format!(
            "{}.{}.{}({})",
            cx.cap_deps_expr,
            cap,
            method.name,
            args_lowered.join(", ")
        );
    }
    // v0.6 cross-context service call: receiver is an alias or the
    // dotted name of a consumed context.
    if let Some(s) = lower_cross_context_service_call(receiver, method, args, stmts, cx) {
        return s;
    }
    // Capability call: receiver is a bare ident naming a declared
    // capability in `given`. Lower to `<deps>.Capability.op(args)`,
    // where `<deps>` is `deps` in a handler body and `this.deps` in a
    // provider body (v0.12 provider composition).
    if let ExprKind::Ident(id) = &receiver.kind
        && cx.capabilities.contains(&id.name)
    {
        let args_lowered: Vec<String> = args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
        return format!(
            "{}.{}.{}({})",
            cx.cap_deps_expr,
            id.name,
            method.name,
            args_lowered.join(", ")
        );
    }
    // Static call: receiver is a bare ident naming a declared type.
    if let ExprKind::Ident(id) = &receiver.kind
        && cx.commons.types.contains_key(&id.name)
    {
        let args_lowered: Vec<String> = args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
        return format!("{}.{}({})", id.name, method.name, args_lowered.join(", "));
    }
    // v0.7: local service call inside a test case body.
    // `serviceName.method(args)` → `serviceName.method(args, deps)`.
    if let ExprKind::Ident(id) = &receiver.kind
        && cx.test_services.contains(&id.name)
    {
        let args_lowered: Vec<String> = args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
        let mut all = args_lowered;
        all.push("deps".to_string());
        return format!("{}.{}({})", id.name, method.name, all.join(", "));
    }
    // v0.9.2: inline agent invocation. Source form is
    // `Agent(<key>).method(args)`; receiver parses as
    // `Call(Agent, [<key>])`. Lower to
    // `__makeAgent(<key>).method(args, deps)`. Works in service and
    // agent-handler bodies (deps is the handler's deps parameter) and
    // test bodies (deps is the locally-built makeTestDeps record).
    if let ExprKind::Call {
        name,
        args: ctor_args,
        ..
    } = &receiver.kind
        && cx.local_agents.contains(&name.name)
    {
        // v0.104 (real-time track slice 3b): when lowering a `from WebSocket`
        // `on open` body inside its hosting Durable Object, a transfer to the
        // self-agent is a direct `this.method(args, deps)` self-call — the key
        // addresses *this* DO, and the held connection never crosses an RPC
        // boundary (DECISION A). The key expression is **not** emitted: the shape
        // constraint (`bynk.ws.open_transfer_shape`, D2) restricts it to a
        // request-derivable param ident — side-effect-free and equal to this
        // instance's own key — so dropping it is sound.
        if cx.ws_self_agent.as_deref() == Some(name.name.as_str()) {
            cx.record_agent_call(&name.name, &method.name);
            let args_lowered: Vec<String> = args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
            let mut all = args_lowered;
            all.push("deps".to_string());
            return format!(
                "this.{method}({args})",
                method = method.name,
                args = all.join(", ")
            );
        }
        cx.record_agent_call(&name.name, &method.name);
        let key_arg = ctor_args
            .first()
            .map(|a| lower_expr(a, stmts, cx))
            .unwrap_or_else(|| "\"default\"".to_string());
        let instance = cx.agent_construct(&name.name, &key_arg);
        let args_lowered: Vec<String> = args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
        let mut all = args_lowered;
        all.push("deps".to_string());
        return format!(
            "{instance}.{method}({args})",
            method = method.name,
            args = all.join(", ")
        );
    }
    // Let-bound agent invocation. `let x = Agent(key); x.method(args)`
    // — the statement emitter recorded `x` as an agent variable when
    // it lowered the let. Method calls on `x` go straight to the
    // class instance with `deps` threaded through.
    if let ExprKind::Ident(id) = &receiver.kind
        && cx.local_agent_vars.contains_key(&id.name)
    {
        if let Some(agent) = cx.local_agent_vars.get(&id.name).cloned() {
            cx.record_agent_call(&agent, &method.name);
        }
        let args_lowered: Vec<String> = args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
        let mut all = args_lowered;
        all.push("deps".to_string());
        return format!("{}.{}({})", id.name, method.name, all.join(", "));
    }
    // v0.20b: built-in kernel methods on the collection types,
    // dispatched on the receiver's checked type. Emitted inline
    // (typed IIFEs / spreads) — no runtime imports, so files that
    // never touch collections emit byte-identically to v0.20a.
    if let Some(recv_ty) = cx.commons.expr_types.get(&receiver.span).cloned() {
        match &recv_ty {
            Ty::List(elem) => {
                if let Some(s) = lower_list_kernel(e, receiver, method, args, elem, stmts, cx) {
                    return s;
                }
            }
            // v0.91 (ADR 0119): a chained op on a lazy `Query` — the source is
            // the receiver thunk, invoked (`(recv)()`).
            Ty::Query(_) => {
                let recv = lower_expr(receiver, stmts, cx);
                let a: Vec<String> = args.iter().map(|x| lower_expr(x, stmts, cx)).collect();
                let result_ty = cx.commons.expr_types.get(&e.span).cloned();
                if let Some(s) =
                    lower_query_method(format!("({recv})()"), method, &a, result_ty.as_ref())
                {
                    return s;
                }
            }
            // v0.100: a chained op on a `Stream` — the receiver already *is* an
            // async iterable, so it is the source directly. Emitted inline as
            // async-generator IIFEs (builders) / an async drain (`collect`).
            Ty::Stream(_) => {
                let recv = lower_expr(receiver, stmts, cx);
                let a: Vec<String> = args.iter().map(|x| lower_expr(x, stmts, cx)).collect();
                if let Some(s) = lower_stream_method(recv, method, &a) {
                    return s;
                }
            }
            // v0.102: the held-resource operations on a `Connection[F]` lower to
            // method calls on the runtime `Connection` object — `send(frame)` and
            // `close()`. The linearity pass has already verified ownership.
            Ty::Connection(_) => {
                let recv = lower_expr(receiver, stmts, cx);
                let a: Vec<String> = args.iter().map(|x| lower_expr(x, stmts, cx)).collect();
                return format!("({recv}).{}({})", method.name, a.join(", "));
            }
            Ty::Map(key, val) => {
                if let Some(s) = lower_map_kernel(receiver, method, args, key, val, stmts, cx) {
                    return s;
                }
            }
            // v0.21: the numeric kernel. `toFloat` is the identity
            // at runtime (the Int/Float distinction is erased);
            // the four `Float -> Int` roundings map onto `Math.*`.
            // v0.22a extends it (abs/min/max/clamp, isNaN/isFinite).
            Ty::Base(BaseType::Int | BaseType::Float) => {
                if let Some(s) = lower_numeric_kernel(receiver, method, args, stmts, cx) {
                    return s;
                }
            }
            // v0.86 (ADR 0112): the `Duration` kernel. `toMillis` is the identity
            // at runtime (a `Duration` already *is* its milliseconds); `toString`
            // renders the number.
            Ty::Base(BaseType::Duration) => {
                if let Some(s) = lower_duration_kernel(receiver, method, args, stmts, cx) {
                    return s;
                }
            }
            // v0.90 (ADR 0114): the `Instant` kernel. `toEpochMillis` is the
            // identity (an `Instant` lowers to its epoch millis); `toString`
            // renders the number.
            Ty::Base(BaseType::Instant) => {
                if let Some(s) = lower_instant_kernel(receiver, method, args, stmts, cx) {
                    return s;
                }
            }
            // v0.110 (ADR 0142): the `Bytes` kernel. `length` is the octet
            // count; `toBase64` encodes; `decodeUtf8` is a guarded UTF-8 decode
            // returning `Option`.
            Ty::Base(BaseType::Bytes) => {
                if let Some(s) = lower_bytes_kernel(receiver, method, args, stmts, cx) {
                    return s;
                }
            }
            // v0.22a: the string kernel (ADR 0046).
            Ty::Base(BaseType::String) => {
                if let Some(s) = lower_string_kernel(receiver, method, args, stmts, cx) {
                    return s;
                }
            }
            // v0.22a: Option/Result combinators (ADR 0048).
            Ty::Option(inner) => {
                if let Some(s) = lower_option_kernel(e, receiver, method, args, inner, stmts, cx) {
                    return s;
                }
            }
            Ty::Result(ok, err) => {
                if let Some(s) = lower_result_kernel(e, receiver, method, args, ok, err, stmts, cx)
                {
                    return s;
                }
            }
            // §2.8.3: the `Effect[Result[T, E]]` combinators — `mapOk`/`mapErr`/
            // `flatMapOk`/`flatMapErr`. Lowered as an `async` IIFE that awaits the
            // receiver `Promise<Result<…>>` and rebuilds the transformed Result.
            Ty::Effect(inner) => {
                if let Ty::Result(ok, err) = inner.as_ref()
                    && let Some(s) =
                        lower_effect_result_kernel(e, receiver, method, args, ok, err, stmts, cx)
                {
                    return s;
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
            } if !cx
                .commons
                .methods
                .get(name)
                .is_some_and(|t| t.instance.contains_key(&method.name)) =>
            {
                let lowered = match base {
                    BaseType::Int | BaseType::Float => {
                        lower_numeric_kernel(receiver, method, args, stmts, cx)
                    }
                    BaseType::String => lower_string_kernel(receiver, method, args, stmts, cx),
                    BaseType::Duration => lower_duration_kernel(receiver, method, args, stmts, cx),
                    BaseType::Instant => lower_instant_kernel(receiver, method, args, stmts, cx),
                    BaseType::Bytes => lower_bytes_kernel(receiver, method, args, stmts, cx),
                    BaseType::Bool => None,
                };
                if let Some(s) = lowered {
                    return s;
                }
            }
            _ => {}
        }
    }
    // Instance call: UFCS lowering with the receiver as first arg.
    let ns = cx
        .receiver_namespace(receiver)
        .unwrap_or_else(|| "/* unknown */".to_string());
    let recv = lower_expr(receiver, stmts, cx);
    let mut all = vec![recv];
    for a in args {
        all.push(lower_expr(a, stmts, cx));
    }
    format!("{ns}.{}({})", method.name, all.join(", "))
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
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<String> {
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == JSON
        && args.len() == 1
    {
        if method.name == "encode"
            && let Some(arg_ty) = cx.commons.expr_types.get(&args[0].span).cloned()
            && let Some(tref) = ty_to_type_ref(&arg_ty)
        {
            let v = lower_expr(&args[0], stmts, cx);
            let ser = serialisation::serialise_expr(&tref, &v);
            return Some(format!("JSON.stringify({ser})"));
        }
        if method.name == "decode"
            && let Some(Ty::Result(t, _)) = cx.commons.expr_types.get(&e.span).cloned()
            && let Some(tref) = ty_to_type_ref(&t)
        {
            let ts = ts_ty(&t);
            let des = serialisation::deserialise_expr(&tref, "__j", "$");
            let arg = lower_expr(&args[0], stmts, cx);
            return Some(format!(
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
            ));
        }
    }
    None
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
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<String> {
    if let Some((consumed, key)) = cross_context_lowering_prefix(receiver, cx) {
        cx.cross_context_used = true;
        match cx.target {
            BuildTarget::Bundle => {
                let args_lowered: Vec<String> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let lowered = lower_expr(a, stmts, cx);
                        param_cast(&consumed, cx.cross_context, method, i, lowered)
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
                Some(lower_workers_cross_context_call(
                    &consumed, method, args, stmts, cx,
                ))
            }
        }
    } else {
        None
    }
}

fn lower_val(
    type_ref: &TypeRef,
    args: &[Expr],
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> String {
    // Resolve the mocked type straight from the AST node rather than the
    // checker's `expr_types` side-table — the static type table is always
    // populated, whereas a test body's per-expression types may not be visible
    // to the emitter.
    let ty = match bynk_check::checker::resolve_type_ref(type_ref, &cx.commons.types) {
        Some(t) => t,
        None => return "undefined /* mock: unresolved type */".to_string(),
    };
    // Refined literal pin → `T.unsafe(<literal>)`.
    if let (
        Some(arg),
        Ty::Named {
            name,
            kind: NamedKind::Refined(_),
        },
    ) = (args.first(), &ty)
    {
        let raw = lower_const_literal_raw(arg).unwrap_or_else(|| lower_expr(arg, stmts, cx));
        return format!("{name}.unsafe({raw})");
    }
    // Bare mock (refined / opaque / sum / record).
    mock_value(&ty, cx, MOCK_DEPTH)
}

/// When we encounter `lhs && rhs`, see if lhs is an `is` (possibly wrapped
/// in parens or nested `&&`) and if so collect the bindings to inject into
/// rhs. Returns `(binding_const_decls, lowered_lhs, lowered_rhs)` if
/// special handling is appropriate; otherwise returns None.
fn lower_and_with_is(
    lhs: &Expr,
    rhs: &Expr,
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<(Vec<String>, String, String)> {
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
    let lhs_expr = lower_expr(lhs, stmts, cx);
    let mut bindings = Vec::new();
    let mut found = false;
    gather_is_bindings_for_emit(lhs, cx, &mut bindings, &mut found);
    let _ = found; // guaranteed true by the `cond_contains_is` guard above
    // We lower rhs ourselves (not into the outer stmts), so that any
    // statement-style prefix from rhs is folded into the IIFE properly.
    let mut rhs_stmts = Vec::new();
    let rhs_expr = lower_expr(rhs, &mut rhs_stmts, cx);
    let mut rhs_full = String::new();
    for s in &rhs_stmts {
        rhs_full.push_str(s);
        rhs_full.push(' ');
    }
    rhs_full.push_str(&rhs_expr);
    Some((bindings, lhs_expr, rhs_full))
}

/// Walk an expression collecting `const name = expr.field;` strings for
/// any `is`-pattern bindings on the truthy path. `found` indicates whether
/// at least one `is` was seen.
fn gather_is_bindings_for_emit(e: &Expr, cx: &LowerCtx, out: &mut Vec<String>, found: &mut bool) {
    match &e.kind {
        ExprKind::Is { value, pattern } => {
            *found = true;
            let value_text = cx.is_receiver_text(value);
            let disc_ty = cx.commons.expr_types.get(&value.span).cloned();
            if let Pattern::Variant {
                variant, bindings, ..
            } = pattern
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
                            let field =
                                cx.positional_field_name(disc_ty.as_ref(), &variant.name, i);
                            out.push(format!(
                                "const {name} = {value}.{field};",
                                name = ts_ident(&name.name),
                                value = value_text,
                                field = field
                            ));
                        }
                    }
                }
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
fn list_ok_elem_ts(call_ty: Option<&Ty>) -> String {
    match call_ty {
        Some(Ty::Effect(inner)) => match inner.as_ref() {
            Ty::Result(ok, _) => match ok.as_ref() {
                Ty::List(u) => ts_ty(u),
                other => ts_ty(other),
            },
            other => ts_ty(other),
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
    elem: &Ty,
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<String> {
    let elem_ts = ts_ty(elem);
    match (method.name.as_str(), args) {
        ("length", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("({recv}).length"))
        }
        ("get", [index]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let idx = lower_expr(index, stmts, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[], __i: number) => __i >= 0 && __i < __xs.length ? Some(__xs[__i] as {elem_ts}) : None)({recv}, {idx})"
            ))
        }
        ("prepend", [head]) => {
            let head = lower_expr(head, stmts, cx);
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("[{head}, ...{recv}]"))
        }
        ("fold", [init, f]) => {
            // The call's checked type is the accumulator type.
            let acc_ts = cx
                .commons
                .expr_types
                .get(&e.span)
                .map(ts_ty)
                .unwrap_or_else(|| "unknown".to_string());
            let recv = lower_expr(receiver, stmts, cx);
            let init = lower_expr(init, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[], __acc: {acc_ts}, __f: (acc: {acc_ts}, x: {elem_ts}) => {acc_ts}) => {{ for (const __x of __xs) __acc = __f(__acc, __x); return __acc; }})({recv}, {init}, {f})"
            ))
        }
        (FOLD_EFF, [init, f]) => {
            // The call's checked type is `Effect[Acc]` — peel for the TS
            // accumulator annotation.
            let acc_ts = match cx.commons.expr_types.get(&e.span) {
                Some(Ty::Effect(acc)) => ts_ty(acc),
                Some(other) => ts_ty(other),
                _ => "unknown".to_string(),
            };
            let recv = lower_expr(receiver, stmts, cx);
            let init = lower_expr(init, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "(async (__xs: readonly {elem_ts}[], __acc: {acc_ts}, __f: (acc: {acc_ts}, x: {elem_ts}) => Promise<{acc_ts}>) => {{ for (const __x of __xs) __acc = await __f(__acc, __x); return __acc; }})({recv}, {init}, {f})"
            ))
        }
        // v0.146 (ADR 0170): `forEach` — run an effectful step per element in
        // order, awaiting each; yields `Promise<void>`. The eager `List`
        // analogue of the `Query.forEach` terminal, emitted inline.
        (FOR_EACH, [f]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "(async (__xs: readonly {elem_ts}[]) => {{ for (const __x of __xs) {{ await ({f})(__x); }} }})({recv})"
            ))
        }
        // v0.147 (ADR 0171): `parTraverse` — issue the effectful fn over every
        // element concurrently and await them together, so one slow element does
        // not head-of-line-block the rest. The eager `List` analogue of the
        // `Query.parTraverse` terminal, emitted inline.
        (PAR_TRAVERSE, [f]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
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
            let res_ts = match cx.commons.expr_types.get(&e.span) {
                Some(Ty::Effect(inner)) => match &**inner {
                    Ty::List(el) => ts_ty(el),
                    other => ts_ty(other),
                },
                _ => "unknown".to_string(),
            };
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "(async (__xs: readonly {elem_ts}[]) => {{ const __out: {res_ts}[] = []; for (const __x of __xs) {{ __out.push(await ({f})(__x)); }} return __out; }})({recv})"
            ))
        }
        (PAR_TRAVERSE_ALL, [f]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "(async (__xs: readonly {elem_ts}[]) => await Promise.all(__xs.map((__x: {elem_ts}) => ({f})(__x))))({recv})"
            ))
        }
        // v0.150 (ADR 0174): the short-circuit collect iterators — stop at the
        // first `Err` and return it (`Effect[Result[List[U], E]]`). `traverseTry`
        // awaits each in order, bailing on the first `Err`; `parTraverseTry`
        // issues all at once, then scans the resolved `Result`s in input order.
        (TRAVERSE_TRY, [f]) => {
            let u_ts = list_ok_elem_ts(cx.commons.expr_types.get(&e.span));
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "(async (__xs: readonly {elem_ts}[]) => {{ const __out: {u_ts}[] = []; for (const __x of __xs) {{ const __r = await ({f})(__x); if (__r.tag === \"Err\") {{ return Err(__r.error); }} __out.push(__r.value); }} return Ok(__out); }})({recv})"
            ))
        }
        (PAR_TRAVERSE_TRY, [f]) => {
            let u_ts = list_ok_elem_ts(cx.commons.expr_types.get(&e.span));
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "(async (__xs: readonly {elem_ts}[]) => {{ const __rs = await Promise.all(__xs.map((__x: {elem_ts}) => ({f})(__x))); const __out: {u_ts}[] = []; for (const __r of __rs) {{ if (__r.tag === \"Err\") {{ return Err(__r.error); }} __out.push(__r.value); }} return Ok(__out); }})({recv})"
            ))
        }
        // v0.88 (ADR 0116): the eager builder/terminal vocabulary. Most lower
        // to native array methods; callbacks are wrapped in a single-arg arrow
        // so the array index/array extra args never reach a Bynk one-param fn.
        ("map", [f]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!("({recv}).map((__x: {elem_ts}) => ({f})(__x))"))
        }
        ("filter", [p]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let p = lower_expr(p, stmts, cx);
            Some(format!("({recv}).filter((__x: {elem_ts}) => ({p})(__x))"))
        }
        ("flatMap", [f]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!("({recv}).flatMap((__x: {elem_ts}) => ({f})(__x))"))
        }
        ("take", [n]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let n = lower_expr(n, stmts, cx);
            Some(format!("({recv}).slice(0, Math.max(0, {n}))"))
        }
        ("skip", [n]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let n = lower_expr(n, stmts, cx);
            Some(format!("({recv}).slice(Math.max(0, {n}))"))
        }
        ("count", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("({recv}).length"))
        }
        ("any", [p]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let p = lower_expr(p, stmts, cx);
            Some(format!("({recv}).some((__x: {elem_ts}) => ({p})(__x))"))
        }
        ("all", [p]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let p = lower_expr(p, stmts, cx);
            Some(format!("({recv}).every((__x: {elem_ts}) => ({p})(__x))"))
        }
        // v0.119 (ADR 0155, DECISION C-a): `run.upTo(step)` — the driven history
        // strictly before `step`. Steps are distinct object instances in the run
        // array, so `indexOf` is reference identity. An IIFE avoids re-evaluating
        // the receiver.
        ("upTo", [step]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let step = lower_expr(step, stmts, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[], __s: {elem_ts}) => __xs.slice(0, __xs.indexOf(__s)))({recv}, {step})"
            ))
        }
        ("first", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[]) => __xs.length > 0 ? Some(__xs[0]) : None)({recv})"
            ))
        }
        ("firstOrElse", [default]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let default = lower_expr(default, stmts, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[], __d: {elem_ts}) => __xs.length > 0 ? __xs[0] : __d)({recv}, {default})"
            ))
        }
        // v0.88 (ADR 0116 D2/D3/D4): ordering + aggregates. The comparator
        // `<`/`>` works for the numeric- and string-erased orderable keys
        // alike, so no key-type branch is needed (except average's rounding).
        ("sortBy", [key]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let key = lower_expr(key, stmts, cx);
            Some(format!(
                "[...{recv}].sort((__a: {elem_ts}, __b: {elem_ts}) => {{ const __ka = ({key})(__a), __kb = ({key})(__b); return __ka < __kb ? -1 : __ka > __kb ? 1 : 0; }})"
            ))
        }
        ("distinct", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("[...new Set({recv})]"))
        }
        ("distinctBy", [key]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let key = lower_expr(key, stmts, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[]) => {{ const __seen = new Set(); const __out: {elem_ts}[] = []; for (const __x of __xs) {{ const __k = ({key})(__x); if (!__seen.has(__k)) {{ __seen.add(__k); __out.push(__x); }} }} return __out; }})({recv})"
            ))
        }
        ("sum", [key]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let key = lower_expr(key, stmts, cx);
            Some(format!(
                "({recv}).reduce((__s: number, __x: {elem_ts}) => __s + ({key})(__x), 0)"
            ))
        }
        ("min" | "max", [key]) => {
            let cmp = if method.name == "min" { "<" } else { ">" };
            let recv = lower_expr(receiver, stmts, cx);
            let key = lower_expr(key, stmts, cx);
            Some(format!(
                "((__xs: readonly {elem_ts}[]) => {{ if (__xs.length === 0) return None; let __m = ({key})(__xs[0]); for (const __x of __xs) {{ const __k = ({key})(__x); if (__k {cmp} __m) __m = __k; }} return Some(__m); }})({recv})"
            ))
        }
        ("average", [key]) => {
            // D3: Duration averages round to integer millis; Int/Float -> Float.
            let round = matches!(
                cx.commons.expr_types.get(&e.span),
                Some(Ty::Option(inner)) if matches!(inner.as_ref(), Ty::Base(BaseType::Duration))
            );
            let mean = if round {
                "Math.round(__s / __xs.length)"
            } else {
                "__s / __xs.length"
            };
            let recv = lower_expr(receiver, stmts, cx);
            let key = lower_expr(key, stmts, cx);
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
            let recv = lower_expr(receiver, stmts, cx);
            let u_ts = join_other_elem_ts(args, cx);
            let other = lower_expr(other, stmts, cx);
            let left = lower_expr(left, stmts, cx);
            let right = lower_expr(right, stmts, cx);
            let into = lower_expr(into, stmts, cx);
            Some(format!(
                "(() => {{ const __h: Record<string, {u_ts}[]> = {{}}; for (const __u of {other}) {{ const __k = String(({right})(__u)); (__h[__k] = __h[__k] ?? []).push(__u); }} return ({recv}).flatMap((__t: {elem_ts}) => {{ const __m = __h[String(({left})(__t))] ?? []; return __m.map((__u: {u_ts}) => ({into})(__t, __u)); }}); }})()"
            ))
        }
        ("leftJoin", [other, left, right, into]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let u_ts = join_other_elem_ts(args, cx);
            let other = lower_expr(other, stmts, cx);
            let left = lower_expr(left, stmts, cx);
            let right = lower_expr(right, stmts, cx);
            let into = lower_expr(into, stmts, cx);
            Some(format!(
                "(() => {{ const __h: Record<string, {u_ts}[]> = {{}}; for (const __u of {other}) {{ const __k = String(({right})(__u)); (__h[__k] = __h[__k] ?? []).push(__u); }} return ({recv}).flatMap((__t: {elem_ts}) => {{ const __m = __h[String(({left})(__t))] ?? []; return __m.length > 0 ? __m.map((__u: {u_ts}) => ({into})(__t, Some(__u))) : [({into})(__t, None)]; }}); }})()"
            ))
        }
        ("join", [other, on, into]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let u_ts = join_other_elem_ts(args, cx);
            let other = lower_expr(other, stmts, cx);
            let on = lower_expr(on, stmts, cx);
            let into = lower_expr(into, stmts, cx);
            Some(format!(
                "(() => {{ const __b: readonly {u_ts}[] = {other}; return ({recv}).flatMap((__t: {elem_ts}) => __b.filter((__u: {u_ts}) => ({on})(__t, __u)).map((__u: {u_ts}) => ({into})(__t, __u))); }})()"
            ))
        }
        ("groupBy", [key, into]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let key = lower_expr(key, stmts, cx);
            let into = lower_expr(into, stmts, cx);
            Some(format!(
                "(() => {{ const __h: Record<string, {elem_ts}[]> = {{}}; const __order: string[] = []; for (const __t of {recv}) {{ const __k = String(({key})(__t)); if (!(__k in __h)) {{ __h[__k] = []; __order.push(__k); }} __h[__k].push(__t); }} return __order.map((__k) => {{ const __rows = __h[__k]; return ({into})(({key})(__rows[0]), __rows); }}); }})()"
            ))
        }
        _ => None,
    }
}

/// v0.94 (ADR 0120): the TS element type of a join's `other` collection (its
/// `List`/`Query` element), for typing the hash-map buckets. Falls back to
/// `unknown` if the checker recorded no type (an already-diagnosed program).
fn join_other_elem_ts(args: &[Expr], cx: &LowerCtx) -> String {
    match cx.commons.expr_types.get(&args[0].span) {
        Some(Ty::List(u) | Ty::Query(u)) => ts_ty(u),
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
    result_ty: Option<&Ty>,
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
                result_ty,
                Some(Ty::Effect(inner)) if matches!(inner.as_ref(), Ty::Option(o) if matches!(o.as_ref(), Ty::Base(BaseType::Duration)))
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
            let res_ts = match result_ty {
                Some(Ty::Effect(inner)) => match inner.as_ref() {
                    Ty::List(el) => ts_ty(el),
                    other => ts_ty(other),
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
            let u_ts = list_ok_elem_ts(result_ty);
            format!(
                "(async () => {{ const __out: {u_ts}[] = []; for (const __x of {source}) {{ const __r = await ({f})(__x); if (__r.tag === \"Err\") {{ return Err(__r.error); }} __out.push(__r.value); }} return Ok(__out); }})()"
            )
        }
        ("parTraverseTry", [f]) => {
            let u_ts = list_ok_elem_ts(result_ty);
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
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<String> {
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
    let v = lower_expr(value, stmts, cx);
    Some(format!(
        "(() => ({var}.{map}__idx_{field}[String({v})] ?? []).map((__pk) => {m}[__pk]))"
    ))
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
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<String> {
    match (method.name.as_str(), args) {
        ("toFloat", []) => Some(lower_expr(receiver, stmts, cx)),
        ("round" | "floor" | "ceil" | "abs", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("Math.{}({recv})", method.name))
        }
        ("truncate", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("Math.trunc({recv})"))
        }
        ("min" | "max", [other]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let other = lower_expr(other, stmts, cx);
            Some(format!("Math.{}({recv}, {other})", method.name))
        }
        ("clamp", [lo, hi]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let lo = lower_expr(lo, stmts, cx);
            let hi = lower_expr(hi, stmts, cx);
            Some(format!("Math.min(Math.max({recv}, {lo}), {hi})"))
        }
        ("isNaN" | "isFinite", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("Number.{}({recv})", method.name))
        }
        // v0.42 (ADR 0074): host number→string — `String(n)` is ECMAScript's
        // Number::toString (shortest round-trip; `1e21`/`Infinity`/`NaN` as the
        // host renders them). The normative contract is the platform's.
        ("toString", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("String({recv})"))
        }
        _ => None,
    }
}

/// v0.86 (ADR 0112): lower a `Duration` kernel method. `toMillis` is the
/// identity (a `Duration` lowers to its milliseconds); `toString` renders it.
fn lower_duration_kernel(
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<String> {
    match (method.name.as_str(), args) {
        ("toMillis", []) => Some(lower_expr(receiver, stmts, cx)),
        ("toString", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("String({recv})"))
        }
        _ => None,
    }
}

/// v0.90 (ADR 0114): lower an `Instant` kernel method. `toEpochMillis` is the
/// identity (an `Instant` lowers to its epoch milliseconds); `toString` renders
/// it.
fn lower_instant_kernel(
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<String> {
    match (method.name.as_str(), args) {
        ("toEpochMillis", []) => Some(lower_expr(receiver, stmts, cx)),
        ("toString", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("String({recv})"))
        }
        _ => None,
    }
}

/// v0.110 (ADR 0142 D3/D4): lower a `Bytes` kernel method. `length` is the
/// `Uint8Array.length` (octet count, not any string length); `toBase64` is a
/// total encode; `decodeUtf8` is a guarded fatal decode returning `Option`
/// (`None` on an invalid UTF-8 sequence).
fn lower_bytes_kernel(
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<String> {
    match (method.name.as_str(), args) {
        ("length", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("({recv}).length"))
        }
        ("toBase64", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("__bynkBytesToBase64({recv})"))
        }
        ("decodeUtf8", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("__bynkBytesDecodeUtf8({recv})"))
        }
        _ => None,
    }
}

/// v0.22a: lower a built-in `String` kernel method (ADR 0046). Pinned
/// semantics: `replace` is replace-**all** (`replaceAll`); `chars()` is
/// code **points** (`[...s]`), not code units; `slice` clamps negative
/// indices to `0` (no TS wrap-around); `indexOf` turns `-1` into `None`.
fn lower_string_kernel(
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<String> {
    match (method.name.as_str(), args) {
        ("length", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("({recv}).length"))
        }
        ("trim", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("{recv}.trim()"))
        }
        ("toUpper", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("{recv}.toUpperCase()"))
        }
        ("toLower", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("{recv}.toLowerCase()"))
        }
        ("chars", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("[...{recv}]"))
        }
        ("split", [sep]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let sep = lower_expr(sep, stmts, cx);
            Some(format!("{recv}.split({sep})"))
        }
        ("contains", [sub]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let sub = lower_expr(sub, stmts, cx);
            Some(format!("{recv}.includes({sub})"))
        }
        ("startsWith" | "endsWith", [sub]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let sub = lower_expr(sub, stmts, cx);
            Some(format!("{recv}.{}({sub})", method.name))
        }
        ("concat", [other]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let other = lower_expr(other, stmts, cx);
            Some(format!("{recv}.concat({other})"))
        }
        ("replace", [from, to]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let from = lower_expr(from, stmts, cx);
            let to = lower_expr(to, stmts, cx);
            Some(format!("{recv}.replaceAll({from}, {to})"))
        }
        ("slice", [lo, hi]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let lo = lower_expr(lo, stmts, cx);
            let hi = lower_expr(hi, stmts, cx);
            Some(format!(
                "{recv}.slice(Math.max(0, {lo}), Math.max(0, {hi}))"
            ))
        }
        ("indexOf", [sub]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let sub = lower_expr(sub, stmts, cx);
            Some(format!(
                "((__i: number) => __i < 0 ? None : Some(__i))({recv}.indexOf({sub}))"
            ))
        }
        _ => None,
    }
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
    inner: &Ty,
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<String> {
    let t = ts_ty(inner);
    match (method.name.as_str(), args) {
        ("map", [f]) => {
            // The call's checked type is `Option[B]` — peel for B.
            let b = match cx.commons.expr_types.get(&e.span) {
                Some(Ty::Option(b)) => ts_ty(b),
                _ => "unknown".to_string(),
            };
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "((__o: Option<{t}>, __f: (x: {t}) => {b}) => __o.tag === \"Some\" ? Some(__f(__o.value)) : None)({recv}, {f})"
            ))
        }
        ("andThen", [f]) => {
            let b = match cx.commons.expr_types.get(&e.span) {
                Some(Ty::Option(b)) => ts_ty(b),
                _ => "unknown".to_string(),
            };
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "((__o: Option<{t}>, __f: (x: {t}) => Option<{b}>) => __o.tag === \"Some\" ? __f(__o.value) : None)({recv}, {f})"
            ))
        }
        ("getOrElse", [fallback]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let fallback = lower_expr(fallback, stmts, cx);
            Some(format!(
                "((__o: Option<{t}>, __d: {t}) => __o.tag === \"Some\" ? __o.value : __d)({recv}, {fallback})"
            ))
        }
        ("isSome", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("({recv}.tag === \"Some\")"))
        }
        ("okOr", [error]) => {
            // The call's checked type is `Result[T, E]` — peel for E.
            let err = match cx.commons.expr_types.get(&e.span) {
                Some(Ty::Result(_, err)) => ts_ty(err),
                _ => "unknown".to_string(),
            };
            let recv = lower_expr(receiver, stmts, cx);
            let error = lower_expr(error, stmts, cx);
            Some(format!(
                "((__o: Option<{t}>, __e: {err}) => __o.tag === \"Some\" ? Ok(__o.value) : Err(__e))({recv}, {error})"
            ))
        }
        _ => None,
    }
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
    ok: &Ty,
    err: &Ty,
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<String> {
    let t = ts_ty(ok);
    let et = ts_ty(err);
    match (method.name.as_str(), args) {
        ("map", [f]) => {
            // The call's checked type is `Result[B, E]` — peel for B.
            let b = match cx.commons.expr_types.get(&e.span) {
                Some(Ty::Result(b, _)) => ts_ty(b),
                _ => "unknown".to_string(),
            };
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "((__r: Result<{t}, {et}>, __f: (x: {t}) => {b}) => __r.tag === \"Ok\" ? Ok(__f(__r.value)) : __r)({recv}, {f})"
            ))
        }
        ("andThen", [f]) => {
            let b = match cx.commons.expr_types.get(&e.span) {
                Some(Ty::Result(b, _)) => ts_ty(b),
                _ => "unknown".to_string(),
            };
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "((__r: Result<{t}, {et}>, __f: (x: {t}) => Result<{b}, {et}>) => __r.tag === \"Ok\" ? __f(__r.value) : __r)({recv}, {f})"
            ))
        }
        ("mapErr", [f]) => {
            // The call's checked type is `Result[T, F]` — peel for F.
            let fts = match cx.commons.expr_types.get(&e.span) {
                Some(Ty::Result(_, f)) => ts_ty(f),
                _ => "unknown".to_string(),
            };
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "((__r: Result<{t}, {et}>, __f: (e: {et}) => {fts}) => __r.tag === \"Err\" ? Err(__f(__r.error)) : __r)({recv}, {f})"
            ))
        }
        ("getOrElse", [fallback]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let fallback = lower_expr(fallback, stmts, cx);
            Some(format!(
                "((__r: Result<{t}, {et}>, __d: {t}) => __r.tag === \"Ok\" ? __r.value : __d)({recv}, {fallback})"
            ))
        }
        ("isOk", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("({recv}.tag === \"Ok\")"))
        }
        _ => None,
    }
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
    ok: &Ty,
    err: &Ty,
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<String> {
    let t = ts_ty(ok);
    let et = ts_ty(err);
    // The call's checked type is `Effect[Result[a, b]]`; peel `(a, b)` for the
    // transformed parameter's TS type.
    let (a_ts, b_ts) = match cx.commons.expr_types.get(&e.span) {
        Some(Ty::Effect(inner)) => match inner.as_ref() {
            Ty::Result(a, b) => (ts_ty(a), ts_ty(b)),
            _ => ("unknown".to_string(), "unknown".to_string()),
        },
        _ => ("unknown".to_string(), "unknown".to_string()),
    };
    match (method.name.as_str(), args) {
        ("mapOk", [f]) => {
            // result `Effect[Result[U, E]]` — `a_ts` is `U`.
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "(async (__e: Promise<Result<{t}, {et}>>, __f: (x: {t}) => {a_ts}) => {{ const __r = await __e; return __r.tag === \"Ok\" ? Ok(__f(__r.value)) : __r; }})({recv}, {f})"
            ))
        }
        ("mapErr", [f]) => {
            // result `Effect[Result[T, F]]` — `b_ts` is `F`.
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "(async (__e: Promise<Result<{t}, {et}>>, __f: (e: {et}) => {b_ts}) => {{ const __r = await __e; return __r.tag === \"Err\" ? Err(__f(__r.error)) : __r; }})({recv}, {f})"
            ))
        }
        ("flatMapOk", [f]) => {
            // `f: T -> Effect[Result[U, E]]`; result `Effect[Result[U, E]]`.
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "(async (__e: Promise<Result<{t}, {et}>>, __f: (x: {t}) => Promise<Result<{a_ts}, {et}>>) => {{ const __r = await __e; return __r.tag === \"Ok\" ? await __f(__r.value) : __r; }})({recv}, {f})"
            ))
        }
        ("flatMapErr", [f]) => {
            // `f: E -> Effect[Result[T, F]]`; result `Effect[Result[T, F]]`.
            let recv = lower_expr(receiver, stmts, cx);
            let f = lower_expr(f, stmts, cx);
            Some(format!(
                "(async (__e: Promise<Result<{t}, {et}>>, __f: (e: {et}) => Promise<Result<{t}, {b_ts}>>) => {{ const __r = await __e; return __r.tag === \"Err\" ? await __f(__r.error) : __r; }})({recv}, {f})"
            ))
        }
        _ => None,
    }
}

/// v0.20b: lower a built-in `Map` kernel method. `insert` copies — the
/// emitted `ReadonlyMap` is never mutated in place; updating an existing key
/// keeps its insertion position (JS `Map` semantics, normative in §7).
fn lower_map_kernel(
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    key: &Ty,
    val: &Ty,
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<String> {
    let key_ts = ts_ty(key);
    let val_ts = ts_ty(val);
    match (method.name.as_str(), args) {
        ("length", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("({recv}).size"))
        }
        ("keys", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("[...({recv}).keys()]"))
        }
        // v0.149 (ADR 0173): the values in key order — the `keys()` sibling over
        // the in-memory `ReadonlyMap`.
        ("values", []) => {
            let recv = lower_expr(receiver, stmts, cx);
            Some(format!("[...({recv}).values()]"))
        }
        ("get", [k]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let k = lower_expr(k, stmts, cx);
            Some(format!(
                "((__m: ReadonlyMap<{key_ts}, {val_ts}>, __k: {key_ts}) => __m.has(__k) ? Some(__m.get(__k) as {val_ts}) : None)({recv}, {k})"
            ))
        }
        ("insert", [k, v]) => {
            let recv = lower_expr(receiver, stmts, cx);
            let k = lower_expr(k, stmts, cx);
            let v = lower_expr(v, stmts, cx);
            Some(format!("new Map({recv}).set({k}, {v})"))
        }
        _ => None,
    }
}

fn lower_if(
    cond: &Expr,
    then_block: &Block,
    else_block: &Block,
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> String {
    let cond_expr = lower_expr(cond, stmts, cx);
    // If the cond contains `is`-bindings, the then-branch needs a place
    // for the `const name = receiver.field;` declarations — a ternary
    // has no such place. Force the IIFE form.
    if both_simple(then_block, else_block) && !cond_has_is_bindings(cond, cx) {
        let mut tstmts = Vec::new();
        let testr = lower_expr(&then_block.tail, &mut tstmts, cx);
        debug_assert!(tstmts.is_empty());
        let mut estmts = Vec::new();
        let eestr = lower_expr(&else_block.tail, &mut estmts, cx);
        debug_assert!(estmts.is_empty());
        format!("({cond_expr} ? {testr} : {eestr})")
    } else {
        let mut iife = String::new();
        iife.push_str("(() => {\n");
        iife.push_str("    if (");
        iife.push_str(&cond_expr);
        iife.push_str(") {\n");
        // Inject is-binding declarations on the truthy side.
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
        emit_block_as_function_body(&mut iife, then_block, cx, INDENT_STEP * 3, false);
        for _ in 0..(INDENT_STEP * 2) {
            iife.push(' ');
        }
        iife.push_str("} else {\n");
        emit_block_as_function_body(&mut iife, else_block, cx, INDENT_STEP * 3, false);
        for _ in 0..(INDENT_STEP * 2) {
            iife.push(' ');
        }
        iife.push_str("}\n");
        for _ in 0..INDENT_STEP {
            iife.push(' ');
        }
        iife.push_str("})()");
        iife
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
        ExprKind::Is {
            value,
            pattern: Pattern::Variant {
                variant, bindings, ..
            },
        } => {
            bindings.iter().any(|b| !b.is_wildcard())
                // v0.13: a refined `is`-narrowing introduces a (shadow) binding
                // even though the pattern carries none.
                || (bindings.is_empty() && cx.is_refined_is_check(value, &variant.name))
        }
        ExprKind::BinOp(BinOp::And, l, r) | ExprKind::BinOp(BinOp::Or, l, r) => {
            cond_has_is_bindings(l, cx) || cond_has_is_bindings(r, cx)
        }
        ExprKind::Paren(inner) => cond_has_is_bindings(inner, cx),
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
    let mut pre = Vec::new();
    let cond_expr = lower_expr(cond, &mut pre, cx);
    for s in &pre {
        write_line(out, indent, s);
    }
    write_line(out, indent, &format!("if ({cond_expr}) {{"));
    // is-binding declarations on the truthy path.
    let mut is_bindings = Vec::new();
    let mut found = false;
    gather_is_bindings_for_emit(cond, cx, &mut is_bindings, &mut found);
    for b in &is_bindings {
        write_line(out, indent + INDENT_STEP, b);
    }
    emit_block_as_function_body(out, then_block, cx, indent + INDENT_STEP, async_tail);
    write_line(out, indent, "} else {");
    emit_block_as_function_body(out, else_block, cx, indent + INDENT_STEP, async_tail);
    write_line(out, indent, "}");
}

fn both_simple(a: &Block, b: &Block) -> bool {
    a.statements.is_empty()
        && b.statements.is_empty()
        && simple_expr(&a.tail)
        && simple_expr(&b.tail)
}

fn simple_expr(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Question(_) => false,
        ExprKind::Match { .. } => false,
        ExprKind::Block(b) => b.statements.is_empty() && simple_expr(&b.tail),
        ExprKind::If {
            then_block,
            else_block,
            cond,
        } => simple_expr(cond) && both_simple(then_block, else_block),
        ExprKind::Ok(i) | ExprKind::Err(i) | ExprKind::Some(i) => simple_expr(i),
        ExprKind::Paren(i) | ExprKind::UnaryOp(_, i) => simple_expr(i),
        ExprKind::BinOp(_, l, r) => simple_expr(l) && simple_expr(r),
        ExprKind::Call { args, .. } | ExprKind::ConstructorCall { args, .. } => {
            args.iter().all(simple_expr)
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            simple_expr(receiver) && args.iter().all(simple_expr)
        }
        ExprKind::FieldAccess { receiver, .. } => simple_expr(receiver),
        ExprKind::RecordConstruction { fields, .. } => fields.iter().all(|f| match &f.value {
            Some(v) => simple_expr(v),
            None => true,
        }),
        ExprKind::Is { value, .. } => simple_expr(value),
        _ => true,
    }
}

fn lower_ident(e: &Expr, id: &Ident, cx: &mut LowerCtx) -> String {
    // v0.116: inside a `transition` predicate, the contextual `old`/`new` idents
    // are the pre-/post-commit state records, lowered to their JS names (`new` is
    // reserved, so both are renamed). Field access `old.<f>` reads off the record.
    // Checked first so `old`/`new` never collide with the heuristics below.
    if let Some((old_var, new_var)) = &cx.transition_states {
        if id.name == "old" {
            return old_var.clone();
        }
        if id.name == "new" {
            return new_var.clone();
        }
    }
    // v0.80: inside an invariant predicate, a bare ident naming a state field
    // reads it off the proposed-state value (`s.<field>`). Checked first so a
    // field never collides with the variant-constructor heuristics below.
    if let Some((var, fields)) = &cx.invariant_state
        && fields.contains(&id.name)
    {
        return format!("{var}.{}", id.name);
    }
    // v0.81: inside a `store`-agent handler, a bare ident naming a `Cell` field
    // reads it off the mutable working state (`__state.<cell>`), so a read after
    // a `:=` write in the same handler sees the written value (read-your-writes).
    if let Some((var, cells)) = &cx.agent_store_state
        && cells.contains(&id.name)
    {
        return format!("{var}.{}", id.name);
    }
    // v0.104/v0.105 (real-time track slice 3b): a bare held-`Map` ident used as a
    // value is a lazy `Query` over its **resolved** connections — the persisted
    // `Record<K, connId>` mapped through `resolveConnection`, keeping the present
    // ones. Checked before the persisted-`Map` branch (held maps are excluded from
    // `agent_store_maps`).
    if let Some(f_ts) = cx.agent_held_maps.get(&id.name) {
        let var = cx
            .agent_store_state
            .as_ref()
            .map(|(v, _)| v.as_str())
            .unwrap_or("__state");
        return format!(
            "(() => Object.values({var}.{name}).flatMap((__cid) => {{ const __c = resolveConnection<{f_ts}>(this.state, __cid); return __c.tag === \"Some\" ? [__c.value] : []; }}))",
            name = id.name
        );
    }
    // v0.94 (ADR 0120): a bare `store Map` ident used as a **value** — not a
    // method receiver (those are handled in the method dispatch) — is a lazy
    // `Query` over the whole map, e.g. the `other` side of a join. It lowers to
    // the same deferred thunk a query builder yields: `() => Object.values(map)`.
    if cx.agent_store_maps.contains(&id.name) {
        let var = cx
            .agent_store_state
            .as_ref()
            .map(|(v, _)| v.as_str())
            .unwrap_or("__state");
        return format!("(() => Object.values({var}.{}))", id.name);
    }
    // v0.95 (ADR 0121): a bare `store Log` ident used as a value is a lazy
    // `Query` over its entry values — `() => log.map((__e) => __e.v)`.
    if cx.agent_store_logs.contains_key(&id.name) {
        let var = cx
            .agent_store_state
            .as_ref()
            .map(|(v, _)| v.as_str())
            .unwrap_or("__state");
        return format!("(() => {var}.{}.map((__e) => __e.v))", id.name);
    }
    // v0.9: a nullary HttpResult variant (whose checker type is
    // `HttpResult[_]`) constructs an HttpResult.<Variant>.
    if matches!(cx.commons.expr_types.get(&e.span), Some(Ty::HttpResult(_)))
        && http_variant(&id.name).is_some()
    {
        return format!("HttpResult.{}", id.name);
    }
    // v0.44: a nullary QueueResult variant (`Ack`) constructs `QueueResult.Ack`.
    if matches!(cx.commons.expr_types.get(&e.span), Some(Ty::QueueResult))
        && bynk_syntax::ast::queue_variant(&id.name).is_some()
    {
        return format!("QueueResult.{}", id.name);
    }
    // A bare ident whose name matches a declared variant of a sum
    // type (and whose checker type is that sum) is a nullary
    // variant constructor reference. Qualify it as `Type.Variant`.
    // Otherwise (locals, params, `self`) emit the identifier as-is.
    if let Some(Ty::Named {
        kind: NamedKind::Sum,
        name: type_name,
    }) = cx.commons.expr_types.get(&e.span)
        && let Some(decl) = cx.commons.types.get(type_name)
        && let TypeBody::Sum(s) = &decl.body
        && s.variants.iter().any(|v| v.name.name == id.name)
    {
        return format!("{}.{}", type_name, id.name);
    }
    // v0.52: the multi-actor sum binder is not a runtime local — the resolved
    // actor is threaded through `deps.who` at the boundary wrapper, so the
    // binder ident lowers to it (the tagged union the body `match`es).
    if cx.actor_sum_binder.as_deref() == Some(id.name.as_str())
        && matches!(cx.commons.expr_types.get(&e.span), Some(Ty::ActorSum(_)))
    {
        return "deps.who".to_string();
    }
    ts_ident(&id.name)
}

fn lower_call(
    e: &Expr,
    name: &Ident,
    args: &[Expr],
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> String {
    // Bare variant constructor with payload → qualify.
    let args_lowered: Vec<String> = args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
    // v0.9: HttpResult variant call.
    if matches!(cx.commons.expr_types.get(&e.span), Some(Ty::HttpResult(_)))
        && http_variant(&name.name).is_some()
    {
        return format!("HttpResult.{}({})", name.name, args_lowered.join(", "));
    }
    // v0.44: a QueueResult variant call (`Retry(reason)`) → `QueueResult.Retry(...)`.
    if matches!(cx.commons.expr_types.get(&e.span), Some(Ty::QueueResult))
        && bynk_syntax::ast::queue_variant(&name.name).is_some()
    {
        return format!("QueueResult.{}({})", name.name, args_lowered.join(", "));
    }
    // v0.9.2: agent instantiation `AgentName(key)` lowers to the
    // generated `__makeAgentName(key)` factory, which obtains the
    // instance for this key (lookup-or-create against the registry in
    // bundle mode, or a typed DO proxy in workers mode). Skipped when
    // this Call is the receiver of a MethodCall — that path folds
    // construction and the method invocation together.
    if cx.local_agents.contains(&name.name) && args_lowered.len() == 1 {
        return cx.agent_construct(&name.name, &args_lowered[0]);
    }
    if let Some(Ty::Named {
        kind: NamedKind::Sum,
        name: type_name,
    }) = cx.commons.expr_types.get(&e.span)
        && type_name != &name.name
        && call_is_sum_variant(cx, type_name, &name.name)
    {
        return format!("{}.{}({})", type_name, name.name, args_lowered.join(", "));
    }
    // #527: a commons-imported fn speaks the *unbranded* commons types, but
    // this context rebrands them (`Event & { __ctxBrand }`); assert the call
    // back into the local namespace so branded positions accept it. The brand
    // is phantom — the value is identical. Pure fns only: an effectful call
    // is awaited by its caller, and the assertion would need to target the
    // Promise, not the value.
    if cx.commons_imported_fns.contains(&name.name)
        && let Some(f) = cx.commons.fns.get(&name.name)
        && !matches!(f.return_type, TypeRef::Effect(..))
        && typeref_mentions_any(&f.return_type, &cx.rebranded_types)
    {
        return format!(
            "({}({}) as {})",
            ts_ident(&name.name),
            args_lowered.join(", "),
            ts_type_ref(&f.return_type)
        );
    }
    format!("{}({})", ts_ident(&name.name), args_lowered.join(", "))
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
        TypeRef::Base(..)
        | TypeRef::QueueResult(_)
        | TypeRef::ValidationError(_)
        | TypeRef::JsonError(_)
        | TypeRef::Unit(_) => false,
    }
}

fn lower_bin_op(
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> String {
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
        && let Some((bindings, lhs_expr, rhs_expr)) = lower_and_with_is(lhs, rhs, stmts, cx)
    {
        if bindings.is_empty() {
            return format!("{lhs_expr} && {rhs_expr}");
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
        return wrap;
    }
    // v0.80: `P implies Q` lowers to `(!(P) || Q)`. As with `&&`, an `is` test in
    // the antecedent binds into the consequent (the consequent is only reached
    // when the antecedent holds), so reuse the same is-binding IIFE flow.
    if op == BinOp::Implies
        && let Some((bindings, lhs_expr, rhs_expr)) = lower_and_with_is(lhs, rhs, stmts, cx)
    {
        if bindings.is_empty() {
            return format!("(!({lhs_expr}) || {rhs_expr})");
        }
        let mut wrap = String::new();
        wrap.push_str(&format!("(!({lhs_expr}) || ((() => {{ "));
        for b in &bindings {
            wrap.push_str(b);
            wrap.push(' ');
        }
        wrap.push_str(&format!("return {rhs_expr}; }})()))"));
        return wrap;
    }
    let l = lower_expr(lhs, stmts, cx);
    let r = lower_expr(rhs, stmts, cx);
    if op == BinOp::Implies {
        // `P implies Q` ≡ `!P || Q` (no `is` bindings in the antecedent).
        return format!("(!({l}) || {r})");
    }
    if op == BinOp::Div {
        // v0.21: division is operand-typed (ADR 0042) — `Float`
        // true-divides; `Int` keeps truncating. The checker rejects
        // mixed operands, so the left operand decides; a missing
        // type entry falls back to the `Int` (truncating) lowering.
        let lhs_is_float =
            cx.commons.expr_types.get(&lhs.span).and_then(|t| t.base()) == Some(BaseType::Float);
        if lhs_is_float {
            format!("{l} / {r}")
        } else {
            format!("Math.trunc({l} / {r})")
        }
    } else if matches!(op, BinOp::Eq | BinOp::NotEq)
        && cx.commons.expr_types.get(&lhs.span).and_then(|t| t.base()) == Some(BaseType::Bytes)
    {
        // v0.110 (ADR 0142 D4): `Bytes` is the one base type whose `==` is not
        // host `===`. It erases to `Uint8Array`, so `===` is reference equality
        // (`Bytes.fromUtf8("a") === Bytes.fromUtf8("a")` is `false`). Equality
        // must compare by content — operand-typed dispatch, exactly like `Div`.
        // The checker rejects mixed operands, so the left operand decides.
        let eq = format!("__bynkBytesEqual({l}, {r})");
        if op == BinOp::Eq {
            eq
        } else {
            format!("!{eq}")
        }
    } else {
        format!("{l} {} {r}", ts_binop(op))
    }
}

fn lower_constructor_call(
    type_name: &Ident,
    method: &Ident,
    args: &[Expr],
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> String {
    let args: Vec<String> = args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
    // Nullary variant qualified construction: `T.V` (no parens) at the
    // source level wouldn't reach here, so `T.V()` always means call.
    format!("{}.{}({})", type_name.name, method.name, args.join(", "))
}

fn lower_record_construction(
    type_name: &Ident,
    fields: &[FieldInit],
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> String {
    let mut parts = Vec::new();
    for f in fields {
        match &f.value {
            Some(v) => {
                let val = lower_expr(v, stmts, cx);
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
    format!("{{ {} }}", parts.join(", "))
}

fn lower_field_access(
    receiver: &Expr,
    field: &Ident,
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> String {
    // v0.9: `HttpResult.Variant` (nullary).
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == HTTP_RESULT
        && http_variant(&field.name).is_some()
    {
        return format!("HttpResult.{}", field.name);
    }
    // Agent-handler `self.<key>` rewrite.
    if cx.in_agent_handler
        && let ExprKind::Ident(id) = &receiver.kind
        && id.name == "self"
        && let Some(k) = &cx.agent_key_field
        && field.name == *k
    {
        return format!("(this.state.id.toString() as {})", k);
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
            cx.commons.expr_types.get(&receiver.span),
            Some(Ty::Actor(_))
        )
    {
        if let (Some(binder), ExprKind::Ident(id)) = (&cx.deps_identity_binder, &receiver.kind)
            && &id.name == binder
        {
            return "deps.identity".to_string();
        }
        return "undefined".to_string();
    }
    let r = lower_expr(receiver, stmts, cx);
    // `.raw` on an opaque value compiles to a TypeScript type
    // assertion back to the base type. The checker has already
    // verified that the receiver is opaque and the call site is
    // inside the defining commons.
    if field.name == RAW
        && let Some(Ty::Named {
            kind: NamedKind::Opaque(base),
            ..
        }) = cx.commons.expr_types.get(&receiver.span)
    {
        return format!("({r} as {})", ts_base(*base));
    }
    format!("{r}.{}", field.name)
}

fn lower_lambda(e: &Expr, lambda: &LambdaExpr, cx: &mut LowerCtx) -> String {
    let is_async = matches!(
        cx.commons.expr_types.get(&e.span),
        Some(bynk_check::checker::Ty::Fn { ret, .. }) if ret.is_effect()
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
    match &lambda.body.kind {
        ExprKind::Block(b) => {
            let mut out = format!("{prefix}({params}) => {{\n");
            emit_block_as_function_body(&mut out, b, cx, INDENT_STEP * 2, is_async);
            for _ in 0..INDENT_STEP {
                out.push(' ');
            }
            out.push('}');
            out
        }
        _ => {
            let mut body_stmts: Vec<String> = Vec::new();
            let body = lower_expr(&lambda.body, &mut body_stmts, cx);
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
    }
}

fn lower_record_spread(
    base: &Expr,
    overrides: &[FieldInit],
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> String {
    let base_expr = lower_expr(base, stmts, cx);
    let mut parts = vec![format!("...{base_expr}")];
    for f in overrides {
        match &f.value {
            Some(v) => {
                let val = lower_expr(v, stmts, cx);
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
    format!("{{ {} }}", parts.join(", "))
}

fn lower_block_as_expr(b: &Block, cx: &mut LowerCtx) -> String {
    let mut iife = String::new();
    iife.push_str("(() => {\n");
    // IIFE is a synchronous arrow function; the surrounding expression context
    // expects a concrete value, so `Effect.pure(...)` must still wrap as
    // `Promise.resolve(...)`.
    emit_block_as_function_body(&mut iife, b, cx, INDENT_STEP * 2, false);
    for _ in 0..INDENT_STEP {
        iife.push(' ');
    }
    iife.push_str("})()");
    iife
}

fn lower_match_as_iife(discriminant: &Expr, arms: &[MatchArm], cx: &mut LowerCtx) -> String {
    let disc_ty = cx.commons.expr_types.get(&discriminant.span).cloned();
    let mut stmts = Vec::new();
    let disc = lower_expr(discriminant, &mut stmts, cx);
    let mut iife = String::new();
    // Pre-statements need to be evaluated before the IIFE; lift them into
    // a sequence: `(prestmt1, prestmt2, iife)`. Since TS doesn't let us
    // evaluate statements inline, we wrap in another arrow.
    let inner_iife = maybe_async_iife(build_match_iife(&disc, &disc_ty, arms, cx));
    if stmts.is_empty() {
        iife.push_str(&inner_iife);
    } else {
        iife.push_str("(() => {\n");
        for s in &stmts {
            for _ in 0..(INDENT_STEP * 2) {
                iife.push(' ');
            }
            iife.push_str(s);
            iife.push('\n');
        }
        for _ in 0..(INDENT_STEP * 2) {
            iife.push(' ');
        }
        iife.push_str("return ");
        iife.push_str(&inner_iife);
        iife.push_str(";\n");
        for _ in 0..INDENT_STEP {
            iife.push(' ');
        }
        iife.push_str("})()");
        iife = maybe_async_iife(iife);
    }
    iife
}

/// v0.9.2: a match lowered to an IIFE in *expression* position may have arms
/// that `await` (an effectful `let x <- …` or an effectful tail). A synchronous
/// arrow can't host `await`, so when the lowered body contains one, make the
/// outermost arrow `async` and `await` its call. Nested matches that already
/// did this surface their own `await`, which propagates the transform outward.
fn maybe_async_iife(iife: String) -> String {
    if !iife.contains("await ") {
        return iife;
    }
    let async_iife = if let Some(rest) = iife.strip_prefix("((__d) =>") {
        format!("(async (__d) =>{rest}")
    } else if let Some(rest) = iife.strip_prefix("(() => {") {
        format!("(async () => {{{rest}")
    } else {
        return iife;
    };
    format!("await {async_iife}")
}

fn build_match_iife(
    disc_expr: &str,
    disc_ty: &Option<Ty>,
    arms: &[MatchArm],
    cx: &mut LowerCtx,
) -> String {
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
        let scrutinee = if is_literal_match(disc_ty) {
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
fn is_literal_match(disc_ty: &Option<Ty>) -> bool {
    match disc_ty {
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
    // Anchor the discriminant lowering + `switch (…) {` header to the match's
    // scrutinee span (slice 1); each arm re-anchors to its own span below.
    cx.record_span(out.len(), discriminant.span);
    let mut pre = Vec::new();
    let mut disc = lower_expr(discriminant, &mut pre, cx);
    let disc_ty = cx.commons.expr_types.get(&discriminant.span).cloned();
    for s in &pre {
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
    let scrutinee = if is_literal_match(&disc_ty) {
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
    disc_ty: &Option<Ty>,
    arm: &MatchArm,
    cx: &mut LowerCtx,
    indent: usize,
    async_tail: bool,
) {
    // Anchor this arm's `case`/binding/`return` lines to the arm's source span
    // (slice 1, ADR 0103 D2) — so stepping a `match` walks arm-to-arm.
    cx.record_span(out.len(), arm.span);
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
                        cx.positional_field_name(disc_ty.as_ref(), &variant.name, i)
                    }
                };
                let local = ts_ident(&name.name);
                write_line(
                    out,
                    indent + INDENT_STEP,
                    &format!("const {local} = {disc_var}.{field};"),
                );
            }
            emit_match_body(out, &arm.body, cx, indent + INDENT_STEP, async_tail);
            write_line(out, indent, "}");
        }
    }
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
            let mut stmts = Vec::new();
            let v = lower_tail_expr(e, &mut stmts, cx, async_tail);
            for s in &stmts {
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
fn match_needs_if_chain(arms: &[MatchArm]) -> bool {
    arms.iter()
        .any(|a| a.guard.is_some() || pattern_has_nested_test(&a.pattern))
}

/// True when `pat` carries a payload sub-pattern that is itself refutable (a
/// nested variant/literal) — i.e. it cannot be tested by a single `.tag` switch.
fn pattern_has_nested_test(pat: &Pattern) -> bool {
    match pat {
        Pattern::Variant { bindings, .. } => bindings.iter().any(|b| {
            let sp = b.pattern();
            !sp.is_irrefutable() || pattern_has_nested_test(sp)
        }),
        _ => false,
    }
}

/// Emit the boolean tests that must hold for `pattern` to match the value at
/// runtime `path` (static type `path_ty`). Irrefutable patterns add nothing;
/// nested variant/literal payloads recurse into `path.<field>`.
fn pattern_match_tests(
    path: &str,
    path_ty: Option<&Ty>,
    pattern: &Pattern,
    cx: &LowerCtx,
    tests: &mut Vec<String>,
) {
    match pattern {
        Pattern::Wildcard(_) | Pattern::Binding(_) => {}
        Pattern::Literal { value, .. } => {
            tests.push(format!("{path} === {}", literal_case_label(value)));
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
                        cx.positional_field_name(path_ty, &variant.name, i)
                    }
                };
                let field_ty = cx.payload_field_ty(path_ty, &variant.name, i);
                pattern_match_tests(&format!("{path}.{field}"), field_ty.as_ref(), sp, cx, tests);
            }
        }
    }
}

/// Emit `const` declarations binding the names in `pattern` from runtime `path`,
/// recursing through nested payloads (ADR 0169).
fn emit_pattern_bindings(
    out: &mut String,
    indent: usize,
    path: &str,
    path_ty: Option<&Ty>,
    pattern: &Pattern,
    cx: &LowerCtx,
) {
    match pattern {
        Pattern::Wildcard(_) | Pattern::Literal { .. } => {}
        Pattern::Binding(id) => {
            write_line(
                out,
                indent,
                &format!("const {} = {path};", ts_ident(&id.name)),
            );
        }
        Pattern::Variant {
            variant, bindings, ..
        } => {
            for (i, b) in bindings.iter().enumerate() {
                let field = match &b.kind {
                    PatternBindingKind::Named { field, .. } => field.name.clone(),
                    PatternBindingKind::Positional { .. } => {
                        cx.positional_field_name(path_ty, &variant.name, i)
                    }
                };
                let field_ty = cx.payload_field_ty(path_ty, &variant.name, i);
                emit_pattern_bindings(
                    out,
                    indent,
                    &format!("{path}.{field}"),
                    field_ty.as_ref(),
                    b.pattern(),
                    cx,
                );
            }
        }
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
    disc_ty: &Option<Ty>,
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
        let mut tests = Vec::new();
        pattern_match_tests(disc_var, disc_ty.as_ref(), &arm.pattern, cx, &mut tests);
        let has_tests = !tests.is_empty();
        if has_tests {
            write_line(out, indent, &format!("if ({}) {{", tests.join(" && ")));
        }
        let body_indent = if has_tests {
            indent + INDENT_STEP
        } else {
            indent
        };
        emit_pattern_bindings(
            out,
            body_indent,
            disc_var,
            disc_ty.as_ref(),
            &arm.pattern,
            cx,
        );
        if let Some(guard) = &arm.guard {
            let mut gstmts = Vec::new();
            let gv = lower_expr(guard, &mut gstmts, cx);
            for s in &gstmts {
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
    }
    if !has_catchall {
        write_line(out, indent, "throw new Error(\"non-exhaustive match\");");
    }
}

fn lower_is(value: &Expr, pattern: &Pattern, stmts: &mut Vec<String>, cx: &mut LowerCtx) -> String {
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
        }) = cx.commons.types.get(&variant.name).map(|d| d.body.clone())
    {
        let recv = cx.is_receiver_ref_forced(value, stmts);
        return refined_check_as_bool(&recv, base, refinement.as_ref());
    }
    let v = cx.is_receiver_ref(value, stmts);
    match pattern {
        Pattern::Wildcard(_) => "true".to_string(),
        // A bare name binding after `is` matches anything over a sum (ADR 0169).
        Pattern::Binding(_) => "true".to_string(),
        // v0.130 (DECISION F): the checker rejects a literal on the RHS of `is`,
        // so this is unreachable for a well-typed program; lower it to the
        // value-equality it would mean, defensively.
        Pattern::Literal { value, .. } => {
            format!("{v} === {}", literal_case_label(value))
        }
        Pattern::Variant { variant, .. } => {
            format!("{v}.tag === \"{}\"", variant.name)
        }
    }
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
