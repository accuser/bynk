//! [`TsProgram`]/[`TsStmt`] — the tree. P7.5 built it wide enough only for
//! the `Verbatim` escape hatch (Q2, `design/tracks/the-typescript-tree.md`
//! §3.2). P7.8 (#1313) adds the real node algebra ([`TsExpr`]/[`TsType`]/
//! [`TsDecl`], plus real [`TsStmt`] variants) — not the full §7.1 reference
//! sketch as literally written (a variant-name list with almost no
//! field-level design), but the subset `bynk-emit/src/emitter/
//! events_fanout.rs` (Arc C's real next file — P7.8's own proposal
//! corrected the track doc's stale schedule, see `design/tracks/
//! the-typescript-tree.md` §6/§9) concretely needs, grounded against that
//! file's own real shape rather than guessed. Building the rest of the
//! sketch's unvalidated variants now would repeat the exact "guessing, not
//! designing" risk `bynk-ts`'s own module doc (`lib.rs`) already named for
//! this layer — Arc C's later slices add more variants file by file, the
//! same precedent [`VerbatimOrigin`] already set.

use bynk_syntax::span::Span;

/// A whole generated TypeScript module, as an ordered sequence of top-level
/// statements. `Vec<TsStmt>`, plain — no richer container yet (P7.6's own
/// `Artefacts { docs: BTreeMap<PathBuf, Document> }` is where a *project's*
/// documents get keyed; this is one document's own tree).
#[derive(Debug, Default)]
pub struct TsProgram {
    pub stmts: Vec<TsStmt>,
}

impl TsProgram {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, stmt: TsStmt) {
        self.stmts.push(stmt);
    }
}

/// One statement — a `Verbatim`-tagged escape hatch (still constructible
/// only via [`TsStmt::verbatim`], per #1307's Decision D — the
/// `verbatim_sites` probe needs exactly one string to line-scan for), or,
/// from this slice, a real structured kind. The real kinds have no such
/// sealing: they're normal typed constructors, not a "wrap opaque text"
/// escape hatch, so the `verbatim_sites` concern that motivates `verbatim`'s
/// own single-constructor discipline doesn't apply to them.
#[derive(Debug)]
pub struct TsStmt {
    pub(crate) kind: TsStmtKind,
    /// Where this statement's content originated in the `.bynk` source, if
    /// known. Only a *top-level* statement's own span is currently recorded
    /// as a source-map checkpoint ([`crate::printer::print`], unchanged
    /// from P7.5/R7.4's own scope) — a nested statement (inside a `Block`,
    /// `If`, `ForOf`, `TryCatch`) still carries this field structurally, for
    /// whichever future slice gives sub-statement source maps real value,
    /// but the printer does not yet record a checkpoint from it. Named here
    /// explicitly (P7.8's own accepted proposal: "an implementation-time
    /// call within this same shape") rather than left ambiguous.
    pub span: Option<Span>,
}

#[derive(Debug)]
pub(crate) enum TsStmtKind {
    Verbatim {
        #[allow(dead_code)]
        // read by the lint's own violation attribution once Arc C gives it real content to report on; not yet, per Decision F
        origin: VerbatimOrigin,
        text: String,
    },
    /// A top-level declaration ([`TsDecl`]) printed as a statement — the
    /// bridge between `TsProgram`'s flat `Vec<TsStmt>` and the reference
    /// sketch's separate `TsDecl` enum (an `import`/`interface`/top-level
    /// `const`/`class` *is* one kind of top-level statement in this tree,
    /// not a different container).
    Decl(TsDecl),
    /// A local `const` binding, e.g. `const { events } = ...;` or
    /// `const subs = ...;` — distinct from [`TsDecl::ConstDecl`], which is
    /// the top-level form. Carries a real destructuring [`TsBindingName`]
    /// because `events_fanout.rs`'s own `const { events } = ...` needs one
    /// (a gap beyond the accepted proposal's own variant list — a bare
    /// `String` name cannot represent it; named explicitly as a deviation,
    /// not invented silently).
    Const {
        name: TsBindingName,
        ty: Option<TsType>,
        init: TsExpr,
    },
    /// `let`'s sibling to `Const` — unused by `events_fanout.rs` itself, but
    /// the `const`/`let` distinction is real TypeScript semantics the
    /// printer must preserve once one of the pair exists (the accepted
    /// proposal's own reasoning for keeping it).
    Let {
        name: TsBindingName,
        ty: Option<TsType>,
        init: Option<TsExpr>,
    },
    /// An expression used as a whole statement (a bare call, e.g.).
    ExprStmt(TsExpr),
    Return(Option<TsExpr>),
    /// `if (cond) <then_branch>` — no `else`: `events_fanout.rs` never uses
    /// one, and the accepted proposal's own variant list doesn't ask for
    /// it; adding one speculatively would repeat the guessing risk Decision
    /// B exists to avoid. `then_branch` may be a [`TsStmtKind::Block`]
    /// (printed with braces) or any other single statement (printed inline
    /// on the same line, matching `if (!Array.isArray(subs)) continue;`'s
    /// own real, brace-free shape).
    If {
        cond: TsExpr,
        then_branch: Box<TsStmt>,
    },
    /// `for (const <binding> of <iter>) <body>`.
    ForOf {
        binding: String,
        iter: TsExpr,
        body: Box<TsStmt>,
    },
    /// `try <try_block> catch (<catch_param>) <catch_block>` — a real gap
    /// beyond the reference sketch (`design/bynk-greenfield-compiler.md`'s
    /// §7.1 has no `TryCatch` at all), found and named by P7.8's own
    /// accepted proposal: `events_fanout.rs`'s subscriber-failure-isolation
    /// `try`/`catch` (ADR 0284) is load-bearing control flow, not
    /// decorative.
    TryCatch {
        try_block: Box<TsStmt>,
        catch_param: String,
        catch_block: Box<TsStmt>,
    },
    /// `{ <stmts> }` — the body container for `If`/`ForOf`/`TryCatch`/
    /// constructor and method bodies.
    Block(Vec<TsStmt>),
    /// A bare `continue;` — a second real gap beyond the accepted
    /// proposal's own variant list: `events_fanout.rs` uses it twice
    /// (`if (!Array.isArray(subs)) continue;` / `if (!binding) continue;`),
    /// load-bearing loop control the accepted proposal's `TsStmt` list
    /// doesn't name. No label — nothing in the grounding file needs one.
    Continue,
    /// `target = value;` — a third real gap, found in review of the
    /// implementing PR (#1313): `events_fanout.rs`'s own constructor body is
    /// exactly one statement, `this.env = (env ?? {}) as
    /// Record<string, ServiceBinding>;` (a field assignment, not a `const`/
    /// `let` binding), which the accepted proposal's own grounding
    /// catalogue missed — it catalogued the `fetch` method's body in detail
    /// but not the constructor's. `target` is deliberately a full `TsExpr`
    /// (not a narrower "assignable" type) so it can hold `this.env` (a
    /// `Member` expression) without a second binding-target type; nothing
    /// about `Assign` validates that `target` is actually assignable
    /// (`bynk-check` already does that on the `.bynk` side before emission
    /// ever runs).
    Assign {
        target: TsExpr,
        value: TsExpr,
    },
    /// `// <text>` — a bare line comment. Added for Arc C's own first real
    /// conversion slice (#1317): every `bynk-emit`-generated `.ts` file
    /// opens with the same two-line header banner
    /// (`// Generated by bynkc — do not edit by hand.` / a file-specific
    /// second line), and nothing before this slice could represent one.
    /// Not speculative — a universal need every later Arc C slice hits on
    /// its own first line, closed once here rather than left as a residual
    /// `Verbatim` wrap that would make `verbatim_sites` show zero
    /// improvement for real, substantial conversion work.
    ///
    /// Carries no semantic content the printer or checker ever reads back —
    /// pure, inert text. Printed one `// `-prefixed line per `\n` in `text`
    /// (`bynk-ts/src/printer.rs`'s own `render_stmt`), so a multi-line
    /// comment is representable as one statement, though
    /// `events_fanout.rs`'s own two-line header is built as two separate
    /// `Comment` statements instead — matching how its two adjacent
    /// `import` lines are two separate `TsDecl::Import` statements, not one
    /// with an embedded line break, and exercising the printer's own new
    /// "no blank line between adjacent `Comment`s" rule (the same
    /// exception already established for adjacent `import`s).
    Comment(String),
}

impl TsStmt {
    /// The one constructor for a `Verbatim`-kinded statement.
    pub fn verbatim(origin: VerbatimOrigin, text: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::Verbatim {
                origin,
                text: text.into(),
            },
            span,
        }
    }

    pub fn decl(decl: TsDecl, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::Decl(decl),
            span,
        }
    }

    pub fn const_stmt(
        name: TsBindingName,
        ty: Option<TsType>,
        init: TsExpr,
        span: Option<Span>,
    ) -> Self {
        Self {
            kind: TsStmtKind::Const { name, ty, init },
            span,
        }
    }

    pub fn let_stmt(
        name: TsBindingName,
        ty: Option<TsType>,
        init: Option<TsExpr>,
        span: Option<Span>,
    ) -> Self {
        Self {
            kind: TsStmtKind::Let { name, ty, init },
            span,
        }
    }

    pub fn expr_stmt(expr: TsExpr, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::ExprStmt(expr),
            span,
        }
    }

    pub fn return_stmt(expr: Option<TsExpr>, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::Return(expr),
            span,
        }
    }

    pub fn if_stmt(cond: TsExpr, then_branch: TsStmt, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::If {
                cond,
                then_branch: Box::new(then_branch),
            },
            span,
        }
    }

    pub fn for_of(
        binding: impl Into<String>,
        iter: TsExpr,
        body: TsStmt,
        span: Option<Span>,
    ) -> Self {
        Self {
            kind: TsStmtKind::ForOf {
                binding: binding.into(),
                iter,
                body: Box::new(body),
            },
            span,
        }
    }

    pub fn try_catch(
        try_block: TsStmt,
        catch_param: impl Into<String>,
        catch_block: TsStmt,
        span: Option<Span>,
    ) -> Self {
        Self {
            kind: TsStmtKind::TryCatch {
                try_block: Box::new(try_block),
                catch_param: catch_param.into(),
                catch_block: Box::new(catch_block),
            },
            span,
        }
    }

    pub fn block(stmts: Vec<TsStmt>, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::Block(stmts),
            span,
        }
    }

    pub fn continue_stmt(span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::Continue,
            span,
        }
    }

    pub fn assign(target: TsExpr, value: TsExpr, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::Assign { target, value },
            span,
        }
    }

    pub fn comment(text: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::Comment(text.into()),
            span,
        }
    }
}

/// A binding's own name, in either of the two shapes `events_fanout.rs`
/// itself uses: a plain identifier (`const subs = ...`), or an
/// object-destructuring pattern (`const { events } = ...`) — naming only
/// the destructured properties themselves (`{ a, b }`), not the renamed
/// (`{ a: renamed }`) or nested (`{ a: { b } }`) forms, since nothing in
/// the grounding file needs either.
#[derive(Debug)]
pub enum TsBindingName {
    Ident(String),
    ObjectPattern(Vec<String>),
}

/// An expression. Only the shapes `events_fanout.rs` concretely uses
/// (Decision B) — not the reference sketch's full `TsExpr` list (`Arrow`,
/// `Cond`, `TemplateLit`, `Spread` are all unused in the grounding file and
/// deliberately not built here).
#[derive(Debug)]
pub enum TsExpr {
    Ident(String),
    /// `object.property`.
    Member {
        object: Box<TsExpr>,
        property: String,
    },
    /// `object[index]`.
    Index {
        object: Box<TsExpr>,
        index: Box<TsExpr>,
    },
    Call {
        callee: Box<TsExpr>,
        args: Vec<TsExpr>,
    },
    New {
        callee: Box<TsExpr>,
        args: Vec<TsExpr>,
    },
    /// A value object literal, e.g. `{ status: 204 }` — comma-separated.
    /// `multiline: false` (the ordinary case, via [`TsExpr::object`])
    /// always prints on one line, matching [`TsType::Object`]'s own
    /// (semicolon-separated) single-line convention for the *type*-position
    /// shape. `multiline: true` (via [`TsExpr::multiline_object`]) is a
    /// real, grounded gap found implementing Arc C's first slice (#1317):
    /// `events_fanout.rs`'s own `__eventRoutes` table is a top-level
    /// `const` initializer with one entry per line, each with its own
    /// trailing comma, closing brace at the *statement's* own indent —
    /// TypeScript's ordinary multi-line object-literal convention, which
    /// nothing in this crate could represent before this addition. Only
    /// statement/declaration-level renderers (which already carry `depth`)
    /// can render this correctly — see `printer.rs`'s own
    /// `render_stmt_level_expr`; a `multiline: true` object nested inside
    /// another expression (an array element, a call argument, …) renders
    /// via the ordinary depth-unaware `render_expr` recursion instead,
    /// which cannot honour `multiline` — not reachable from any real
    /// `bynk-emit` call site today, but worth knowing before nesting one.
    Object {
        entries: Vec<(String, TsExpr)>,
        multiline: bool,
    },
    /// An array literal, e.g. `[{ binding: "x", service: "y" }]`.
    Array(Vec<TsExpr>),
    Await(Box<TsExpr>),
    /// `expr as ty`.
    As {
        expr: Box<TsExpr>,
        ty: TsType,
    },
    Unary {
        op: TsUnaryOp,
        expr: Box<TsExpr>,
    },
    Binary {
        op: TsBinaryOp,
        left: Box<TsExpr>,
        right: Box<TsExpr>,
    },
    Lit(TsLit),
}

impl TsExpr {
    /// The ordinary, single-line object literal.
    pub fn object(entries: Vec<(String, TsExpr)>) -> Self {
        TsExpr::Object {
            entries,
            multiline: false,
        }
    }

    /// One entry per line, each with its own trailing comma — see
    /// [`TsExpr::Object`]'s own doc for the real shape and the
    /// depth-awareness this needs at the print site.
    pub fn multiline_object(entries: Vec<(String, TsExpr)>) -> Self {
        TsExpr::Object {
            entries,
            multiline: true,
        }
    }
}

/// Only `!x` — the one unary operator `events_fanout.rs` uses
/// (`!Array.isArray(subs)`, `!binding`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsUnaryOp {
    Not,
}

/// Only `??` — the one binary operator `events_fanout.rs` uses
/// (`env ?? {}`). Not the full JS/TS operator table (Decision B).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsBinaryOp {
    NullishCoalescing,
}

/// A literal — only the three kinds `events_fanout.rs` uses: a string
/// (`"EventsFanout delivery failed"`), a number (`204`), and `null`.
#[derive(Debug)]
pub enum TsLit {
    Str(String),
    /// Rendered verbatim, as text — TypeScript's own numeric-literal
    /// grammar is not this crate's problem to re-derive; the caller passes
    /// the exact digits it wants printed.
    Num(String),
    Null,
}

/// A type-position node. `Named` (extended with type arguments — a real gap
/// the reference sketch left unaddressed: `Record<string,
/// Array<{...}>>`/`Promise<Response>` both need one, and a bare `Named`
/// with no type-argument slot cannot represent either), `Array` (extended
/// with a `readonly` modifier — P7.9's own real gap: every `List`/`Query`
/// element type `bynk-emit`'s `ts_type_ref*`/`ts_ty` families build is
/// `readonly T[]`, not plain `T[]`), `Object`, and `Fn` (P7.9's own second
/// real gap — the query-thunk wrapper `(() => readonly T[])` and a real
/// parametered function type `(a0: T0, …) => Ret` both need one) — not the
/// sketch's `Union`/`Intersection`/`Literal`/`TypeParam`/`Readonly` (still
/// unused; `readonly` here is a modifier on `Array`, not the sketch's own
/// separate `Readonly` wrapper variant).
#[derive(Debug)]
pub enum TsType {
    /// A named type, optionally generic — `string`/`unknown` (no type
    /// arguments) or `Record<string, T>`/`Promise<Response>` (some).
    Named {
        name: String,
        type_args: Vec<TsType>,
    },
    /// `T[]` (`readonly: false`) or `readonly T[]` (`readonly: true`) —
    /// TypeScript's own postfix array-type syntax (not the equivalent
    /// `Array<T>`/`ReadonlyArray<T>` generic spelling either family uses).
    ///
    /// **Hazard (review of #1315/#1316, not yet closed):** the printer does
    /// not parenthesise `element` when it is itself a [`TsType::Union`] or
    /// [`TsType::Fn`] — `Array { element: Union(..), .. }` prints as
    /// `A | B[]`, which TypeScript reads as `A | (B[])`, not the intended
    /// `(A | B)[]`; likewise a `Fn` element's own `[]` binds to its return
    /// type, not the whole function type. Not a P7.9 regression — every
    /// `bynk-emit` call site building this exact malformed shape
    /// (`Ty::List` of an `Ty::ActorSum`) produced the identical bytes
    /// before this slice too, so the zero-diff bar is genuinely met. Fixing
    /// it is a real output change, out of this slice's own behaviour-
    /// preserving scope — a caller building `Array` over `Union`/`Fn` today
    /// must not trust the printer to parenthesise correctly.
    Array {
        element: Box<TsType>,
        readonly: bool,
    },
    /// A type-position object shape, e.g. `{ type: string; payload: unknown }`
    /// — semicolon-separated, always printed on one line (see
    /// [`TsExpr::Object`]'s own doc for the value-position contrast). An
    /// `interface`'s own *members* are each printed on their own line by
    /// [`TsDecl::Interface`]'s printer, independent of this variant's own
    /// (always-inline) rendering — the two only look different because
    /// `TsDecl::Interface` is the thing choosing to put each member on its
    /// own line, not because `Object` has two rendering modes.
    Object(Vec<(String, TsType)>),
    /// `(a0: T0, a1: T1, …) => Ret` — a function type. Parameters carry no
    /// name of their own (just their type); the printer numbers them
    /// positionally (`a0`, `a1`, …), matching the exact convention
    /// `bynk-emit`'s own pre-P7.9 `ts_type_ref_with`/`ts_ty` already used
    /// (TypeScript requires *some* name in function-type syntax, and
    /// nothing about a `TypeRef::Fn`/`Ty::Fn` parameter carries a real one
    /// to use instead). A zero-`params` `Fn` is the query-thunk wrapper
    /// shape, `() => Ret`. See [`TsType::Array`]'s own doc for a real,
    /// unclosed parenthesisation hazard when a `Fn` sits inside one.
    Fn {
        params: Vec<TsType>,
        ret: Box<TsType>,
    },
    /// `A | B | C` — a type-position union. Added in review of #1315:
    /// `bynk-emit`'s `ts_ty` builds a real union type for a resolved
    /// multi-actor sum (`Ty::ActorSum`, discriminated-union members tagged
    /// by actor name), a shape none of `Named`/`Array`/`Object`/`Fn` can
    /// represent — a real, grounded gap the same way `readonly`/`Fn`
    /// themselves were (P7.9's own accepted proposal), not a speculative
    /// addition. Each member prints through the ordinary `render_type`
    /// recursion; a member that is itself a `Union` is legal to construct
    /// but nothing in `bynk-emit` builds one today. See [`TsType::Array`]'s
    /// own doc for a real, unclosed parenthesisation hazard when a `Union`
    /// sits inside one.
    Union(Vec<TsType>),
}

impl TsType {
    /// A plain named type with no type arguments — `string`, `unknown`,
    /// `Request`, …
    pub fn named(name: impl Into<String>) -> Self {
        TsType::Named {
            name: name.into(),
            type_args: Vec::new(),
        }
    }

    /// A generic named type — `Record<K, V>`, `Promise<T>`, …
    pub fn named_with_args(name: impl Into<String>, type_args: Vec<TsType>) -> Self {
        TsType::Named {
            name: name.into(),
            type_args,
        }
    }

    /// `T[]` — the non-`readonly` array shape.
    pub fn array(element: TsType) -> Self {
        TsType::Array {
            element: Box::new(element),
            readonly: false,
        }
    }

    /// `readonly T[]`.
    pub fn readonly_array(element: TsType) -> Self {
        TsType::Array {
            element: Box::new(element),
            readonly: true,
        }
    }
}

/// One function/method/constructor parameter.
#[derive(Debug)]
pub struct TsParam {
    pub name: String,
    pub ty: Option<TsType>,
    /// `name?: ty` — `events_fanout.rs`'s own `env?: unknown` constructor
    /// parameter needs this; nothing in the grounding file needs a default
    /// value, so only optionality is represented, not defaults.
    pub optional: bool,
}

/// One `class` field.
#[derive(Debug)]
pub struct TsClassField {
    pub name: String,
    pub ty: TsType,
    /// `private` — the one visibility modifier `events_fanout.rs` uses
    /// (`private env: ...`). Not a decorator or a constructor parameter
    /// property (R7.1 forbids both categorically — there is no variant
    /// shape here that could construct either).
    pub private: bool,
}

/// A class's own constructor.
#[derive(Debug)]
pub struct TsClassCtor {
    pub params: Vec<TsParam>,
    pub body: Vec<TsStmt>,
}

/// One class method.
#[derive(Debug)]
pub struct TsClassMethod {
    pub name: String,
    pub is_async: bool,
    pub params: Vec<TsParam>,
    pub return_type: Option<TsType>,
    pub body: Vec<TsStmt>,
}

/// A top-level declaration. Only `Import`, `Export`, `Interface`,
/// `ConstDecl`, and `Class` — not the sketch's `Function`/`TypeAlias`
/// (unused in the grounding file).
#[derive(Debug)]
pub enum TsDecl {
    /// `import { a, b } from "spec";` (`type_only: false`) or
    /// `import type { a, b } from "spec";` (`type_only: true`). Only the
    /// named-imports form — `events_fanout.rs` never uses a default
    /// import, so none is represented.
    Import {
        type_only: bool,
        names: Vec<String>,
        from: String,
    },
    /// Marks the wrapped declaration `export`ed — `export class Foo { .. }`
    /// is `Export(Box::new(Class { .. }))`. A wrapper, not a per-variant
    /// `exported: bool` field, matching the reference sketch's own naming
    /// (`TsDecl::Export` is a peer variant, not a modifier on each other
    /// one).
    Export(Box<TsDecl>),
    Interface {
        name: String,
        members: Vec<(String, TsType)>,
    },
    /// A top-level `const` — distinct from `TsStmtKind::Const` (private —
    /// reachable through [`TsStmt::const_stmt`]), the local
    /// form.
    ConstDecl {
        name: String,
        ty: Option<TsType>,
        init: TsExpr,
    },
    Class {
        name: String,
        fields: Vec<TsClassField>,
        constructor: Option<TsClassCtor>,
        methods: Vec<TsClassMethod>,
    },
}

/// Which family of residual, not-yet-converted emission a [`TsStmt::verbatim`]
/// statement came from. A closed enum, deliberately — "makes the ratchet a
/// compile-time construct, not a grep" (Q2's own settling text). Named
/// file-by-file as Arc C actually needs them (`ast_importers`'s own five-file
/// floor is the precedent for how this track names residue), not
/// pre-populated for the whole ~19-slice Arc C schedule up front.
///
/// Deliberately **not** `#[non_exhaustive]`: a `match` over every variant —
/// in this crate, or in `bynk-emit` once Arc C reads it — must fail to
/// compile the moment a new variant is added, forcing every consumer to
/// account for it explicitly. A non-exhaustive enum would let a wildcard arm
/// silently absorb a new residue family instead, exactly the "grep, not a
/// compile-time construct" Q2's own settling text rejected. P7.8's own
/// grounding work found that `Contracts`/`Secrets`/`RuntimeUse` were seeded
/// against files that turn out not to need `bynk-ts` conversion at all
/// (`design/tracks/the-typescript-tree.md` §9) — recorded there, not fixed
/// by removing the variants here, since that's separate follow-on work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbatimOrigin {
    /// `bynk-emit/src/emitter/contracts.rs`.
    Contracts,
    /// `bynk-emit/src/emitter/secrets.rs`.
    Secrets,
    /// `bynk-emit/src/emitter/runtime_use.rs`.
    RuntimeUse,
    /// P7.6's own transitional wrap (#1309): the whole of a still-`String`-
    /// producing `bynk-emit` document (an entry point, `compose.ts`, the
    /// runtime module, an adapter binding, a test module, …), carried into
    /// `Document::Ts` so `Artefacts` never stores a bare `String` for TS
    /// output (R7.8) even before Arc C converts the function that built it.
    /// Deliberately **not** file-specific like the three variants above —
    /// this is Arc B's own infrastructure slice, not an Arc C conversion, so
    /// it covers everything Arc C hasn't reached yet, one call site per
    /// document `bynk-emit/src/project.rs`/`project/tests_emit.rs`
    /// constructs (not funnelled through a shared helper: a shared wrap
    /// point would collapse every one of those call sites to a single
    /// textual `TsStmt::verbatim(` occurrence, defeating `verbatim_sites`'
    /// own purpose of counting how much is genuinely still unconverted).
    /// Retires site by site as Arc C converts each underlying emitter
    /// function to return a real `TsProgram` directly — at which point that
    /// document's own construction site stops calling `TsStmt::verbatim` at
    /// all, not by this variant being deleted first.
    NotYetConverted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_carries_its_own_span() {
        let span = Span::new(3, 8);
        let stmt = TsStmt::verbatim(VerbatimOrigin::RuntimeUse, "x", Some(span));
        assert_eq!(stmt.span, Some(span));
    }

    #[test]
    fn a_program_prints_its_statements_in_push_order() {
        // Kept as a construction-order check (not a print check — printer.rs
        // owns that) since `TsStmtKind` is `pub(crate)` and no longer
        // exposes a uniform `text()` accessor once non-`Verbatim` kinds
        // exist; span order is what's left to check at this layer.
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(VerbatimOrigin::Contracts, "a", None));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Secrets,
            "b",
            Some(Span::new(0, 1)),
        ));
        assert_eq!(program.stmts[0].span, None);
        assert_eq!(program.stmts[1].span, Some(Span::new(0, 1)));
    }
}
