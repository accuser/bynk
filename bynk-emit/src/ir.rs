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
//! **Decision D's own "never widens beyond the reference's Part 6.2 shape"
//! is not absolute — it held only as long as the reference's own node set
//! was complete** ([`IrExprKind::BinOp`]/[`IrExprKind::Neg`]/
//! [`IrExprKind::InterpStr`], #1189).
//! `design/bynk-greenfield-compiler.md` §6.2's own listing (`Const, Local,
//! Global, Record, Variant, Field, List, Block, If, Match, And, Or, Not,
//! Return, Call, Lambda, Await, Send, Pure`) never names comparison,
//! arithmetic, unary negation, or string interpolation at all — confirmed a
//! true omission, not a deliberately-out-of-scope row this track chose to
//! defer (unlike, say, `Question`/`Is` above, each of which the reference
//! *does* name and this slice explicitly declines). P6.2 (#1143) and P6.3
//! (#1145) each independently confirmed the gap and left it a `todo!()`
//! rather than force-fitting it under `Decision D`'s closed-shape framing;
//! #1189 settled it as buildable, three new variants, added here rather than
//! deferred indefinitely — real programs use `+`/`-`/`<`/`==` routinely (a
//! certified `.bynk` invariant like `balance >= 0` is ordinary, not an edge
//! case), and the checker already resolves every one of these expressions'
//! types (`bynk-check/src/checker/expressions.rs`'s `check_binop`/
//! `check_unary`, and `checker.rs`'s own `InterpStr` hole-checking) — nothing
//! about them was actually blocked on new checker work, only on this type
//! gaining a place to land them.
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
//!
//! **P6.10 (#1169) adds [`IrItem::Agent`]** — the assembly P6.9's own Risks
//! section named without numbering it: every ingredient (`StoreFieldIr`
//! since P6.7, `CommitShape`/`IrPredicate` since P6.8, `IrHandler` since
//! P6.9) was already real, but nothing combined them into one `IrItem`
//! value, and `IrItem` had no variant to carry the result. This is the
//! first `IrItem` variant with a real consumer shape below the top level —
//! [`lower::lower_agent_item_ir`] calls every prior slice's own standalone
//! constructor rather than re-deriving any of their logic, the same
//! "wire, don't re-derive" posture this whole module has held since P6.0.
//! `IrItem::Service`/`Actor`/`Capability`/`Provider` remain deferred, each
//! for its own reason (see [`IrItem`]'s own doc comment).
//!
//! **P6.11 (#1171) adds [`IrItem::Service`]** — the sibling assembly to
//! P6.10's `Agent`, closing both blockers `IrItem`'s own doc comment named:
//! #1170 made a service handler's `binder` readable post-`certify` for the
//! first time, and this slice specifies the two types the reference names
//! but never defines for a service ([`ProtocolIr`], [`PolicyIr`]).
//! [`lower::lower_service_handler_ir`] is a new sibling of
//! [`lower::lower_handler_ir`], not a widening of it — the two seed
//! disjoint scopes, and widening would have deleted the agent-only `by`
//! assertion that today catches a service handler reaching the wrong entry
//! point. A `from websocket` lifecycle handler's own body
//! (`on open`/`on message`/`on close`) is the one named exception: the
//! checker-injected synthetic `connection` param has no IR target yet, so
//! lowering one hits an explicit `todo!()` rather than a silently wrong
//! tree — see [`lower::lower_service_handler_ir`]'s own doc comment.
//! `IrItem::Actor`/`Capability`/`Provider` remain deferred, each with its
//! own already-tracked, genuinely unsettled blocker (see [`IrItem`]'s own
//! doc comment).
//!
//! **P6.12 (#1173) adds [`IrItem::Capability`]** — the reference's own
//! `ops: Vec<OpSig>` sketch (`bynk-greenfield-compiler.md:1134`) named a
//! type it never defined anywhere in the document; [`OpSig`] is that
//! missing shape, adapted from `bynk_syntax::ast::CapabilityOp` under this
//! module's own "no arena" substitution (`Vec<(String, TyId)>` for
//! `params`, mirroring [`IrItem::Fn::params`] exactly).
//! [`lower::lower_capability_item_ir`] resolves each op's own `params`/
//! `return_type` in that op's own rigid-variable scope (`op.type_params`)
//! — a capability op's type parameters are scoped to the op itself, not
//! the capability, unlike a method's generic receiver — the same per-op
//! treatment the checker's own `CapabilityOpInfo` already gives a generic
//! op (`context_checks.rs`'s `build_capability_op_info`). `IrItem::Actor`/
//! `Provider` remain deferred, each with its own already-tracked,
//! genuinely unsettled blocker (see [`IrItem`]'s own doc comment).
//!
//! **P6.14 (#1174) adds [`IrItem::Provider`]** — #1174's own grounding pass
//! found this buildable, unlike its sibling `Actor`: the reference's own
//! `body: ProviderBody // Bynk ops | External(module)` comment names a type
//! it never defines, the same gap [`OpSig`] closed for `Capability`, and
//! `ProviderDecl::external` already carries the exact dispatch it needs.
//! [`lower::lower_provider_op_ir`] is a new sibling of
//! [`lower::lower_fn_body_ir`], not a widening of it, for the same "each
//! body-lowering entry point seeds its own scope" reason
//! [`lower::lower_handler_body_ir`]'s own doc comment already gives: no
//! `self`, no store cells, just a provider op's own params — `given`
//! capability calls need no scope entry of their own, the same
//! already-generic `Callee`-wrapping [`lower::lower_handler_body_ir`]'s own
//! doc comment credits for handler bodies. `IrItem::Actor` remains
//! deferred, its own already-tracked blocker unchanged by this slice (see
//! [`IrItem`]'s own doc comment).

use std::sync::Arc;

use bynk_check::checker::{Callee, TyId};
use bynk_syntax::ast::{BaseType, FnDecl, HandlerKind, Refinement, SchemaVersionPattern, TypeDecl};
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
    /// Comparison/arithmetic (#1189): `Eq`/`NotEq`/`Lt`/`LtEq`/`Gt`/`GtEq`/
    /// `Add`/`Sub`/`Mul`/`Div` — every [`bynk_syntax::ast::BinOp`] member
    /// *except* `And`/`Or`/`Implies`, which stay their own dedicated
    /// variants above (Decision A, #1189: `And`/`Or` exist as their own
    /// nodes specifically to make short-circuit evaluation a structural
    /// property of the tree — R6.3 — and `Implies` desugars away entirely;
    /// none of that applies to a strict, both-operands-always-evaluated
    /// arithmetic/comparison operator, so one shared, `op`-tagged variant
    /// covers all ten without ten near-duplicate variants/lowering arms).
    /// `lhs`/`rhs` are lowered independently — no `is`-binding propagation
    /// exists for these operators, unlike `And`.
    BinOp {
        op: IrBinOp,
        lhs: Box<IrExpr>,
        rhs: Box<IrExpr>,
    },
    /// `-operand` (#1189) — `bynk_syntax::ast::UnaryOp::Neg`'s own
    /// counterpart to `Not` above; the checker requires an `Int` operand and
    /// returns `Int` (`check_unary`, `bynk-check/src/checker/expressions.rs`).
    Neg { operand: Box<IrExpr> },
    /// An interpolated string (#1189) — `bynk_syntax::ast::ExprKind::InterpStr`'s
    /// alternating chunk/hole run, each hole an ordinary lowered expression
    /// (the checker's own hole rule restricts a hole to a scalar or
    /// scalar-refinement type; this module does not re-derive that
    /// restriction, only carries the already-checked result). Always typed
    /// `String`.
    InterpStr { parts: Vec<IrInterpPart> },
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

/// [`IrExprKind::BinOp`]'s own operator tag (#1189) — every
/// [`bynk_syntax::ast::BinOp`] member that lowers into `BinOp` rather than
/// its own dedicated `IrExprKind` variant or a desugar (see `BinOp`'s own
/// doc comment for which and why). Deliberately a plain tag with no payload
/// of its own — `lhs`/`rhs` already carry their own resolved `ty`, and
/// unlike, say, [`StoreKindIr`], no variant here needs anything beyond its
/// own identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IrBinOp {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Add,
    Sub,
    Mul,
    Div,
}

/// [`IrExprKind::InterpStr`]'s own per-part payload (#1189) — the IR-side
/// mirror of [`bynk_syntax::ast::InterpPart`], substituting `Hole`'s raw
/// `Box<Expr>` for an already-lowered `Box<IrExpr>` under this module's
/// usual "carry the lowered form, not the source form" discipline (the same
/// substitution every other `IrExprKind` payload already makes).
#[derive(Debug, Clone)]
pub(crate) enum IrInterpPart {
    /// Literal text between holes, escapes already resolved (as in
    /// [`bynk_syntax::ast::InterpPart::Chunk`]) — an emitter must escape it
    /// for its own target string syntax, as `escape_ts_template` does today.
    Chunk(String),
    /// An interpolated expression, already lowered.
    Hole(Box<IrExpr>),
}

/// P6.6's real declaration IR (`design/bynk-greenfield-compiler.md` §6.6,
/// #1161) — a top-level declaration's own shape, the payload
/// [`lower::lower_type_item_ir`]/[`lower::lower_fn_item_ir`] construct.
/// Identity is adapted per this module's own "no arena" substitution
/// (`DefId -> Arc<TypeDecl>`/`Arc<FnDecl>`, the same substitution
/// `Record`/`GlobalRef` already made).
///
/// **`Type`, `Fn`, `Agent` (P6.10, #1169), `Service` (P6.11, #1171),
/// `Capability` (P6.12, #1173) and `Provider` (P6.14, #1174) exist as
/// variants.** `Actor` is still deferred (Decision D, #1161, matching the
/// issue's own title: "Agent/Service/Actor/Capability/Provider deferred").
/// This is a different posture from
/// `Match`'s own payload (`IrPat`/`IrArm`/`Exhaustive`/`MatchForm`), which
/// had to exist — even genuinely uninhabited — the moment
/// `IrExprKind::Match`'s own field list was written, because `IrExprKind`
/// is one enum whose whole Part 6.2 shape landed in a single slice (P6.1)
/// and never grows a new variant after. `IrItem` carries no such
/// constraint: nothing outside this module matches on it exhaustively yet
/// (no consumer at all, same posture as every prior P6.x slice), so a
/// later slice can add a wholly new variant rather than needing a
/// placeholder reserved here in advance. Each deferred variant has its own
/// real, distinct blocker, not a shared "later" (full grounding in #1161's
/// own Decision D — `Service`'s own two blockers, closed by #1170/#1171,
/// are recorded on `IrItem::Service`'s own doc comment rather than kept
/// here as a stale bullet):
/// - `Actor` — #1172's own investigation settled this one as a **non-build
///   decision**, not an unbuilt gap: the reference's own sketch
///   (`Actor { def, scheme: AuthScheme, identity: Option<TyId>, claims:
///   Option<IrExpr> }`, `bynk-greenfield-compiler.md:1133`) is the wrong
///   shape for this codebase, on three independent findings. (1) `claims:
///   Option<IrExpr>` is unbuildable, not merely unbuilt: an actor
///   refinement's own predicate (`hasClaim`/`claimEquals`,
///   `ActorRefinement::predicate`) is validated only *structurally*
///   (`bynk-check/src/actors.rs`'s `parse_claim_predicate`) — no `Callee`,
///   no `expr_types` entry, no typing at all, because claims are
///   deliberately untyped JSON (`context_checks.rs`'s own
///   `refinement_predicate_unsupported` note) and lowered straight to a JS string by
///   `claim_predicate_to_js`. An `IrExpr` here would mean inventing a
///   typing for a surface the checker deliberately never gave one. (2)
///   `scheme: AuthScheme` names a type the reference defines nowhere else
///   (`:1133`/`:1847` only) — the same "referenced, not specified" gap
///   `Capability`'s own `OpSig` carried before [`OpSig`] settled it (P6.12,
///   #1173), except here a real 5-variant candidate already ships
///   (`bynk-check::actors::Scheme`), so this piece alone is specifiable.
///   (3) The decisive finding: every real consumer of actor data — the
///   five `bynk-check` seam resolvers the shipped emitter
///   consumes (`bearer_seam_for`/`oidc_seam_for`/`signature_seam_for`/
///   `sum_members_for`/`caller_binder_for`, `bynk-check/src/actors.rs`) and
///   the reference's own R8.11 (`deps` derivation)/R8.13 (boundary-wrapper
///   verification) — is **handler-keyed, not declaration-keyed**: binder
///   presence, sum-member ordering, and which scheme's config applies are
///   all facts about a handler's own `by` clause, not the `actor`
///   declaration in isolation. This is exactly why the two actor-shaped IR
///   additions that *did* have a real consumer ([`ActorBinder`], and
///   `IrHandler`'s own `actors: Vec<String>`, review of #1180) both landed
///   on [`IrHandler`], not a new `IrItem` variant — a `{def, scheme,
///   identity}` record beside `IrItem::Service`'s handlers would have zero
///   consumers and would not discharge R6.13 for actors at all, the "carve
///   when a dependency arrives, not on appetite" anti-pattern R10.3 already
///   argues against elsewhere in this track. Revisit only if a real
///   phase-7 printer needs declaration-level actor data beyond what
///   `IrHandler::binder`/`IrHandler::actors` already carry — not before.
/// - `Provider` — #1174's own investigation found this one buildable this
///   slice, unlike `Actor`: the real `ProviderDecl`
///   (`bynk-syntax/src/ast.rs:582-600`) already carries the exact
///   `Bynk`/`External` dispatch the reference's own `// Bynk ops |
///   External(module)` comment names (never a real Rust enum in the
///   document itself — the same "referenced, not specified" gap
///   `Capability`'s own `OpSig` carried before #1173 settled it), via
///   `external: bool` plus `ops: Vec<ProviderOp>` (empty exactly when
///   `external` is `true`, per the field's own doc comment,
///   `bynk-syntax/src/ast.rs:592-595`). And unlike `CapabilityOp`, a
///   `ProviderOp`'s own body is a **real, fully type-checked** handler
///   body — `check_provider_decls`
///   (`bynk-check/src/context_checks.rs:626`) pushes every op through
///   `checker::check_handler_body` (`:667`) with the provider's own
///   `given` as its capability scope and no `self`/state/store binding at
///   all (`HandlerBodyCheck::new`'s own "everything optional empty"
///   default, `bynk-check/src/checker.rs:1055`) — so
///   [`lower::lower_provider_op_ir`] mirrors [`lower::lower_fn_body_ir`]
///   (ADR 0334 panic-on-miss, not [`lower::lower_op_sig_ir`]'s lenient
///   `Ty::Unit` fallback: this body's types are checker-guaranteed to
///   resolve, same as a fn's) with no receiver/self and no rigid type
///   variables (`ProviderOp` carries none of its own,
///   `bynk-syntax/src/ast.rs:604-611`). [`ProviderBody::Bynk`]'s own `ops:
///   Vec<ProviderOpIr>` is real per-op signature-plus-body, one
///   `ProviderOpIr` per [`lower::lower_provider_op_ir`] call, in
///   declaration order.
///
///   **`given` is carried, `module` is not — the two omissions are not
///   symmetric** (review of #1186). R8.1's own `Provider{Bynk}` row is
///   `export class` with a **`deps` constructor**
///   (`bynk-greenfield-compiler.md:1309`), built today straight off
///   `ProviderDecl::given` (`bynk-emit/src/project.rs:2608-2620`) — unlike
///   `module`, `given` is not one-phase-up data, it lives on the very
///   `ProviderDecl` this function already holds, and it is not
///   reconstructible from the lowered op bodies alone: an unused `given`
///   capability, a cross-context `given B.Cap` prefix (`CapRef::context`,
///   v0.15), and declaration order would all silently vanish if this
///   variant carried only `ops`. So [`ProviderBody::Bynk`] also carries
///   `given: Vec<CapRefIr>` — [`CapRefIr`] adapts `CapRef` under this
///   module's usual "no arena, bare name" substitution (`context`'s own
///   `QualifiedName` flattened via `.joined()`), leaving *resolving* a
///   prefix against `consumes`/aliases to whichever future project-level
///   pass also resolves `module` — [`lower::lower_provider_item_ir`] does
///   no more than [`lower::lower_capability_item_ir`] already declines to
///   do for a capability op's own type refs. [`ProviderBody::External`]
///   deliberately carries **no** `module` field, despite the reference's
///   own parenthetical —
///   that name lives one phase up from this per-context `CheckedProgram`,
///   on the adapter's own `binding "<module>"` clause
///   (`BindingDecl::module`, `bynk-syntax/src/ast.rs:186`), resolved only
///   by the whole-project pass into `adapter_bindings: HashMap<String,
///   AdapterBinding>` (`bynk-check/src/project_model.rs:729`) — data no
///   other `lower_X_item_ir` function in this module reaches either.
///   Nothing needs it yet: R8.1's own emission table already gives
///   `Provider{External}` "nothing — the binding module supplies it"
///   (`bynk-greenfield-compiler.md:1310`), i.e. even phase 7's own per-item
///   printer does not consume a module name — only the still-AST-driven
///   `project.rs`'s existing import wiring does
///   (`bynk-emit/src/project.rs:2675-2678`), untouched by this slice.
///   Revisit only once a real project-level IR/linking phase gives this
///   module something to thread the module name through.
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
    /// P6.10's real `IrItem::Agent` (Part 6.6, R6.13, #1169) —
    /// [`lower::lower_agent_item_ir`]'s own return value, assembling every
    /// P6.7–P6.9 ingredient (`StoreFieldIr`, `CommitShape`/`IrPredicate`,
    /// `IrHandler`) that had no `IrItem` variant to land in until now.
    /// Matches the reference's own sketch
    /// (`bynk-greenfield-compiler.md:1129-1131`) field-for-field, under this
    /// module's already-established substitutions.
    Agent {
        /// The agent's own declared name — this module's usual "no arena"
        /// substitution for the reference's own `DefId`, but *not* the same
        /// substitution [`IrItem::Type`]/[`IrItem::Fn`] made
        /// (`Arc<TypeDecl>`/`Arc<FnDecl>`): unlike `types`/`fns`,
        /// `TypedCommons` carries no `Arc`-wrapped agents table to source a
        /// cheap pointer from (`UnitTable::agents`/`CommonsItem::Agent` are
        /// both a plain, owned `AgentDecl`), and `Callee::Agent { agent:
        /// String, handler: String }` already establishes bare-name
        /// identity as sufficient for this checked output — a full
        /// `AgentDecl` clone (its own handler bodies included) would be
        /// needless weight this module's other `String`-identity fields
        /// (`GlobalRef::tag`, `Callee::Store::field`, …) don't carry either.
        def: String,
        /// `key id: Type` adapted from the reference's own bare `(LocalId,
        /// TyId)` (Decision B extended): the bound name plus its resolved
        /// type, the same `Vec<(String, TyId)>`-per-entry substitution
        /// [`IrItem::Fn::params`] already made for a `Vec<LocalId>`.
        key: (String, TyId),
        /// Every `store` field, [`lower::lower_store_field_ir`]'s own
        /// return value, in declaration order.
        state: Vec<StoreFieldIr>,
        /// Every `on call` handler, [`lower::lower_handler_ir`]'s own
        /// return value, in declaration order.
        handlers: Vec<IrHandler>,
        /// Every invariant, [`lower::lower_invariant_ir`]'s own return
        /// value, in declaration order — the same already-lowered list each
        /// `handlers` entry's own `commit: CommitShape::Transactional`
        /// (when it is one) carries a copy of, not a fresh lowering.
        invariants: Vec<IrPredicate>,
        /// Every transition, [`lower::lower_transition_ir`]'s own return
        /// value, in declaration order — same relationship to `handlers` as
        /// `invariants`.
        transitions: Vec<IrPredicate>,
    },
    /// P6.11's real `IrItem::Service` (Part 6.6, R6.13, #1171) —
    /// [`lower::lower_service_item_ir`]'s own return value, the sibling
    /// assembly to [`IrItem::Agent`]. Matches the reference's own sketch
    /// (`bynk-greenfield-compiler.md:1132`) field-for-field. Closes the two
    /// blockers this variant carried while deferred: a real service
    /// `IrHandler` needed the `binder`-persistence `bynk-check` change
    /// #1170 made, and the CORS/security-headers/request-body-size policy
    /// surface the reference sketch never shows at all now has a real,
    /// specified shape ([`ProtocolIr`]/[`PolicyIr`]).
    Service {
        /// The service's own declared name — same reasoning as
        /// `IrItem::Agent`'s own `def`: `UnitTable::services` is a plain
        /// owned `HashMap<String, ServiceDecl>`, no `Arc` to borrow
        /// cheaply, and bare-name identity is already sufficient for this
        /// checked output.
        def: String,
        /// [`lower::lower_protocol_ir`]'s own return value.
        protocol: ProtocolIr,
        /// Every handler, [`lower::lower_service_handler_ir`]'s own return
        /// value, in declaration order.
        handlers: Vec<IrHandler>,
        /// [`lower::lower_policy_ir`]'s own return value — `None` whenever
        /// `protocol` is not [`ProtocolIr::Http`], not just when the source
        /// declares none of `cors`/`security`/`limits`. See [`PolicyIr`]'s
        /// own doc comment for why this is `Option`, not the reference's
        /// own unconditional `PolicyIr` field.
        policy: Option<PolicyIr>,
    },
    /// P6.12's real `IrItem::Capability` (Part 6.6, R6.13, #1173) —
    /// [`lower::lower_capability_item_ir`]'s own return value. Matches the
    /// reference's own sketch (`bynk-greenfield-compiler.md:1134`)
    /// field-for-field, once [`OpSig`] fills in the type the sketch names
    /// but never defines.
    Capability {
        /// The capability's own declared name — same reasoning as
        /// `IrItem::Agent`/`IrItem::Service`'s own `def`:
        /// `UnitTable::capabilities` is a plain owned `HashMap<String,
        /// CapabilityDecl>`, no `Arc` to borrow cheaply, and bare-name
        /// identity is already sufficient — `Callee::Capability { cap:
        /// String, op: String }` (`bynk-check/src/checker.rs`) already
        /// resolves a same-context capability call by name alone.
        def: String,
        /// Every operation, [`lower::lower_op_sig_ir`]'s own return value,
        /// in declaration order.
        ops: Vec<OpSig>,
    },
    /// P6.14's real `IrItem::Provider` (Part 6.6, R6.13, #1174) —
    /// [`lower::lower_provider_item_ir`]'s own return value. Matches the
    /// reference's own sketch (`bynk-greenfield-compiler.md:1135`)
    /// field-for-field, once [`ProviderBody`] fills in the type the
    /// sketch's own comment names but never defines as a real Rust enum.
    Provider {
        /// The provider's own declared name (`ProviderDecl::provider_name`,
        /// "used in tests/config to select impls",
        /// `bynk-syntax/src/ast.rs:586`) — distinct from `cap` below even
        /// though `UnitTable::providers` happens to key by capability name
        /// today ("one provider per capability in v0.5",
        /// `bynk-check/src/symbols.rs`): the reference's own sketch keeps
        /// `def`/`cap` as two separate `DefId`s, and a provider's own
        /// identity is never actually the capability it implements. Same
        /// "no arena, bare name is enough" substitution as
        /// `IrItem::Agent`/`Service`/`Capability`'s own `def`.
        def: String,
        /// The capability this provider implements
        /// (`ProviderDecl::capability`) — same bare-name substitution as
        /// `def`; resolves against `Callee::Capability`'s own `cap: String`
        /// the identical way `IrItem::Capability::def` does.
        cap: String,
        /// [`lower::lower_provider_item_ir`]'s own `Bynk`/`External`
        /// dispatch, read straight off `ProviderDecl::external`.
        body: ProviderBody,
    },
}

/// P6.14's real `ProviderBody` ([DECISION A], #1174) — referenced by the
/// reference's own `IrItem::Provider` sketch
/// (`bynk-greenfield-compiler.md:1135`, `body: ProviderBody // Bynk ops |
/// External(module)`) but never defined anywhere in the document as a real
/// Rust type, the same "referenced, not specified" gap `Capability`'s own
/// `OpSig` carried before #1173 settled it. Mirrors `ProviderDecl::external`
/// (`bynk-syntax/src/ast.rs:592-595`) exactly: `true` (no brace block, an
/// adapter-supplied binding) becomes `External`, `false` (a real Bynk-
/// authored implementation) becomes `Bynk`. See [`IrItem`]'s own doc
/// comment for why `External` carries no `module` field despite the
/// reference's own parenthetical, and why it carries `given` instead of
/// omitting that too.
#[derive(Debug, Clone)]
pub(crate) enum ProviderBody {
    /// A real Bynk-authored implementation.
    Bynk {
        /// `ProviderDecl::given`, adapted to [`CapRefIr`] — needed to build
        /// R8.1's own `Provider{Bynk}` `deps` constructor, and not
        /// reconstructible from `ops` alone ([`IrItem`]'s own doc comment
        /// has the full grounding, review of #1186). In declaration order.
        given: Vec<CapRefIr>,
        /// Every operation, [`lower::lower_provider_op_ir`]'s own return
        /// value, in declaration order. **Not** guaranteed non-empty
        /// (review of #1186): `external` is set purely by brace-block
        /// *absence*, not emptiness (`bynk-syntax/src/parser/declarations.rs:1795-1811`),
        /// so `provides Cap = P {}` also lowers here, with `ops: vec![]` —
        /// the `bynk.provider.missing_operation` check that would reject
        /// it is a whole-project pass (`bynk-check/src/project_model.rs:2468`),
        /// not part of `check_provider_decls`, so a bare `CheckedProgram`
        /// (this function's only input) is certified without it.
        ops: Vec<ProviderOpIr>,
    },
    /// `provides Cap = Name` with no brace block — the adapter's own
    /// binding supplies the implementation; the emitter produces no class
    /// (`bynk-greenfield-compiler.md:1310`, `Provider{External} | nothing`).
    External,
}

/// P6.14's real `CapRefIr` ([DECISION A], #1174, review of #1186) — one
/// `bynk_syntax::ast::CapRef` entry of a provider's own `given` clause,
/// under this module's usual "no arena, bare name" substitution:
/// `context: Option<QualifiedName>` flattens to `Option<String>` via
/// `QualifiedName::joined()` (the same `.`-joined form
/// `resolve_consume_prefix` — `bynk-emit/src/project.rs` — already
/// resolves against `consumes`/aliases), and `name: Ident` flattens to
/// `String`, mirroring every other bare-name identity field in this module.
/// Deliberately **not** resolved further here: which context a `Some`
/// prefix actually names is whole-project `consumes`/alias data, the same
/// phase boundary [`IrItem`]'s own doc comment already names for
/// `ProviderBody::External`'s missing `module` field — this type only
/// preserves what `CapRef` itself carries, unresolved.
#[derive(Debug, Clone)]
pub(crate) struct CapRefIr {
    pub context: Option<String>,
    pub name: String,
}

/// P6.14's real `ProviderOpIr` ([DECISION A], #1174) — one `ProviderOp`
/// (`bynk-syntax/src/ast.rs:604-611`), signature *and* body, unlike
/// [`OpSig`]'s signature-only shape: a `CapabilityOp` never carries a body
/// (a capability is a contract), but a `ProviderOp` always does. No
/// `type_params`, unlike `OpSig`: `ProviderOp` carries none of its own
/// (#1173's own `OpSig::type_params` doc comment names the same absence for
/// `CapabilityDecl`, but a capability op's `[T, …]` (#926) has no
/// provider-op equivalent — nothing in the grammar or the checker's own
/// `check_provider_decls` gives a provider op a `[T, …]` list to parse).
#[derive(Debug, Clone)]
pub(crate) struct ProviderOpIr {
    pub name: String,
    pub params: Vec<(String, TyId)>,
    pub return_ty: TyId,
    /// [`lower::lower_provider_op_ir`]'s own return value — a real,
    /// checker-guaranteed-to-resolve body (see [`IrItem`]'s own doc
    /// comment for why this differs from [`OpSig`]'s lenient treatment).
    pub body: IrExpr,
}

/// P6.12's real `OpSig` ([DECISION A], #1173) — referenced by the
/// reference's own `IrItem::Capability` sketch
/// (`bynk-greenfield-compiler.md:1134`, `ops: Vec<OpSig>`) but never
/// defined anywhere in the document — the same "referenced, not specified"
/// gap #1161's own Decision C named for [`EmbedIr`], #1163's own Decision C
/// named for [`IndexIr`], #1165's own Decision A named for [`IrPredicate`],
/// and #1167's own Decision A named for [`ActorBinder`]. Adapted from
/// `bynk_syntax::ast::CapabilityOp` — a signature only, no body — under
/// this module's already-established "no arena" substitutions: `params:
/// Vec<(String, TyId)>` mirrors [`IrItem::Fn::params`] exactly (Decision E,
/// #1161), and `type_params: Vec<String>` mirrors the checker's own
/// already-resolved `CapabilityOpInfo::type_params`
/// (`bynk-check/src/checker.rs`) — a bare rigid-variable name, not a
/// `TypeParam` AST node, since nothing here re-derives bounds a capability
/// op's own `[T, …]` list never carries in the first place (#926).
/// [`lower::lower_op_sig_ir`] resolves `params`/`return_ty` in the scope
/// `type_params` names, mirroring `context_checks::build_capability_op_info`'s
/// own `vars` treatment (`bynk-check/src/context_checks.rs`) so a generic
/// op's own `T` survives as `Ty::Var("T")` rather than collapsing to
/// `Ty::Unit`. On a genuinely unresolvable name a `params`/`return_ty` entry
/// *is* `Ty::Unit`, deliberately — see [`lower::lower_op_sig_ir`]'s own doc
/// comment for why that mirrors the checker's own fallback rather than
/// panicking.
#[derive(Debug, Clone)]
pub(crate) struct OpSig {
    pub name: String,
    /// The op's own type parameters (#926) — empty for a non-generic op.
    /// Scoped to the op itself, not the capability: `CapabilityDecl` carries
    /// no `type_params` of its own (`bynk-syntax/src/ast.rs:556-562`), so
    /// this is never merged with anything above it, unlike
    /// [`IrItem::Fn::receiver`]'s generic-receiver treatment for a method.
    pub type_params: Vec<String>,
    pub params: Vec<(String, TyId)>,
    pub return_ty: TyId,
}

/// P6.11's real `ProtocolIr` ([DECISION A], #1171) — one variant per
/// `bynk_syntax::ast::ServiceProtocol` variant, [`lower::lower_protocol_ir`]'s
/// own return value. The reference's own sketch specifies only two of the
/// six: `Events { event, pattern, schema_dispatch }`
/// (`bynk-greenfield-compiler.md:1881`) and `WebSocket { in_ty, out_ty }`
/// (`:1959`) — field names taken verbatim from those two rows. `Call`/
/// `Http`/`Cron` carry no payload, not because one was dropped: the actual
/// per-trigger binding (a route, a schedule) lives on each *handler*
/// (`HandlerKind::Http { method, path }`/`Cron { expr }`), already
/// reachable through `IrHandler::kind` — `ServiceProtocol`'s own doc
/// comment says this in as many words ("the endpoint lives on each
/// handler"), which is why the reference never spells these three out
/// either. E2 (`:1737`) constrains the *set*, not the shape: "a closed
/// nominal set … grows one variant per real trigger" — the AST's own closed
/// `ServiceProtocol` already satisfies that exactly, so this type is total
/// over what a certified program's own service can declare, the same claim
/// [`StoreKindIr`]'s own doc makes about `Queue` being gated pre-`certify`.
#[derive(Debug, Clone)]
pub(crate) enum ProtocolIr {
    Call,
    Http,
    Cron,
    /// `from queue("name")`.
    Queue {
        name: String,
    },
    /// `from websocket(in: …, out: …)` — the two frame types, resolved.
    /// As of P6.13 (#1179), [`lower::lower_service_handler_ir`] lowers an
    /// `on open`/`on message`/`on close` handler's own *body* for a
    /// service carrying this protocol too — see [`ConnectionBinder`]'s own
    /// doc comment for how the synthetic `connection` binding reaches that
    /// body's scope.
    WebSocket {
        in_ty: TyId,
        out_ty: TyId,
    },
    /// `from Events(E)` — the subscribed event type, resolved, plus the
    /// two independent, optional filters a subscription may carry.
    Events {
        event: TyId,
        pattern: Option<EventPatternIr>,
        /// [DECISION B]: `SchemaVersionPattern` reused verbatim from
        /// `bynk_syntax::ast` rather than flattened to `Option<i64>` — an
        /// ordinary `Clone` enum with no arena identity, the same "reused,
        /// not adapted" treatment `IrHandler::kind` already gets, and its
        /// own doc comment states that a future range pattern (`via
        /// schema(2..)`) is additive to this exact enum, not a breaking
        /// rename of whoever already matches on it. The `SchemaDispatch`
        /// wrapper itself is dropped — it carries only this `pattern` plus
        /// a parse-only `span`.
        schema_dispatch: Option<SchemaVersionPattern>,
    },
}

/// The payload of `ProtocolIr::Events`'s own `pattern` — a `from
/// Events(E { field: value, .. })` structural filter, [DECISION C] (#1171).
/// **Not** `bynk_syntax::ast::EventPattern` reused verbatim, unlike
/// `SchemaVersionPattern`: that type carries `rest_span` (a parse artefact
/// for the grammar-required trailing `..`, giving a later reader nothing to
/// act on) and its own `EventPatternValue::Variant` is an *unresolved,
/// optionally-qualified* name pair — exactly the shape this module's whole
/// posture rejects everywhere else.
#[derive(Debug, Clone)]
pub(crate) struct EventPatternIr {
    /// `(field name, matched value)`, in source order — no dedicated
    /// `EventPatternFieldIr` struct, mirroring `IrPat::Variant`'s own
    /// `fields: Vec<(String, Box<IrPat>)>` precedent for a two-part fact
    /// with no further structure.
    pub fields: Vec<(String, EventPatternValueIr)>,
}

/// One [`EventPatternIr`] field's own matched value.
#[derive(Debug, Clone)]
pub(crate) enum EventPatternValueIr {
    /// Reuses [`ConstVal`] for the closed `Int`/`Str`/`Bool` literal set —
    /// verbatim the same reuse `IrPat::Const` already made for
    /// `Pattern::Literal`'s identical closed set.
    Const(ConstVal),
    /// A nullary sum-variant tag, resolved and unqualified — bare
    /// `tag: String` mirrors `IrPat::Variant`'s own `tag`/[`GlobalRef`]'s
    /// own `tag`. The AST's own optional qualifying `type_name` is
    /// dropped, not lost: the sole consumer
    /// ([`crate::emitter::lower::event_pattern_guard_ir`], #1187's slice 5)
    /// already destructures down to the bare tag alone — the qualification
    /// is disambiguation for the *checker*, resolved against the field's
    /// declared sum type before this point.
    Variant { tag: String },
}

/// P6.11's real `PolicyIr`/`CorsIr`/`SecurityIr` ([DECISION D], #1171) —
/// the interpreted (not passed-through) form of a `from http` service's
/// `cors`/`security`/`limits` blocks, [`lower::lower_policy_ir`]'s own
/// return value. The reference names `PolicyIr` once, in the
/// `IrItem::Service` sketch itself (`:1132`), and never elsewhere — the
/// same "referenced, not specified" gap every trailing struct in this
/// module has carried before its own slice specified it.
///
/// **Interpreted, not passed through** — the one discipline this struct
/// exists to enforce. Every AST policy type
/// (`bynk_syntax::ast::CorsPolicy`/`SecurityPolicy`/`LimitsPolicy`) stores
/// raw, unvalidated `{name, value: Expr}` pairs; meaning only exists
/// through each type's own already-shipped typed accessor
/// (`CorsPolicy::origins()`/`credentials()`/`allow_headers()`/
/// `max_age_secs()`, `SecurityPolicy::nosniff()`/`hsts_max_age_secs()`,
/// `LimitsPolicy::max_body()`) — carrying the raw AST struct here instead
/// would embed exactly the kind of unresolved value this whole track
/// (R6.13) exists to remove from the IR. The shipped emitter already never
/// reads a policy's own `.fields` directly (`emitter/workers_entry.rs`),
/// so this is that emitter's own already-established reading of the AST,
/// moved upstream, not a new interpretation invented here.
#[derive(Debug, Clone)]
pub(crate) struct PolicyIr {
    pub cors: Option<CorsIr>,
    /// **Not `Option`** — unlike `cors`/`limits`, `security: None` on the
    /// AST means *defaults* (`nosniff` on), not *no headers* (ADR 0164
    /// Decision A) — keeping this `Option` here would re-export exactly
    /// the ambiguity this struct exists to remove. The `None`-source arm
    /// of [`lower::lower_policy_ir`] materialises `SecurityIr { nosniff:
    /// true, hsts_max_age_secs: None }` — the emitter's own already-shipped
    /// default, verbatim, not invented here.
    pub security: SecurityIr,
    /// `LimitsPolicy::max_body()`'s own return value, flattened directly —
    /// no dedicated `LimitsIr` struct. `LimitsPolicy` has exactly one
    /// accessor; a one-field wrapper would carry no more information than
    /// this `Option<i64>` itself, the same "no further structure"
    /// precedent [`EmbedIr`]/[`IndexIr`] already set.
    pub max_body_bytes: Option<i64>,
}

/// The payload of `PolicyIr::cors` — present only when the source declares
/// a `cors { }` block at all (ADR 0159's own opt-in posture); the
/// asymmetry with `PolicyIr::security` is this struct's load-bearing
/// content, not an inconsistency.
#[derive(Debug, Clone)]
pub(crate) struct CorsIr {
    pub origins: Vec<String>,
    pub credentials: bool,
    /// The author's own explicit `headers:` override, kept `Option` rather
    /// than materialising the emitter's own smart default (`content-type`
    /// and `authorization` when a `Bearer`-authed route exists,
    /// `emitter/workers_entry.rs`) — that default reads *route* facts
    /// (whether any handler's actor is `Bearer`/OIDC-scheme), not policy
    /// facts, the same reason `StoreFieldIr::init` stays `Option` rather
    /// than a materialised zero value.
    pub allow_headers: Option<Vec<String>>,
    pub max_age_secs: Option<i64>,
}

/// The payload of `PolicyIr::security` — always present for an HTTP
/// service (see `PolicyIr::security`'s own doc comment).
#[derive(Debug, Clone)]
pub(crate) struct SecurityIr {
    pub nosniff: bool,
    pub hsts_max_age_secs: Option<i64>,
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
/// between an agent and a service handler ([DECISION F]): as of P6.11
/// (#1171), [`lower::lower_service_handler_ir`] is the real service call
/// site this decision predicted, passing empty `invariants`/`transitions`
/// slices, and the identical write-detection walk
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
    /// alongside a `CommitShape` in the first place — `IrHandler` itself
    /// (real as of P6.9, #1167) is exactly that consumer, carrying both
    /// `commit`/`body` side by side.
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
/// "no further structure, plain tuple/alias" precedent — as of P6.11
/// (#1171), [`lower::lower_service_handler_ir`] is the real caller that
/// reads `TypedCommons::actor_bindings` (#1170) and writes
/// `ActorBinder { binder, ty }` directly; [`lower::lower_handler_ir`]
/// (agent-only) still never does — see [`IrHandler`]'s own doc comment.
#[derive(Debug, Clone)]
pub(crate) struct ActorBinder {
    pub binder: String,
    pub ty: TyId,
}

/// P6.13's real `ConnectionBinder` ([DECISION G], #1179) — the synthetic
/// leading `connection: Connection[out]` binding a `from websocket`
/// lifecycle handler's body receives, [`lower::lower_service_handler_ir`]'s
/// own return value for exactly the `on open`/`on message`/`on close`
/// handlers of a `ServiceProtocol::WebSocket` service. Mirrors the
/// checker's own `open_connection_param`
/// (`bynk-check/src/context_checks.rs:2020-2032`): `ty` is the resolved
/// `Ty::Connection(out_ty)`, always present regardless of handler kind —
/// unlike `ActorBinder`, there is no `None` case here, since a websocket
/// lifecycle handler is checker-guaranteed to receive this binding
/// (`bynk.service.websocket_open_arity`, `context_checks.rs:742-763`, plus
/// the unconditional injection at `context_checks.rs:1944-1954`).
///
/// Deliberately **not** folded into `IrHandler::params`: the checker keeps
/// this binding out of `handler.params` itself (only `params_for_check`,
/// a check-local, carries it) — `IrHandler::params` mirrors `h.params`
/// exactly for every handler kind, and widening it here would be a real
/// behavior change beyond what this slice asks for, not a faithful mirror
/// of the checker's own asymmetry. Consumers that need "does this handler
/// have a connection" read `IrHandler::connection` instead.
///
/// `borrowed` is this slice's own IR target for the checker's own
/// owned-vs-borrowed linearity distinction (`borrowed_held`,
/// `context_checks.rs:1955-1963`): `false` for `on open` (a fresh owned
/// socket the handler must dispose/transfer), `true` for `on message`/
/// `on close` (the borrowed firing socket, no disposal obligation). A
/// bare `bool`, not a richer type: `borrowed_held` itself is only ever a
/// `HashSet<String>` keyed by binding name on the checker side, so there
/// is no further structure to represent for a type that names exactly one
/// binding (`"connection"`, never persisted as a field here — every
/// reader already knows the name statically, the same reasoning
/// `ActorBinder`'s own doc comment gives for omitting a redundant
/// discriminant). No dedicated `lower_connection_binder_ir` constructor,
/// following `ActorBinder`'s own precedent: the pair has no further
/// structure to derive.
#[derive(Debug, Clone)]
pub(crate) struct ConnectionBinder {
    pub ty: TyId,
    pub borrowed: bool,
}

/// P6.9's real `IrHandler` ([DECISION C], #1167) — an agent `on call`
/// handler's own resolved shape, [`lower::lower_handler_ir`]'s own return
/// value. Six of the reference's own eight sketched fields are its
/// verbatim shape (`bynk-greenfield-compiler.md:1169-1177`) under this module's
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
/// **`actors: Vec<String>` is a second addition beyond the reference's own
/// sketch** (review of #1171/#1180) — the actor name(s) a `by` clause
/// itself names, read directly from `h.by_clause.as_ref().map(|by|
/// &by.actors)` rather than from `binder`. Needed because `binder` alone
/// cannot represent the gate: a binder-less `by <Actor>` (verify-and-
/// discard, `ByClause::binder: Option<Ident>`) resolves to `binder: None`
/// with nothing else distinguishing it from a handler with no `by` clause
/// at all, and the same erasure hits a binder that shadowed a param and
/// was suppressed (`context_checks.rs:2050-2055`) — in both cases the
/// authorization gate is real even though no identity got bound to a
/// local. Even the happy path loses information a single-actor `binder`
/// alone can't recover: `ActorBinder::ty` is `Ty::Actor(identity_ty)`,
/// which carries the *sealed identity type*, not the actor's own
/// *declared name* — `by u: Buyer` and `by u: Seller` sharing one identity
/// type would otherwise lower to byte-identical `ActorBinder`s. The sum
/// path already avoids this (`Ty::ActorSum(Vec<(String, TyId)>)` retains
/// member names); `actors` gives the single-actor and no-binder cases the
/// same guarantee, uniformly, mirroring [`IrHandler::given`]'s own "read
/// straight off the AST, not through any checker-persisted resolution"
/// shape. Empty for an agent handler unconditionally — the same
/// `bynk.actor.by_on_agent` guarantee [DECISION D] already grounds for
/// `binder`.
///
/// [`lower::lower_handler_ir`] is agent-only **by design, still** ([DECISION
/// D]) — a real service handler's `IrHandler` (specifically, a non-`None`
/// `binder`) is never constructed by *this* function, and that stays true
/// even now that #1170 persisted `handler_actor_binding`'s own resolved
/// `(String, TyId)` into `TypedCommons::actor_bindings`/`CheckedProgram`.
/// P6.11 (#1171) built the real service-handler path as a **sibling**,
/// [`lower::lower_service_handler_ir`], not a widening of this one —
/// [`lower::lower_handler_ir`]'s own doc comment names exactly why
/// (disjoint scopes, and widening would delete the `by_clause.is_none()`
/// assertion that today catches a service handler reaching the wrong
/// entry point). Not a functional gap for R6.16's own claim (invocation
/// origin-independence is specifically about an *agent* handler): an agent
/// handler's own `binder` is `None` unconditionally and by construction —
/// `bynk.actor.by_on_agent` (`context_checks.rs:2986-2996`) rejects any
/// `by` clause on an agent handler outright, so [`lower::lower_handler_ir`]
/// never has one to lower in the first place.
#[derive(Debug, Clone)]
pub(crate) struct IrHandler {
    pub kind: HandlerKind,
    pub params: Vec<(String, TyId)>,
    pub given: Vec<String>,
    /// The actor name(s) a `by` clause names — see this struct's own doc
    /// comment for why `binder` alone cannot represent the gate. Empty iff
    /// `h.by_clause.is_none()`.
    pub actors: Vec<String>,
    pub binder: Option<ActorBinder>,
    /// P6.13 ([DECISION G], #1179): `Some` iff this handler is a `from
    /// websocket` service's `on open`/`on message`/`on close` — see
    /// [`ConnectionBinder`]'s own doc comment. `None` unconditionally for
    /// every other handler kind/protocol, including an agent handler
    /// (`binder`'s own sibling gate: [`lower::lower_handler_ir`] never
    /// sets this either).
    pub connection: Option<ConnectionBinder>,
    pub body: IrExpr,
    pub commit: CommitShape,
    /// The handler's own declared return type, resolved (#1187's slice 5,
    /// the `Service` emitter cutover) — mirrors [`IrItem::Fn::ret`]'s
    /// identical field, added here
    /// for the identical reason: [`lower::lower_handler_signature_ir`]
    /// already resolved this value to compute `effectful` below and
    /// discarded it, leaving a service emitter with no IR-native way to
    /// render a handler's own return-type annotation without re-walking
    /// `Handler::return_type` (`bynk_syntax::ast::TypeRef`) itself.
    pub ret: TyId,
    pub effectful: bool,
    pub method_name: Option<String>,
}
