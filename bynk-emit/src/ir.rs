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
//! type. `Match`'s own payload is four types: [`IrPat`]/[`IrArm`]/
//! [`Exhaustive`] are P6.4's own commission (#1157, Part 5.1/5.2 of the
//! reference); [`MatchForm`] is P6.5's own (#1159, R5.2/R5.3, scoped to
//! shape only — Decision A). All four are real, constructible types as of
//! P6.5, wired into a real [`IrExprKind::Match`] by [`lower`]'s
//! `ExprKind::Match` arm, which calls P6.4's own standalone constructors
//! ([`lower::lower_pattern_ir`]/[`lower::lower_arm_ir`]/
//! [`lower::lower_exhaustive_ir`]) verbatim. `Question`/`Is` stay
//! desugars-to-`Match` in name only — neither gets real construction this
//! slice, each for a reason specific to it (see [`lower`]'s own `todo!()`
//! text for each).

use std::sync::Arc;

use bynk_check::checker::{Callee, TyId};
use bynk_syntax::ast::{Refinement, TypeDecl};
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

/// P6.4's real Pattern IR (`design/bynk-greenfield-compiler.md` §5.1, #1157):
/// a pattern's own recursive shape, six variants mapping one-to-one onto
/// `bynk_syntax::ast::Pattern`'s own six (`Wildcard`, `Binding`, `Literal`,
/// `Variant`, `Refined`, `Or`). [`lower::lower_pattern_ir`] is the
/// `&Pattern -> IrPat` constructor, tested standalone — not yet wired into
/// [`IrExprKind::Match`]/`Question`/`Is` construction (P6.5's own
/// commission). No `PatId` arena — a pattern owns its children directly
/// (`Box<IrPat>`), the same "no arena exists in this codebase" substitution
/// this module's own doc comment already applies throughout.
#[derive(Debug, Clone)]
pub(crate) enum IrPat {
    /// `_` — matches anything, binds nothing.
    Wild,
    /// A name binding — matches anything, binds `local` to the whole value
    /// at this position (Decision B substitution: `String`, no `LocalId`
    /// arena).
    Bind { local: String },
    /// A literal pattern — `Int`/`Str`/`Bool` only (ADR 0001's closed
    /// literal-pattern set), reusing [`ConstVal`] rather than inventing a
    /// narrower value type for the variants (`Float`/`DurationMillis`/
    /// `Unit`) no `Pattern::Literal` ever produces.
    Const { value: ConstVal },
    /// A sum-variant pattern, `Variant` or `Variant(bindings)` — not just a
    /// user-declared sum (Decision A): `scrutinee_ty` is resolved through
    /// the checker's own `variants_of`, the same function that already
    /// flattens a user sum, `Result`, `Option`, `ActorSum` and `HttpResult`
    /// into one uniform shape, rather than `Callee::Ctor`'s
    /// `Arc<TypeDecl>`-keyed identity scheme, which never fires for
    /// `Ok`/`Err`/`Some`/`None` (`#1145`'s own Decision B, left open).
    Variant {
        /// The value this pattern matches against — resolved via
        /// `variants_of(scrutinee_ty, ..)` to find `tag`'s own payload
        /// shape. Not the sum's own declaration identity: no `Arc<TypeDecl>`
        /// exists for a built-in sum like `Option`/`Result`.
        scrutinee_ty: TyId,
        tag: String,
        /// Exactly the payload bindings the source pattern names — a named
        /// form may bind a strict subset of the variant's payload fields
        /// (`bynk-check`'s own `check_pattern` allows this); empty for a
        /// nullary pattern, even over a non-nullary variant (`Miss` without
        /// `(..)` binds nothing and only tests the tag).
        fields: Vec<(String, Box<IrPat>)>,
    },
    /// `p 'where' predicate` — R5.4: a refinement is a *test*, ordered after
    /// structural matching and before the guard; never a binding site of its
    /// own.
    Refined {
        inner: Box<IrPat>,
        refinement: Refinement,
    },
    /// `p1 | p2 | … | pn` — matches if any alternative matches. R5.5's own
    /// binding-mode consequence lives on [`IrArm::binding_mode`], not here —
    /// an `Or` node is a pure structural fact about the pattern's shape.
    Or { alts: Vec<IrPat> },
}

/// P6.4's real `IrArm` (Part 5.1, #1157) — the reference's own bare sketch
/// (`struct IrArm { pat, guard, body, binds }`) adapted per this module's
/// "no arena" substitution (`local: LocalId -> String`, `pat: PatId ->
/// IrPat` owned directly) plus one field the sketch doesn't carry:
/// `binding_mode` (Decision C, R5.5, computed once during this arm's own
/// construction rather than re-walked by any later reader —
/// `design/bynk-greenfield-compiler.md:749-751`). See [`BindingMode`]'s own
/// doc comment for exactly what this one arm-level flag does and doesn't
/// tell a future reader.
#[derive(Debug, Clone)]
pub(crate) struct IrArm {
    pub pat: IrPat,
    pub guard: Option<IrExpr>,
    pub body: IrExpr,
    /// Every name this arm's pattern binds (Decision B substitution:
    /// `String`, no `LocalId`) — `Pattern::bound_names`'s own "first
    /// alternative" defensive default for an `Or` (the checker separately
    /// verifies every alternative binds the same set at the same types).
    pub binds: Vec<String>,
    pub binding_mode: BindingMode,
}

/// R5.5, Decision C — `OrDispatch` iff `IrPat::Or` occurs anywhere in the
/// arm's own pattern tree, computed once by [`lower::lower_arm_ir`].
///
/// **Arm-level granularity, not node-level** — worth being precise about,
/// since `emit_pattern_bindings` (`emitter/lower.rs:5401-5503`) is not the
/// single top-level check this doc comment used to claim: it is itself a
/// recursive walk, and its `Pattern::Or` arm fires wherever an `Or` node
/// occurs, at whatever depth — a nested `Or` inside a `Variant` payload
/// really parses (`Hit(a, Sub(b) | Other(b))`) and really reaches it. So
/// this flag answers "does *any* node in this arm's pattern need `let` +
/// per-alternative dispatch", not "which node" — a consumer that needs the
/// latter still walks `IrPat`'s own recursive shape to find it (P6.5's own
/// job, whichever future consumer needs it). What this flag removes is
/// having to do that walk at all just to answer the yes/no question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingMode {
    /// No `Or` anywhere in the pattern's tree — every bound name can emit
    /// as a plain `const`.
    Direct,
    /// An `Or` occurs somewhere in the pattern's tree — at least one shared
    /// name can bind at a different structural path across alternatives, so
    /// emission needs `let` plus a per-alternative dispatch somewhere in
    /// this arm (see this type's own doc comment for why "somewhere" is as
    /// precise as this flag alone gets).
    OrDispatch,
}

/// P6.4's real `Exhaustive` (Part 5.2, R5.6/R5.7, #1157) — `Partial`'s own
/// witness payload reuses the checker's already-shipped `missing_patterns`
/// shape (`Vec<String>`, human-readable witness descriptions) rather than
/// inventing the reference's own unspecified `PatternWitness` struct
/// (Decision B). Both variants are real and matchable, but this slice's own
/// [`lower::lower_exhaustive_ir`] only ever constructs `Total` — see that
/// function's own doc comment for why `Partial` is real, inhabited code,
/// yet unreached here.
#[derive(Debug, Clone)]
pub(crate) enum Exhaustive {
    Total,
    Partial(Vec<String>),
}

/// P6.5's own real `MatchForm` (#1159, R5.2/R5.3) — scoped to shape only
/// (Decision A). The reference's own table crosses tail-vs-value position
/// with flat-vs-if-chain shape into four printed forms, but position is
/// decided by *where in the AST the caller already is* — the same mechanism
/// that already decides tail-vs-value for every other `IrExprKind`,
/// including `If`, which P6.1 already committed to modelling
/// position-agnostically (this module's own `IrExprKind::If` doc comment).
/// `Match`'s own `scrutinee`/`arms`/`exhaustive` are identical regardless of
/// position, so only the shape bit is recorded here — a future printer
/// derives the tail-vs-value physical shape itself, the same way it would
/// for `If`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchForm {
    /// A flat `switch` on the scrutinee's own tag/value — no arm carries a
    /// guard or a refutable nested payload pattern.
    Flat,
    /// An if/else-if chain (ADR 0169) — at least one arm carries a guard or
    /// a refutable nested payload pattern a single-level `switch` can't
    /// express.
    IfChain,
}

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
    /// Record construction. `fields` is always complete — every field the
    /// record declares is present, exactly once, a shorthand field (`{ x }`)
    /// resolved to its full `(name, value)` pair during lowering same as
    /// every other field — and *ordered by evaluation order*, left to
    /// right: a reader walking `fields` in order reproduces the same
    /// left-to-right effect sequencing a value expression's own evaluation
    /// has, so this is never re-sorted to, say, the record's own declared
    /// field order once evaluation order and declaration order diverge
    /// (`RecordSpread`'s own lowering, `ir/lower.rs`'s
    /// `lower_record_spread_ir`, is the one producer where they can).
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
    /// Pattern matching. `scrutinee`/`arms`/`exhaustive`/`form` are all
    /// real, constructible values as of P6.5 (#1159) — `arms`/`exhaustive`
    /// built by calling P6.4's own `lower_pattern_ir`/`lower_arm_ir`/
    /// `lower_exhaustive_ir` verbatim (#1157), `form` by reusing the
    /// shipped string emitter's own `match_needs_if_chain`/
    /// `pattern_has_nested_test` (Decision B) rather than re-deriving an
    /// equivalent predicate over `IrPat`'s own (slightly different)
    /// recursive shape.
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
