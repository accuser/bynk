//! P8.5 (#1516): `Body(DefId)`/`TypeOf(DefId)` — R3.13's definition-level
//! query row, realised as real, callable, `DefId`-keyed functions wrapping
//! the checker's own existing per-definition entry points
//! ([`checker::check_body`], [`checker::check_handler_body`]) rather than
//! rewriting them. Per this slice's own [DECISION C],
//! `check_file_core`/`analyse_project`/`check_unit_files` are entirely
//! unchanged — nothing in the tree calls [`body`]/[`type_of`] yet; a future
//! scheduler slice (R3.15, explicitly deferred by this phase's own Q3) is
//! what would wire them in.
//!
//! **[DECISION A]**: [`DefId`] identifies any checkable definition — a free
//! function, a method, or a handler — split into [`DefId::Fn`]/
//! [`DefId::Handler`] rather than one unified shape, because
//! [`checker::check_handler_body`] returns `()` while [`checker::check_body`]
//! returns `Option<TyId>`: a handler has no single value a caller reads, so
//! `TypeOf` is a type-level impossibility for it, not a documented `None`
//! case. [`FnDefId`]/[`HandlerDefId`] are both scoped by
//! [`UnitId`] (P8.1) plus a stable,
//! span-free, body-free name — reusing `bynk-check`'s own existing
//! `String`-keyed `UnitTable.fns`/`UnitTable.methods`/`UnitTable.services`/
//! `UnitTable.agents` maps as the precedent, rather than introducing a
//! fresh interned identity scheme.
//!
//! **[DECISION D]**: the outward-facing signatures stay `fn body(id: DefId,
//! …)` / `fn type_of(id: DefId, …)` — plain `DefId`, not `DefId::Fn` — so
//! `xtask`'s `defid_query_fn_present` probe (a same-line substring match for
//! `fn_needle` + `DefId`) keeps working unmodified. [`type_of`] is
//! therefore the one place [DECISION A]'s "invalid states unrepresentable"
//! ideal is deliberately relaxed: called on a [`DefId::Handler`], it returns
//! `None` without doing any checking work, documented here rather than
//! discovered at a call site.
//!
//! **[DECISION B]**: both [`body`] and [`type_of`] allocate a **fresh**
//! [`checker::CheckSinks`] per call — not the file-wide one
//! `context_checks.rs`'s own per-context passes thread across every
//! provider op/service handler/agent handler in one context. This is safe
//! by construction for `expr_types`/`callees` (plain maps, keyed by
//! [`ExprId`], with no accumulation semantics of their own) and for
//! `refs`/`hints`/`locals`/`requirements` (each sink's own doc comment
//! already states it "records nothing until `enter_file` attributes it" —
//! a fresh sink plus one `enter_file` call per query is exactly the shape
//! each sink already assumes a caller can take, not a new assumption this
//! slice invents). What per-call isolation does **not** yet prove is
//! whether any real call site's accumulated diagnostic behaviour depends on
//! being threaded across a whole file/context's definitions rather than
//! isolated per call — the Risks section of #1516 names this explicitly;
//! this module's own test module has a byte-for-byte fixture proving
//! isolation is safe for the one shape this slice actually exercises (a
//! single provider op, no sibling definitions in the same context to
//! accumulate against), not a proof for every shape `CheckSinks`'s seven
//! fields could ever see.

use std::collections::HashMap;
use std::path::Path;

use bynk_syntax::ast::{ActorDecl, Block, Expr, ExprId, HandlerKind};
use bynk_syntax::error::CompileError;
use bynk_syntax::span::Span;

use crate::checker::{
    self, Callee, CapabilityCtx, CheckSinks, HandlerBodyCheck, TestServiceSig, TyId, TypedExpr,
    Types,
};
use crate::hints::HintSink;
use crate::index::RefSink;
use crate::locals::LocalsSink;
use crate::requirements::RequirementSink;
use crate::resolver::ResolvedCommons;
use crate::unit_signature::UnitId;

/// [DECISION A]: a free function or a method — [`UnitTable.fns`]/
/// [`UnitTable.methods`]'s own identity shape (a unit-scoped name, plus the
/// attached type's name for a method), reused rather than reinvented.
///
/// [`UnitTable.fns`]: crate::symbols::UnitTable::fns
/// [`UnitTable.methods`]: crate::symbols::UnitTable::methods
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FnDefId {
    pub unit: UnitId,
    /// `Some(type_name)` for a method attached to `type_name`; `None` for a
    /// free function.
    pub owner: Option<String>,
    pub name: String,
}

/// [DECISION A]: a service or agent handler — identified by its owning
/// unit, the declaration that owns it (a service or agent's own name), its
/// [`HandlerKind`] (body-free, span-free as of this slice — see the `Hash`
/// derive added to it), and, for an agent handler, its own `method_name`
/// (`on call addItem(...)`'s `addItem`) — the one field
/// [`bynk_syntax::ast::Handler`]'s own doc comment already names as the
/// disambiguator multiple `on call` methods need. `None` for a service
/// handler (`kind` alone is unique within one service's protocol).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HandlerDefId {
    pub unit: UnitId,
    pub owner: String,
    pub kind: HandlerKind,
    pub method_name: Option<String>,
}

/// R3.13's own definition-level identity: a free function, a method, or a
/// handler. See [DECISION A] (this module's own doc comment) for why this
/// is a split enum rather than one unified shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DefId {
    Fn(FnDefId),
    Handler(HandlerDefId),
}

/// The inputs [`checker::check_body`] needs beyond `input`/`sinks` — every
/// positional argument of that function, bundled the same way `#522`
/// bundled `check_handler_body`'s own into [`HandlerBodyCheck`].
pub struct FnBodyInputs<'a> {
    pub body: &'a Block,
    pub return_ty: TyId,
    pub return_ty_span: Span,
    pub scope: HashMap<String, TyId>,
    pub caps: CapabilityCtx,
    pub test_services: HashMap<String, TestServiceSig>,
    pub test_actors: HashMap<String, ActorDecl>,
    pub where_pred: Option<&'a Expr>,
}

/// [DECISION A]: [`body`]'s own input bundle mirrors [`DefId`]'s split —
/// a caller assembling a [`FnDefId`]'s inputs cannot accidentally pass a
/// [`HandlerBodyCheck`] bundle (or vice versa); a mismatch between `id`'s
/// variant and `inputs`'s variant is a caller bug [`body`] rejects with a
/// `panic!`, not a silently-ignored input.
pub enum BodyInputs<'a> {
    Fn(FnBodyInputs<'a>),
    Handler(HandlerBodyCheck<'a>),
}

/// R3.13's own `Body(DefId)` row: the self-contained result of checking one
/// definition's body against a **fresh** [`checker::CheckSinks`] (see
/// [DECISION B]). `ty` is `check_body`'s own `Option<TyId>` answer for a
/// [`DefId::Fn`]; always `None` for a [`DefId::Handler`], matching
/// `check_handler_body`'s own `()` return — a handler is checked for
/// internal consistency, not typed as a value.
pub struct Body {
    pub id: DefId,
    pub ty: Option<TyId>,
    pub expr_types: HashMap<ExprId, TypedExpr>,
    pub errors: Vec<CompileError>,
    pub refs: RefSink,
    pub hints: HintSink,
    pub locals: LocalsSink,
    pub requirements: RequirementSink,
    pub callees: HashMap<ExprId, Callee>,
}

/// R3.13's own `TypeOf(DefId)` row — meaningful only for [`DefId::Fn`]; see
/// [DECISION A]/[DECISION D] (this module's own doc comment) for why
/// [`type_of`] returns `Option<TypeOf>` (`None` for a handler) rather than
/// being unrepresentable for one at the type level.
pub struct TypeOf {
    pub id: DefId,
    pub ty: Option<TyId>,
}

/// Everything [`body`]/[`type_of`] need beyond `id`/`inputs` — bundled so
/// both functions' own signatures fit on one line (`fn body(id: DefId, …)
/// -> Body {`), which is what `xtask`'s `defid_query_fn_present` probe
/// scans for (see that function's own doc comment: it requires `DefId` on
/// the *same line* as the `fn` needle, not merely present in the
/// signature).
pub struct QueryCtx<'a> {
    pub input: &'a ResolvedCommons,
    pub tys: &'a Types,
    /// Attributes the fresh sinks exactly as `check_pipeline.rs`'s own
    /// per-file loop already does for the shared sinks it threads.
    pub file: &'a Path,
    pub muted: bool,
}

/// R3.13's `Body(DefId)`. See [DECISION B] (this module's own doc comment)
/// for why `ctx`'s sinks are fresh per call.
///
/// # Panics
///
/// If `id`'s variant does not match `inputs`'s variant (a [`DefId::Fn`]
/// paired with [`BodyInputs::Handler`], or vice versa) — a caller bug, not
/// a recoverable input.
pub fn body(id: DefId, inputs: BodyInputs<'_>, ctx: QueryCtx<'_>) -> Body {
    let QueryCtx {
        input,
        tys,
        file,
        muted,
    } = ctx;
    let mut expr_types = HashMap::new();
    let mut errors = Vec::new();
    let mut refs = RefSink::new();
    let mut hints = HintSink::new();
    let mut locals = LocalsSink::new();
    let mut requirements = RequirementSink::new();
    let mut callees = HashMap::new();

    let namespace = match &id {
        DefId::Fn(f) => f.unit.0.as_str(),
        DefId::Handler(h) => h.unit.0.as_str(),
    };
    let owner = match &id {
        DefId::Fn(f) => f.owner.as_deref().unwrap_or(f.name.as_str()),
        DefId::Handler(h) => h.owner.as_str(),
    };
    refs.enter_file(file, namespace, muted);
    refs.set_owner(owner);
    hints.enter_file(file, muted);
    locals.enter_file(file, muted);
    requirements.enter_file(file, muted);

    let ty = match (&id, inputs) {
        (DefId::Fn(_), BodyInputs::Fn(fi)) => checker::check_body(
            input,
            fi.body,
            fi.return_ty,
            fi.return_ty_span,
            fi.scope,
            fi.caps,
            fi.test_services,
            fi.test_actors,
            fi.where_pred,
            CheckSinks {
                tys,
                expr_types: &mut expr_types,
                errors: &mut errors,
                refs: &mut refs,
                hints: &mut hints,
                locals: &mut locals,
                requirements: &mut requirements,
                callees: &mut callees,
            },
        ),
        (DefId::Handler(_), BodyInputs::Handler(check)) => {
            checker::check_handler_body(
                input,
                check,
                CheckSinks {
                    tys,
                    expr_types: &mut expr_types,
                    errors: &mut errors,
                    refs: &mut refs,
                    hints: &mut hints,
                    locals: &mut locals,
                    requirements: &mut requirements,
                    callees: &mut callees,
                },
            );
            None
        }
        (DefId::Fn(_), BodyInputs::Handler(_)) | (DefId::Handler(_), BodyInputs::Fn(_)) => {
            panic!(
                "bynk internal error (P8.5): `body`'s own `id` and `inputs` disagree on \
                 whether this definition is a function or a handler"
            )
        }
    };

    Body {
        id,
        ty,
        expr_types,
        errors,
        refs,
        hints,
        locals,
        requirements,
        callees,
    }
}

/// R3.13's `TypeOf(DefId)`. `None` for a [`DefId::Handler`] — see
/// [DECISION A]/[DECISION D] (this module's own doc comment).
pub fn type_of(id: DefId, inputs: FnBodyInputs<'_>, ctx: QueryCtx<'_>) -> Option<TypeOf> {
    match id {
        DefId::Fn(_) => {
            let result = body(id, BodyInputs::Fn(inputs), ctx);
            Some(TypeOf {
                id: result.id,
                ty: result.ty,
            })
        }
        DefId::Handler(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker;
    use crate::resolver;
    use bynk_syntax::ast::{BaseType, Commons, CommonsItem, FnDecl, FnName, SourceUnit};
    use bynk_syntax::{lexer, parser};
    use std::path::PathBuf;

    /// Parse+resolve+check a whole single-file `commons` unit — the same
    /// `lexer::tokenize`/`parser::parse_with_warnings`/`resolver::resolve`/
    /// `checker::check` pipeline `bynk-check/tests/callee_classification.rs`
    /// already uses for its own fixtures.
    fn checked_commons(source: &str) -> checker::TypedCommons {
        let tokens = lexer::tokenize(source).expect("lex");
        let (commons, _warnings) = parser::parse_with_warnings(&tokens, source).expect("parse");
        let resolved = resolver::resolve(commons).expect("resolve");
        checker::check(resolved).expect("check")
    }

    /// `capability`/`provides` are context-only (parser-enforced, not just
    /// checker-enforced — `bynk.capability.outside_context` fires during
    /// parsing itself). Mirrors `context_checks.rs`'s own
    /// `checked_context_commons` test helper's `Context` → `Commons`
    /// conversion, minus the `services`/`actors` table this module's own
    /// provider-op fixture doesn't need.
    fn checked_context(source: &str) -> checker::TypedCommons {
        let tokens = lexer::tokenize(source).expect("lex");
        let unit = parser::parse_unit(&tokens, source).expect("parse");
        let SourceUnit::Context(ctx) = unit else {
            panic!("expected a context unit, got {unit:?}")
        };
        let commons = Commons {
            name: ctx.name,
            items: ctx.items,
            uses: ctx.uses,
            documentation: ctx.documentation,
            form: ctx.form,
            span: ctx.span,
            trivia: ctx.trivia,
            trailing_comments: ctx.trailing_comments,
        };
        let resolved = resolver::resolve(commons).expect("resolve");
        checker::check(resolved).expect("check")
    }

    /// Rebuilds a [`ResolvedCommons`] from an already-`TypedCommons` fixture
    /// — the same "check once to get a real, resolved unit; re-wrap its own
    /// tables into a fresh `ResolvedCommons` for a second, isolated check"
    /// shape `test_suites.rs`'s own `check_body` call sites already use.
    fn resolved_from(typed: &checker::TypedCommons) -> ResolvedCommons {
        let no_events = HashMap::new();
        ResolvedCommons::new(
            typed.commons.clone(),
            typed.types.clone(),
            &typed.types,
            typed.fns.clone(),
            typed.methods.clone(),
            HashMap::new(),
            &no_events,
            resolver::CrossContextInfo::default(),
            HashMap::new(),
            false,
            Default::default(),
        )
    }

    fn find_fn<'a>(typed: &'a checker::TypedCommons, name: &str) -> &'a FnDecl {
        typed
            .commons
            .items
            .iter()
            .find_map(|item| match item {
                CommonsItem::Fn(f) if matches!(&f.name, FnName::Free(id) if id.name == name) => {
                    Some(f)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no free function named `{name}` in this fixture"))
    }

    /// [`type_of`]'s own smoke test: a real free function, checked through
    /// the [`DefId::Fn`] path, produces the same answer `check_body` itself
    /// would — `TypeOf(DefId)` is a real wrapper, not a stub.
    #[test]
    fn type_of_reports_a_free_functions_own_return_type() {
        let typed = checked_commons(
            r#"
commons demo {
  fn double(x: Int) -> Int {
    x * 2
  }
}
"#,
        );
        let decl = find_fn(&typed, "double");
        let int_ty = typed.ty_intern.intern(checker::Ty::Base(BaseType::Int));
        let scope: HashMap<String, TyId> = decl
            .params
            .iter()
            .map(|p| (p.name.name.clone(), int_ty))
            .collect();

        let id = DefId::Fn(FnDefId {
            unit: UnitId("demo".to_string()),
            owner: None,
            name: "double".to_string(),
        });
        let resolved = resolved_from(&typed);
        let result = type_of(
            id,
            FnBodyInputs {
                body: &decl.body,
                return_ty: int_ty,
                return_ty_span: decl.return_type.span(),
                scope,
                caps: CapabilityCtx::default(),
                test_services: HashMap::new(),
                test_actors: HashMap::new(),
                where_pred: None,
            },
            QueryCtx {
                input: &resolved,
                tys: &typed.ty_intern,
                file: &PathBuf::from("demo.bynk"),
                muted: false,
            },
        )
        .expect("TypeOf(DefId::Fn) is always Some");

        assert_eq!(
            result.ty.map(|t| t.display(&typed.ty_intern)),
            Some("Int".to_string()),
            "double(x: Int) -> Int should check to Int, unaffected by the fresh, per-call \
             CheckSinks this query wraps `check_body` in"
        );
        assert!(matches!(result.id, DefId::Fn(_)));
    }

    /// [DECISION A]/[DECISION D]: calling `type_of` on a handler identity is
    /// a documented `None`, not a panic and not a type it happens to
    /// accept — the one place this slice's `DefId` split is enforced at
    /// runtime rather than at the type level. The `FnBodyInputs` passed
    /// here are never read (`type_of` returns before touching them); the
    /// `double` fixture's own body/return type stand in for "a real,
    /// well-typed body that just happens to be attached to the wrong kind
    /// of `DefId`" rather than a hand-rolled, possibly-invalid AST node.
    #[test]
    fn type_of_is_none_for_a_handler_def_id() {
        let typed = checked_commons(
            r#"
commons demo {
  fn double(x: Int) -> Int {
    x * 2
  }
}
"#,
        );
        let decl = find_fn(&typed, "double");
        let int_ty = typed.ty_intern.intern(checker::Ty::Base(BaseType::Int));
        let resolved = resolved_from(&typed);

        let id = DefId::Handler(HandlerDefId {
            unit: UnitId("demo".to_string()),
            owner: "Api".to_string(),
            kind: HandlerKind::Call,
            method_name: None,
        });
        let result = type_of(
            id,
            FnBodyInputs {
                body: &decl.body,
                return_ty: int_ty,
                return_ty_span: decl.return_type.span(),
                scope: HashMap::new(),
                caps: CapabilityCtx::default(),
                test_services: HashMap::new(),
                test_actors: HashMap::new(),
                where_pred: None,
            },
            QueryCtx {
                input: &resolved,
                tys: &typed.ty_intern,
                file: &PathBuf::from("demo.bynk"),
                muted: false,
            },
        );
        assert!(result.is_none());
    }

    /// #1516's own Risks section: a fresh, per-call `CheckSinks` (DECISION
    /// B) must not silently drop diagnostics a file-wide accumulation would
    /// have kept, for the one shape this slice actually exercises — a
    /// single provider op with no sibling definition in the same context to
    /// accumulate against. Checks the same provider-op body twice: once
    /// through `check_handler_body` directly against a file-wide-shaped
    /// sink bundle (mirroring `context_checks.rs::check_provider_decls`'s
    /// own call shape byte-for-byte), once through `body`'s own
    /// fresh-sink wrapper — and asserts the two runs' `errors`/
    /// `expr_types` agree.
    #[test]
    fn body_matches_check_handler_body_for_one_provider_op_in_isolation() {
        let source = r#"
context demo {
  capability Greeter {
    fn greet(name: String) -> String
  }

  provides Greeter = StubGreeter {
    fn greet(name: String) -> String {
      name
    }
  }
}
"#;
        let typed = checked_context(source);
        let provider = typed
            .commons
            .items
            .iter()
            .find_map(|item| match item {
                CommonsItem::Provider(p) if p.provider_name.name == "StubGreeter" => {
                    Some(p.clone())
                }
                _ => None,
            })
            .expect("StubGreeter provider in fixture");
        let op = provider.ops.first().expect("StubGreeter has one op");
        let resolved = resolved_from(&typed);

        let handler_check = || checker::HandlerBodyCheck {
            capabilities: HashMap::new(),
            declared_capabilities: HashMap::new(),
            ..checker::HandlerBodyCheck::new(&op.body, &op.return_type, &op.params, &[])
        };

        // Run 1: file-wide-shaped sinks, mirroring `check_provider_decls`.
        let mut fw_expr_types = HashMap::new();
        let mut fw_errors = Vec::new();
        let mut fw_callees = HashMap::new();
        let mut fw_refs = RefSink::new();
        let mut fw_hints = HintSink::new();
        let mut fw_locals = LocalsSink::new();
        let mut fw_requirements = RequirementSink::new();
        fw_refs.enter_file(&PathBuf::from("demo.bynk"), "demo", false);
        fw_hints.enter_file(&PathBuf::from("demo.bynk"), false);
        fw_locals.enter_file(&PathBuf::from("demo.bynk"), false);
        fw_requirements.enter_file(&PathBuf::from("demo.bynk"), false);
        checker::check_handler_body(
            &resolved,
            handler_check(),
            CheckSinks {
                tys: &typed.ty_intern,
                expr_types: &mut fw_expr_types,
                errors: &mut fw_errors,
                refs: &mut fw_refs,
                hints: &mut fw_hints,
                locals: &mut fw_locals,
                requirements: &mut fw_requirements,
                callees: &mut fw_callees,
            },
        );

        // Run 2: this slice's own `body`, fresh sinks per call.
        let id = DefId::Handler(HandlerDefId {
            unit: UnitId("demo".to_string()),
            owner: "Greeter".to_string(),
            kind: HandlerKind::Call,
            method_name: None,
        });
        let result = body(
            id,
            BodyInputs::Handler(handler_check()),
            QueryCtx {
                input: &resolved,
                tys: &typed.ty_intern,
                file: &PathBuf::from("demo.bynk"),
                muted: false,
            },
        );

        assert_eq!(
            fw_errors.len(),
            result.errors.len(),
            "a fresh, per-call CheckSinks must report the same errors a file-wide sink would \
             for one isolated provider op: {:?} vs {:?}",
            fw_errors,
            result.errors
        );
        assert_eq!(
            fw_expr_types.len(),
            result.expr_types.len(),
            "a fresh, per-call CheckSinks must record the same number of typed expressions a \
             file-wide sink would for one isolated provider op"
        );
    }
}
