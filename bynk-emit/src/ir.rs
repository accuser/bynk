//! P6.1 (design/tracks/the-ir.md §6, #1141): the IR's core node types —
//! `IrExpr`/`IrExprKind`/`IrStmt`, Part 6.2 of `design/bynk-greenfield-compiler.md`.
//!
//! R6.1 — "Every `IrExpr` carries its type. The constructor requires it;
//! there is no side table and no fallible lookup from the emitter to the
//! checker." This module is the type only; [`lower`] is the
//! `&CheckedProgram → Ir` pass that constructs values of it.
//!
//! **Identity fields are adapted, not literal** (Decision B, extending P6.0's
//! own precedent, ADR 0333 `the-ir-callee-in-bynk-check`): the reference's
//! `DefId`/`FieldId`/`LocalId`/`VariantId` arena does not
//! exist in this codebase (no `Resolve` phase mints them —
//! `project-model.md` §3.4 deferred that to phase 8), so every such slot
//! becomes whatever cheap resolved handle the checker already has —
//! `Arc<TypeDecl>`/`Arc<FnDecl>` for a declaration, `String` for a name with
//! no arena of its own. `Call`'s payload is [`Callee`] verbatim — P6.0
//! already did this exact substitution for call-dispatch identity, so `Call`
//! needs no adaptation of its own here.
//!
//! **The whole Part 6.2 shape lands in one piece** (Decision D): every
//! variant below exists, including `Match`/`Variant`/`Call`/`Lambda`, so a
//! later slice (P6.2, P6.4, P6.5) widens only [`lower`]'s match, never this
//! type. `Match`'s own payload (`IrArm`/`Exhaustive`/`MatchForm`) is *not*
//! designed here — that is P6.4/P6.5's named commission (Part 5.1/5.2 of the
//! reference) — so those three are left genuinely uninhabited (an empty
//! `enum`, constructible by no one) rather than a guessed-at shape a real
//! slice would have to unpick later.

use std::sync::Arc;

use bynk_check::checker::{Callee, TyId};
use bynk_syntax::ast::TypeDecl;
use bynk_syntax::span::Span;

pub(crate) mod lower;

/// A lowered expression: its shape, its checked type, and the source span it
/// came from. `ty` is required at construction (R6.1) — never `Option`,
/// never looked up lazily by a reader.
#[derive(Debug, Clone)]
pub(crate) struct IrExpr {
    pub kind: IrExprKind,
    pub ty: TyId,
    pub span: Span,
}

/// A constant value — the payload of [`IrExprKind::Const`]. Adapted from the
/// reference's own `ConstVal` (`Int Float Str Bool Unit Bytes`, Part 6.2's
/// comment): `DurationMillis` replaces `Bytes` because Bynk has a real
/// `<int>.<unit>` duration *literal* (`ExprKind::DurationLit`) `Const` must
/// cover, while `Bytes` has no literal AST form at all in this language —
/// every `Bytes` value comes from a static-constructor *call*
/// (`Bytes.fromUtf8`/`fromBase64`/`empty()`, `Callee::Intrinsic` territory,
/// not a literal).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ConstVal {
    Int(i64),
    Float(f64),
    DurationMillis(i64),
    Str(String),
    Bool(bool),
    Unit,
}

/// The payload of [`IrExprKind::Global`] — adapted per Decision C's narrow
/// scope, refined during implementation: a bare nullary sum-variant
/// constructor reference (`Miss`, `PaymentDeclined`), identified structurally
/// (exactly one sum type in `TypedCommons::types` owns a variant of this
/// name with an empty payload — the same "unique owner" test
/// `check_ident`'s own unconditional fallback arm uses, `bynk-check/src/checker/expressions.rs:103-130`).
///
/// The proposal's own Decision C also named a bare `HttpResult`/
/// `QueueResult` nullary built-in variant reference (`NotFound`, `Ack`) as
/// in scope — dropped during implementation: the checker's own detection for
/// that case (`checker.rs`'s `type_of`, `ExprKind::Ident` arm) is gated on
/// `expected`/`ctx.return_ty` ("resolve to `HttpResult` only when the
/// surrounding type implies it, or no user sum-type variant of the same name
/// exists") — contextual, position-dependent disambiguation this pass has no
/// sink to read back, the exact re-derivation this track's whole `Callee`
/// precedent (P6.0) exists to avoid. Left for a slice that either adds that
/// sink or accepts re-deriving it, not built worse here to hit a self-set
/// scope target.
#[derive(Debug, Clone)]
pub(crate) struct GlobalRef {
    pub sum: Arc<TypeDecl>,
    pub tag: String,
}

/// Placeholder for P6.4's Pattern IR (`design/bynk-greenfield-compiler.md`
/// §5.1) — not designed by this slice. Exists only so
/// [`IrExprKind::Match`] matches Part 6.2's own field list; genuinely
/// uninhabited (no variant), since `Match`'s lowering arm is `todo!()` and
/// nothing here ever constructs one.
#[derive(Debug, Clone)]
pub(crate) enum IrArm {}

/// Placeholder — see [`IrArm`]'s doc comment. P6.4 builds the real
/// witness-carrying shape (Part 5.2, R5.6).
#[derive(Debug, Clone)]
pub(crate) enum Exhaustive {}

/// Placeholder — see [`IrArm`]'s doc comment. P6.5, not P6.4, decides and
/// records this (R5.2: tail-vs-value position × flat-vs-if-chain shape).
#[derive(Debug, Clone)]
pub(crate) enum MatchForm {}

/// A lowered expression's shape. Every node kind from Part 6.2 exists here
/// (Decision D); [`lower`] implements real construction only for the
/// subset named in `design/tracks/the-ir.md`'s own P6.1 row — every other
/// arm is a named `todo!()` in the lowering pass, not a missing variant
/// here.
#[derive(Debug, Clone)]
pub(crate) enum IrExprKind {
    /// A literal value.
    Const(ConstVal),
    /// Reading a function-scoped local or parameter, by name (Decision B —
    /// no `LocalId` arena exists).
    Local(String),
    /// A bare reference to something with no scope of its own — narrowly
    /// scoped per Decision C; see [`GlobalRef`].
    Global(GlobalRef),
    /// Record construction. `fields` is always complete and ordered — a
    /// shorthand field (`{ x }`) is resolved to its full `(name, value)`
    /// pair during lowering, same as every other field.
    Record {
        def: Arc<TypeDecl>,
        fields: Vec<(String, IrExpr)>,
    },
    /// Sum-variant construction. Lowering deferred to P6.2 (driven by
    /// `Callee::Ctor`, alongside `Call`) — the variant exists now so `Call`'s
    /// sibling shapes don't need a second widening later.
    Variant {
        sum: Arc<TypeDecl>,
        tag: String,
        payload: Vec<IrExpr>,
    },
    /// Field access on a record value.
    Field { base: Box<IrExpr>, field: String },
    /// A list literal.
    List { elems: Vec<IrExpr> },
    /// A `{ ... }` block: statements, then a tail value.
    Block {
        stmts: Vec<IrStmt>,
        tail: Box<IrExpr>,
    },
    /// `if cond { then } else { else }` — both branches always present (an
    /// else-less `if` already carries a synthesised unit `else` at the AST
    /// level, `Block::is_synth_unit`).
    If {
        cond: Box<IrExpr>,
        then_: Box<IrExpr>,
        else_: Box<IrExpr>,
    },
    /// Pattern matching. Lowering deferred to P6.4/P6.5 — see [`IrArm`]'s
    /// doc comment for why its payload types are placeholders.
    Match {
        scrutinee: Box<IrExpr>,
        arms: Vec<IrArm>,
        exhaustive: Exhaustive,
        form: MatchForm,
    },
    /// `lhs && rhs` — short-circuit, structurally (R6.3, already true of the
    /// existing string-based emitter; this tree shape makes it true of the
    /// IR too, not just the emission-time machinery threaded to preserve it).
    And { lhs: Box<IrExpr>, rhs: Box<IrExpr> },
    /// `lhs || rhs`.
    Or { lhs: Box<IrExpr>, rhs: Box<IrExpr> },
    /// `!operand`.
    Not { operand: Box<IrExpr> },
    /// A function/handler body's own tail value, in return position. Built,
    /// not parsed — Bynk has no `return` keyword; this node is constructed
    /// only by `lower::lower_fn_body_ir` wrapping a body block's tail (the
    /// `?` operator's early-return desugar is P6.3's row, a second future
    /// producer of this same node).
    Return { value: Box<IrExpr> },
    /// A call, classified by P6.0's `Callee` — no adaptation needed here,
    /// `Callee` already resolves identity the way this module's other
    /// `DefId`-shaped slots do. Lowering deferred to P6.2.
    Call {
        callee: Callee,
        targs: Vec<TyId>,
        args: Vec<IrExpr>,
    },
    /// A lambda. Lowering deferred to P6.2, alongside `Call` (a lambda's
    /// only use today is as a kernel-method argument, `Callee::Kernel`
    /// territory).
    Lambda {
        params: Vec<String>,
        body: Box<IrExpr>,
        captures: Vec<String>,
    },
    /// `<- effect` — await an `Effect[T]`'s value.
    Await { effect: Box<IrExpr> },
    /// `~> effect` — fire-and-forget; typed `Unit`.
    Send { effect: Box<IrExpr> },
    /// `Effect.pure(value)` — introduce a synchronous value as `Effect[T]`.
    Pure { value: Box<IrExpr> },
}

/// A lowered statement — Bynk's real statement surface
/// (`Let`/`EffectLet`/`Expect`/`Send`/`Do`/`Assign`) folds down onto the
/// reference's own two-variant `IrStmt` (Part 6.2): `Send`/`Do` become
/// `Expr` wrapping [`IrExprKind::Send`]/[`IrExprKind::Await`]; `EffectLet`
/// becomes `Let` wrapping an `Await`. `Assign` (a `Cell` store write) and
/// `Expect` (test-only) have no target here — `Assign` is `Callee::Store`
/// territory (P6.2/P6.7), and `Expect` is not named by any rule this track
/// commissions — both are `todo!()` in the lowering pass, not silently
/// dropped.
#[derive(Debug, Clone)]
pub(crate) enum IrStmt {
    Let { local: String, value: IrExpr },
    Expr { value: IrExpr },
}
