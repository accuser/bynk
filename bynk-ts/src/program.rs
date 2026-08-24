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
#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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
    /// `throw <expr>;` — #1353's own real gap: `emit_contract_guarded_body`'s
    /// own precondition/postcondition guards (`bynk-emit/src/emitter/
    /// emit.rs`) each throw a constructed `Error` on violation — no prior
    /// slice needed a bare `throw` (every other error-signalling site in
    /// this tree so far has been a `Return`). Mirrors `Return`'s own shape
    /// exactly, minus the `Option` (a `throw` always carries a value; there
    /// is no bare `throw;` in JS/TS).
    Throw(TsExpr),
    /// `if (cond) <then_branch>` (optionally `else <else_branch>`).
    /// `then_branch`/`else_branch` may each be a [`TsStmtKind::Block`]
    /// (printed with braces) or any other single statement (printed inline
    /// on the same line, matching `if (!Array.isArray(subs)) continue;`'s
    /// own real, brace-free shape). `else_branch` is #1323's own real gap:
    /// `workers_entry.rs`'s queue-consumer ack/retry dispatch (`if
    /// (result.tag === "Ack") msg.ack(); else { ...; msg.retry(); }`) —
    /// `events_fanout.rs`'s own grounding never used one, so P7.8 named
    /// omitting it a deliberate choice, not an oversight; this slice's own
    /// real content needs it for real.
    ///
    /// `same_line_else` is #1325's own real gap: `workers_entry.rs`'s own
    /// real `else` sits on its own fresh line (`}\nelse {`), but
    /// `emit_test_main`'s own real content wants the conventional `} else
    /// {` — two already-real files disagreeing on the same construct, the
    /// same class of tension the `Await`-under-`As` correction (#1323/
    /// #1324) found for parenthesisation. `false` (fresh line) is the
    /// existing, already-tested default via [`TsStmt::if_else_stmt`]; `true`
    /// (same line) is reached only through
    /// [`TsStmt::if_else_same_line_stmt`].
    If {
        cond: TsExpr,
        then_branch: Box<TsStmt>,
        else_branch: Option<Box<TsStmt>>,
        same_line_else: bool,
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
    /// decorative. `catch_param: None` prints the bare `catch { ... }`
    /// form (ES2019's optional catch binding) — #1321's own real gap:
    /// `workers.rs`'s `emit_http_sum_wrapper` reads the raw request body in
    /// a `try`/`catch` that never uses the caught error (`} catch {`, no
    /// `(e)` at all).
    TryCatch {
        try_block: Box<TsStmt>,
        catch_param: Option<String>,
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
    /// `/** ... */` — a JSDoc block comment, distinct from
    /// [`TsStmtKind::Comment`]'s own `//`-per-line form. #1333's own real
    /// need: `emit_doc_block` (`bynk-emit/src/emitter/emit.rs`) renders a
    /// Bynk `///` doc comment as a real JSDoc block — printed via
    /// [`crate::printer::print_stmt`], not [`crate::printer::print`], since
    /// every real call site today is a shared helper spliced into a
    /// still-unconverted caller's own buffer, the same P7.9 pattern
    /// `ts_type_ref`/`ts_ty` already used (keep the caller's own signature,
    /// build a real node internally, print just that fragment). Escaping
    /// (a literal `*/` inside the text becomes `*\/`, so it can't
    /// prematurely close the comment and let trailing text land as
    /// executable top-level TypeScript — issue #720) and the blank-line
    /// convention (a blank source line prints as a bare ` *`, no trailing
    /// space) live in the printer (`render_stmt`'s own `DocComment` arm,
    /// `crate::printer`'s own private renderer), matching
    /// where every other statement kind's rendering rules live.
    DocComment(String),
    /// A bare blank line — no statement content. #1323's own real, narrow
    /// need: `workers_entry.rs`'s `fetch` method body has an unconditional
    /// blank line after its internal Service-Binding dispatch block, and a
    /// conditional one after the WebSocket-upgrade dispatch (when present)
    /// and the Events dispatch (when present) — three specific points the
    /// pre-conversion `writeln!(out).unwrap()` wrote directly, nested
    /// *inside* the `try` block's own statement list. Distinct from
    /// [`crate::printer::print`]'s own top-level blank-line policy (this
    /// module's own printer doc), which only separates `TsProgram`'s own
    /// top-level statements — nothing before this slice needed a blank line
    /// anywhere inside a nested block, so that policy was never generalized
    /// to every nesting depth. A single, narrow statement kind represents
    /// exactly this, rather than widening the top-level-only policy to a
    /// depth it has no other real content to justify.
    Blank,
    /// `switch (<discriminant>) { <cases> }` — #1323's own largest gap: the
    /// first genuinely new statement-*grouping* construct in this tree
    /// (every prior addition was a single expression/type variant or a
    /// straightforward one-block wrapper). `workers_entry.rs` has four real
    /// `switch` statements (the internal `/_bynk/call/` and `/_bynk/event/`
    /// dispatches, the `scheduled` handler's `event.cron` dispatch, the
    /// `queue` handler's `batch.queue` dispatch); every real `case` here is
    /// a `{ ... }`-blocked body ending in a terminal statement (`return`/
    /// `continue`), so no fallthrough/shared-body grouping is represented —
    /// extend narrowly, the same posture every other addition here takes.
    Switch {
        discriminant: TsExpr,
        cases: Vec<TsSwitchCase>,
    },
    /// `{ stmt; stmt; ... }` — a braced, multi-statement block printed on
    /// ONE generated line, distinct from [`TsStmtKind::Block`] (always
    /// multi-line). #1323's own real gap: `workers_entry.rs`'s queue
    /// consumer has two real sites (`if (__r.tag === "Err") { console.
    /// error(...); msg.retry(); continue; }`; the `else` branch of the
    /// ack/retry dispatch) where the pre-conversion `writeln!` code
    /// deliberately packed several short statements onto one physical
    /// line — a real, distinct formatting choice from every other
    /// multi-statement body in this tree, which always prints one
    /// statement per line. Only reachable through [`TsStmt::if_stmt`]/
    /// [`TsStmt::if_else_stmt`]'s own branches in real content today.
    InlineBlock(Vec<TsStmt>),
    /// `<expr>++;` — a postfix increment used as a whole statement. #1325's
    /// own real, narrow gap: `emit_test_main`'s own `passed++;`/`failed++;`
    /// counters. No prefix form, no decrement, no expression-position use
    /// (every real site increments a bare counter as its own statement) —
    /// [`TsExpr::Unary`] stays prefix-only (`!`/`typeof`), since a postfix
    /// operator used as a *statement* is a different grammatical position
    /// from a prefix operator used as an *operand*, not a shape `Unary`
    /// could represent by adding a variant.
    Increment(TsExpr),
    /// A pre-rendered, unconditional-passthrough statement blob — printed
    /// exactly as given, with no leading indent, no added semicolon, no
    /// added braces (matching [`TsStmtKind::Verbatim`]'s own `out.push_str(
    /// text)` rendering, not the ordinary per-kind `indent(depth)`-prefixed
    /// shape every other variant gets). #1337's own real need:
    /// `emit_method`'s own body is delegated wholesale to
    /// `emit_block_as_function_body_with_return` (`bynk-emit`'s
    /// `emitter/lower.rs:201`) — the one splice boundary ADR
    /// `arc-c-lower-rs-permanent-exclusion` names as a *permanent*,
    /// deliberate exclusion from this tree (`lower.rs` is the compiler's
    /// own second code-generation pass, comprehensive language-surface
    /// work Arc C was never scoped to cover), not a not-yet-converted
    /// residue [`TsStmtKind::Verbatim`] would misrepresent it as.
    ///
    /// Deliberately a SEPARATE variant from `Verbatim`, not a reuse of it,
    /// even though both render identically (`out.push_str(text)`): the
    /// `verbatim_sites`/`verbatim_origins` probes exist specifically to
    /// track Arc C's own *temporary* conversion residue trending toward
    /// zero — a `Raw` site is never counted there (it has no
    /// `VerbatimOrigin` at all), because using `Verbatim` here would make
    /// this permanent exclusion look like unfinished Arc C work a future
    /// slice is expected to close, which it structurally cannot.
    ///
    /// #1339's own second real use, broadening (not narrowing) the
    /// above: `emit_refined_type`'s own `of()` guard block splices
    /// `emit_refined_checks`'s own output this same way — that function
    /// keeps its exact `out: &mut String` signature (the P7.9/step-1
    /// pattern applied one level down, not a `lower.rs`-style permanent
    /// exclusion), but its real content is ALREADY built and printed from
    /// real `bynk_ts::TsStmt`/`print_stmt` calls internally — genuinely
    /// statement-shaped pre-rendered text, the same mechanical fit `Raw`
    /// already provides, just for a different underlying reason than
    /// `lower.rs`'s own permanent exclusion. Both real uses share the one
    /// property that actually matters for this variant's own existence:
    /// *real, already-correctly-indented statement text this call site
    /// cannot restructure into a `Vec<TsStmt>` without changing scope it
    /// isn't the one converting* — not whether the reason is permanent
    /// or temporary.
    Raw(String),
}

/// One `case`/`default` arm of a `TsStmtKind::Switch`. `test: None` is the
/// `default:` case — every real `default` in `workers_entry.rs` prints its
/// body directly under `default:` with no `{ }` block, while every real
/// non-`default` `case` prints a `{ }`-blocked body; the printer's own
/// renderer follows that exact split (see its own doc), not a per-case flag,
/// since nothing in the grounding file needs a braceless non-`default` case
/// or a braced `default`.
#[derive(Debug, Clone)]
pub struct TsSwitchCase {
    pub test: Option<TsExpr>,
    pub body: Vec<TsStmt>,
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

    pub fn throw_stmt(expr: TsExpr, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::Throw(expr),
            span,
        }
    }

    pub fn if_stmt(cond: TsExpr, then_branch: TsStmt, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::If {
                cond,
                then_branch: Box::new(then_branch),
                else_branch: None,
                same_line_else: false,
            },
            span,
        }
    }

    pub fn if_else_stmt(
        cond: TsExpr,
        then_branch: TsStmt,
        else_branch: TsStmt,
        span: Option<Span>,
    ) -> Self {
        Self {
            kind: TsStmtKind::If {
                cond,
                then_branch: Box::new(then_branch),
                else_branch: Some(Box::new(else_branch)),
                same_line_else: false,
            },
            span,
        }
    }

    /// [`TsStmt::if_else_stmt`]'s own sibling with `} else {` on one line —
    /// #1325's own real gap, `emit_test_main`'s own real `else` spacing. See
    /// `TsStmtKind::If`'s own doc for why this needs to be a distinct
    /// constructor rather than a change to the existing default.
    pub fn if_else_same_line_stmt(
        cond: TsExpr,
        then_branch: TsStmt,
        else_branch: TsStmt,
        span: Option<Span>,
    ) -> Self {
        Self {
            kind: TsStmtKind::If {
                cond,
                then_branch: Box::new(then_branch),
                else_branch: Some(Box::new(else_branch)),
                same_line_else: true,
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
        catch_param: Option<impl Into<String>>,
        catch_block: TsStmt,
        span: Option<Span>,
    ) -> Self {
        Self {
            kind: TsStmtKind::TryCatch {
                try_block: Box::new(try_block),
                catch_param: catch_param.map(Into::into),
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

    pub fn doc_comment(text: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::DocComment(text.into()),
            span,
        }
    }

    pub fn blank(span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::Blank,
            span,
        }
    }

    pub fn switch_stmt(discriminant: TsExpr, cases: Vec<TsSwitchCase>, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::Switch {
                discriminant,
                cases,
            },
            span,
        }
    }

    pub fn inline_block(stmts: Vec<TsStmt>, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::InlineBlock(stmts),
            span,
        }
    }

    pub fn increment(expr: TsExpr, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::Increment(expr),
            span,
        }
    }

    /// The one constructor for a `Raw`-kinded statement — `text` is printed
    /// verbatim, exactly as given (see `TsStmtKind::Raw`'s own doc for
    /// why this is a distinct kind from `Verbatim`, not a reuse of it).
    pub fn raw(text: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::Raw(text.into()),
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
#[derive(Debug, Clone)]
pub enum TsBindingName {
    Ident(String),
    ObjectPattern(Vec<String>),
}

/// An expression. Only the shapes `events_fanout.rs` concretely uses
/// (Decision B) — not the reference sketch's full `TsExpr` list (`Arrow`,
/// `Cond`, `TemplateLit`, `Spread` are all unused in the grounding file and
/// deliberately not built here). Arc C slice 3 (#1321, `workers.rs`) adds
/// `Arrow`, `OptionalMember`/`OptionalIndex` (Decision A, gaps 3/4) — this
/// slice's own real, grounded needs.
#[derive(Debug, Clone)]
pub enum TsExpr {
    Ident(String),
    /// `object.property`.
    Member {
        object: Box<TsExpr>,
        property: String,
    },
    /// `object?.property` — the optional-chaining form of [`TsExpr::Member`].
    /// A distinct variant, not an `optional: bool` flag on `Member` itself
    /// (#1321's own Decision A, gap 4, left the mechanism to the
    /// implementation): a flag would touch every one of `Member`'s
    /// already-many real call sites (`events_fanout.rs`, this crate's own
    /// printer tests) that never need one, where a separate variant touches
    /// none of them — narrower, matching this track's own repeated
    /// "smallest correct scope" judgment. `workers.rs`'s own secret-probe
    /// idiom (`emit_websocket_upgrade`/`emit_http_wrapper`/
    /// `emit_secret_lookup`, three identical real sites) is the only
    /// grounding.
    OptionalMember {
        object: Box<TsExpr>,
        property: String,
    },
    /// `object[index]`.
    Index {
        object: Box<TsExpr>,
        index: Box<TsExpr>,
    },
    /// `object?.[index]` — the optional-chaining form of [`TsExpr::Index`],
    /// the same real grounding and the same "separate variant, not a flag"
    /// reasoning as [`TsExpr::OptionalMember`].
    OptionalIndex {
        object: Box<TsExpr>,
        index: Box<TsExpr>,
    },
    /// `(params) => body` (`is_async: false`) or `async (params) => body`
    /// (`is_async: true`) — an expression-bodied arrow function. No
    /// block-body variant: `workers.rs`'s own one real site
    /// (`emit_worker_compose`'s own `__eventsDispatch` entry,
    /// `dispatchToEventsFanout(env.<bind>, events)`) has an expression body,
    /// not a block, and nothing else in that slice's own grounding needed
    /// one — extend narrowly, the same way `TsBinaryOp` only has the
    /// operators real content needs rather than the full JS/TS table.
    /// `is_async` added by #1327: `emit_composition_root`'s own
    /// `__eventsDispatch` closure (`async (events: Array<...>) => {...}`) is
    /// the first real `Arrow` site that's async — mirrors
    /// `TsDecl::Function`'s own `is_async` field (#1325), added for the same
    /// reason. `generics`/`return_type` added by #1339: `emit_sum_type`'s
    /// own generic payload-constructor arrows (`<T>(name: T): Sum<T> =>
    /// (...)`) need both — bare generic names only (empty for every
    /// non-generic site, the overwhelming majority), matching
    /// `TsObjectEntry::Method.generics`'s own (#1337) identical convention;
    /// `return_type` mirrors `TsDecl::Function.return_type`'s own existing
    /// `Option<TsType>` shape — a real gap the accepted proposal's own
    /// grounding named `generics` for but missed: every one of this file's
    /// own real generic-payload arrows carries an explicit return-type
    /// annotation the arrow itself owns (`: {name}{params}`), not something
    /// the body's own type alone determines.
    Arrow {
        params: Vec<TsParam>,
        is_async: bool,
        generics: Vec<String>,
        return_type: Option<TsType>,
        body: Box<TsExpr>,
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
        entries: Vec<TsObjectEntry>,
        multiline: bool,
    },
    /// An array literal, e.g. `[{ binding: "x", service: "y" }]`.
    /// `multiline: false` (the ordinary case, via [`TsExpr::array`]) always
    /// prints on one line, comma-separated — every real site before this
    /// slice. `multiline: true` (via [`TsExpr::multiline_array`]) is #1325's
    /// own real gap: `emit_test_main`'s own `modules` array (one `{ name,
    /// run }` entry per test, one per line, each with its own trailing
    /// comma, closing `]` at the statement's own indent) — the exact same
    /// shape [`TsExpr::Object`]'s own `multiline` field already represents
    /// for object literals, just for an array. Same reachability boundary as
    /// `Object`'s own `multiline` field: only `render_stmt_level_expr`
    /// (which carries `depth`) honours it; a `multiline: true` array nested
    /// inside another expression falls back to single-line via the ordinary
    /// depth-unaware `render_expr` recursion.
    Array {
        items: Vec<TsExpr>,
        multiline: bool,
    },
    /// `` `text${expr}more text` `` — a template literal. `parts.len()` is
    /// always `exprs.len() + 1` (`parts[0]` before the first substitution,
    /// `parts[i+1]` after `exprs[i]`, …). #1325's own real, first grounding
    /// for this shape (`bynk-ts`'s own module doc named `TemplateLit`
    /// explicitly "unused in the grounding file" until now):
    /// `emit_test_main`'s own `` `${m.name}:` ``/`` `${passed} passed,
    /// ${failed} failed.` `` lines.
    ///
    /// **`parts` are printed verbatim — the printer applies no escaping of
    /// its own.** The same "the field is already a raw-text slot" reasoning
    /// [`TsDecl::Import`]'s own `names` field doc already uses, not a new
    /// pattern: `emit_test_main`'s own two real ✓/✗ substitution lines embed
    /// a literal `✓`/`✗` JS unicode escape as pre-formed ASCII
    /// text (six literal characters, not the actual glyph) — an escaper
    /// mirroring [`TsLit::Str`]'s own (which escapes every `\` it sees)
    /// would double that literal backslash into `\\u2713`, corrupting the
    /// exact byte-golden output this slice must match. Every real part in
    /// `emit_test_main` is static, compiler-authored text (never Bynk user
    /// data), so there is no real content this boundary loses safety on
    /// today — a future caller passing untrusted/dynamic text into `parts`
    /// is responsible for pre-escaping backtick/`` ${ ``/`\` itself before
    /// constructing one.
    TemplateLit {
        parts: Vec<String>,
        exprs: Vec<TsExpr>,
    },
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
    /// `test ? consequent : alternate` — #1323's own real gap: `method ===
    /// "HEAD" ? headResponse(__response) : __response` (once per `GET`
    /// route) and `method === "OPTIONS" ? 204 : 405` (the method-fallthrough
    /// path). The lowest-precedence expression form after `Arrow` — needs
    /// the same parenthesization-rule coverage `Arrow` got in review of
    /// #1322 (`needs_parens_as_operand`/`render_binary_operand`/`As`'s own
    /// local rule), added proactively in this same slice rather than left
    /// for a review round to re-find.
    Conditional {
        test: Box<TsExpr>,
        consequent: Box<TsExpr>,
        alternate: Box<TsExpr>,
    },
    /// An explicit, printer-preserved parenthesization — distinct from every
    /// other variant's own precedence-*derived* parens (`render_operand`/
    /// `render_binary_operand`), which the printer adds or omits based on
    /// what the wrapped expression *is*. `Paren` instead always prints
    /// `(<inner>)` regardless of `inner`'s own shape or precedence. #1323's
    /// own real, narrow need: `workers_entry.rs`'s CORS-preflight guard
    /// wraps its own path-match condition in unconditional parens
    /// (`&& ({cond})`) even when `cond` reduces to a single equality check
    /// with no operator lower-precedence than the outer `&&` — a real case
    /// the precedence-derived rules correctly do *not* parenthesize (they're
    /// answering "is this needed for correctness", not "did the source
    /// always write parens here"). Not a general escape hatch: every other
    /// real site in this file's own content still goes through the ordinary
    /// precedence machinery unchanged.
    Paren(Box<TsExpr>),
    Lit(TsLit),
}

impl TsExpr {
    /// The ordinary, single-line object literal, `Prop`-only — every entry
    /// is `key: value`. `events_fanout.rs`'s own entries (and this crate's
    /// own printer tests) are all this shape; kept taking a plain
    /// `Vec<(String, TsExpr)>` rather than `Vec<TsObjectEntry>` so none of
    /// those existing call sites needed to change when `TsObjectEntry` was
    /// added (#1321) — see [`TsExpr::object_entries`] for the mixed-entry
    /// form `workers.rs` itself needs (shorthand/spread/method entries).
    pub fn object(entries: Vec<(String, TsExpr)>) -> Self {
        TsExpr::Object {
            entries: entries
                .into_iter()
                .map(|(k, v)| TsObjectEntry::Prop(k, v))
                .collect(),
            multiline: false,
        }
    }

    /// One entry per line, each with its own trailing comma — see
    /// [`TsExpr::Object`]'s own doc for the real shape and the
    /// depth-awareness this needs at the print site. `Prop`-only, the same
    /// convenience [`TsExpr::object`]'s own doc explains.
    pub fn multiline_object(entries: Vec<(String, TsExpr)>) -> Self {
        TsExpr::Object {
            entries: entries
                .into_iter()
                .map(|(k, v)| TsObjectEntry::Prop(k, v))
                .collect(),
            multiline: true,
        }
    }

    /// The single-line object literal, taking [`TsObjectEntry`] directly —
    /// for a mixed entry list (shorthand/spread/method alongside `Prop`),
    /// which [`TsExpr::object`]'s own `Vec<(String, TsExpr)>` convenience
    /// can't represent. #1321's own real grounding: `workers.rs`'s local
    /// capability-provider `deps` object mixes shorthand names
    /// (`{ cap1, cap2 }`) with explicit `key: value` entries
    /// (`__exec: exec`) in one literal.
    pub fn object_entries(entries: Vec<TsObjectEntry>) -> Self {
        TsExpr::Object {
            entries,
            multiline: false,
        }
    }

    /// [`TsExpr::multiline_object`]'s own `TsObjectEntry` sibling —
    /// #1321's own real grounding: `workers.rs`'s `compose`-returned
    /// surface object is one shorthand-async-`Method` entry per wrapper,
    /// one per line.
    pub fn multiline_object_entries(entries: Vec<TsObjectEntry>) -> Self {
        TsExpr::Object {
            entries,
            multiline: true,
        }
    }

    /// The ordinary, single-line array literal — every real site before
    /// #1325.
    pub fn array(items: Vec<TsExpr>) -> Self {
        TsExpr::Array {
            items,
            multiline: false,
        }
    }

    /// One entry per line, each with its own trailing comma — see
    /// [`TsExpr::Array`]'s own doc for the real shape and the
    /// depth-awareness this needs at the print site. #1325's own real
    /// grounding: `emit_test_main`'s own `modules` array.
    pub fn multiline_array(items: Vec<TsExpr>) -> Self {
        TsExpr::Array {
            items,
            multiline: true,
        }
    }

    /// A template literal — see [`TsExpr::TemplateLit`]'s own doc for the
    /// real shape and its no-escaping boundary. The sole caller-facing
    /// invariant (`parts.len() == exprs.len() + 1`) is asserted here rather
    /// than left to `render_expr`'s own `parts`-driven loop, which would
    /// otherwise silently drop trailing `exprs` on a malformed tree instead
    /// of failing loudly (review of #1326, finding 1).
    pub fn template_lit(parts: Vec<String>, exprs: Vec<TsExpr>) -> Self {
        debug_assert_eq!(
            parts.len(),
            exprs.len() + 1,
            "TsExpr::template_lit: parts.len() must be exprs.len() + 1 \
             (parts[0] before the first substitution, parts[i+1] after \
             exprs[i], …) — got {} parts and {} exprs",
            parts.len(),
            exprs.len()
        );
        TsExpr::TemplateLit { parts, exprs }
    }
}

/// One entry of a [`TsExpr::Object`] literal. Only `Prop` existed before
/// #1321 (`events_fanout.rs`'s own grounding never needed the other three) —
/// `workers.rs`'s own dominant shape (Decision A, gap 1) is a literal
/// object whose entries are shorthand async methods (every `on call`/`on
/// http`/… wrapper attaches this way), its local capability-`deps` object
/// mixes bare shorthand names with explicit `key: value` pairs, and three
/// real sites spread another object into one (gap 2, folded in here rather
/// than as its own top-level `TsExpr` variant — a spread only ever appears
/// as an object- or array-literal entry in this file, never as a
/// standalone expression, so scoping it to entry position is the narrower
/// correct change).
#[derive(Debug, Clone)]
pub enum TsObjectEntry {
    /// `key: value`.
    Prop(String, TsExpr),
    /// `key` alone — object-literal property shorthand, e.g. `{ cap1,
    /// cap2 }`. Distinct from `Prop(name, TsExpr::Ident(name))`, which
    /// prints `name: name`, not the shorthand form real TypeScript
    /// property-shorthand text actually is.
    Shorthand(String),
    /// A shorthand async method entry, e.g. `async foo(a: T) { ... },` —
    /// `workers.rs`'s own dominant shape (Decision A, gap 1): every
    /// wrapper helper (`emit_call_wrapper`, `emit_event_wrapper`, …)
    /// attaches to the returned `compose` surface this way — none of
    /// `workers.rs`'s own real sites annotates a return type, so
    /// `return_type` stayed unneeded until #1323's own real gap:
    /// `workers_entry.rs`'s `export default { fetch, scheduled?, queue? }`
    /// entries all carry one (`: Promise<Response>`/`: Promise<void>`), the
    /// same field [`TsClassMethod::return_type`] already has for the
    /// declaration-position sibling this mirrors.
    ///
    /// `generics` and `doc` (#1337's own real gaps, both found only by the
    /// zero-diff fixture check — not the accepted proposal's own citation,
    /// which searched the project-form fixture corpus only;
    /// `402_generic_instance_method` is single-file form, and while
    /// `65_money_uses_time`/`64_full_time_commons` ARE project form, the
    /// citation's own doc-comment search used the wrong marker, `///`
    /// rather than this language's real `---`-delimited doc block —
    /// both gaps outside what that first pass actually checked):
    ///
    /// - `generics`: a method on a generic type erases to
    ///   `{name}<{generics}>(self: {Type}<{generics}>, …)` — `Box.map<A,
    ///   U>`, the type's own `A` plus the method's own `U`. Bare names
    ///   only, no bounds/defaults — every real generic parameter list this
    ///   tree ever builds is (matching [`crate::printer::print_type`]'s
    ///   own `TsType::Named`-only-name convention for the identical
    ///   reason), so a full `Vec<TsParam>` (with its own unneeded
    ///   `ty`/`optional` fields) would be the wrong shape here.
    /// - `doc`: a JSDoc block immediately preceding the method entry, at
    ///   the same indent — `Timestamp.diff`/`Timestamp.add`
    ///   (`65_money_uses_time`) both carry one. Printed the identical way
    ///   `TsStmtKind::DocComment` is (same escaping, same blank-line
    ///   convention), reusing that one renderer rather than a second copy
    ///   — this field exists because an object-entry method has no
    ///   `TsStmt` slot of its own to hold a preceding statement in.
    Method {
        name: String,
        is_async: bool,
        generics: Vec<String>,
        params: Vec<TsParam>,
        return_type: Option<TsType>,
        doc: Option<String>,
        /// `false` (every method entry landed before #1337): `body` renders
        /// as an ordinary braced, multi-line block. `true`: `body` renders
        /// compactly on the SAME line as the signature — `{ <stmts>; }`,
        /// reusing this crate's own established compact-statement
        /// machinery (`TsStmtKind::InlineBlock`'s own sibling shape). A
        /// real gap #1337's own zero-diff check found:
        /// `emit_forwarded_methods`'s own pre-conversion `writeln!` always
        /// built the WHOLE entry — signature and one-statement body alike —
        /// on one physical line (`"  {method}({params}): {ret} {{ return
        /// …; }},"`), a genuinely different real shape from every other
        /// `Method` entry in this tree, all of which are multi-line.
        inline: bool,
        body: Vec<TsStmt>,
    },
    /// `...expr` — an object-spread entry (Decision A, gap 2), e.g. `{
    /// ...deps, identity: __caller }`.
    Spread(TsExpr),
}

/// `!x` (`events_fanout.rs`'s `!Array.isArray(subs)`, `!binding`) and
/// `typeof x` (#1321, `workers.rs`'s own secret-probe idiom: `typeof
/// __secret !== "string"`) — the two unary operators real content uses, not
/// the full JS/TS table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsUnaryOp {
    Not,
    Typeof,
}

/// `??` (`events_fanout.rs`'s `env ?? {}`), plus three more real operators
/// #1321 (`workers.rs`) grounds: `||`/`&&` (the Bearer-header presence
/// checks, `__authz === null || !__authz.startsWith(...)` /
/// `__authz !== null && __authz.startsWith(...)`) and `===`/`!==`
/// (pervasive throughout the file's own tagged-result and header checks).
/// #1323 (`workers_entry.rs`) grounds one more: `>` (the request-body-
/// ceiling guard's own `Number(__contentLength) > <cap>` — the one real
/// site anywhere in `bynk-emit` that needs a relational, not equality,
/// comparison). Not the
/// full JS/TS operator table (Decision B's own "extend narrowly" posture) —
/// see the printer's own `binary_precedence` (`bynk-ts/src/printer.rs`,
/// private) for why a nested `Binary` operand's parenthesisation needed to
/// become precedence-aware once more than one operator existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsBinaryOp {
    NullishCoalescing,
    Or,
    And,
    StrictEq,
    StrictNotEq,
    GreaterThan,
}

/// A literal — the three kinds `events_fanout.rs` uses (a string, a number,
/// `null`), plus `Bool` (#1323's own real gap: `workers_entry.rs`'s
/// `CorsPolicy.credentials`/`SecurityPolicy.nosniff` object-literal fields
/// are real booleans, e.g. `credentials: true`, `nosniff: false` — nothing
/// before this slice's own grounding ever built one).
#[derive(Debug, Clone)]
pub enum TsLit {
    Str(String),
    /// Rendered verbatim, as text — TypeScript's own numeric-literal
    /// grammar is not this crate's problem to re-derive; the caller passes
    /// the exact digits it wants printed.
    Num(String),
    Null,
    Bool(bool),
    /// The complete literal text, printed exactly as given — no quoting, no
    /// escaping, nothing added or removed. #1325's own real, narrow gap:
    /// `emit_test_main`'s own `const PREFIX = "integration · ";` embeds
    /// a literal JS unicode escape (six ASCII characters, `·`) as
    /// pre-formed text — [`TsLit::Str`]'s own escaper (which turns every `\`
    /// it sees into `\\`) would double that literal backslash, corrupting
    /// this specific byte-golden output. The same "already a raw-text slot"
    /// boundary [`TsExpr::TemplateLit`]'s own `parts` field documents, just
    /// for a whole literal (quotes included) rather than a template
    /// literal's own static segments. One real site — not a general
    /// escaping bypass for ordinary string content, which stays on
    /// `TsLit::Str`.
    Raw(String),
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
#[derive(Debug, Clone)]
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
    ///
    /// #1323 replaced the plain `Vec<(String, TsType, bool)>` (`optional`
    /// only, #1321) with [`TsTypeMember`] — `workers_entry.rs`'s own real
    /// gap needed both a `readonly` modifier (every structural type this
    /// file builds is `readonly`-qualified, e.g. `{ readonly cron: string;
    /// readonly scheduledTime: number }`) and a method-signature member
    /// (`ack(): void`, `retry(): void`, `waitUntil(promise: Promise<
    /// unknown>): void`), neither representable by one more anonymous
    /// tuple `bool` — a fourth positional `bool` next to `optional` would
    /// be genuinely ambiguous at call sites (`readonly` vs. `optional`, ordered
    /// how?), the same reasoning [`TsObjectEntry`] already used over a raw
    /// tuple for the value-position sibling.
    Object(Vec<TsTypeMember>),
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
    ///
    /// `multiline` (#1339): `false` (via [`TsType::union`]) is this variant's
    /// original single-line `A | B | C` form, unchanged. `true` (via
    /// [`TsType::multiline_union`]) is `emit_sum_type`'s own real
    /// discriminated-union shape — one variant per line, a leading `|` on
    /// every line *except* the first (which gets equivalent spacing
    /// instead), the closing `;` appended directly to the last variant's own
    /// line. Mirrors [`TsExpr::Object`]'s own `multiline` field precedent
    /// (#1317), but — unlike that one — needs no depth-aware wrapper: the
    /// pre-conversion `writeln!` code this reproduces always used a fixed
    /// two-space indent regardless of nesting (this shape is only ever a
    /// top-level `export type` alias body, never nested inside another
    /// type), so `render_type` stays entirely depth-unaware here too. Only
    /// rendered correctly from [`TsDecl::TypeAlias`]'s own top-level
    /// render — not reachable, and not given a defined rendering, from any
    /// nested position (an array element, a type argument, …), the same
    /// named boundary [`TsExpr::Object`]'s own `multiline` doc already
    /// draws for its own nested case.
    Union {
        members: Vec<TsType>,
        multiline: bool,
    },
    /// `A & B` — a type-position intersection. #1339's own real gap:
    /// `emit_refined_type`'s own branded-type alias,
    /// `{base} & { readonly __brand: "..." }`, has no representation among
    /// `Named`/`Array`/`Object`/`Fn`/`Union` — mirrors `Union`'s own
    /// shape/precedent exactly (a flat `Vec`, each member printed through
    /// the ordinary `render_type` recursion, joined by ` & `), single-line
    /// only (nothing in `bynk-emit` builds a multi-line intersection).
    Intersection(Vec<TsType>),
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

    /// `A | B | C` — the original single-line union form, unchanged.
    pub fn union(members: Vec<TsType>) -> Self {
        TsType::Union {
            members,
            multiline: false,
        }
    }

    /// `emit_sum_type`'s own real multi-line discriminated-union shape — see
    /// [`TsType::Union`]'s own doc for the exact rendering rules and why
    /// this needs no depth parameter.
    pub fn multiline_union(members: Vec<TsType>) -> Self {
        TsType::Union {
            members,
            multiline: true,
        }
    }

    /// `A & B` — an intersection type.
    pub fn intersection(members: Vec<TsType>) -> Self {
        TsType::Intersection(members)
    }
}

/// One member of a [`TsType::Object`] structural type — a property (`Prop`)
/// or a method signature (`Method`, #1323's own real gap: `ack(): void`,
/// `retry(): void`, `waitUntil(promise: Promise<unknown>): void` — no body,
/// a type-position sibling to [`TsObjectEntry::Method`], which does carry
/// one). `Method`'s own parameters reuse [`TsParam`] directly (not a bare
/// `Vec<TsType>` the way [`TsType::Fn`]'s anonymous, positionally-numbered
/// parameters do) — `waitUntil`'s own real parameter has a real name
/// (`promise`) the printed text must show, unlike `Fn`'s callers, none of
/// which have one to show.
#[derive(Debug, Clone)]
pub enum TsTypeMember {
    Prop {
        name: String,
        ty: TsType,
        optional: bool,
        readonly: bool,
    },
    Method {
        name: String,
        params: Vec<TsParam>,
        ret: TsType,
    },
    /// `[key_name: key_ty]: value_ty` — a TypeScript index signature.
    /// #1323's own real gap: `workers_entry.rs`'s multi-param `on call`
    /// dispatch casts its raw JSON args object through `{ [k: string]:
    /// JsonValue }` before indexing it by field name — textually distinct
    /// from the semantically-equivalent `Record<string, JsonValue>` (a
    /// `Named` type with type arguments), which `bynk-emit`'s own
    /// pre-conversion `writeln!` never wrote here. The one real site.
    Index {
        key_name: String,
        key_ty: TsType,
        value_ty: TsType,
    },
}

impl TsTypeMember {
    /// `name: ty` — the ordinary, non-`optional`, non-`readonly` case.
    pub fn prop(name: impl Into<String>, ty: TsType) -> Self {
        TsTypeMember::Prop {
            name: name.into(),
            ty,
            optional: false,
            readonly: false,
        }
    }

    /// `name?: ty`.
    pub fn optional_prop(name: impl Into<String>, ty: TsType) -> Self {
        TsTypeMember::Prop {
            name: name.into(),
            ty,
            optional: true,
            readonly: false,
        }
    }

    /// `readonly name: ty`.
    pub fn readonly_prop(name: impl Into<String>, ty: TsType) -> Self {
        TsTypeMember::Prop {
            name: name.into(),
            ty,
            optional: false,
            readonly: true,
        }
    }

    /// `name(params): ret` — no body.
    pub fn method(name: impl Into<String>, params: Vec<TsParam>, ret: TsType) -> Self {
        TsTypeMember::Method {
            name: name.into(),
            params,
            ret,
        }
    }

    /// `[key_name: key_ty]: value_ty`.
    pub fn index(key_name: impl Into<String>, key_ty: TsType, value_ty: TsType) -> Self {
        TsTypeMember::Index {
            key_name: key_name.into(),
            key_ty,
            value_ty,
        }
    }
}

/// One function/method/constructor parameter.
#[derive(Debug, Clone)]
pub struct TsParam {
    pub name: String,
    pub ty: Option<TsType>,
    /// `name?: ty` — `events_fanout.rs`'s own `env?: unknown` constructor
    /// parameter needs this; nothing in the grounding file needs a default
    /// value, so only optionality is represented, not defaults.
    pub optional: bool,
}

/// One `class` field.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct TsClassCtor {
    pub params: Vec<TsParam>,
    pub body: Vec<TsStmt>,
}

/// One class method.
#[derive(Debug, Clone)]
pub struct TsClassMethod {
    pub name: String,
    pub is_async: bool,
    pub params: Vec<TsParam>,
    pub return_type: Option<TsType>,
    pub body: Vec<TsStmt>,
}

/// A top-level declaration. `Import`, `Export`, `Interface`, `ConstDecl`,
/// and `Class` were `events_fanout.rs`'s own grounding (P7.8's own note:
/// "not the sketch's `Function`/`TypeAlias` — unused in the grounding
/// file"). #1321 (`workers.rs`) needed both after all — a real gap the
/// accepted proposal's own Framing didn't name (its own "no other gap
/// surfaced" checked `TsStmt`/`TsExpr`/`TsType` shapes, not `TsDecl` ones):
/// `compose.ts`'s own `export function compose(env: Env, …) { … }` is a
/// top-level function declaration, and (when the Worker has agents or
/// publishes events) a `type DurableObjectNamespace = { … };` fallback
/// alias sits alongside it. Named explicitly as a deviation from the
/// accepted proposal's own catalogue, not silently added — both are
/// mechanical, direct `TsDecl` siblings of `ConstDecl`/`Class` already
/// here, the same "small, grounded, same class of gap" this track's own
/// history repeatedly found and closed (P7.8's `Assign`/`Continue`/
/// `TryCatch`, Arc C slice 1's `Comment`).
#[derive(Debug, Clone)]
pub enum TsDecl {
    /// `import { a, b } from "spec";` (`type_only: false`) or
    /// `import type { a, b } from "spec";` (`type_only: true`). Only the
    /// named-imports form — `events_fanout.rs` never uses a default
    /// import, so none is represented. `names` entries are pushed verbatim
    /// (`printer.rs`'s own `names.join(", ")`), so a renamed import
    /// (`messagesLocales as __locale_declaredLocales`, `workers.rs`'s own
    /// locale-negotiation import) is one opaque `String` entry, not a
    /// structured rename — the same "the field is already a raw-text slot"
    /// reasoning that also covers a `type`-prefixed single specifier
    /// (`"type KVNamespace"`) inside an otherwise non-`type_only` import.
    Import {
        type_only: bool,
        names: Vec<String>,
        from: String,
    },
    /// `import * as alias from "spec";` — a namespace import, structurally
    /// different from [`TsDecl::Import`]'s braced named-imports form (no
    /// `{ }`, one bound name for the whole module). #1321's own real gap:
    /// `workers.rs`'s `compose.ts` imports `handlers.js` and each
    /// referenced unit's binding module this way; `events_fanout.rs` never
    /// used one.
    ImportNamespace { alias: String, from: String },
    /// `export { a, b } from "spec";` — a re-export, structurally distinct
    /// from both [`TsDecl::Import`] (which binds locally, carries no
    /// `export` keyword) and [`TsDecl::Export`] (which wraps a whole
    /// declaration, not a braced name list re-pointed at another module).
    /// #1323's own real gap: `workers_entry.rs` re-exports each agent's
    /// Durable Object class from `./handlers.js`, and (when the context
    /// publishes events) the fan-out DO's class from `./events_fanout.js`.
    /// No `type_only` form — nothing in the grounding file re-exports a
    /// type-only name.
    ReExport { names: Vec<String>, from: String },
    /// `export * from "spec";` — a wildcard re-export, structurally distinct
    /// from [`TsDecl::ReExport`] (which always carries a braced name list —
    /// an empty `names` there would render the ill-formed `export {  }
    /// from "spec";`, not this). Mirrors [`TsDecl::ImportNamespace`]'s own
    /// "no braces, no name list, one bound target" shape, on the export
    /// side. #1329's own real gap: `emit_commons_barrel`
    /// (`bynk-emit/src/project/tests_emit.rs`) builds a multi-file
    /// `commons` unit's barrel module as one `export *` line per
    /// constituent source file.
    ReExportAll { from: String },
    /// Marks the wrapped declaration `export`ed — `export class Foo { .. }`
    /// is `Export(Box::new(Class { .. }))`. A wrapper, not a per-variant
    /// `exported: bool` field, matching the reference sketch's own naming
    /// (`TsDecl::Export` is a peer variant, not a modifier on each other
    /// one).
    Export(Box<TsDecl>),
    /// `type_params`/per-member `readonly` (#1339's own real gap):
    /// `emit_record_type`'s own `export interface {name}{params} {
    /// readonly {field}: {ty}; ... }` — bare generic names (matching
    /// `ts_type_params`'s/`TsObjectEntry::Method.generics`'s own
    /// convention) and a `readonly` modifier every real field here
    /// carries. `members` reuses [`TsTypeMember`] (the exact same shape
    /// [`TsType::Object`]'s own structural members already use) rather
    /// than a bespoke tuple, since `readonly` is already that type's own
    /// field — this interface's real content has never needed
    /// `Method`/`Index`, but nothing about reusing the shared type
    /// forecloses a future slice that does.
    Interface {
        name: String,
        type_params: Vec<String>,
        members: Vec<TsTypeMember>,
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
    /// `function name(params): ret { body }` (`is_async: false`) or `async
    /// function name(params): ret { body }` (`is_async: true`) — a top-level
    /// function declaration. `is_async` added by #1325: `workers.rs`'s own
    /// one real site (`compose`) is never async, so the field didn't exist
    /// until `emit_test_main`'s own top-level `async function main() {...}`
    /// needed it — the exact "extend narrowly, add it when a future slice's
    /// own grounding needs it" deferral this variant's own history already
    /// named. `generics` added by #1351: `emit_free_fn`'s own v0.20a erased
    /// generics (`export function foo<T>(...)`) — bare names only, empty
    /// for every non-generic site (`compose`/`main`), matching every other
    /// real generics-list precedent in this crate
    /// (`TsObjectEntry::Method.generics` #1337, `TsExpr::Arrow.generics`
    /// #1339).
    Function {
        name: String,
        generics: Vec<String>,
        params: Vec<TsParam>,
        return_type: Option<TsType>,
        body: Vec<TsStmt>,
        is_async: bool,
    },
    /// `type name = ty;` (non-generic) or `type name<T, U> = ty;` (via
    /// `type_params`, #1339's own real gap: `emit_sum_type`'s own generic
    /// sum types erase to `export type Foo<T> = ...`) — a top-level type
    /// alias. `workers.rs`'s own real site: the `DurableObjectNamespace`
    /// local fallback type (emitted only when the Worker has agents or
    /// publishes events), never generic — `type_params` is empty there.
    TypeAlias {
        name: String,
        type_params: Vec<String>,
        ty: TsType,
    },
    /// `export default <expr>;` — a default export of an *expression*, not
    /// a declaration. #1323's own real gap (`workers_entry.rs`'s own
    /// top-level shape, `export default { fetch, scheduled?, queue? }`):
    /// [`TsDecl::Export`] wraps a [`TsDecl`] (a `Class`/`ConstDecl`/etc.,
    /// something with its own name), which cannot represent exporting a
    /// bare, unnamed object-literal expression — so this is its own
    /// variant, not `Export(Box::new(ConstDecl { .. }))` with a synthetic
    /// name that would print wrong.
    ExportDefault(TsExpr),
    /// `declare const name: ty;` — an ambient binding with no initialiser,
    /// narrowing a global TypeScript otherwise doesn't know about. #1325's
    /// own real, narrow site: `emit_test_main`'s own `declare const process:
    /// { exit(code: number): never; env: { [k: string]: string | undefined
    /// } };`, narrowing Node's `process` global without a `@types/node`
    /// dependency. Distinct from [`TsDecl::ConstDecl`] (always has a real
    /// `init` expression) — a `declare const` has none, by definition; a
    /// `TsDecl::ConstDecl` with a synthetic placeholder `init` would print
    /// wrong (`= <something>;` where nothing should appear at all).
    DeclareConst { name: String, ty: TsType },
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

    /// Review of #1326, finding 1: `TsExpr::template_lit`'s own
    /// `debug_assert_eq!` must actually fire on a malformed tree, not just
    /// exist as documentation — proves the guard guards, the same "prove
    /// it" discipline this crate applies to every other invariant.
    #[test]
    #[should_panic(expected = "parts.len() must be exprs.len() + 1")]
    fn template_lit_rejects_a_parts_exprs_count_mismatch() {
        let _ = TsExpr::template_lit(
            vec!["a".to_string()],
            vec![TsExpr::Ident("too_many".to_string())],
        );
    }
}
