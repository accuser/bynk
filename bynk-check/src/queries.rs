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
//! function, a method, a service/agent handler, or a provider op — split
//! into [`DefId::Fn`]/[`DefId::Handler`]/[`DefId::ProviderOp`] rather than
//! one unified shape, because [`checker::check_handler_body`] returns `()`
//! while [`checker::check_body`] returns `Option<TyId>`: neither a handler
//! nor a provider op has a single value a caller reads, so `TypeOf` is a
//! type-level impossibility for either, not a documented `None` case.
//! [`HandlerDefId`] and [`ProviderOpDefId`] are two variants, not one,
//! because a provider op carries no [`HandlerKind`]/`method_name` of its
//! own — reusing `HandlerDefId`'s shape for a provider op let two ops of the
//! same provider collide on one identity (a real gap this PR's own review
//! caught, not a hypothetical). [`FnDefId`]/[`HandlerDefId`]/
//! [`ProviderOpDefId`] are all scoped by [`UnitId`] (P8.1) plus a stable,
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
//! ideal is deliberately relaxed: called on a [`DefId::Handler`] or
//! [`DefId::ProviderOp`], it returns `None` without doing any checking
//! work, documented here rather than discovered at a call site.
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

/// [DECISION A]: a **service or agent handler only** — a provider op is a
/// separate [`ProviderOpDefId`], not a `HandlerDefId` (see this module's own
/// doc comment). Identified by its owning unit, the service/agent's own
/// name, its [`HandlerKind`] (body-free, span-free as of this slice — see
/// the `Hash` derive added to it), and, for an agent handler, its own
/// `method_name` (`on call addItem(...)`'s `addItem`) — the one field
/// [`bynk_syntax::ast::Handler`]'s own doc comment already names as the
/// disambiguator multiple `on call` methods need. `None` for a service
/// handler (`kind` alone is unique within one service's protocol — an
/// assumption this type trusts rather than checks; nothing here rejects a
/// `HandlerDefId` built in violation of it).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HandlerDefId {
    pub unit: UnitId,
    pub owner: String,
    pub kind: HandlerKind,
    pub method_name: Option<String>,
}

/// [DECISION A]: a provider operation
/// (`provides Cap = Provider { fn op(...) { ... } }`) — checked via the same
/// `check_handler_body` entry point [`HandlerDefId`] is
/// (`context_checks.rs::check_provider_decls`,
/// `bynk-check/src/context_checks.rs:665`), but a distinct `DefId` variant:
/// a provider op has no `HandlerKind`/`method_name` of its own, so reusing
/// `HandlerDefId`'s shape would give every op of one provider the identical
/// identity. `provider` (the provider's own name, e.g. `StubGreeter`) is
/// the identity's own `owner` — matching `check_provider_decls`'s own
/// `refs.set_owner(&provider.provider_name.name)`, not `capability`, since a
/// capability may have more than one provider (one per adapter/binding) and
/// `capability` alone would under-identify.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderOpDefId {
    pub unit: UnitId,
    pub capability: String,
    pub provider: String,
    pub op_name: String,
}

/// R3.13's own definition-level identity: a free function, a method, a
/// service/agent handler, or a provider op. See [DECISION A] (this module's
/// own doc comment) for why this is a split enum rather than one unified
/// shape, and why handlers and provider ops are two variants rather than
/// one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DefId {
    Fn(FnDefId),
    Handler(HandlerDefId),
    ProviderOp(ProviderOpDefId),
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

/// The ref-attribution owner for `id` — mirrors `checker::check`'s own
/// `refs.set_owner(f.name.display())` (`checker.rs:910`), whose `FnName::
/// display()` renders a method as `"Type.method"`, not the bare type name.
/// An earlier version of this function used `f.owner` alone for a method
/// (just `"Money"` for `Money.add`), silently diverging from the
/// production attribution — a real gap this PR's own review caught, not a
/// hypothetical.
fn def_owner(id: &DefId) -> String {
    match id {
        DefId::Fn(f) => match &f.owner {
            Some(ty) => format!("{ty}.{}", f.name),
            None => f.name.clone(),
        },
        DefId::Handler(h) => h.owner.clone(),
        DefId::ProviderOp(p) => p.provider.clone(),
    }
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
        DefId::ProviderOp(p) => p.unit.0.as_str(),
    };
    refs.enter_file(file, namespace, muted);
    refs.set_owner(def_owner(&id));
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
        (DefId::Handler(_) | DefId::ProviderOp(_), BodyInputs::Handler(check)) => {
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
        (DefId::Fn(_), BodyInputs::Handler(_))
        | (DefId::Handler(_) | DefId::ProviderOp(_), BodyInputs::Fn(_)) => {
            panic!(
                "bynk internal error (P8.5): `body`'s own `id` and `inputs` disagree on \
                 whether this definition is a function, a handler, or a provider op"
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

/// R3.13's `TypeOf(DefId)`. `None` for a [`DefId::Handler`] or
/// [`DefId::ProviderOp`] — see [DECISION A]/[DECISION D] (this module's own
/// doc comment).
pub fn type_of(id: DefId, inputs: FnBodyInputs<'_>, ctx: QueryCtx<'_>) -> Option<TypeOf> {
    match id {
        DefId::Fn(_) => {
            let result = body(id, BodyInputs::Fn(inputs), ctx);
            Some(TypeOf {
                id: result.id,
                ty: result.ty,
            })
        }
        DefId::Handler(_) | DefId::ProviderOp(_) => None,
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

    /// Regression for this PR's own review: `def_owner` for a method
    /// `DefId::Fn` must render `"Type.method"`, matching `checker::check`'s
    /// own `refs.set_owner(f.name.display())` (`checker.rs:910`) — not the
    /// bare type name. No fixture-based test can observe this through
    /// `body`'s own public surface (`check_body`'s callers pass an
    /// already-resolved `return_ty: TyId`, never a raw `TypeRef`, so its own
    /// `RefSink` stays empty regardless of `owner`'s value — the divergence
    /// is latent, not visible in `expr_types`/`errors`), so this tests
    /// `def_owner` directly rather than fabricate a fixture that cannot
    /// actually exercise the bug.
    #[test]
    fn def_owner_renders_a_method_as_type_dot_method() {
        let id = DefId::Fn(FnDefId {
            unit: UnitId("demo".to_string()),
            owner: Some("Money".to_string()),
            name: "add".to_string(),
        });
        assert_eq!(def_owner(&id), "Money.add");
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
        // Two ops, not one: `greet` is the sibling checked first (so the
        // file-wide run's sinks are genuinely non-empty when `broken` is
        // checked — a lone op would make the file-wide and fresh-sink runs
        // identical by construction, a real gap this PR's own review
        // caught). `broken` genuinely mistypes its return (`1`, not a
        // `String`), so the errors comparison below is not `0 == 0`.
        let source = r#"
context demo {
  capability Greeter {
    fn greet(name: String) -> String
    fn broken(name: String) -> String
  }

  provides Greeter = StubGreeter {
    fn greet(name: String) -> String {
      let greeting = name
      greeting
    }

    fn broken(name: String) -> String {
      1
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
        let greet_op = provider
            .ops
            .iter()
            .find(|op| op.name.name == "greet")
            .expect("greet op in fixture");
        let broken_op = provider
            .ops
            .iter()
            .find(|op| op.name.name == "broken")
            .expect("broken op in fixture");
        let resolved = resolved_from(&typed);
        let demo_path = PathBuf::from("demo.bynk");

        // A local `fn`, not a closure: `HandlerBodyCheck`'s return borrows
        // from `op`, a lifetime pattern a closure can't infer here.
        fn handler_check(op: &bynk_syntax::ast::ProviderOp) -> checker::HandlerBodyCheck<'_> {
            checker::HandlerBodyCheck {
                capabilities: HashMap::new(),
                declared_capabilities: HashMap::new(),
                ..checker::HandlerBodyCheck::new(&op.body, &op.return_type, &op.params, &[])
            }
        }
        let provider_op_id = |op_name: &str| {
            DefId::ProviderOp(ProviderOpDefId {
                unit: UnitId("demo".to_string()),
                capability: "Greeter".to_string(),
                provider: "StubGreeter".to_string(),
                op_name: op_name.to_string(),
            })
        };
        let query_ctx = || QueryCtx {
            input: &resolved,
            tys: &typed.ty_intern,
            file: &demo_path,
            muted: false,
        };

        // Both ops via this slice's own `body`, each with a fresh sink.
        // Destructured immediately into independent owned bindings so
        // nothing below has to reason about partial moves out of `Body`.
        let Body {
            expr_types: greet_expr_types,
            errors: greet_errors,
            refs: greet_refs,
            hints: mut greet_hints,
            locals: mut greet_locals,
            requirements: mut greet_requirements,
            ..
        } = body(
            provider_op_id("greet"),
            BodyInputs::Handler(handler_check(greet_op)),
            query_ctx(),
        );
        let Body {
            expr_types: broken_expr_types,
            errors: broken_errors,
            refs: broken_refs,
            hints: mut broken_hints,
            locals: mut broken_locals,
            requirements: mut broken_requirements,
            ..
        } = body(
            provider_op_id("broken"),
            BodyInputs::Handler(handler_check(broken_op)),
            query_ctx(),
        );
        assert!(
            !broken_errors.is_empty(),
            "the `broken` op returns `1` from a `String`-returning op — this fixture is only a \
             real test of error-dropping if that actually produces a diagnostic"
        );

        // File-wide-shaped sinks, mirroring `check_provider_decls`'s own
        // `for op in &provider.ops` loop: one `enter_file`/`set_owner` for
        // the whole provider, `greet` (the sibling) checked first, `broken`
        // (the op under test) checked second into the SAME accumulated
        // sinks — never drained in between.
        let mut fw_expr_types = HashMap::new();
        let mut fw_errors = Vec::new();
        let mut fw_callees = HashMap::new();
        let mut fw_refs = RefSink::new();
        let mut fw_hints = HintSink::new();
        let mut fw_locals = LocalsSink::new();
        let mut fw_requirements = RequirementSink::new();
        fw_refs.enter_file(&demo_path, "demo", false);
        fw_refs.set_owner("StubGreeter");
        fw_hints.enter_file(&demo_path, false);
        fw_locals.enter_file(&demo_path, false);
        fw_requirements.enter_file(&demo_path, false);
        for op in [greet_op, broken_op] {
            checker::check_handler_body(
                &resolved,
                handler_check(op),
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
        }

        // `expr_types`/`callees`: plain `ExprId`-keyed maps, so a length
        // comparison (no accumulation semantics to isolate — see this
        // module's own doc comment) is the right check.
        assert_eq!(
            fw_expr_types.len(),
            greet_expr_types.len() + broken_expr_types.len(),
            "a fresh, per-call CheckSinks must record the same number of typed expressions a \
             file-wide sink accumulating both ops would"
        );

        // `errors`: content, not just length — two different errors at the
        // same count must not pass. `Vec` order is deterministic (insertion
        // order, no `HashMap` involved), so a `Debug`-string comparison of
        // the combined order is safe.
        let expected_errors = format!("{:?}", [greet_errors, broken_errors].concat());
        assert_eq!(
            format!("{fw_errors:?}"),
            expected_errors,
            "a fresh, per-call CheckSinks must report the same diagnostics (content, not just \
             count) a file-wide sink accumulating both ops would"
        );

        // `refs`: the sink DECISION B's own "records nothing until
        // `enter_file` attributes it" argument is about, and the one this
        // PR's own review found silently unchecked (a provider-op owner
        // mismatch would have been invisible here). Neither op body
        // references a named type, so both sides are typically empty — but
        // the comparison is exercised, not skipped, and would catch an
        // owner/attribution regression the moment either op's body does
        // reference one.
        let expected_refs = format!("{:?}", [greet_refs.edges, broken_refs.edges].concat());
        assert_eq!(
            format!("{:?}", fw_refs.edges),
            expected_refs,
            "a fresh, per-call CheckSinks must record the same ref edges (including `owner`) a \
             file-wide sink accumulating both ops would"
        );

        // `hints`/`locals`/`requirements`: each keyed by file path; extract
        // this fixture's one file's own `Vec` and compare content.
        let combined_hints = format!(
            "{:?}",
            fw_hints.take_files().remove(&demo_path).unwrap_or_default()
        );
        let expected_hints = format!(
            "{:?}",
            concat_file_entries(
                greet_hints.take_files(),
                broken_hints.take_files(),
                &demo_path
            )
        );
        assert_eq!(
            combined_hints, expected_hints,
            "a fresh, per-call CheckSinks must record the same inlay hints a file-wide sink \
             accumulating both ops would"
        );

        let combined_locals = format!(
            "{:?}",
            fw_locals
                .take_files()
                .remove(&demo_path)
                .unwrap_or_default()
        );
        let expected_locals = format!(
            "{:?}",
            concat_file_entries(
                greet_locals.take_files(),
                broken_locals.take_files(),
                &demo_path
            )
        );
        assert_eq!(
            combined_locals, expected_locals,
            "a fresh, per-call CheckSinks must record the same local bindings a file-wide sink \
             accumulating both ops would"
        );

        let combined_requirements = format!(
            "{:?}",
            fw_requirements
                .take_files()
                .remove(&demo_path)
                .unwrap_or_default()
        );
        let expected_requirements = format!(
            "{:?}",
            concat_file_entries(
                greet_requirements.take_files(),
                broken_requirements.take_files(),
                &demo_path
            )
        );
        assert_eq!(
            combined_requirements, expected_requirements,
            "a fresh, per-call CheckSinks must record the same capability requirements a \
             file-wide sink accumulating both ops would"
        );
    }

    /// Concatenates one file's own entries from two separately-drained
    /// per-file maps, in call order — the shape [`body_matches_check_handler_body_for_one_provider_op_in_isolation`]
    /// needs to build "what the sibling-then-target op would have produced,
    /// combined" from two independent fresh-sink runs.
    fn concat_file_entries<T: Clone>(
        mut a: HashMap<PathBuf, Vec<T>>,
        mut b: HashMap<PathBuf, Vec<T>>,
        file: &PathBuf,
    ) -> Vec<T> {
        let mut combined = a.remove(file).unwrap_or_default();
        combined.extend(b.remove(file).unwrap_or_default());
        combined
    }
}
