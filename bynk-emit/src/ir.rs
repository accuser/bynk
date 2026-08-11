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
//!
//! **P6.6 (#1161) adds [`IrItem`]/[`TypeShape`] (Part 6.6)** — a *separate*
//! top-level type from [`IrExpr`], not a new [`IrExprKind`] variant:
//! declarations and expressions are different node families in the
//! reference sketch, and unlike `Match`'s own payload, `IrItem` carries no
//! constraint that its whole seven-variant design-sketch shape must exist as
//! of this slice — see [`IrItem`]'s own doc comment for exactly which are
//! real.
//!
//! **P6.7 (#1163) adds [`StoreFieldIr`]/[`StoreKindIr`]/[`IndexIr`]** (Part
//! 6.6's own trailing two structs, R6.14) — an agent `store` field's own
//! state shape and index-table keys, derived once here rather than
//! re-derived independently by both the checker's own ephemeral
//! `checker::StoreField` dispatch and the shipped emitter's own
//! `store_map_fields`/`store_cache_fields`/`store_log_fields`/…
//! (`bynk-emit/src/emitter/emit.rs`). Same posture as every prior P6.x
//! slice: no `IrItem` variant references these yet — `IrItem::Agent`/
//! `Service` remain unconstructed (see [`IrItem`]'s own doc comment for
//! exactly what still blocks them).
//!
//! **P6.8 (#1165) adds [`CommitShape`]/[`IrPredicate`]** (Part 6.7's own
//! trailing two types, R6.15) — a handler body's own resolved one-of-three
//! commit shape, decided once here from a mutating `Callee::Store` write or
//! a bare `:=`, rather than re-derived at emission time by the shipped
//! emitter's own name-matching `block_writes_state` (R6.5). Same posture
//! again: no `IrItem` variant references either yet — `IrHandler` itself
//! still does not exist, and no rule in `design/tracks/the-ir.md`'s own
//! slice table commissions it (see [`IrItem`]'s own doc comment).
//!
//! **P6.9 (#1167) adds [`IrStmt::Assign`]/[`ActorBinder`]/[`IrHandler`]**
//! (Part 6.7's own trailing construct, R6.16) — a real, standalone,
//! constructible agent `on call` handler, plus the `Statement::Assign`
//! prerequisite gap (`todo!()` since P6.1, twice deferred by P6.7/P6.8's own
//! Risks) it needed closed first: [`lower::lower_handler_ir`] cannot lower a
//! store-writing handler body without a real `IrStmt` target for `:=`.
//! `binder: Option<ActorBinder>` is always `None` from
//! [`lower::lower_handler_ir`] this slice — an agent handler structurally
//! cannot carry one (`bynk.actor.by_on_agent`); a real service handler's own
//! non-`None` `binder` needs a `bynk-check` change this slice does not make
//! (see [`IrHandler`]'s own doc comment). Same posture again: no `IrItem`
//! variant references `IrHandler` yet — `IrItem::Agent`/`Service` remain
//! unconstructed (see [`IrItem`]'s own doc comment).

use std::sync::Arc;

use bynk_check::checker::{Callee, TyId};
use bynk_syntax::ast::{BaseType, FnDecl, HandlerKind, Refinement, TypeDecl};
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
/// reference's own two-variant `IrStmt` (Part 6.2), extended by P6.9
/// (#1167) with a third: `Send`/`Do` become `Expr` wrapping
/// [`IrExprKind::Send`]/[`IrExprKind::Await`]; `EffectLet` becomes `Let`
/// wrapping an `Await`. `Assign` (a `Cell` `:=` write) is real as of P6.9
/// ([DECISION B], #1167) — this comment used to (twice) forward-reference
/// `Callee::Store` territory for it, on a premise P6.9's own grounding pass
/// found false: `checker.rs`'s own `Statement::Assign` arm resolves
/// `a.target.name` directly against `ctx.store_fields` by bare name and
/// never keys a `Callee` at all (only `a.value`, an ordinary sub-expression,
/// ever gets one), so no `ExprId`-keyed sink was ever actually needed —
/// [`IrStmt::Assign`] is the ordinary two-field `Let`-shaped fix that was
/// available the whole time. `Expect` (test-only) has no target here — not
/// named by any rule this track commissions — and stays `todo!()` in the
/// lowering pass, not silently dropped.
#[derive(Debug, Clone)]
pub(crate) enum IrStmt {
    Let {
        local: String,
        value: IrExpr,
    },
    Expr {
        value: IrExpr,
    },
    /// `cell := value` — [DECISION B] (#1167). `field` is the target
    /// `Cell` field's own bare name (`AssignStmt.target.name`, this
    /// module's usual "no arena" substitution); `value` is the ordinary
    /// lowered RHS.
    Assign {
        field: String,
        value: IrExpr,
    },
}

/// P6.6's real declaration IR (`design/bynk-greenfield-compiler.md` §6.6,
/// #1161) — a top-level declaration's own shape, the payload
/// [`lower::lower_type_item_ir`]/[`lower::lower_fn_item_ir`] construct.
/// Identity is adapted per this module's own "no arena" substitution
/// (`DefId -> Arc<TypeDecl>`/`Arc<FnDecl>`, the same substitution
/// `Record`/`GlobalRef` already made).
///
/// **Only `Type` and `Fn` exist as variants this slice** (Decision D,
/// #1161, matching the issue's own title: "Agent/Service/Actor/Capability/
/// Provider deferred"). This is a different posture from `Match`'s own
/// payload (`IrPat`/`IrArm`/`Exhaustive`/`MatchForm`), which had to exist —
/// even genuinely uninhabited — the moment `IrExprKind::Match`'s own field
/// list was written, because `IrExprKind` is one enum whose whole Part 6.2
/// shape landed in a single slice (P6.1) and never grows a new variant
/// after. `IrItem` carries no such constraint: nothing outside this module
/// matches on it exhaustively yet (no consumer at all, same posture as
/// every prior P6.x slice), so a later slice can add a wholly new variant
/// rather than needing a placeholder reserved here in advance. Each
/// deferred variant has its own real, distinct blocker, not a shared
/// "later" (full grounding in #1161's own Decision D):
/// - `Agent`/`Service` both need `IrHandler`/`StoreFieldIr`/`CommitShape` —
///   all three are real as of P6.9 (`StoreFieldIr`/`StoreKindIr` since P6.7,
///   R6.14, #1163; `CommitShape` since P6.8, R6.15, #1165; `IrHandler`
///   since P6.9, R6.16, #1167) — but assembling them into one
///   `IrItem::Agent` value, and giving `IrItem` a variant to carry it at
///   all, is nobody's row yet (`design/tracks/the-ir.md`'s own slice table
///   has none past P6.9) — #1167's own Risks section names this gap
///   precisely, the same "named, not silently absorbed" treatment #1163's
///   and #1165's own Risks gave the gap each of them closed. `Service`
///   additionally carries a materially larger surface than its own
///   reference sketch shows at all (CORS/security-headers/request-body-size
///   policy structs, `ServiceDecl.cors`/`security`/`limits`), and a real
///   service `IrHandler` additionally needs the `binder`-persistence
///   `bynk-check` change [`IrHandler`]'s own doc comment names — P6.9 built
///   the agent-only path only ([`IrHandler`]'s own Decision D).
/// - `Actor` has no emitted artefact of its own today (R8.1: "no
///   declaration; drives the boundary wrapper in `compose.ts`") — genuinely
///   unsettled how `auth`/`identity`/`claims` map onto the reference's own
///   sketch, not safely guessable from the reference alone.
/// - `Capability`'s own `ops: Vec<OpSig>` names a type (`OpSig`) the
///   reference never defines anywhere — the same "referenced, not
///   specified" gap as `Actor`'s `AuthScheme`.
/// - `Provider`'s `body: ProviderBody` needs modelling `ProviderDecl`'s own
///   `Bynk`/`External(module)` dispatch — not named closely enough in the
///   reference to build without its own grounding pass.
#[derive(Debug, Clone)]
pub(crate) enum IrItem {
    /// A `type` declaration. `shape` covers all three real [`TypeShape`]
    /// forms.
    Type {
        def: Arc<TypeDecl>,
        shape: TypeShape,
    },
    /// A `fn` declaration — free function or method alike (`FnName::Free`/
    /// `FnName::Method`); which `IrItem::Fn`s a future printer re-attaches
    /// under which `IrItem::Type`'s own namespace (R8.1) is phase 7's own
    /// concern, not decided here.
    Fn {
        def: Arc<FnDecl>,
        /// The method receiver's own type, generic in the owning type's own
        /// rigid variables (e.g. `Box[A]`'s `self` is `Ty::Named { name:
        /// "Box", args: [Ty::Var("A")], .. }`) — `None` for a free function.
        /// **Not** in `params`: `self` is never in `f.params` either
        /// (`FnDecl::has_self` gates it, mirrored by
        /// [`lower::lower_fn_body_ir`]'s own binding), but `body` still
        /// references it as `Local { name: "self" }` when `has_self` is
        /// true — a consumer that walks `body` needs this field to know
        /// what that bare name resolves to, rather than re-deriving the
        /// generic-receiver type itself from `def`.
        receiver: Option<TyId>,
        /// Adapts the reference's own bare `Vec<LocalId>` to `Vec<(String,
        /// TyId)>` (Decision E, #1161) — no arena exists to look a param's
        /// type back up from its name alone once this `IrItem` outlives the
        /// `LowerIrCtx` that resolved it, unlike every other Decision-B
        /// "name only" substitution in this module, where the type is
        /// either implied by context or carried alongside on the same node
        /// (e.g. `IrExpr::ty`).
        params: Vec<(String, TyId)>,
        ret: TyId,
        /// [`lower::lower_fn_body_ir`]'s own return value, unchanged
        /// (#1141) — this constructor adds no further transformation.
        body: IrExpr,
        /// Derived once from `ret`'s structural shape (`Ty::Fn`'s own doc
        /// comment, `bynk-check/src/checker.rs`: effectful iff `ret` is
        /// `Effect[_]`), never threaded from anywhere else — the same
        /// single-source-of-truth discipline this module already follows
        /// for [`BindingMode`]/[`Exhaustive`].
        effectful: bool,
    },
}

/// P6.6's real `TypeShape` (Part 6.6, #1161) — a declared type's own
/// resolved structure, the payload of [`IrItem::Type`]. Covers the AST's
/// four `TypeBody` variants (`Refined`/`Record`/`Sum`/`Opaque`) with the
/// reference's own three ([DECISION A]): `Opaque` unifies into `Refined`
/// via its own `opaque: bool` field, mirroring `emitter/emit.rs`'s own
/// `RefinedShape { base, refinement, is_opaque }` — the shipped emitter's
/// own precedent for exactly this unification (`emit_type`,
/// `emitter/emit.rs:19`).
#[derive(Debug, Clone)]
pub(crate) enum TypeShape {
    /// Every field the record declares, in declaration order ([DECISION B]
    /// extended: a field's own inline `refinement` is dropped — a
    /// construction-time constraint the checker already enforces, not part
    /// of the emitted shape. Scoped claim: no reader on the record-*type*
    /// emission path (`emit_record_type`, `emitter/emit.rs:234-263`, reads
    /// `type_ref` alone). There *is* one `.refinement` reader in the
    /// emitter overall — `emitter/emit.rs:2781`, agent-state zero-value
    /// construction — but that is `StoreFieldIr` territory (P6.7), out of
    /// this variant's scope; do not read this comment as licence to drop
    /// `refinement` from a future store field too.
    Record { fields: Vec<(String, TyId)> },
    /// Every variant the sum declares, each with its own payload field
    /// list, plus any `embeds` clauses ([DECISION C]: [`EmbedIr`]).
    Sum {
        variants: Vec<(String, Vec<(String, TyId)>)>,
        embeds: Vec<EmbedIr>,
    },
    /// `type X = base where refinement` (`Refined`) or `type X = unsafe
    /// base ...` (`Opaque`, `opaque: true`). `refinement` is `Option`, not
    /// the reference's own bare `RefinementId` ([DECISION B]) — a bare
    /// `type X = Int` (no `where` clause) is legal and carries none.
    Refined {
        base: BaseType,
        refinement: Option<Refinement>,
        opaque: bool,
    },
}

/// The payload of [`TypeShape::Sum`]'s own `embeds` — a resolved `embeds`
/// clause ([DECISION C], #1161): the source type paired with the target
/// variant's own tag name. A plain tuple, not a dedicated struct, mirroring
/// [`IrPat::Variant`]'s own `fields: Vec<(String, Box<IrPat>)>` precedent
/// for a two-part fact with no further structure.
pub(crate) type EmbedIr = (TyId, String);

/// P6.7's real store-field state shape (`design/bynk-greenfield-compiler.md`
/// §6.6, R6.14, #1163) — the payload of an agent `store` field declaration,
/// [`lower::lower_store_field_ir`]'s own return value. Mirrors
/// `checker::StoreField`'s own five-kind dispatch
/// (`bynk-check/src/checker.rs`) in shape, but is persistent IR data with no
/// consumer yet, not that checking pass's own ephemeral, per-agent scratch
/// value — the two are deliberately not unified (see
/// [`lower::lower_store_field_ir`]'s own doc comment). No `IrItem` variant
/// references this yet — `IrItem::Agent`/`Service` remain unconstructed
/// (`IrItem`'s own doc comment names exactly what still blocks them).
#[derive(Debug, Clone)]
pub(crate) struct StoreFieldIr {
    /// The field's own declared name ([DECISION A]: `String`, sourced
    /// directly from `StoreField.name.name` — this module's own "no arena
    /// exists in this codebase" substitution, applied to the reference's
    /// own `FieldId` arena slot).
    pub field: String,
    pub kind: StoreKindIr,
    /// The fresh-key initialiser, constructed only for a `Cell` field
    /// ([DECISION D]) — `None` for every other kind, regardless of whether
    /// the AST grammatically parsed one there. A non-`Cell` field's `init`
    /// expression is parsed but never type-checked (a real, pre-existing
    /// checker gap; see [`lower::lower_store_field_ir`]'s own doc comment),
    /// so on a certified program it has no `expr_types` entry to lower.
    pub init: Option<IrExpr>,
    /// `@indexed(by: …)` sibling-table keys, in the annotation's own
    /// `by:`-argument order — one entry per *distinct* `by:` argument
    /// ([DECISION C]), no sort ([DECISION E]). Deduplicated: the checker
    /// validates each `by:` argument independently with no duplicate check
    /// (`validate_indexed_keys`), so `@indexed(by: k, by: k)` certifies —
    /// [`lower::lower_store_field_ir`] guards against it, mirroring the
    /// shipped emitter's own `store_map_indexes` dedup. Empty for every kind
    /// but `Map`, the only kind `@indexed` attaches to (`ANNOTATIONS`'s own
    /// registry, `bynk-check/src/context_checks.rs`).
    pub indexed: Vec<IndexIr>,
}

/// P6.7's real `StoreKindIr` (Part 6.6, R6.14, #1163) — five variants, one
/// per functional storage kind (`Cell`/`Map`/`Set`/`Cache`/`Log`). `Queue`
/// is not a variant here: `bynk.store.kind_unsupported` gates it before
/// `certify` (R3.10), so this type is total over what a certified program's
/// own store fields can actually contain, not a subset some later slice
/// needs to extend. [DECISION B]: `Duration` substitutes to `i64`
/// milliseconds throughout — the same substitution [`ConstVal::DurationMillis`]
/// and `checker::StoreField::Cache`'s own already-resolved TTL already made.
#[derive(Debug, Clone)]
pub(crate) enum StoreKindIr {
    /// `Cell[T]` — element type.
    Cell(TyId),
    /// `Map[K, V]` — key, value.
    Map(TyId, TyId),
    /// `Set[T]` — element type.
    Set(TyId),
    /// `Cache[K, V] @ttl(...)` — key, value, TTL in milliseconds.
    Cache(TyId, TyId, i64),
    /// `Log[T] [@retain(...)]` — element type, optional retain millis.
    Log(TyId, Option<i64>),
}

/// The payload of [`StoreFieldIr::indexed`] — one `@indexed(by: …)` key,
/// identified by the indexed value-field's own name ([DECISION C], #1163):
/// referenced by the reference's own `StoreFieldIr.indexed: Vec<IndexIr>`
/// but never defined anywhere in the document, the same "referenced, not
/// specified" gap #1161's own Decision C named for [`EmbedIr`]. No
/// dedicated struct: the sibling table's own emitted shape
/// (`Record<string, string[]>`) is fixed by the *map's own key type*, not
/// the indexed field's, so the indexed field's resolved type is not needed
/// downstream — mirrors `EmbedIr`'s own "no further structure" precedent.
pub(crate) type IndexIr = String;

/// P6.8's real `IrPredicate` ([DECISION A], #1165) — referenced by the
/// reference's own `CommitShape::Transactional { invariants: Vec<IrPredicate>,
/// transitions: Vec<IrPredicate> }` and `IrItem::Agent`'s own sketch
/// (`bynk-greenfield-compiler.md:1130-1131`/`1182`), but never defined
/// anywhere in the document — the same "referenced, not specified" gap
/// #1161's own Decision C named for [`EmbedIr`] and #1163's own Decision C
/// named for [`IndexIr`]. One type serves both an agent's own `invariants`
/// and `transitions` fields, rather than two near-identical structs:
/// `Invariant`/`Transition` (`bynk_syntax::ast`) already share this exact
/// shape — a name plus a `Bool`-typed predicate expression — and
/// [`lower::lower_invariant_ir`]/[`lower::lower_transition_ir`] differ only
/// in how they seed the predicate's own scope (an invariant over the
/// agent's `store` `Cell` fields, a transition over `old`/`new`), not in
/// what they produce. `name: String` is this module's own "no arena"
/// substitution ([DECISION B] extended) — a predicate has no `DefId` of its
/// own in the reference either, referenced only by position within its
/// owning `Vec`.
#[derive(Debug, Clone)]
pub(crate) struct IrPredicate {
    pub name: String,
    pub predicate: IrExpr,
}

/// P6.8's real `CommitShape` (Part 6.7, R6.15, #1165) — a handler body's own
/// resolved one-of-three commit shape, [`lower::lower_commit_shape_ir`]'s own
/// return value. Matches the reference's own three-variant shape verbatim
/// (`bynk-greenfield-compiler.md:1179-1183`) — no substitution needed,
/// `Transactional`'s own payload already reuses [`IrPredicate`] rather than
/// carrying `Invariant`/`Transition` AST nodes directly. Shape-agnostic
/// between an agent and a service handler ([DECISION F]): a service
/// handler's own call site passes empty `invariants`/`transitions` slices,
/// and the identical write-detection walk
/// ([`lower::lower_commit_shape_ir`]'s own doc comment) naturally finds
/// neither a mutating `Callee::Store` nor a bare `:=` in a service body (a
/// service declares no `store` fields to write), so `Transactional` is
/// never constructed for one — the shipped emitter's own `emit_service`
/// already only ever produces the other two shapes, for the same reason.
#[derive(Debug, Clone)]
pub(crate) enum CommitShape {
    /// No store write and no `Events.emit` — the body splices flat, no
    /// commit or flush of any kind.
    ReadOnly,
    /// No store write, but the body emits at least one event — `__events`
    /// is flushed at the end of the handler, state is not copied.
    FlushEvents,
    /// A mutating `Callee::Store` write or a bare `:=` reaches this body —
    /// state is snapshotted, the body runs in an IIFE, then the snapshot is
    /// committed (and `__events`, if any, flushed alongside it). Carries
    /// the agent's own already-lowered invariants/transitions
    /// ([`lower::lower_invariant_ir`]/[`lower::lower_transition_ir`]), not
    /// the raw AST lists — a future consumer checking them at commit time
    /// reads real `IrPredicate`s, not `Invariant`/`Transition` nodes it
    /// would have to lower itself.
    ///
    /// **Does not carry its own `emits` bit** — matches the reference's own
    /// verbatim shape (`bynk-greenfield-compiler.md:1182`), which gives
    /// `Transactional` no such field even though a writing handler that also
    /// emits is real (the shipped emitter's own `writes_state`/
    /// `body_emits_directly`, `emitter/emit.rs:3124`/`3290`, are independent
    /// booleans — a store write does not preclude an emit). A future
    /// `IrHandler` consumer that needs both facts at once — to decide
    /// whether *this* `Transactional` handler also flushes `__events` —
    /// re-derives it the same way `lower_commit_shape_ir`'s own caller
    /// already must ([DECISION D]: `crate::emitter::block_uses_emit(body)`),
    /// not from this variant. Named here rather than silently assumed lost:
    /// the fact is recoverable from `body`, which every real consumer holds
    /// alongside a `CommitShape` in the first place ([`crate::ir::IrItem`]'s
    /// own doc comment — no `IrHandler` exists yet to pin this against).
    Transactional {
        invariants: Vec<IrPredicate>,
        transitions: Vec<IrPredicate>,
    },
}

/// P6.9's real `ActorBinder` ([DECISION A], #1167) — referenced by the
/// reference's own `IrHandler` sketch (`bynk-greenfield-compiler.md:1169-1177`,
/// `binder: Option<ActorBinder>`) but never defined anywhere in the
/// document — the same "referenced, not specified" gap #1161's own
/// Decision C named for [`EmbedIr`], #1163's own Decision C named for
/// [`IndexIr`], and #1165's own Decision A named for [`IrPredicate`].
/// Mirrors the checker's own already-resolved
/// `actor_binding: Option<(String, TyId)>` shape
/// (`bynk-check/src/checker.rs`'s `HandlerBodyCheck::actor_binding`):
/// `binder` is the bound name, `ty` the sealed `Ty::Actor(identity)` or
/// `Ty::ActorSum(members)` — both ordinary, already-real `TyId`s, no
/// synthetic type needed the way `<Agent>State` was for P6.8's `state_ty`.
/// No dedicated `lower_actor_binder_ir` constructor: the pair has no
/// further structure to derive, mirroring [`EmbedIr`]'s/[`IndexIr`]'s own
/// "no further structure, plain tuple/alias" precedent — a future caller
/// with the resolved pair in hand (once the `bynk-check` change
/// [`IrHandler`]'s own doc comment names persists `handler_actor_binding`'s
/// own output) writes `ActorBinder { binder, ty }` directly.
#[derive(Debug, Clone)]
pub(crate) struct ActorBinder {
    pub binder: String,
    pub ty: TyId,
}

/// P6.9's real `IrHandler` ([DECISION C], #1167) — an agent `on call`
/// handler's own resolved shape, [`lower::lower_handler_ir`]'s own return
/// value. Six of the eight fields are the reference's own verbatim sketch
/// (`bynk-greenfield-compiler.md:1169-1177`) under this module's
/// already-established substitutions: `kind: HandlerKind` reused verbatim
/// from `bynk_syntax::ast` (an ordinary `Clone`/`PartialEq` enum with no
/// arena identity — the same "reused, not adapted" treatment
/// [`IrExprKind::Call`]'s own `Callee` payload already got); `params`/
/// `given` are this module's standard "no arena" substitution (`params:
/// Vec<(String, TyId)>` mirrors [`IrItem::Fn::params`] exactly; `given:
/// Vec<String>` reads each `CapRef::key()`, the same identity
/// `Callee::Capability` already uses); `binder: Option<ActorBinder>` per
/// [`ActorBinder`]'s own doc comment; `body: IrExpr` is
/// [`lower::lower_handler_ir`]'s own new handler-body lowering entry point
/// (parallel to, but distinct from, [`lower::lower_fn_body_ir`] — that
/// entry point's own doc comment names exactly why a handler body cannot
/// reuse it); `commit: CommitShape` calls [`lower::lower_commit_shape_ir`]
/// (P6.8, unchanged); `effectful: bool` reuses [`IrItem::Fn::effectful`]'s
/// own derivation (`Ty::Fn`'s doc: effectful iff `ret` is `Effect[_]`)
/// unchanged.
///
/// **`method_name: Option<String>` is added beyond the reference's own
/// sketch** — the same class of addition #1162's own review made for
/// [`IrItem::Fn::receiver`] (#1161): the reference sketch has no slot for a
/// handler's own declared name at all, but a future printer (or R8.10's own
/// handler-key mangling) structurally needs it to know *which* of an
/// agent's several `on call <name>` handlers this is. `None` for the shapes
/// that have none today (a service's bare `on call`).
///
/// [`lower::lower_handler_ir`] is agent-only this slice ([DECISION D]) — a
/// real service handler's `IrHandler` (specifically, a non-`None` `binder`)
/// is not constructed here: `handler_actor_binding`'s own resolved
/// `(String, TyId)` (`bynk-check/src/context_checks.rs`) is checking-time-
/// only scratch with no persisted home in `TypedCommons`/`CheckedProgram`
/// today, so closing that is a `bynk-check` change this lowering-only slice
/// does not make speculatively. Not a functional gap for R6.16's own claim
/// (invocation origin-independence is specifically about an *agent*
/// handler): an agent handler's own `binder` is `None` unconditionally and
/// by construction — `bynk.actor.by_on_agent`
/// (`context_checks.rs:2986-2996`) rejects any `by` clause on an agent
/// handler outright, so [`lower::lower_handler_ir`] never has one to lower
/// in the first place.
#[derive(Debug, Clone)]
pub(crate) struct IrHandler {
    pub kind: HandlerKind,
    pub params: Vec<(String, TyId)>,
    pub given: Vec<String>,
    pub binder: Option<ActorBinder>,
    pub body: IrExpr,
    pub commit: CommitShape,
    pub effectful: bool,
    pub method_name: Option<String>,
}
