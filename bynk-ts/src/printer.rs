//! The printer — `TsProgram -> Artefacts` (R7.3: "printing is `TsProgram ->
//! Artefacts`. The printer owns the buffer, the indentation and the offset
//! arithmetic. It is the only code in the compiler that writes a
//! character."). Owns one buffer for the whole [`TsProgram`], so unlike
//! `bynk-emit`'s own splice-based lowering (`crate::source_map`'s own module
//! doc), it never needs to rebase a sub-buffer's checkpoints — it records
//! directly from each statement's own `span` as it writes, which is what
//! makes R7.4 ("the source map is produced by the printer from
//! `TsNode.span`. No phase before the printer records an offset") true by
//! construction for this path, from this slice.
//!
//! # Readability policy (R7.5)
//!
//! R7.5: "Readable output is a printer policy with a name and a test, not a
//! property of how carefully strings were typed." P7.7 named the policy's
//! then-whole surface (statement separation); P7.8 (#1313) extends it now
//! that the printer has real structured nodes to have an opinion about
//! (`TsExpr`/`TsType`/`TsDecl`, plus real `TsStmt` variants):
//!
//! - **Every statement starts on its own generated line, so two statements
//!   can never share one.** Unchanged from P7.7 — see [`print()`]'s own
//!   loop. Still true for the new node kinds: every one of their own
//!   renderers ends its own output in `\n`.
//! - **Two-space indentation, one level per nesting depth.** The only
//!   indentation width this printer produces — chosen because it's the one
//!   `events_fanout.rs` (this slice's own grounding file) already uses, not
//!   because of any earlier general readability decision; matches the
//!   depth threaded through [`render_stmt`]/[`render_block_body`] exactly.
//! - **One blank line separates top-level declarations, except two
//!   consecutive `import`s or two consecutive `Comment`s.** [`print()`]'s
//!   own loop implements this by peeking at the next statement — matching
//!   `events_fanout.rs`'s own real spacing (its two `import` lines sit
//!   adjacent, and so do its own two header-comment lines; every other pair
//!   of top-level declarations has a blank line between). Added for Arc C's
//!   own first real conversion slice (#1317): every `bynk-emit`-generated
//!   file opens with a multi-line header banner, one [`TsStmtKind::Comment`]
//!   per real line.
//! - **Inside a `class`, no blank line between fields and the constructor;
//!   one blank line before each method.** [`render_decl_body`]'s own
//!   `TsDecl::Class` arm — again, this is what `events_fanout.rs` itself
//!   does, not a general rule derived some other way.
//! - **`if`/`for...of`'s own body prints with braces when it's a
//!   `TsStmtKind::Block`, and inline on the same line otherwise** —
//!   `if (!Array.isArray(subs)) continue;` has no braces in the grounding
//!   file; a `for...of` always does. See [`render_branch`].
//! - **A statement's own interior is not the printer's concern for
//!   `Verbatim` content — still.** `Verbatim` text renders exactly as
//!   written (P7.7's own boundary, unchanged); everything above applies
//!   only to the new, real node kinds this slice adds.
//!
//! None of the above is claimed as *the* TypeScript style this printer will
//! use forever — it's what this slice's own grounding file needs, named
//! rather than left implicit, the same posture the node algebra itself
//! takes (`program.rs`'s own module doc). A future file with a construct
//! this policy doesn't cover yet (an `else` branch, a multi-field class with
//! blank lines between fields, …) extends it the same way `program.rs`'s
//! own node list grows: file by file, against real content.

use crate::program::{
    TsBinaryOp, TsBindingName, TsDecl, TsExpr, TsLit, TsParam, TsProgram, TsStmt, TsStmtKind,
    TsType, TsUnaryOp,
};
use crate::source_map::SourceMapBuilder;

/// The result of printing a [`TsProgram`]: the emitted text, and its source
/// map — `None` when no checkpoint resolved, either because no statement
/// carried a span or every span fell outside `source_text`
/// ([`SourceMapBuilder::to_v3`]'s own "nothing resolves" case).
#[derive(Debug)]
pub struct Printed {
    pub text: String,
    pub source_map: Option<String>,
}

/// Print `program` to TypeScript text. `source_name`/`source_text` register
/// the `.bynk` source every statement's own span is measured against —
/// today always exactly one, since nothing spans two files yet;
/// `output_file` names the generated file in the source map's own `file`
/// field. Only a *top-level* statement's own span is recorded as a
/// checkpoint (R7.4's existing scope, unchanged by P7.8 — see
/// [`TsStmt::span`]'s own doc for why a nested statement's span isn't
/// recorded yet).
pub fn print(
    program: &TsProgram,
    source_name: &str,
    source_text: &str,
    output_file: &str,
) -> Printed {
    let mut out = String::new();
    let mut map = SourceMapBuilder::new();
    map.add_source(source_name, source_text);
    for (i, stmt) in program.stmts.iter().enumerate() {
        if let Some(span) = stmt.span {
            map.record(out.len(), span);
        }
        render_stmt(&mut out, stmt, 0);
        // The printer owns line structure (R7.3), so a statement's own text
        // not ending in its own newline can't leave two statements sharing
        // a generated line — review of #1308, finding 2: nothing required
        // `Verbatim` text to be newline-terminated, and two that weren't
        // would jam onto one line *and* silently lose the earlier
        // statement's own checkpoint (`SourceMapBuilder::record`'s
        // same-offset dedup, and `to_v3`'s one-checkpoint-per-line forward
        // pass, both keep only the later one). Every real (non-`Verbatim`)
        // renderer already ends its own output in `\n`, so this is a no-op
        // for them; kept unconditional so `Verbatim`'s own guarantee stays
        // exactly as it was.
        if !out.ends_with('\n') {
            out.push('\n');
        }
        // Readability policy (this module's own doc): one blank line
        // between top-level declarations, except two consecutive
        // `import`s or two consecutive `Comment`s, and never after
        // `Verbatim` (P7.7's own boundary — `Verbatim` content's own
        // spacing is not this printer's decision).
        if let Some(next) = program.stmts.get(i + 1) {
            let both_imports = matches!(&stmt.kind, TsStmtKind::Decl(TsDecl::Import { .. }))
                && matches!(&next.kind, TsStmtKind::Decl(TsDecl::Import { .. }));
            let both_comments = matches!(&stmt.kind, TsStmtKind::Comment(_))
                && matches!(&next.kind, TsStmtKind::Comment(_));
            if !both_imports && !both_comments && !matches!(stmt.kind, TsStmtKind::Verbatim { .. })
            {
                out.push('\n');
            }
        }
    }
    let source_map = map.to_v3(&out, output_file);
    Printed {
        text: out,
        source_map,
    }
}

fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

/// Render one statement, including its own leading indent.
fn render_stmt(out: &mut String, stmt: &TsStmt, depth: usize) {
    match &stmt.kind {
        TsStmtKind::Verbatim { text, .. } => {
            out.push_str(text);
        }
        TsStmtKind::Decl(decl) => render_decl(out, decl, depth),
        TsStmtKind::Const { name, ty, init } => {
            out.push_str(&indent(depth));
            out.push_str("const ");
            render_binding_name(out, name);
            if let Some(ty) = ty {
                out.push_str(": ");
                render_type(out, ty);
            }
            out.push_str(" = ");
            render_stmt_level_expr(out, init, depth);
            out.push_str(";\n");
        }
        TsStmtKind::Let { name, ty, init } => {
            out.push_str(&indent(depth));
            out.push_str("let ");
            render_binding_name(out, name);
            if let Some(ty) = ty {
                out.push_str(": ");
                render_type(out, ty);
            }
            if let Some(init) = init {
                out.push_str(" = ");
                render_stmt_level_expr(out, init, depth);
            }
            out.push_str(";\n");
        }
        TsStmtKind::ExprStmt(expr) => {
            out.push_str(&indent(depth));
            render_expr(out, expr);
            out.push_str(";\n");
        }
        TsStmtKind::Return(expr) => {
            out.push_str(&indent(depth));
            out.push_str("return");
            if let Some(expr) = expr {
                out.push(' ');
                render_expr(out, expr);
            }
            out.push_str(";\n");
        }
        TsStmtKind::If { cond, then_branch } => {
            out.push_str(&indent(depth));
            out.push_str("if (");
            render_expr(out, cond);
            out.push(')');
            render_branch(out, then_branch, depth);
        }
        TsStmtKind::ForOf {
            binding,
            iter,
            body,
        } => {
            out.push_str(&indent(depth));
            out.push_str("for (const ");
            out.push_str(binding);
            out.push_str(" of ");
            render_expr(out, iter);
            out.push(')');
            render_branch(out, body, depth);
        }
        TsStmtKind::TryCatch {
            try_block,
            catch_param,
            catch_block,
        } => {
            out.push_str(&indent(depth));
            out.push_str("try");
            render_block_body(out, try_block, depth);
            out.push_str(" catch (");
            out.push_str(catch_param);
            out.push(')');
            render_block_body(out, catch_block, depth);
            out.push('\n');
        }
        TsStmtKind::Block(stmts) => {
            out.push_str(&indent(depth));
            out.push_str("{\n");
            for s in stmts {
                render_stmt(out, s, depth + 1);
            }
            out.push_str(&indent(depth));
            out.push_str("}\n");
        }
        TsStmtKind::Continue => {
            out.push_str(&indent(depth));
            out.push_str("continue;\n");
        }
        TsStmtKind::Assign { target, value } => {
            out.push_str(&indent(depth));
            render_expr(out, target);
            out.push_str(" = ");
            render_stmt_level_expr(out, value, depth);
            out.push_str(";\n");
        }
        TsStmtKind::Comment(text) => {
            for line in text.split('\n') {
                out.push_str(&indent(depth));
                out.push_str("// ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }
}

/// `if`/`for...of`'s own body: braces (and a nested block) when `branch` is
/// itself a `Block`, otherwise printed inline on the same line — matching
/// `events_fanout.rs`'s own `if (!Array.isArray(subs)) continue;` (no
/// braces) alongside its always-braced `for...of` bodies.
fn render_branch(out: &mut String, branch: &TsStmt, depth: usize) {
    match &branch.kind {
        TsStmtKind::Block(_) => {
            render_block_body(out, branch, depth);
            out.push('\n');
        }
        _ => {
            out.push(' ');
            render_inline_stmt(out, branch);
        }
    }
}

/// A statement rendered without its own leading indent, for the "no braces"
/// half of [`render_branch`] — only the shapes `events_fanout.rs` actually
/// needs inline (`continue`, `return`, a bare expression) get dedicated
/// handling; anything else falls back to a normal indented render (depth 0)
/// rather than panicking, so an unanticipated future shape still prints
/// something plausible instead of crashing.
fn render_inline_stmt(out: &mut String, stmt: &TsStmt) {
    match &stmt.kind {
        TsStmtKind::Continue => out.push_str("continue;\n"),
        TsStmtKind::Return(expr) => {
            out.push_str("return");
            if let Some(expr) = expr {
                out.push(' ');
                render_expr(out, expr);
            }
            out.push_str(";\n");
        }
        TsStmtKind::ExprStmt(expr) => {
            render_expr(out, expr);
            out.push_str(";\n");
        }
        // Review of #1317/#1318, finding 2: every other variant in the
        // fallback group below is safe to render via `render_stmt`'s own
        // top-level `//`-line-comment form — `Comment` is not. A `//`
        // comment run through the generic fallback (`if (cond) // text`)
        // comments out the rest of the physical line, leaving the `if`
        // with no body at all — a TypeScript parse error, not merely an
        // unlikely shape. Not reachable today (`events_fanout.rs` never
        // puts a bare `Comment` in a brace-free `if`/`for...of` body), but
        // the fallback group exists precisely so a future slice trips over
        // a missing case at compile time, not at parse time in emitted
        // output — so `Comment` needs its own real inline shape now, a
        // block comment (`/* text */`), which cannot swallow anything
        // after it. Embedded newlines flatten to spaces, keeping this
        // strictly one generated line like every other inline shape here.
        TsStmtKind::Comment(text) => {
            out.push_str("/* ");
            out.push_str(&text.replace('\n', " "));
            out.push_str(" */\n");
        }
        // No inline shape of its own — `events_fanout.rs` never puts one of
        // these in a brace-free `if`/`for...of` body — but listed by name,
        // not a wildcard (review of #1314, finding 3): a future Arc C
        // slice's new `TsStmtKind` variant must fail to compile here rather
        // than silently inherit this fallback, the same exhaustiveness
        // discipline `VerbatimOrigin`'s own doc states for `bynk-emit`.
        TsStmtKind::Verbatim { .. }
        | TsStmtKind::Decl(_)
        | TsStmtKind::Const { .. }
        | TsStmtKind::Let { .. }
        | TsStmtKind::If { .. }
        | TsStmtKind::ForOf { .. }
        | TsStmtKind::TryCatch { .. }
        | TsStmtKind::Block(_)
        | TsStmtKind::Assign { .. } => render_stmt(out, stmt, 0),
    }
}

/// `{ <stmts> }`, with the closing brace at `depth`'s own indent and no
/// trailing newline — the shared shape [`TsStmtKind::TryCatch`] needs to
/// keep writing on the same generated line (`} catch (e) {`) and
/// [`render_branch`]'s own `Block` case needs with a `\n` appended after.
/// `block` is expected to be a [`TsStmtKind::Block`]; anything else is
/// still rendered sensibly (as a single-statement body) rather than
/// panicking.
fn render_block_body(out: &mut String, block: &TsStmt, depth: usize) {
    out.push_str(" {\n");
    if let TsStmtKind::Block(stmts) = &block.kind {
        for s in stmts {
            render_stmt(out, s, depth + 1);
        }
    } else {
        render_stmt(out, block, depth + 1);
    }
    out.push_str(&indent(depth));
    out.push('}');
}

fn render_binding_name(out: &mut String, name: &TsBindingName) {
    match name {
        TsBindingName::Ident(s) => out.push_str(s),
        TsBindingName::ObjectPattern(names) => {
            out.push_str("{ ");
            for (i, n) in names.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(n);
            }
            out.push_str(" }");
        }
    }
}

/// Whether `expr`, printed in a position member access, indexing, a call
/// callee, `await`, or unary `!` binds tighter than, needs its own parens
/// to preserve the built tree's meaning — `Binary`/`As` both bind looser
/// than any of those (review of #1314, finding 2: `!a ?? b`/`a ?? b.c`/
/// `await a ?? b` all silently changed meaning without this). Deliberately
/// conservative: with only one binary operator (`??`) in this slice's
/// algebra, a nested `Binary` is always parenthesized here even in a
/// left-associative chain that would read the same without it (`a ?? b ??
/// c`) — correct in every case that class of bug can hit, at the cost of
/// an occasional pair that a real precedence table (once more than one
/// binary operator exists) could omit.
fn needs_parens_as_operand(expr: &TsExpr) -> bool {
    matches!(expr, TsExpr::Binary { .. } | TsExpr::As { .. })
}

fn render_operand(out: &mut String, expr: &TsExpr) {
    if needs_parens_as_operand(expr) {
        out.push('(');
        render_expr(out, expr);
        out.push(')');
    } else {
        render_expr(out, expr);
    }
}

/// Render `expr` as a statement or declaration's own top-level expression
/// (a `const`/`let` initialiser, an assignment's own value, …) — the only
/// place `depth` is available to render a `multiline: true`
/// [`TsExpr::Object`] correctly (see its own doc). Every other shape defers
/// to the ordinary, depth-unaware [`render_expr`] unchanged; this exists
/// only to intercept the one shape that needs `depth` before it gets there.
fn render_stmt_level_expr(out: &mut String, expr: &TsExpr, depth: usize) {
    if let TsExpr::Object {
        entries,
        multiline: true,
    } = expr
    {
        render_multiline_object(out, entries, depth);
    } else {
        render_expr(out, expr);
    }
}

/// `{ <newline> ("  "*depth+1)<key>: <value>,<newline> ... ("  "*depth)}` —
/// one entry per line, each with its own trailing comma (including the
/// last), closing brace back at `depth`'s own indent. `events_fanout.rs`'s
/// own `__eventRoutes` table is the real, grounded shape this exists for
/// (#1317) — TypeScript's ordinary multi-line object-literal convention.
fn render_multiline_object(out: &mut String, entries: &[(String, TsExpr)], depth: usize) {
    // Deliberately no empty-entries shortcut (review of #1317/#1318, finding
    // 1): `events_fanout.rs`'s own `__eventRoutes` table is reachable with
    // zero entries (a context can `ctx_uses_emit` while publishing only
    // events nobody subscribes to — `own_event_routes` filters down to
    // exactly that empty case, `bynk-emit/src/project.rs`'s own
    // `own_event_routes` computation), and the pre-conversion `writeln!`
    // code always wrote the open-brace line and the closing `};` line
    // unconditionally, regardless of how many times its own `for` loop
    // iterated — an empty table printed `{\n};`, not the tight `{}` a
    // single-line object's own empty case uses. Matching that byte-for-byte
    // means never taking the single-line shortcut here, not even for zero
    // entries.
    out.push_str("{\n");
    for (k, v) in entries {
        out.push_str(&indent(depth + 1));
        out.push_str(k);
        out.push_str(": ");
        render_expr(out, v);
        out.push_str(",\n");
    }
    out.push_str(&indent(depth));
    out.push('}');
}

fn render_expr(out: &mut String, expr: &TsExpr) {
    match expr {
        TsExpr::Ident(name) => out.push_str(name),
        TsExpr::Member { object, property } => {
            render_operand(out, object);
            out.push('.');
            out.push_str(property);
        }
        TsExpr::Index { object, index } => {
            render_operand(out, object);
            out.push('[');
            render_expr(out, index);
            out.push(']');
        }
        TsExpr::Call { callee, args } => {
            render_operand(out, callee);
            out.push('(');
            render_expr_list(out, args);
            out.push(')');
        }
        TsExpr::New { callee, args } => {
            out.push_str("new ");
            render_operand(out, callee);
            out.push('(');
            render_expr_list(out, args);
            out.push(')');
        }
        // `multiline` is ignored here deliberately: this recursion has no
        // `depth` to render a multi-line object correctly against (see
        // `TsExpr::Object`'s own doc). Every real `bynk-emit` call site
        // that needs `multiline: true` reaches it through
        // `render_stmt_level_expr` instead, which intercepts the shape
        // before it gets here.
        TsExpr::Object { entries, .. } => {
            if entries.is_empty() {
                out.push_str("{}");
            } else {
                out.push_str("{ ");
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(k);
                    out.push_str(": ");
                    render_expr(out, v);
                }
                out.push_str(" }");
            }
        }
        TsExpr::Array(items) => {
            out.push('[');
            render_expr_list(out, items);
            out.push(']');
        }
        TsExpr::Await(inner) => {
            out.push_str("await ");
            render_operand(out, inner);
        }
        TsExpr::As { expr, ty } => {
            // `(await x) as T` / `(a ?? b) as T` — an `await` or a binary
            // expression needs parens before `as`; nothing else this slice
            // prints does (a bare `Ident`/`Call`/`Object` reads correctly
            // without them, matching `events_fanout.rs`'s own two real
            // `as` casts exactly).
            let needs_parens = matches!(**expr, TsExpr::Await(_) | TsExpr::Binary { .. });
            if needs_parens {
                out.push('(');
                render_expr(out, expr);
                out.push(')');
            } else {
                render_expr(out, expr);
            }
            out.push_str(" as ");
            render_type(out, ty);
        }
        TsExpr::Unary { op, expr } => {
            out.push_str(match op {
                TsUnaryOp::Not => "!",
            });
            render_operand(out, expr);
        }
        TsExpr::Binary { op, left, right } => {
            render_operand(out, left);
            out.push_str(match op {
                TsBinaryOp::NullishCoalescing => " ?? ",
            });
            render_operand(out, right);
        }
        TsExpr::Lit(lit) => render_lit(out, lit),
    }
}

fn render_expr_list(out: &mut String, exprs: &[TsExpr]) {
    for (i, e) in exprs.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        render_expr(out, e);
    }
}

fn render_lit(out: &mut String, lit: &TsLit) {
    match lit {
        TsLit::Str(s) => {
            out.push('"');
            // Matches `bynk_check::wire_default::escape_ts_literal`
            // byte-for-byte (review of #1314, finding 1) — the previous
            // version escaped only `"`/`\`, so a `\n`/`\t`/`\r` inside a
            // literal printed raw into the output, an unterminated string
            // literal `tsc` can't parse. `bynk-ts` can't depend on
            // `bynk-check` to share that function directly (this crate's
            // own module doc: `bynk-syntax` only) — inlined instead, kept
            // deliberately identical rather than reinvented, since both
            // this printer and `bynk-emit`'s `escape_ts_string` splice into
            // generated TypeScript and must never disagree.
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    '\r' => out.push_str("\\r"),
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        TsLit::Num(n) => out.push_str(n),
        TsLit::Null => out.push_str("null"),
    }
}

fn render_type(out: &mut String, ty: &TsType) {
    match ty {
        TsType::Named { name, type_args } => {
            out.push_str(name);
            if !type_args.is_empty() {
                out.push('<');
                for (i, arg) in type_args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    render_type(out, arg);
                }
                out.push('>');
            }
        }
        TsType::Array { element, readonly } => {
            if *readonly {
                out.push_str("readonly ");
            }
            render_type(out, element);
            out.push_str("[]");
        }
        TsType::Object(members) => {
            if members.is_empty() {
                out.push_str("{}");
            } else {
                out.push_str("{ ");
                for (i, (k, t)) in members.iter().enumerate() {
                    if i > 0 {
                        out.push_str("; ");
                    }
                    out.push_str(k);
                    out.push_str(": ");
                    render_type(out, t);
                }
                out.push_str(" }");
            }
        }
        TsType::Fn { params, ret } => {
            out.push('(');
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push('a');
                out.push_str(&i.to_string());
                out.push_str(": ");
                render_type(out, p);
            }
            out.push_str(") => ");
            render_type(out, ret);
        }
        TsType::Union(members) => {
            for (i, m) in members.iter().enumerate() {
                if i > 0 {
                    out.push_str(" | ");
                }
                render_type(out, m);
            }
        }
    }
}

/// Print a single [`TsType`] on its own — the real callers this closes R7.2
/// for (`bynk-emit`'s `ts_type_ref*`/`ts_ty` families, P7.9, #1315) each
/// want one type fragment to interpolate into a larger, still-hand-built
/// line (a field's own type annotation, a parameter list, …), not a whole
/// [`TsProgram`]. No source-map/buffer machinery — [`print()`] owns that
/// for a whole document; this is the printer's other, narrower entry
/// point, sharing the same internal recursion rather than a second copy.
pub fn print_type(ty: &TsType) -> String {
    let mut out = String::new();
    render_type(&mut out, ty);
    out
}

fn render_params(out: &mut String, params: &[TsParam]) {
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&p.name);
        if p.optional {
            out.push('?');
        }
        if let Some(ty) = &p.ty {
            out.push_str(": ");
            render_type(out, ty);
        }
    }
}

fn render_decl(out: &mut String, decl: &TsDecl, depth: usize) {
    // `render_decl_body`'s own `TsDecl::Export` arm already writes
    // `"export "` before recursing into the inner declaration — the
    // `if let Export` special case this replaced produced byte-identical
    // output by duplicating that same logic one layer up (review of
    // #1314, smaller note: the two branches were provably the same for
    // every input, so only one needs to exist).
    out.push_str(&indent(depth));
    render_decl_body(out, decl, depth);
}

/// The declaration's own text, with no leading indent — [`render_decl`]
/// writes the indent (and, for `Export`, the `export ` keyword) once, then
/// hands off here so `Export(inner)` doesn't duplicate `inner`'s own
/// leading whitespace.
fn render_decl_body(out: &mut String, decl: &TsDecl, depth: usize) {
    match decl {
        TsDecl::Import {
            type_only,
            names,
            from,
        } => {
            out.push_str(if *type_only {
                "import type { "
            } else {
                "import { "
            });
            out.push_str(&names.join(", "));
            out.push_str(" } from \"");
            out.push_str(from);
            out.push_str("\";\n");
        }
        TsDecl::Export(inner) => {
            out.push_str("export ");
            render_decl_body(out, inner, depth);
        }
        TsDecl::Interface { name, members } => {
            out.push_str("interface ");
            out.push_str(name);
            out.push_str(" {\n");
            for (member_name, member_ty) in members {
                out.push_str(&indent(depth + 1));
                out.push_str(member_name);
                out.push_str(": ");
                render_type(out, member_ty);
                out.push_str(";\n");
            }
            out.push_str(&indent(depth));
            out.push_str("}\n");
        }
        TsDecl::ConstDecl { name, ty, init } => {
            out.push_str("const ");
            out.push_str(name);
            if let Some(ty) = ty {
                out.push_str(": ");
                render_type(out, ty);
            }
            out.push_str(" = ");
            render_stmt_level_expr(out, init, depth);
            out.push_str(";\n");
        }
        TsDecl::Class {
            name,
            fields,
            constructor,
            methods,
        } => {
            out.push_str("class ");
            out.push_str(name);
            out.push_str(" {\n");
            for f in fields {
                out.push_str(&indent(depth + 1));
                if f.private {
                    out.push_str("private ");
                }
                out.push_str(&f.name);
                out.push_str(": ");
                render_type(out, &f.ty);
                out.push_str(";\n");
            }
            // Readability policy (this module's own doc): no blank line
            // between fields and the constructor; one blank line before
            // each method — `events_fanout.rs`'s own real class spacing.
            let mut wrote_member = !fields.is_empty();
            if let Some(ctor) = constructor {
                out.push_str(&indent(depth + 1));
                out.push_str("constructor(");
                render_params(out, &ctor.params);
                out.push(')');
                render_block_stmts(out, &ctor.body, depth + 1);
                out.push('\n');
                wrote_member = true;
            }
            for m in methods {
                if wrote_member {
                    out.push('\n');
                }
                out.push_str(&indent(depth + 1));
                if m.is_async {
                    out.push_str("async ");
                }
                out.push_str(&m.name);
                out.push('(');
                render_params(out, &m.params);
                out.push(')');
                if let Some(rt) = &m.return_type {
                    out.push_str(": ");
                    render_type(out, rt);
                }
                render_block_stmts(out, &m.body, depth + 1);
                out.push('\n');
                wrote_member = true;
            }
            out.push_str(&indent(depth));
            out.push_str("}\n");
        }
    }
}

/// `{ <stmts> }` for a constructor/method body — same shape as
/// [`render_block_body`] but over a plain `&[TsStmt]` (a `TsClassCtor`/
/// `TsClassMethod`'s own `body` field, not a boxed `TsStmt`), and always
/// followed by the caller's own `\n` (constructor/method declarations, not
/// a same-line `try`/`catch` continuation).
fn render_block_stmts(out: &mut String, stmts: &[TsStmt], depth: usize) {
    out.push_str(" {\n");
    for s in stmts {
        render_stmt(out, s, depth + 1);
    }
    out.push_str(&indent(depth));
    out.push('}');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::{TsClassCtor, TsClassField, TsClassMethod, VerbatimOrigin};
    use bynk_syntax::span::Span;

    /// Pins the readability policy's statement-separation guarantee (R7.5,
    /// this module's own doc): every statement starts on its own generated
    /// line.
    #[test]
    fn prints_every_statement_in_order() {
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Contracts,
            "const a = 1;\n",
            None,
        ));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Secrets,
            "const b = 2;\n",
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "const a = 1;\nconst b = 2;\n");
    }

    /// Pins the readability policy's statement-separation guarantee (R7.5,
    /// this module's own doc) in its sharpest form: nothing requires
    /// `Verbatim` text to be newline-terminated — the printer owns line
    /// structure (R7.3), so it's the one that guarantees two statements
    /// never share a generated line, not a `TsStmt::verbatim` caller
    /// obligation nobody enforces (review of #1308, finding 2).
    #[test]
    fn a_statement_missing_its_own_trailing_newline_still_gets_its_own_line() {
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Contracts,
            "const a = 1;",
            None,
        ));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Secrets,
            "const b = 2;",
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "const a = 1;\nconst b = 2;\n");
    }

    /// Production `Verbatim` content is rarely one line — `VerbatimOrigin::
    /// NotYetConverted` (P7.6) wraps a whole generated document per
    /// statement, so one statement routinely spans dozens of generated
    /// lines. Neither of the two tests above exercised that shape (review
    /// of #1312, finding 1) — this one confirms the readability policy's
    /// real invariant (the *next* statement always starts on a fresh line)
    /// holds for multi-line content too, not just the single-line text
    /// those tests happened to use.
    #[test]
    fn a_multi_line_statement_still_starts_the_next_statement_on_a_fresh_line() {
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Contracts,
            "function a() {\n  return 1;\n}\n",
            None,
        ));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Secrets,
            "const b = 2;\n",
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(
            printed.text,
            "function a() {\n  return 1;\n}\nconst b = 2;\n"
        );
    }

    /// `print` decides whether to append a trailing newline by checking the
    /// whole *buffer*, not the statement's own text (review of #1312,
    /// finding 2) — an empty statement's text changes nothing, so whether
    /// it gets a line depends entirely on what's already in the buffer.
    /// First in a program, the buffer is still empty, so the check fires
    /// and the output opens with a blank line. Pinned here so this edge
    /// stays a known, deliberate reading of the code rather than an
    /// untested accident.
    #[test]
    fn an_empty_statement_first_in_the_program_yields_a_leading_blank_line() {
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(VerbatimOrigin::Contracts, "", None));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Secrets,
            "const b = 2;\n",
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "\nconst b = 2;\n");
    }

    /// The other half of the buffer-vs-statement-text distinction (review
    /// of #1312, finding 2): an empty statement following one that already
    /// ended its own text in `\n` contributes no line at all — the buffer
    /// already ends in `\n`, so the check doesn't fire, and the empty
    /// statement's own (non-)content and the next statement's text end up
    /// on what reads as one generated line for the next statement alone.
    #[test]
    fn an_empty_statement_after_a_newline_terminated_one_contributes_no_line_of_its_own() {
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Contracts,
            "const a = 1;\n",
            None,
        ));
        program.push(TsStmt::verbatim(VerbatimOrigin::Secrets, "", None));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::RuntimeUse,
            "const c = 3;\n",
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "const a = 1;\nconst c = 3;\n");
    }

    #[test]
    fn no_spans_at_all_means_no_source_map() {
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(VerbatimOrigin::Contracts, "x;\n", None));
        let printed = print(&program, "x.bynk", "let x = 1\n", "x.ts");
        assert_eq!(printed.source_map, None);
    }

    /// The property that actually matters (R7.4): the printer's own
    /// buffer-position bookkeeping is correct — a checkpoint recorded from a
    /// later statement's span still resolves to the *generated* line that
    /// statement's own text landed on, not (as the old splice-based
    /// mechanism could get wrong, #4) some other buffer's offset.
    ///
    /// Review of #1308, finding 3: the original version of this test
    /// asserted only that the source map's `sources` array was present —
    /// true even with the second statement's checkpoint silently dropped,
    /// exactly the failure this doc comment claims is ruled out. Asserting
    /// full equality against a map built independently (replicating the
    /// printer's own record-then-write order by hand, not calling `print`)
    /// actually pins both statements' own offsets.
    #[test]
    fn each_statements_span_resolves_to_its_own_generated_line() {
        let source = "let a = 1\nlet b = 2\n";
        let off_a = source.find("let a").unwrap();
        let off_b = source.find("let b").unwrap();
        let span_a = Span::new(off_a, off_a + 5);
        let span_b = Span::new(off_b, off_b + 5);
        let text_a = "const a = 1;\n";
        let text_b = "const b = 2;\n";
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Contracts,
            text_a,
            Some(span_a),
        ));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Secrets,
            text_b,
            Some(span_b),
        ));

        let printed = print(&program, "x.bynk", source, "x.ts");

        let mut expected_map = SourceMapBuilder::new();
        expected_map.add_source("x.bynk", source);
        expected_map.record(0, span_a);
        expected_map.record(text_a.len(), span_b);
        let expected = expected_map.to_v3(&printed.text, "x.ts");

        assert_eq!(printed.source_map, expected);
    }

    // -- P7.8 (#1313): real node rendering. --

    #[test]
    fn prints_a_const_statement_with_a_destructured_binding_and_a_type_cast() {
        let mut program = TsProgram::new();
        program.push(TsStmt::const_stmt(
            TsBindingName::ObjectPattern(vec!["events".to_string()]),
            None,
            TsExpr::As {
                expr: Box::new(TsExpr::Await(Box::new(TsExpr::Call {
                    callee: Box::new(TsExpr::Member {
                        object: Box::new(TsExpr::Ident("request".to_string())),
                        property: "json".to_string(),
                    }),
                    args: vec![],
                }))),
                ty: TsType::Object(vec![(
                    "events".to_string(),
                    TsType::array(TsType::named("FanoutEvent")),
                )]),
            },
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(
            printed.text,
            "const { events } = (await request.json()) as { events: FanoutEvent[] };\n"
        );
    }

    #[test]
    fn prints_if_without_braces_when_the_body_is_not_a_block() {
        let mut program = TsProgram::new();
        program.push(TsStmt::if_stmt(
            TsExpr::Unary {
                op: TsUnaryOp::Not,
                expr: Box::new(TsExpr::Call {
                    callee: Box::new(TsExpr::Member {
                        object: Box::new(TsExpr::Ident("Array".to_string())),
                        property: "isArray".to_string(),
                    }),
                    args: vec![TsExpr::Ident("subs".to_string())],
                }),
            },
            TsStmt::continue_stmt(None),
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "if (!Array.isArray(subs)) continue;\n");
    }

    /// Review of #1317/#1318, finding 2: a bare `Comment` as an `if`'s
    /// brace-free body must not fall through to the top-level `//`-line
    /// form — `if (cond) // text` comments out the rest of the physical
    /// line, leaving `if` with no body at all (a parse error). Renders as
    /// a block comment instead, which cannot swallow anything after it.
    #[test]
    fn a_comment_as_an_ifs_brace_free_body_renders_as_a_block_comment() {
        let mut program = TsProgram::new();
        program.push(TsStmt::if_stmt(
            TsExpr::Ident("cond".to_string()),
            TsStmt::comment("annotation", None),
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "if (cond) /* annotation */\n");
    }

    #[test]
    fn prints_a_for_of_loop_with_a_braced_body() {
        let mut program = TsProgram::new();
        program.push(TsStmt::for_of(
            "ev",
            TsExpr::Ident("events".to_string()),
            TsStmt::block(
                vec![TsStmt::return_stmt(
                    Some(TsExpr::Ident("ev".to_string())),
                    None,
                )],
                None,
            ),
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(
            printed.text,
            "for (const ev of events) {\n  return ev;\n}\n"
        );
    }

    #[test]
    fn prints_a_try_catch_on_the_shared_closing_brace_line() {
        let mut program = TsProgram::new();
        program.push(TsStmt::try_catch(
            TsStmt::block(
                vec![TsStmt::expr_stmt(
                    TsExpr::Await(Box::new(TsExpr::Call {
                        callee: Box::new(TsExpr::Ident("deliverEvent".to_string())),
                        args: vec![TsExpr::Ident("binding".to_string())],
                    })),
                    None,
                )],
                None,
            ),
            "e",
            TsStmt::block(
                vec![TsStmt::expr_stmt(
                    TsExpr::Call {
                        callee: Box::new(TsExpr::Member {
                            object: Box::new(TsExpr::Ident("console".to_string())),
                            property: "error".to_string(),
                        }),
                        args: vec![TsExpr::Lit(TsLit::Str("failed".to_string()))],
                    },
                    None,
                )],
                None,
            ),
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(
            printed.text,
            "try {\n  await deliverEvent(binding);\n} catch (e) {\n  console.error(\"failed\");\n}\n"
        );
    }

    #[test]
    fn prints_an_interface_with_an_inline_nested_object_type() {
        let mut program = TsProgram::new();
        program.push(TsStmt::decl(
            TsDecl::Interface {
                name: "FanoutEvent".to_string(),
                members: vec![
                    ("type".to_string(), TsType::named("string")),
                    ("payload".to_string(), TsType::named("unknown")),
                    (
                        "envelope".to_string(),
                        TsType::Object(vec![
                            ("eventId".to_string(), TsType::named("string")),
                            ("publisherId".to_string(), TsType::named("string")),
                        ]),
                    ),
                ],
            },
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(
            printed.text,
            "interface FanoutEvent {\n  type: string;\n  payload: unknown;\n  envelope: { eventId: string; publisherId: string };\n}\n"
        );
    }

    #[test]
    fn prints_two_adjacent_imports_with_no_blank_line_between_them() {
        let mut program = TsProgram::new();
        program.push(TsStmt::decl(
            TsDecl::Import {
                type_only: true,
                names: vec!["A".to_string()],
                from: "../x.js".to_string(),
            },
            None,
        ));
        program.push(TsStmt::decl(
            TsDecl::Import {
                type_only: false,
                names: vec!["b".to_string()],
                from: "../x.js".to_string(),
            },
            None,
        ));
        program.push(TsStmt::decl(
            TsDecl::ConstDecl {
                name: "c".to_string(),
                ty: None,
                init: TsExpr::Lit(TsLit::Null),
            },
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(
            printed.text,
            "import type { A } from \"../x.js\";\nimport { b } from \"../x.js\";\n\nconst c = null;\n"
        );
    }

    /// The required "does the algebra hold against real content" test
    /// (P7.8's own accepted proposal, #1313): builds
    /// `bynk-emit/src/emitter/events_fanout.rs`'s own `EventsFanoutDO`
    /// class as real nodes (its field, constructor, and `fetch` method,
    /// with the exact control flow the real function emits — `for...of`,
    /// nested `if`-without-braces, `try`/`catch`) and asserts the printed
    /// text is byte-identical to what `emit_events_fanout_do` produces
    /// today for that class (transcribed directly from
    /// `bynk-emit/src/emitter/events_fanout.rs`'s own `write!` calls, not
    /// re-derived — including the constructor body, `this.env = (env ??
    /// {}) as Record<string, ServiceBinding>;`, via `TsStmtKind::Assign`,
    /// added in review of #1313: the accepted proposal's own grounding
    /// catalogue detailed the `fetch` method's body but missed the
    /// constructor's one real statement, which turned out to need a
    /// variant Decision B's list didn't name). The leading header comment
    /// and the `__eventRoutes` table's own per-project literal entries are
    /// deliberately not included here — a representative fragment covering
    /// the class is exactly what the accepted proposal's own "Done when"
    /// allows, and this is the shape that actually exercises every new
    /// node kind this slice adds.
    #[test]
    fn prints_events_fanout_dos_own_class_byte_identical_to_the_real_emitter() {
        let real_ctor = TsClassCtor {
            params: vec![
                TsParam {
                    name: "_state".to_string(),
                    ty: Some(TsType::named("DurableObjectState")),
                    optional: false,
                },
                TsParam {
                    name: "env".to_string(),
                    ty: Some(TsType::named("unknown")),
                    optional: true,
                },
            ],
            body: vec![TsStmt::assign(
                TsExpr::Member {
                    object: Box::new(TsExpr::Ident("this".to_string())),
                    property: "env".to_string(),
                },
                TsExpr::As {
                    expr: Box::new(TsExpr::Binary {
                        op: TsBinaryOp::NullishCoalescing,
                        left: Box::new(TsExpr::Ident("env".to_string())),
                        right: Box::new(TsExpr::object(vec![])),
                    }),
                    ty: TsType::named_with_args(
                        "Record",
                        vec![TsType::named("string"), TsType::named("ServiceBinding")],
                    ),
                },
                None,
            )],
        };

        let fetch = TsClassMethod {
            name: "fetch".to_string(),
            is_async: true,
            params: vec![TsParam {
                name: "request".to_string(),
                ty: Some(TsType::named("Request")),
                optional: false,
            }],
            return_type: Some(TsType::named_with_args(
                "Promise",
                vec![TsType::named("Response")],
            )),
            body: vec![
                TsStmt::const_stmt(
                    TsBindingName::ObjectPattern(vec!["events".to_string()]),
                    None,
                    TsExpr::As {
                        expr: Box::new(TsExpr::Await(Box::new(TsExpr::Call {
                            callee: Box::new(TsExpr::Member {
                                object: Box::new(TsExpr::Ident("request".to_string())),
                                property: "json".to_string(),
                            }),
                            args: vec![],
                        }))),
                        ty: TsType::Object(vec![(
                            "events".to_string(),
                            TsType::array(TsType::named("FanoutEvent")),
                        )]),
                    },
                    None,
                ),
                TsStmt::for_of(
                    "ev",
                    TsExpr::Ident("events".to_string()),
                    TsStmt::block(
                        vec![
                            TsStmt::const_stmt(
                                TsBindingName::Ident("subs".to_string()),
                                None,
                                TsExpr::Index {
                                    object: Box::new(TsExpr::Ident("__eventRoutes".to_string())),
                                    index: Box::new(TsExpr::Member {
                                        object: Box::new(TsExpr::Ident("ev".to_string())),
                                        property: "type".to_string(),
                                    }),
                                },
                                None,
                            ),
                            TsStmt::if_stmt(
                                TsExpr::Unary {
                                    op: TsUnaryOp::Not,
                                    expr: Box::new(TsExpr::Call {
                                        callee: Box::new(TsExpr::Member {
                                            object: Box::new(TsExpr::Ident("Array".to_string())),
                                            property: "isArray".to_string(),
                                        }),
                                        args: vec![TsExpr::Ident("subs".to_string())],
                                    }),
                                },
                                TsStmt::continue_stmt(None),
                                None,
                            ),
                            TsStmt::for_of(
                                "sub",
                                TsExpr::Ident("subs".to_string()),
                                TsStmt::block(
                                    vec![
                                        TsStmt::const_stmt(
                                            TsBindingName::Ident("binding".to_string()),
                                            None,
                                            TsExpr::Index {
                                                object: Box::new(TsExpr::Member {
                                                    object: Box::new(TsExpr::Ident(
                                                        "this".to_string(),
                                                    )),
                                                    property: "env".to_string(),
                                                }),
                                                index: Box::new(TsExpr::Member {
                                                    object: Box::new(TsExpr::Ident(
                                                        "sub".to_string(),
                                                    )),
                                                    property: "binding".to_string(),
                                                }),
                                            },
                                            None,
                                        ),
                                        TsStmt::if_stmt(
                                            TsExpr::Unary {
                                                op: TsUnaryOp::Not,
                                                expr: Box::new(TsExpr::Ident(
                                                    "binding".to_string(),
                                                )),
                                            },
                                            TsStmt::continue_stmt(None),
                                            None,
                                        ),
                                        TsStmt::try_catch(
                                            TsStmt::block(
                                                vec![TsStmt::expr_stmt(
                                                    TsExpr::Await(Box::new(TsExpr::Call {
                                                        callee: Box::new(TsExpr::Ident(
                                                            "deliverEvent".to_string(),
                                                        )),
                                                        args: vec![
                                                            TsExpr::Ident("binding".to_string()),
                                                            TsExpr::Member {
                                                                object: Box::new(TsExpr::Ident(
                                                                    "sub".to_string(),
                                                                )),
                                                                property: "service".to_string(),
                                                            },
                                                            TsExpr::Member {
                                                                object: Box::new(TsExpr::Ident(
                                                                    "ev".to_string(),
                                                                )),
                                                                property: "payload".to_string(),
                                                            },
                                                            TsExpr::Member {
                                                                object: Box::new(TsExpr::Ident(
                                                                    "ev".to_string(),
                                                                )),
                                                                property: "envelope".to_string(),
                                                            },
                                                        ],
                                                    })),
                                                    None,
                                                )],
                                                None,
                                            ),
                                            "e",
                                            TsStmt::block(
                                                vec![TsStmt::expr_stmt(
                                                    TsExpr::Call {
                                                        callee: Box::new(TsExpr::Member {
                                                            object: Box::new(TsExpr::Ident(
                                                                "console".to_string(),
                                                            )),
                                                            property: "error".to_string(),
                                                        }),
                                                        args: vec![
                                                            TsExpr::Lit(TsLit::Str(
                                                                "EventsFanout delivery failed"
                                                                    .to_string(),
                                                            )),
                                                            TsExpr::object(vec![
                                                                (
                                                                    "event".to_string(),
                                                                    TsExpr::Member {
                                                                        object: Box::new(
                                                                            TsExpr::Ident(
                                                                                "ev".to_string(),
                                                                            ),
                                                                        ),
                                                                        property: "type"
                                                                            .to_string(),
                                                                    },
                                                                ),
                                                                (
                                                                    "service".to_string(),
                                                                    TsExpr::Member {
                                                                        object: Box::new(
                                                                            TsExpr::Ident(
                                                                                "sub".to_string(),
                                                                            ),
                                                                        ),
                                                                        property: "service"
                                                                            .to_string(),
                                                                    },
                                                                ),
                                                                (
                                                                    "error".to_string(),
                                                                    TsExpr::Call {
                                                                        callee: Box::new(
                                                                            TsExpr::Ident(
                                                                                "String"
                                                                                    .to_string(),
                                                                            ),
                                                                        ),
                                                                        args: vec![TsExpr::Ident(
                                                                            "e".to_string(),
                                                                        )],
                                                                    },
                                                                ),
                                                            ]),
                                                        ],
                                                    },
                                                    None,
                                                )],
                                                None,
                                            ),
                                            None,
                                        ),
                                    ],
                                    None,
                                ),
                                None,
                            ),
                        ],
                        None,
                    ),
                    None,
                ),
                TsStmt::return_stmt(
                    Some(TsExpr::New {
                        callee: Box::new(TsExpr::Ident("Response".to_string())),
                        args: vec![
                            TsExpr::Lit(TsLit::Null),
                            TsExpr::object(vec![(
                                "status".to_string(),
                                TsExpr::Lit(TsLit::Num("204".to_string())),
                            )]),
                        ],
                    }),
                    None,
                ),
            ],
        };

        let class = TsDecl::Export(Box::new(TsDecl::Class {
            name: "EventsFanoutDO".to_string(),
            fields: vec![TsClassField {
                name: "env".to_string(),
                ty: TsType::named_with_args(
                    "Record",
                    vec![TsType::named("string"), TsType::named("ServiceBinding")],
                ),
                private: true,
            }],
            constructor: Some(real_ctor),
            methods: vec![fetch],
        }));

        let mut program = TsProgram::new();
        program.push(TsStmt::decl(class, None));
        let printed = print(&program, "x.bynk", "", "x.ts");

        // Transcribed directly from `bynk-emit/src/emitter/events_fanout.rs`'s
        // own `write!` calls, byte-for-byte including the constructor body.
        let expected_lines = [
            "export class EventsFanoutDO {",
            "  private env: Record<string, ServiceBinding>;",
            "  constructor(_state: DurableObjectState, env?: unknown) {",
            "    this.env = (env ?? {}) as Record<string, ServiceBinding>;",
            "  }",
            "",
            "  async fetch(request: Request): Promise<Response> {",
            "    const { events } = (await request.json()) as { events: FanoutEvent[] };",
            "    for (const ev of events) {",
            "      const subs = __eventRoutes[ev.type];",
            "      if (!Array.isArray(subs)) continue;",
            "      for (const sub of subs) {",
            "        const binding = this.env[sub.binding];",
            "        if (!binding) continue;",
            "        try {",
            "          await deliverEvent(binding, sub.service, ev.payload, ev.envelope);",
            "        } catch (e) {",
            "          console.error(\"EventsFanout delivery failed\", { event: ev.type, service: sub.service, error: String(e) });",
            "        }",
            "      }",
            "    }",
            "    return new Response(null, { status: 204 });",
            "  }",
            "}",
        ];
        let expected = format!("{}\n", expected_lines.join("\n"));
        assert_eq!(printed.text, expected);
    }

    /// Coverage gap named in review of #1314, finding 4: `Let` had no test
    /// anywhere, including its distinct `init: None` branch (`Const` always
    /// has an initialiser; `Let` is the one new statement kind that can
    /// omit one).
    #[test]
    fn prints_a_let_statement_with_no_initialiser() {
        let mut program = TsProgram::new();
        program.push(TsStmt::let_stmt(
            TsBindingName::Ident("subs".to_string()),
            None,
            None,
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "let subs;\n");
    }

    /// Coverage gap named in review of #1314, finding 4: `TsExpr::Array`
    /// was never constructed in any test.
    #[test]
    fn prints_an_array_literal() {
        let mut program = TsProgram::new();
        program.push(TsStmt::expr_stmt(
            TsExpr::Array(vec![
                TsExpr::Lit(TsLit::Num("1".to_string())),
                TsExpr::Ident("x".to_string()),
            ]),
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "[1, x];\n");
    }

    /// P7.9 (#1315): `TsType::Array`'s new `readonly` modifier — the shape
    /// every `List`/`Query` element type `bynk-emit`'s `ts_type_ref*`/
    /// `ts_ty` families build (`readonly T[]`), which `TsType::array`'s own
    /// `readonly: false` default cannot represent.
    #[test]
    fn print_type_renders_a_readonly_array() {
        assert_eq!(
            print_type(&TsType::readonly_array(TsType::named("Order"))),
            "readonly Order[]"
        );
        assert_eq!(
            print_type(&TsType::array(TsType::named("Order"))),
            "Order[]"
        );
    }

    /// P7.9 (#1315): `TsType::Fn`'s zero-`params` form — the query-thunk
    /// wrapper shape, `bynk-emit`'s own `(() => readonly T[])`.
    #[test]
    fn print_type_renders_a_zero_param_function_type() {
        let ty = TsType::Fn {
            params: vec![],
            ret: Box::new(TsType::readonly_array(TsType::named("Order"))),
        };
        assert_eq!(print_type(&ty), "() => readonly Order[]");
    }

    /// P7.9 (#1315): `TsType::Fn`'s real parametered form — positional
    /// `a0`/`a1`/… names, matching `bynk-emit`'s own pre-P7.9 convention
    /// exactly (TypeScript requires *some* name in function-type syntax).
    #[test]
    fn print_type_renders_a_parametered_function_type_with_positional_names() {
        let ty = TsType::Fn {
            params: vec![TsType::named("string"), TsType::named("number")],
            ret: Box::new(TsType::named("void")),
        };
        assert_eq!(print_type(&ty), "(a0: string, a1: number) => void");
    }

    /// `TsType::Union`, added during review of #1315's own implementation —
    /// a real gap `bynk-emit`'s `ts_ty` needed for a resolved multi-actor
    /// sum (`Ty::ActorSum`), beyond the accepted proposal's own `readonly`/
    /// `Fn` gap list. Members print `" | "`-joined, in order.
    #[test]
    fn print_type_renders_a_union_of_named_types() {
        let ty = TsType::Union(vec![
            TsType::named("string"),
            TsType::named("number"),
            TsType::Object(vec![("tag".to_string(), TsType::named("\"literal\""))]),
        ]);
        assert_eq!(print_type(&ty), "string | number | { tag: \"literal\" }");
    }

    /// `TsStmtKind::Comment`, added for Arc C's own first real conversion
    /// slice (#1317). A multi-line comment (embedded `\n`) prints one
    /// `// `-prefixed line per real line.
    #[test]
    fn prints_a_comment_one_prefixed_line_per_embedded_newline() {
        let mut program = TsProgram::new();
        program.push(TsStmt::comment(
            "Generated by bynkc — do not edit by hand.\nSecond line.",
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(
            printed.text,
            "// Generated by bynkc — do not edit by hand.\n// Second line.\n"
        );
    }

    /// Two adjacent top-level `Comment` statements get no blank line
    /// between them — the same exception already established for two
    /// adjacent `import`s, matching `events_fanout.rs`'s own real two-line
    /// header banner (built as two separate `Comment` statements, not one
    /// with an embedded newline).
    #[test]
    fn no_blank_line_between_two_adjacent_comment_statements() {
        let mut program = TsProgram::new();
        program.push(TsStmt::comment(
            "Generated by bynkc — do not edit by hand.",
            None,
        ));
        program.push(TsStmt::comment("Second line.", None));
        program.push(TsStmt::decl(
            TsDecl::Interface {
                name: "A".to_string(),
                members: vec![],
            },
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(
            printed.text,
            "// Generated by bynkc — do not edit by hand.\n// Second line.\n\ninterface A {\n}\n"
        );
    }

    /// `TsExpr::multiline_object`, added for Arc C's own first real
    /// conversion slice (#1317): `events_fanout.rs`'s own `__eventRoutes`
    /// table is a top-level `const` initialiser with one entry per line,
    /// each with its own trailing comma (including the last), closing
    /// brace back at the statement's own indent — TypeScript's ordinary
    /// multi-line object-literal convention, which `TsExpr::object`'s
    /// single-line form cannot represent. Only reachable through a
    /// statement/declaration-level renderer that carries `depth`
    /// (`render_stmt_level_expr`) — this exercises it via a top-level
    /// `ConstDecl`, the real shape `events_fanout.rs` itself uses.
    #[test]
    fn prints_a_multiline_object_as_a_top_level_const_initialiser() {
        let mut program = TsProgram::new();
        program.push(TsStmt::decl(
            TsDecl::ConstDecl {
                name: "table".to_string(),
                ty: None,
                init: TsExpr::multiline_object(vec![
                    ("a".to_string(), TsExpr::Lit(TsLit::Num("1".to_string()))),
                    ("b".to_string(), TsExpr::Lit(TsLit::Num("2".to_string()))),
                ]),
            },
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "const table = {\n  a: 1,\n  b: 2,\n};\n");
    }

    /// Review of #1317/#1318, finding 1: an empty `multiline_object` still
    /// prints the open/close-brace-on-separate-lines shape, not the tight
    /// `{}` a single-line empty object uses — matching the pre-conversion
    /// `writeln!` code's own real behaviour (its `for` loop simply not
    /// iterating, while the open-brace and closing `};` lines were written
    /// unconditionally either way). This is a real, reachable shape:
    /// `events_fanout.rs`'s own `__eventRoutes` table can be empty for a
    /// context that publishes only events nobody subscribes to.
    #[test]
    fn an_empty_multiline_object_still_prints_the_open_and_close_brace_on_separate_lines() {
        let mut program = TsProgram::new();
        program.push(TsStmt::decl(
            TsDecl::ConstDecl {
                name: "table".to_string(),
                ty: None,
                init: TsExpr::multiline_object(vec![]),
            },
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "const table = {\n};\n");
    }

    /// A `multiline_object` reached through the ordinary, depth-unaware
    /// `render_expr` recursion (nested inside another expression, not a
    /// statement's own top-level initialiser) falls back to single-line
    /// rendering — documented on `TsExpr::Object` itself as a real,
    /// deliberate boundary, not silently wrong. Pinned here so a future
    /// change to that boundary is a visible, intentional decision.
    #[test]
    fn a_nested_multiline_object_falls_back_to_single_line() {
        let mut program = TsProgram::new();
        program.push(TsStmt::expr_stmt(
            TsExpr::Array(vec![TsExpr::multiline_object(vec![(
                "a".to_string(),
                TsExpr::Lit(TsLit::Num("1".to_string())),
            )])]),
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "[{ a: 1 }];\n");
    }

    /// Coverage gap named in review of #1314, finding 4: the blank-line-
    /// between-top-level-declarations policy was never tested for the
    /// mixed `Verbatim` + real-`Decl` case — the case that actually keeps
    /// every P7.6 fixture's zero-diff claim true today, since every P7.6
    /// construction site is all-`Verbatim`. Pins both halves of the rule's
    /// asymmetry: no blank line is added *after* a `Verbatim` statement
    /// (P7.7's own boundary — its content's own trailing spacing isn't the
    /// printer's decision), but a blank line *is* added after a real
    /// `Decl` even when `Verbatim` follows it.
    #[test]
    fn no_blank_line_after_verbatim_but_one_before_it() {
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Contracts,
            "const legacy = 1;\n",
            None,
        ));
        program.push(TsStmt::decl(
            TsDecl::Interface {
                name: "A".to_string(),
                members: vec![],
            },
            None,
        ));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Secrets,
            "const trailing = 2;\n",
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(
            printed.text,
            "const legacy = 1;\ninterface A {\n}\n\nconst trailing = 2;\n"
        );
    }

    /// Coverage gap named in review of #1314, finding 4: nothing exercised
    /// `render_block_body`'s own graceful-degradation path — its doc
    /// comment promises a single non-`Block` statement still renders
    /// sensibly as a one-statement body rather than panicking, but no test
    /// ever passed one. `TryCatch` is the real caller that could hit this
    /// (its `try_block`/`catch_block` are typed as a bare `TsStmt`, not
    /// required to be a `Block`).
    #[test]
    fn a_try_block_that_is_not_a_block_statement_still_prints_as_a_braced_body() {
        let mut program = TsProgram::new();
        program.push(TsStmt::try_catch(
            TsStmt::expr_stmt(
                TsExpr::Call {
                    callee: Box::new(TsExpr::Ident("risky".to_string())),
                    args: vec![],
                },
                None,
            ),
            "e",
            TsStmt::expr_stmt(
                TsExpr::Call {
                    callee: Box::new(TsExpr::Ident("handle".to_string())),
                    args: vec![TsExpr::Ident("e".to_string())],
                },
                None,
            ),
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(
            printed.text,
            "try {\n  risky();\n} catch (e) {\n  handle(e);\n}\n"
        );
    }

    /// Coverage gap named in review of #1314, finding 4: `render_inline_stmt`'s
    /// shared fallback arm (finding 3's fix — explicit variants, not a
    /// wildcard) was never actually exercised by a test; every existing
    /// brace-free `if`/`for...of` body used `Continue`/`Return`/`ExprStmt`,
    /// which each have their own dedicated inline arm.
    #[test]
    fn a_brace_free_if_body_that_is_not_one_of_the_dedicated_inline_kinds_still_prints() {
        let mut program = TsProgram::new();
        program.push(TsStmt::if_stmt(
            TsExpr::Ident("cond".to_string()),
            TsStmt::const_stmt(
                TsBindingName::Ident("x".to_string()),
                None,
                TsExpr::Lit(TsLit::Num("1".to_string())),
                None,
            ),
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "if (cond) const x = 1;\n");
    }

    /// Review of #1314, finding 2: nothing checked operator-precedence
    /// parenthesisation outside `As` — a `Binary` nested as the operand of
    /// `Unary`/`Member`/`Await` printed with no parens at all, silently
    /// changing what the text parses back to. Three of the review's own
    /// examples, pinned directly.
    #[test]
    fn parenthesises_a_binary_operand_of_unary_member_and_await() {
        let bin = || TsExpr::Binary {
            op: TsBinaryOp::NullishCoalescing,
            left: Box::new(TsExpr::Ident("a".to_string())),
            right: Box::new(TsExpr::Ident("b".to_string())),
        };

        let mut not_program = TsProgram::new();
        not_program.push(TsStmt::expr_stmt(
            TsExpr::Unary {
                op: TsUnaryOp::Not,
                expr: Box::new(bin()),
            },
            None,
        ));
        assert_eq!(
            print(&not_program, "x.bynk", "", "x.ts").text,
            "!(a ?? b);\n"
        );

        let mut member_program = TsProgram::new();
        member_program.push(TsStmt::expr_stmt(
            TsExpr::Member {
                object: Box::new(bin()),
                property: "c".to_string(),
            },
            None,
        ));
        assert_eq!(
            print(&member_program, "x.bynk", "", "x.ts").text,
            "(a ?? b).c;\n"
        );

        let mut await_program = TsProgram::new();
        await_program.push(TsStmt::expr_stmt(TsExpr::Await(Box::new(bin())), None));
        assert_eq!(
            print(&await_program, "x.bynk", "", "x.ts").text,
            "await (a ?? b);\n"
        );
    }

    /// Review of #1314, finding 2's fourth example: a right-nested `Binary`
    /// (`a ?? (b ?? c)`, as built) must not print as the bare `a ?? b ?? c`
    /// that a naive left-to-right walk would produce — that text parses
    /// back as `(a ?? b) ?? c`, a different tree than what was built.
    #[test]
    fn parenthesises_a_nested_binary_operand_of_another_binary() {
        let mut program = TsProgram::new();
        program.push(TsStmt::expr_stmt(
            TsExpr::Binary {
                op: TsBinaryOp::NullishCoalescing,
                left: Box::new(TsExpr::Ident("a".to_string())),
                right: Box::new(TsExpr::Binary {
                    op: TsBinaryOp::NullishCoalescing,
                    left: Box::new(TsExpr::Ident("b".to_string())),
                    right: Box::new(TsExpr::Ident("c".to_string())),
                }),
            },
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "a ?? (b ?? c);\n");
    }

    /// Review of #1314, finding 1: `TsLit::Str` escaped only `"`/`\`, so a
    /// literal containing a newline, tab, or carriage return printed raw —
    /// unterminated-string-literal TypeScript `tsc` can't parse. Pins the
    /// fix against every character `bynk_check::wire_default::
    /// escape_ts_literal` escapes.
    #[test]
    fn escapes_newline_tab_and_carriage_return_in_a_string_literal() {
        let mut program = TsProgram::new();
        program.push(TsStmt::expr_stmt(
            TsExpr::Lit(TsLit::Str("line one\nline two\ttabbed\r".to_string())),
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "\"line one\\nline two\\ttabbed\\r\";\n");
    }
}
