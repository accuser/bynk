//! Type checker and refinement validator (spec §§5–6, v0.1 §4.2, v0.2 §4.2).
//!
//! Operates on a [`ResolvedCommons`]. Walks declarations, validates each
//! refinement against the spec's predicate-base compatibility and combination
//! rules, then type-checks every function and method body.
//!
//! v0.2 extensions:
//! - Record types (compatibility, field access, construction).
//! - Sum types and variant construction (qualified and unqualified).
//! - Methods (instance and static) with UFCS-style call resolution.
//! - Pattern matching with exhaustiveness checking.
//! - The `is` operator with binding flow into truthy contexts.
//! - The built-in generic `Option[T]`.

use std::collections::{HashMap, HashSet};

use regex::Regex;

use crate::ast::*;
use crate::builtin_names::methods::*;
use crate::builtin_names::types::*;
use crate::error::{Applicability, CompileError};
use crate::hints::HintSink;
use crate::index::{RefSink, SymbolKind};
use crate::resolver::{MethodTable, ResolvedCommons};
use crate::span::Span;

/// A resolved type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// A base type (`Int`, `String`, `Bool`).
    Base(BaseType),
    /// A user-declared named type. `kind` records the declaration's shape
    /// for compatibility / dispatch decisions.
    Named { name: String, kind: NamedKind },
    /// `Result[T, E]`.
    Result(Box<Ty>, Box<Ty>),
    /// `Option[T]`.
    Option(Box<Ty>),
    /// `Effect[T]` (v0.5).
    Effect(Box<Ty>),
    /// `HttpResult[T]` (v0.9).
    HttpResult(Box<Ty>),
    /// `List[T]` — built-in immutable list (v0.20b).
    List(Box<Ty>),
    /// `Map[K, V]` — built-in immutable map (v0.20b). The key type is
    /// confined to value-keyable types at TypeRef resolution.
    Map(Box<Ty>, Box<Ty>),
    /// `ValidationError` — built-in error type.
    ValidationError,
    /// `JsonError` — built-in JSON-decode error type (v0.22b). A uniform
    /// record: `kind`/`path`/`message`, all `String`.
    JsonError,
    /// `()` — the unit type (v0.5).
    Unit,
    /// `A -> B` — a function type (v0.20a). Effectful iff `ret` is
    /// `Effect[_]` (the structural rule); no separate flag, so there is a
    /// single source of truth.
    Fn { params: Vec<Ty>, ret: Box<Ty> },
    /// A function type parameter (v0.20a). Two lives: *rigid* while checking
    /// a generic function's own body (name-equality in `compatible`), and
    /// *flexible* during call-site instantiation, where it is matched by
    /// `unify` and fully eliminated by `substitute` before any `compatible`
    /// runs against argument types. Vars never escape call checking into the
    /// caller's expression types.
    Var(String),
}

/// The shape of a named type — what its declaration looks like.
///
/// `Refined` widens to its base type when used in arithmetic, comparisons,
/// and other operations on the base. `Opaque` does NOT widen — its identity
/// is nominal and the base type is hidden outside the defining commons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedKind {
    /// Refined-base type: widens to the recorded base.
    Refined(BaseType),
    /// Record type.
    Record,
    /// Sum type.
    Sum,
    /// Opaque base type. The base is hidden; identity is purely nominal.
    /// The recorded base is used by the type checker (for `.raw`, `.of`,
    /// `.unsafe`) and by the emitter, but not for compatibility widening.
    Opaque(BaseType),
}

impl Ty {
    /// Display name for diagnostics.
    pub fn display(&self) -> String {
        match self {
            Ty::Base(b) => b.name().to_string(),
            Ty::Named { name, .. } => name.clone(),
            Ty::Result(t, e) => format!("Result[{}, {}]", t.display(), e.display()),
            Ty::Option(t) => format!("Option[{}]", t.display()),
            Ty::Effect(t) => format!("Effect[{}]", t.display()),
            Ty::HttpResult(t) => format!("HttpResult[{}]", t.display()),
            Ty::List(t) => format!("List[{}]", t.display()),
            Ty::Map(k, v) => format!("Map[{}, {}]", k.display(), v.display()),
            Ty::ValidationError => "ValidationError".to_string(),
            Ty::JsonError => "JsonError".to_string(),
            Ty::Unit => "()".to_string(),
            Ty::Fn { params, ret } => {
                let params = match params.len() {
                    0 => "()".to_string(),
                    // A single Fn-typed param needs parens to stay readable
                    // under right-associativity.
                    1 if !matches!(params[0], Ty::Fn { .. }) => params[0].display(),
                    _ => format!(
                        "({})",
                        params
                            .iter()
                            .map(|p| p.display())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                };
                format!("{params} -> {}", ret.display())
            }
            Ty::Var(name) => name.clone(),
        }
    }

    /// True if this type is `Effect[_]`.
    pub fn is_effect(&self) -> bool {
        matches!(self, Ty::Effect(_))
    }

    /// The underlying base type, if this type widens to a base type.
    /// Opaque types deliberately do NOT widen — that's the whole point of
    /// the opacity — so `Ty::Named { kind: Opaque(_), .. }` returns None.
    pub(crate) fn base(&self) -> Option<BaseType> {
        match self {
            Ty::Base(b) => Some(*b),
            Ty::Named {
                kind: NamedKind::Refined(b),
                ..
            } => Some(*b),
            _ => None,
        }
    }
}

/// Output of type checking.
pub struct TypedCommons {
    pub commons: Commons,
    pub types: HashMap<String, TypeDecl>,
    pub fns: HashMap<String, FnDecl>,
    pub methods: HashMap<String, MethodTable>,
    pub expr_types: HashMap<Span, Ty>,
}

pub fn check(input: ResolvedCommons) -> Result<TypedCommons, Vec<CompileError>> {
    check_record(input, &mut RefSink::new(), &mut HintSink::new())
}

/// [`check`], recording binding edges into `refs` at the checker's
/// resolution sites (v0.25). A fresh sink records nothing.
pub fn check_record(
    input: ResolvedCommons,
    refs: &mut RefSink,
    hints: &mut HintSink,
) -> Result<TypedCommons, Vec<CompileError>> {
    let mut errors = Vec::new();
    let mut expr_types: HashMap<Span, Ty> = HashMap::new();

    // 1. Validate each type declaration.
    for item in &input.commons.items {
        if let CommonsItem::Type(t) = item {
            check_type_decl(t, &input.types, &mut errors);
        }
    }

    // 2. Type-check each function and method body.
    for item in &input.commons.items {
        if let CommonsItem::Fn(f) = item {
            refs.set_owner(f.name.display());
            check_fn(f, &input, &mut expr_types, &mut errors, refs, hints);
            refs.clear_owner();
        }
    }

    if errors.is_empty() {
        Ok(TypedCommons {
            commons: input.commons,
            types: input.types,
            fns: input.fns,
            methods: input.methods,
            expr_types,
        })
    } else {
        Err(errors)
    }
}

/// Check a single handler body (used for service and agent handlers).
///
/// `capabilities_in_scope` is the set of capabilities the handler may
/// reference. `agent_state_ty` is set when checking an agent handler — it
/// determines the type the `commit` statement must produce.
#[allow(clippy::too_many_arguments)]
pub fn check_handler_body(
    body: &Block,
    return_type: &TypeRef,
    return_ty_span: Span,
    params: &[Param],
    input: &ResolvedCommons,
    expr_types: &mut HashMap<Span, Ty>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
    hints: &mut HintSink,
    capabilities: HashMap<String, CapabilityInfo>,
    declared_capabilities: HashMap<String, CapabilityInfo>,
    agent_state_ty: Option<Ty>,
    agent_self_scope: Option<HashMap<String, Ty>>,
    given: &[CapRef],
    given_anchor: Option<Span>,
    report_unused: bool,
) {
    let Some(return_ty) = resolve_type_ref(return_type, &input.types) else {
        return;
    };
    let no_vars = HashSet::new();
    record_type_refs(return_type, &input.types, &no_vars, refs);
    // Build the parameter scope.
    let mut param_scope: HashMap<String, Ty> = HashMap::new();
    for p in params {
        if let Some(t) = resolve_type_ref(&p.type_ref, &input.types) {
            record_type_refs(&p.type_ref, &input.types, &no_vars, refs);
            param_scope.insert(p.name.name.clone(), t);
        }
    }
    if let Some(self_scope) = agent_self_scope {
        param_scope.extend(self_scope);
    }
    let effectful = matches!(&return_ty, Ty::Effect(_));
    let given_entries: Vec<(String, Span)> = given
        .iter()
        .map(|c| (c.key().to_string(), c.span))
        .collect();
    let given_remaining: HashSet<String> = given_entries.iter().map(|(k, _)| k.clone()).collect();
    let mut ctx = Ctx {
        input,
        expr_types,
        errors,
        refs,
        hints,
        scopes: vec![param_scope],
        return_ty: return_ty.clone(),
        return_ty_span,
        effectful,
        agent_state_ty,
        commit_seen: false,
        capabilities,
        declared_capabilities,
        given_remaining,
        given_used: HashSet::new(),
        given_entries: given_entries.clone(),
        given_anchor,
        in_test_body: false,
        test_services: HashSet::new(),
        type_vars: HashSet::new(),
    };
    // Check the body and validate it matches the return type.
    let Some(body_ty) = type_of_block(body, Some(&return_ty), &mut ctx) else {
        return;
    };
    if !compatible(&body_ty, &return_ty) {
        ctx.errors.push(
            CompileError::new(
                "karn.types.return_mismatch",
                body.tail.span,
                format!(
                    "handler body has type `{}`, but the declared return type is `{}`",
                    body_ty.display(),
                    return_ty.display()
                ),
            )
            .with_label(return_ty_span, "declared return type"),
        );
    }
    // Bidirectional `given` check.
    // 1) Every used capability is declared. (Handled in capability-call site.)
    // 2) Every declared capability is used — anything left in given_remaining
    //    minus given_used is unused. Emit as a warning-category error so the
    //    test harness can match it. Entries are walked in declaration order
    //    (deduplicated by key) so diagnostics and their fixes are stable.
    let mut reported: HashSet<&str> = HashSet::new();
    for (i, (c, _)) in given_entries.iter().enumerate() {
        if !report_unused {
            break;
        }
        if ctx.given_used.contains(c) || !reported.insert(c) {
            continue;
        }
        ctx.errors.push(
            CompileError::new(
                "karn.given.unused_capability",
                return_ty_span,
                format!("capability `{c}` is declared in `given` but never used in the body"),
            )
            .with_note(
                "remove the capability from the `given` clause, or use it in the handler body",
            )
            // v0.26 (ADR 0054): the removal is list-aware — only `report_unused`
            // sites are handlers, where the clause follows the return type, so
            // `return_ty_span` anchors the only-entry case.
            .with_suggestion(
                format!("remove `{c}` from the `given` clause"),
                vec![(
                    given_removal_span(&given_entries, i, return_ty_span),
                    String::new(),
                )],
                Applicability::MachineApplicable,
            ),
        );
    }
}

/// v0.26 (ADR 0054): the deletion span for `given` entry `i`, list-aware so
/// the result never double-commas, leading-commas, or leaves `given ,`:
/// an entry with a successor deletes through the successor's start
/// (`C1, `); a final entry deletes from its predecessor's end (`, C2`); the
/// only entry deletes from the return type's end — the `given` keyword goes
/// with it (no dangling `given`).
fn given_removal_span(entries: &[(String, Span)], i: usize, return_ty_span: Span) -> Span {
    if entries.len() == 1 {
        Span::new(return_ty_span.end, entries[0].1.end)
    } else if i + 1 < entries.len() {
        Span::new(entries[i].1.start, entries[i + 1].1.start)
    } else {
        Span::new(entries[i - 1].1.end, entries[i].1.end)
    }
}

/// v0.26 (ADR 0054): the insertion edit that adds `name` to the `given`
/// clause — `, name` after the last entry, or ` given name` synthesised at
/// the anchor (the handler's return type) when the clause is absent. `None`
/// when there is no clause and no sound anchor (provider bodies — their
/// clause lives on the `provides` line, not at the op's return type).
fn given_insertion_edit(
    entries: &[(String, Span)],
    anchor: Option<Span>,
    name: &str,
) -> Option<(Span, String)> {
    if let Some((_, last)) = entries.last() {
        Some((Span::new(last.end, last.end), format!(", {name}")))
    } else {
        anchor.map(|a| (Span::new(a.end, a.end), format!(" given {name}")))
    }
}

// -- type-declaration validation --

fn check_type_decl(
    t: &TypeDecl,
    types: &HashMap<String, TypeDecl>,
    errors: &mut Vec<CompileError>,
) {
    match &t.body {
        TypeBody::Refined {
            base,
            base_span,
            refinement,
        } => {
            check_refinement(*base, *base_span, refinement.as_ref(), errors);
        }
        TypeBody::Opaque {
            base,
            base_span,
            refinement,
        } => {
            // Opaque types share refinement-validity rules with refined types.
            check_refinement(*base, *base_span, refinement.as_ref(), errors);
        }
        TypeBody::Record(r) => {
            for f in &r.fields {
                if let Some(ref_r) = &f.refinement {
                    // Inline refinements on fields must apply to the field's base type.
                    if let Some(b) = field_base_type(&f.type_ref, types) {
                        check_refinement(b, f.type_ref.span(), Some(ref_r), errors);
                    } else {
                        errors.push(CompileError::new(
                            "karn.types.field_refinement_not_base",
                            ref_r.span,
                            format!(
                                "inline refinement on field `{}` requires a base or refined type",
                                f.name.name
                            ),
                        ));
                    }
                }
            }
        }
        TypeBody::Sum(_) => {
            // No further per-variant checks at the type level.
        }
    }
}

/// The base type of a field's type-ref (chasing through named refined types).
fn field_base_type(r: &TypeRef, types: &HashMap<String, TypeDecl>) -> Option<BaseType> {
    match r {
        TypeRef::Base(b, _) => Some(*b),
        TypeRef::Named(id) => match types.get(&id.name).map(|t| &t.body) {
            Some(TypeBody::Refined { base, .. }) => Some(*base),
            _ => None,
        },
        _ => None,
    }
}

/// The implicit base type of a TypeDecl whose constructor would be `T.of`:
/// Refined and Opaque types alike share the `of(base) -> Result[T, _]` shape.
/// Returns None for record / sum types.
fn type_decl_base(decl: &TypeDecl) -> Option<BaseType> {
    match &decl.body {
        TypeBody::Refined { base, .. } => Some(*base),
        TypeBody::Opaque { base, .. } => Some(*base),
        _ => None,
    }
}

/// The refinement attached to a refined or opaque type declaration, if any.
fn type_decl_refinement(decl: &TypeDecl) -> Option<&Refinement> {
    match &decl.body {
        TypeBody::Refined { refinement, .. } | TypeBody::Opaque { refinement, .. } => {
            refinement.as_ref()
        }
        _ => None,
    }
}

/// v0.9.4: a compile-time-constant literal usable for static refinement
/// discharge during `T.of(...)` construction.
enum ConstLit {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Unit,
}

impl ConstLit {
    fn display(&self) -> String {
        match self {
            ConstLit::Int(n) => n.to_string(),
            ConstLit::Float(v) => v.to_string(),
            ConstLit::Str(s) => format!("{s:?}"),
            ConstLit::Bool(b) => b.to_string(),
            ConstLit::Unit => "()".to_string(),
        }
    }
}

/// Extract a compile-time literal from an expression, if it is one v0.9.4's
/// static refinement check accepts: an int/string/bool/unit literal, or a unary
/// minus applied directly to an int literal. Anything else (arithmetic, idents,
/// calls) is not statically evaluated and keeps the runtime `Result` path.
fn const_literal(e: &Expr) -> Option<ConstLit> {
    match &e.kind {
        ExprKind::IntLit(n) => Some(ConstLit::Int(*n)),
        ExprKind::FloatLit { value, .. } => Some(ConstLit::Float(*value)),
        ExprKind::StrLit(s) => Some(ConstLit::Str(s.clone())),
        ExprKind::BoolLit(b) => Some(ConstLit::Bool(*b)),
        ExprKind::UnitLit => Some(ConstLit::Unit),
        ExprKind::UnaryOp(UnaryOp::Neg, inner) => match &inner.kind {
            ExprKind::IntLit(n) => Some(ConstLit::Int(n.checked_neg()?)),
            ExprKind::FloatLit { value, .. } => Some(ConstLit::Float(-*value)),
            _ => None,
        },
        _ => None,
    }
}

/// Evaluate a single predicate against a constant literal. A predicate whose
/// expected base type doesn't match the literal (e.g. a length predicate on an
/// int) returns `true` here — the base/predicate mismatch is a declaration-time
/// error reported by `check_refinement`, not a construction concern. String
/// length is measured in Unicode scalar values, which agrees with JS `.length`
/// for the BMP (the range fixtures use ASCII).
fn eval_predicate(pred: &PredKind, lit: &ConstLit) -> bool {
    match (pred, lit) {
        (PredKind::NonNegative, ConstLit::Int(n)) => *n >= 0,
        (PredKind::Positive, ConstLit::Int(n)) => *n > 0,
        (PredKind::InRange(lo, hi), ConstLit::Int(n)) => *lo <= *n && *n <= *hi,
        (PredKind::NonNegative, ConstLit::Float(v)) => *v >= 0.0,
        (PredKind::Positive, ConstLit::Float(v)) => *v > 0.0,
        (PredKind::InRangeF(lo, hi), ConstLit::Float(v)) => lo.value <= *v && *v <= hi.value,
        (PredKind::MinLength(k), ConstLit::Str(s)) => s.chars().count() as i64 >= *k,
        (PredKind::MaxLength(k), ConstLit::Str(s)) => (s.chars().count() as i64) <= *k,
        (PredKind::Length(k), ConstLit::Str(s)) => s.chars().count() as i64 == *k,
        (PredKind::NonEmpty, ConstLit::Str(s)) => !s.is_empty(),
        (PredKind::Matches(pat), ConstLit::Str(s)) => Regex::new(&format!("^(?:{pat})$"))
            .map(|re| re.is_match(s))
            .unwrap_or(false),
        _ => true,
    }
}

/// The first predicate the literal fails, or `None` if it satisfies them all.
fn first_failed_predicate<'a>(refinement: &'a Refinement, lit: &ConstLit) -> Option<&'a PredKind> {
    for p in &refinement.predicates {
        if !eval_predicate(&p.kind, lit) {
            return Some(&p.kind);
        }
    }
    None
}

fn literal_matches_base(lit: &ConstLit, base: BaseType) -> bool {
    matches!(
        (lit, base),
        (ConstLit::Int(_), BaseType::Int)
            | (ConstLit::Str(_), BaseType::String)
            | (ConstLit::Bool(_), BaseType::Bool)
            | (ConstLit::Float(_), BaseType::Float)
    )
}

/// v0.9.4: expected-type-directed literal admission. When a position expects a
/// **refined** type `T` and `expr` is a compile-time literal of `T`'s base, the
/// literal takes the type `T` directly (the emitter lowers it to
/// `T.unsafe(...)`); a literal that violates the refinement is a compile error.
/// Returns `None` when no refined type is expected (so the caller keeps the
/// literal's base type) — `.of` remains the only constructor for runtime values.
/// Opaque types are intentionally excluded: their representation is hidden, so
/// they are still built via `T.of(...)`.
fn admit_refined_literal(expr: &Expr, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    let Some(Ty::Named {
        name,
        kind: NamedKind::Refined(base),
    }) = expected
    else {
        return None;
    };
    let lit = const_literal(expr)?;
    if !literal_matches_base(&lit, *base) {
        return None;
    }
    let decl = ctx.input.types.get(name)?.clone();
    if let Some(refinement) = type_decl_refinement(&decl)
        && let Some(failed) = first_failed_predicate(refinement, &lit)
    {
        ctx.errors.push(CompileError::new(
            "karn.refine.literal_violates",
            expr.span,
            format!(
                "literal {} does not satisfy `{}` required by type `{}`",
                lit.display(),
                failed.name(),
                name
            ),
        ));
    }
    Some(named_ty(&decl))
}

fn check_refinement(
    base: BaseType,
    base_span: Span,
    refinement: Option<&Refinement>,
    errors: &mut Vec<CompileError>,
) {
    let Some(refinement) = refinement else {
        return;
    };

    for pred in &refinement.predicates {
        if !pred_applies_to(&pred.kind, base) {
            // v0.21: `InRange` bounds must match the numeric base type —
            // `Float where InRange(0, 1)` is the no-coercion rule applied
            // to refinement bounds, not a predicate/base mismatch.
            let numeric_bound_mismatch = matches!(
                (&pred.kind, base),
                (PredKind::InRange(_, _), BaseType::Float)
                    | (PredKind::InRangeF(_, _), BaseType::Int)
            );
            if numeric_bound_mismatch {
                let (bounds, want) = if base == BaseType::Float {
                    ("`Int`", "`InRange(0.0, 1.0)`")
                } else {
                    ("`Float`", "`InRange(0, 1)`")
                };
                errors.push(
                    CompileError::new(
                        "karn.types.no_numeric_coercion",
                        pred.span,
                        format!(
                            "`InRange` bounds are {bounds} literals, but the base type is `{}`",
                            base.name()
                        ),
                    )
                    .with_label(
                        base_span,
                        format!("base type `{}` declared here", base.name()),
                    )
                    .with_note(format!(
                        "refinement bounds must match the base type — e.g. {want}"
                    )),
                );
                continue;
            }
            errors.push(
                CompileError::new(
                    "karn.types.predicate_base_mismatch",
                    pred.span,
                    format!(
                        "predicate `{}` cannot be applied to base type `{}`",
                        pred.kind.name(),
                        base.name()
                    ),
                )
                .with_label(
                    base_span,
                    format!("base type `{}` declared here", base.name()),
                )
                .with_note(predicate_base_help(pred.kind.name())),
            );
        }
        match &pred.kind {
            PredKind::Matches(pat) => {
                if let Err(e) = Regex::new(pat) {
                    errors.push(
                        CompileError::new(
                            "karn.types.invalid_regex",
                            pred.span,
                            format!("invalid regular expression in `Matches(\"{pat}\")`"),
                        )
                        .with_note(format!("regex parse error: {e}")),
                    );
                }
            }
            PredKind::InRange(lo, hi) => {
                if lo > hi {
                    errors.push(
                        CompileError::new(
                            "karn.types.inverted_range",
                            pred.span,
                            format!(
                                "`InRange({lo}, {hi})` has its bounds inverted (`min` must be ≤ `max`)"
                            ),
                        )
                        .with_note("swap the arguments, e.g. `InRange(min, max)`"),
                    );
                }
            }
            PredKind::InRangeF(lo, hi) => {
                if lo.value > hi.value {
                    errors.push(
                        CompileError::new(
                            "karn.types.inverted_range",
                            pred.span,
                            format!(
                                "`InRange({}, {})` has its bounds inverted (`min` must be ≤ `max`)",
                                lo.lexeme, hi.lexeme
                            ),
                        )
                        .with_note("swap the arguments, e.g. `InRange(min, max)`"),
                    );
                }
            }
            PredKind::MinLength(n) | PredKind::MaxLength(n) | PredKind::Length(n) => {
                if *n < 0 {
                    errors.push(CompileError::new(
                        "karn.types.negative_length",
                        pred.span,
                        format!("length argument must be non-negative, got {n}"),
                    ));
                }
            }
            PredKind::NonNegative | PredKind::Positive | PredKind::NonEmpty => {}
        }
    }

    let all_compatible = refinement
        .predicates
        .iter()
        .all(|p| pred_applies_to(&p.kind, base));
    if !all_compatible {
        return;
    }
    match base {
        BaseType::Int => check_int_refinement_consistency(refinement, errors),
        BaseType::String => check_string_refinement_consistency(refinement, errors),
        BaseType::Bool => {}
        BaseType::Float => check_float_refinement_consistency(refinement, errors),
    }
}

fn pred_applies_to(pred: &PredKind, base: BaseType) -> bool {
    matches!(
        (pred, base),
        (PredKind::Matches(_), BaseType::String)
            | (PredKind::InRange(_, _), BaseType::Int)
            | (PredKind::InRangeF(_, _), BaseType::Float)
            | (PredKind::MinLength(_), BaseType::String)
            | (PredKind::MaxLength(_), BaseType::String)
            | (PredKind::Length(_), BaseType::String)
            | (PredKind::NonNegative, BaseType::Int | BaseType::Float)
            | (PredKind::Positive, BaseType::Int | BaseType::Float)
            | (PredKind::NonEmpty, BaseType::String)
    )
}

fn predicate_base_help(name: &str) -> &'static str {
    match name {
        "Matches" | "MinLength" | "MaxLength" | "Length" | "NonEmpty" => {
            "this predicate applies to `String` only"
        }
        "NonNegative" | "Positive" => "this predicate applies to `Int` and `Float` only",
        "InRange" => {
            "this predicate applies to `Int` and `Float` only, with bounds matching the base"
        }
        _ => "see the documentation for valid predicate-base combinations",
    }
}

fn check_int_refinement_consistency(refinement: &Refinement, errors: &mut Vec<CompileError>) {
    let mut lo: i64 = i64::MIN;
    let mut hi: i64 = i64::MAX;
    for p in &refinement.predicates {
        match &p.kind {
            PredKind::Positive => lo = lo.max(1),
            PredKind::NonNegative => lo = lo.max(0),
            PredKind::InRange(a, b) => {
                lo = lo.max(*a);
                hi = hi.min(*b);
            }
            _ => {}
        }
    }
    if lo > hi {
        errors.push(
            CompileError::new(
                "karn.types.empty_refinement",
                refinement.span,
                "this refinement has no valid values — the predicates contradict each other",
            )
            .with_note(format!(
                "the effective range is `{lo}..={hi}`, which is empty"
            )),
        );
    }
}

fn check_float_refinement_consistency(refinement: &Refinement, errors: &mut Vec<CompileError>) {
    let mut lo = f64::NEG_INFINITY;
    let mut hi = f64::INFINITY;
    // `Positive` excludes the lower endpoint (0.0 itself is not positive).
    let mut lo_exclusive = false;
    for p in &refinement.predicates {
        match &p.kind {
            PredKind::Positive if 0.0 >= lo => {
                lo = 0.0;
                lo_exclusive = true;
            }
            PredKind::NonNegative if 0.0 > lo => {
                lo = 0.0;
                lo_exclusive = false;
            }
            PredKind::InRangeF(a, b) => {
                if a.value > lo {
                    lo = a.value;
                    lo_exclusive = false;
                }
                hi = hi.min(b.value);
            }
            _ => {}
        }
    }
    if lo > hi || (lo == hi && lo_exclusive) {
        errors.push(
            CompileError::new(
                "karn.types.empty_refinement",
                refinement.span,
                "this refinement has no valid values — the predicates contradict each other",
            )
            .with_note(format!(
                "the effective range is `{lo}..={hi}`{}, which is empty",
                if lo_exclusive {
                    " (lower bound exclusive)"
                } else {
                    ""
                }
            )),
        );
    }
}

fn check_string_refinement_consistency(refinement: &Refinement, errors: &mut Vec<CompileError>) {
    let mut min_len: i64 = 0;
    let mut max_len: i64 = i64::MAX;
    let mut exact_len: Option<i64> = None;
    for p in &refinement.predicates {
        match &p.kind {
            PredKind::MinLength(n) => min_len = min_len.max(*n),
            PredKind::MaxLength(n) => max_len = max_len.min(*n),
            PredKind::NonEmpty => min_len = min_len.max(1),
            PredKind::Length(n) => {
                if let Some(prev) = exact_len {
                    if prev != *n {
                        errors.push(CompileError::new(
                            "karn.types.empty_refinement",
                            refinement.span,
                            format!(
                                "conflicting exact lengths: `Length({prev})` and `Length({n})` cannot both hold"
                            ),
                        ));
                    }
                } else {
                    exact_len = Some(*n);
                }
                min_len = min_len.max(*n);
                max_len = max_len.min(*n);
            }
            _ => {}
        }
    }
    if min_len > max_len {
        errors.push(
            CompileError::new(
                "karn.types.empty_refinement",
                refinement.span,
                "this refinement has no valid values — minimum length exceeds maximum length",
            )
            .with_note(format!(
                "the effective length range is `{min_len}..={max_len}`, which is empty"
            )),
        );
    }
}

// -- function body type checking --

/// Mutable per-function context.
pub struct Ctx<'a> {
    pub input: &'a ResolvedCommons,
    pub expr_types: &'a mut HashMap<Span, Ty>,
    pub errors: &'a mut Vec<CompileError>,
    /// v0.25 (ADR 0053): binding edges recorded at the checker's own
    /// resolution sites — capability/service dispatch, typed call dispatch,
    /// annotation resolution. Handler/test/provider bodies never pass
    /// through the resolver's reference walk, so the checker is their only
    /// recording point.
    pub refs: &'a mut RefSink,
    /// v0.27 (ADR 0056): inferred-type inlay hints recorded at the
    /// annotation-absent binding sites (`let` / `let <-` / lambda params)
    /// as the binding's final type is computed.
    pub hints: &'a mut HintSink,
    /// Stack of in-scope name → type frames.
    pub scopes: Vec<HashMap<String, Ty>>,
    pub return_ty: Ty,
    pub return_ty_span: Span,
    /// True if the enclosing function/handler returns `Effect[T]` (v0.5).
    /// Determines whether `<-` and capability calls are permitted.
    pub effectful: bool,
    /// If inside an agent handler, the agent's state type and the agent's
    /// name. Used to validate `commit` statements.
    pub agent_state_ty: Option<Ty>,
    /// True if a `commit` has been seen on the current control-flow path.
    /// Used to detect "two reachable commits".
    pub commit_seen: bool,
    /// Capabilities in scope for the current handler, as a name → CapabilityInfo
    /// map. Empty for pure functions and non-context code.
    pub capabilities: HashMap<String, CapabilityInfo>,
    /// All capabilities declared in the surrounding context (for diagnostic
    /// purposes — used to detect `<Cap>.op(...)` calls where the capability is
    /// declared in the context but not listed in `given`).
    pub declared_capabilities: HashMap<String, CapabilityInfo>,
    /// Names of capabilities the user listed in `given`, but haven't yet
    /// observed used. After checking the body, anything left here is
    /// unused — a warning.
    pub given_remaining: HashSet<String>,
    /// Names of capabilities actually used in the body so far.
    pub given_used: HashSet<String>,
    /// v0.26 (ADR 0054): the `given` clause's entries in declaration order —
    /// (deps key, source span) — so the `given` quick-fixes can author
    /// list-aware edits at the diagnosis site. Empty where no `given` clause
    /// applies (fns, mock ops, state initialisers).
    pub given_entries: Vec<(String, Span)>,
    /// v0.26: where the add-capability fix synthesises an *absent* `given`
    /// clause — the handler's return type (the clause follows it). `None`
    /// where the clause lives elsewhere (a provider's `provides … given`
    /// line); the fix is then offered only when entries already exist.
    pub given_anchor: Option<Span>,
    /// True when the body being checked is a test case body. Permits
    /// `assert` statements (v0.7).
    pub in_test_body: bool,
    /// The target unit's service names, populated for test case bodies
    /// (v0.25). `svc.call(args)` in a test invokes the target's service —
    /// the emitter wires it from the same set; the checker records the
    /// binding edge here so test-file references index.
    pub test_services: HashSet<String>,
    /// v0.20a: the enclosing function's type parameters (rigid vars), so
    /// nested explicit type arguments (`identity[A](x)` inside a generic
    /// body) resolve. Empty outside generic fn bodies.
    pub type_vars: HashSet<String>,
}

/// Per-capability info for checker dispatch within a handler body.
#[derive(Debug, Clone)]
pub struct CapabilityInfo {
    pub name: String,
    pub ops: Vec<CapabilityOpInfo>,
}

#[derive(Debug, Clone)]
pub struct CapabilityOpInfo {
    pub name: String,
    pub params: Vec<Ty>,
    pub return_ty: Ty,
}

impl<'a> Ctx<'a> {
    pub fn lookup(&self, name: &str) -> Option<Ty> {
        for scope in self.scopes.iter().rev() {
            if let Some(t) = scope.get(name) {
                return Some(t.clone());
            }
        }
        None
    }

    /// Returns the type of an expression's "root identifier" — for `a.b.c`
    /// that's `a`; for a bare `a` it's `a`. Used to detect whether a chain's
    /// outermost name shadows an alias / consumed-context prefix.
    pub fn lookup_root_ident(&self, expr: &Expr) -> Option<Ty> {
        match &expr.kind {
            ExprKind::Ident(id) => self.lookup(&id.name),
            ExprKind::FieldAccess { receiver, .. } => self.lookup_root_ident(receiver),
            ExprKind::MethodCall { receiver, .. } => self.lookup_root_ident(receiver),
            _ => None,
        }
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }
    pub fn bind(&mut self, name: String, ty: Ty) {
        self.scopes.last_mut().unwrap().insert(name, ty);
    }
}

fn check_fn(
    f: &FnDecl,
    input: &ResolvedCommons,
    expr_types: &mut HashMap<Span, Ty>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
    hints: &mut HintSink,
) {
    // v0.20a: the fn's type parameters are *rigid* type variables while
    // checking its own body. A type param shadowing a declared type is
    // confusing — diagnose the collision.
    let vars: HashSet<String> = f
        .type_params
        .iter()
        .map(|tp| tp.name.name.clone())
        .collect();
    for tp in &f.type_params {
        if input.types.contains_key(&tp.name.name) {
            errors.push(
                CompileError::new(
                    "karn.generics.type_arg_mismatch",
                    tp.span,
                    format!(
                        "type parameter `{}` shadows the declared type of the same name",
                        tp.name.name
                    ),
                )
                .with_note("rename the type parameter"),
            );
        }
    }
    let return_ty = match resolve_type_ref_in(&f.return_type, &input.types, &vars) {
        Some(t) => t,
        None => return,
    };
    record_type_refs(&f.return_type, &input.types, &vars, refs);
    let mut param_scope: HashMap<String, Ty> = HashMap::new();
    // For methods, the implicit `self` parameter has the attached type.
    if let FnName::Method { type_name, .. } = &f.name
        && f.has_self
        && let Some(self_ty) = type_from_decl(type_name, &input.types)
    {
        param_scope.insert("self".to_string(), self_ty);
    }
    for p in &f.params {
        if let Some(ty) = resolve_type_ref_in(&p.type_ref, &input.types, &vars) {
            record_type_refs(&p.type_ref, &input.types, &vars, refs);
            param_scope.insert(p.name.name.clone(), ty);
        }
    }
    let effectful = matches!(&return_ty, Ty::Effect(_));
    let mut ctx = Ctx {
        input,
        expr_types,
        errors,
        refs,
        hints,
        scopes: vec![param_scope],
        return_ty: return_ty.clone(),
        return_ty_span: f.return_type.span(),
        effectful,
        agent_state_ty: None,
        commit_seen: false,
        capabilities: HashMap::new(),
        declared_capabilities: HashMap::new(),
        given_remaining: HashSet::new(),
        given_used: HashSet::new(),
        given_entries: Vec::new(),
        given_anchor: None,
        in_test_body: false,
        test_services: HashSet::new(),
        type_vars: vars.clone(),
    };
    let Some(body_ty) = type_of_block(&f.body, Some(&return_ty), &mut ctx) else {
        return;
    };
    if !compatible(&body_ty, &return_ty) {
        ctx.errors.push(
            CompileError::new(
                "karn.types.return_mismatch",
                f.body.tail.span,
                format!(
                    "function body has type `{}`, but the declared return type is `{}`",
                    body_ty.display(),
                    return_ty.display()
                ),
            )
            .with_label(f.return_type.span(), "declared return type"),
        );
    }
}

/// Build a `Ty` from a TypeDecl name reference.
pub fn type_from_decl(id: &Ident, types: &HashMap<String, TypeDecl>) -> Option<Ty> {
    let decl = types.get(&id.name)?;
    Some(named_ty(decl))
}

/// Build a `Ty::Named` for the given declaration.
pub fn named_ty(decl: &TypeDecl) -> Ty {
    let kind = match &decl.body {
        TypeBody::Refined { base, .. } => NamedKind::Refined(*base),
        TypeBody::Record(_) => NamedKind::Record,
        TypeBody::Sum(_) => NamedKind::Sum,
        TypeBody::Opaque { base, .. } => NamedKind::Opaque(*base),
    };
    Ty::Named {
        name: decl.name.name.clone(),
        kind,
    }
}

/// v0.20a: like [`resolve_type_ref`], with a set of in-scope **type
/// parameters**: a `Named` reference matching one resolves to [`Ty::Var`]
/// (checked before the type-table lookup — a type parameter shadows a
/// same-named declaration; the collision is diagnosed at the declaration).
pub fn resolve_type_ref_in(
    r: &TypeRef,
    types: &HashMap<String, TypeDecl>,
    vars: &HashSet<String>,
) -> Option<Ty> {
    match r {
        TypeRef::Named(id) if vars.contains(&id.name) => Some(Ty::Var(id.name.clone())),
        TypeRef::Result(t, e, _) => Some(Ty::Result(
            Box::new(resolve_type_ref_in(t, types, vars)?),
            Box::new(resolve_type_ref_in(e, types, vars)?),
        )),
        TypeRef::Option(t, _) => Some(Ty::Option(Box::new(resolve_type_ref_in(t, types, vars)?))),
        TypeRef::Effect(t, _) => Some(Ty::Effect(Box::new(resolve_type_ref_in(t, types, vars)?))),
        TypeRef::HttpResult(t, _) => Some(Ty::HttpResult(Box::new(resolve_type_ref_in(
            t, types, vars,
        )?))),
        TypeRef::List(t, _) => Some(Ty::List(Box::new(resolve_type_ref_in(t, types, vars)?))),
        TypeRef::Map(k, v, _) => Some(Ty::Map(
            Box::new(resolve_type_ref_in(k, types, vars)?),
            Box::new(resolve_type_ref_in(v, types, vars)?),
        )),
        TypeRef::Fn(params, ret, _) => {
            let params: Option<Vec<Ty>> = params
                .iter()
                .map(|p| resolve_type_ref_in(p, types, vars))
                .collect();
            Some(Ty::Fn {
                params: params?,
                ret: Box::new(resolve_type_ref_in(ret, types, vars)?),
            })
        }
        _ => resolve_type_ref(r, types),
    }
}

/// v0.20a: substitute type variables in `t` per `subst`. Must be total when
/// instantiating a call (the uninferable check runs first); an unbound Var
/// passes through unchanged for partial substitution during inference.
fn substitute(t: &Ty, subst: &HashMap<String, Ty>) -> Ty {
    match t {
        Ty::Var(n) => subst.get(n).cloned().unwrap_or_else(|| t.clone()),
        Ty::Result(a, b) => Ty::Result(
            Box::new(substitute(a, subst)),
            Box::new(substitute(b, subst)),
        ),
        Ty::Option(a) => Ty::Option(Box::new(substitute(a, subst))),
        Ty::Effect(a) => Ty::Effect(Box::new(substitute(a, subst))),
        Ty::HttpResult(a) => Ty::HttpResult(Box::new(substitute(a, subst))),
        Ty::List(a) => Ty::List(Box::new(substitute(a, subst))),
        Ty::Map(k, v) => Ty::Map(
            Box::new(substitute(k, subst)),
            Box::new(substitute(v, subst)),
        ),
        Ty::Fn { params, ret } => Ty::Fn {
            params: params.iter().map(|p| substitute(p, subst)).collect(),
            ret: Box::new(substitute(ret, subst)),
        },
        _ => t.clone(),
    }
}

/// v0.20a: does `t` still contain a type variable?
fn contains_var(t: &Ty) -> bool {
    match t {
        Ty::Var(_) => true,
        Ty::Result(a, b) | Ty::Map(a, b) => contains_var(a) || contains_var(b),
        Ty::Option(a) | Ty::Effect(a) | Ty::HttpResult(a) | Ty::List(a) => contains_var(a),
        Ty::Fn { params, ret } => params.iter().any(contains_var) || contains_var(ret),
        _ => false,
    }
}

/// v0.20b: does `t` contain a type variable that is NOT one of the enclosing
/// function's rigid type parameters? Rigid vars are fully constrained inside
/// the body; only flexible (call-site instantiation) vars mean "still being
/// inferred".
fn contains_flexible_var(t: &Ty, rigid: &HashSet<String>) -> bool {
    match t {
        Ty::Var(n) => !rigid.contains(n),
        Ty::Result(a, b) | Ty::Map(a, b) => {
            contains_flexible_var(a, rigid) || contains_flexible_var(b, rigid)
        }
        Ty::Option(a) | Ty::Effect(a) | Ty::HttpResult(a) | Ty::List(a) => {
            contains_flexible_var(a, rigid)
        }
        Ty::Fn { params, ret } => {
            params.iter().any(|p| contains_flexible_var(p, rigid))
                || contains_flexible_var(ret, rigid)
        }
        _ => false,
    }
}

/// v0.20a: argument-directed unification. Walks `pattern` (possibly
/// Var-bearing) against the ground `actual`; a Var binds on first sight and
/// must match its prior binding **exactly** afterwards (keep inference dumb
/// and predictable — the explicit `name[T](…)` form is the pressure valve).
/// Returns false on a conflict; structural mismatches are NOT reported here —
/// the post-substitution `compatible` check owns those diagnostics.
fn unify(pattern: &Ty, actual: &Ty, subst: &mut HashMap<String, Ty>) -> bool {
    match (pattern, actual) {
        (Ty::Var(n), a) => match subst.get(n) {
            Some(bound) => bound == a,
            None => {
                subst.insert(n.clone(), a.clone());
                true
            }
        },
        (Ty::Result(a1, b1), Ty::Result(a2, b2)) | (Ty::Map(a1, b1), Ty::Map(a2, b2)) => {
            unify(a1, a2, subst) && unify(b1, b2, subst)
        }
        (Ty::Option(a1), Ty::Option(a2))
        | (Ty::Effect(a1), Ty::Effect(a2))
        | (Ty::HttpResult(a1), Ty::HttpResult(a2))
        | (Ty::List(a1), Ty::List(a2)) => unify(a1, a2, subst),
        (
            Ty::Fn {
                params: p1,
                ret: r1,
            },
            Ty::Fn {
                params: p2,
                ret: r2,
            },
        ) => {
            p1.len() == p2.len()
                && p1.iter().zip(p2).all(|(a, b)| unify(a, b, subst))
                && unify(r1, r2, subst)
        }
        // Ground-vs-ground: any pair is fine here; `compatible` owns the
        // real check after substitution.
        _ => true,
    }
}

/// v0.25 (ADR 0053): record a binding edge for every `Named` reference
/// inside a type-ref that resolved. Called alongside the `resolve_type_ref*`
/// annotation sites; `skip` holds the enclosing fn's type parameters (rigid
/// vars are not type symbols). Handler signatures and body annotations never
/// pass through the resolver's reference walk, so these sites are their only
/// recording point; where both passes run, assembly dedupes.
pub(crate) fn record_type_refs(
    r: &TypeRef,
    types: &HashMap<String, TypeDecl>,
    skip: &HashSet<String>,
    refs: &mut RefSink,
) {
    match r {
        TypeRef::Named(id) => {
            if types.contains_key(&id.name) && !skip.contains(&id.name) {
                refs.record(id.span, SymbolKind::Type, &id.name);
            }
        }
        TypeRef::Fn(params, ret, _) => {
            for p in params {
                record_type_refs(p, types, skip, refs);
            }
            record_type_refs(ret, types, skip, refs);
        }
        TypeRef::Result(a, b, _) | TypeRef::Map(a, b, _) => {
            record_type_refs(a, types, skip, refs);
            record_type_refs(b, types, skip, refs);
        }
        TypeRef::Option(t, _)
        | TypeRef::Effect(t, _)
        | TypeRef::HttpResult(t, _)
        | TypeRef::List(t, _) => record_type_refs(t, types, skip, refs),
        TypeRef::Base(..)
        | TypeRef::ValidationError(_)
        | TypeRef::JsonError(_)
        | TypeRef::Unit(_) => {}
    }
}

pub fn resolve_type_ref(r: &TypeRef, types: &HashMap<String, TypeDecl>) -> Option<Ty> {
    match r {
        TypeRef::Base(b, _) => Some(Ty::Base(*b)),
        TypeRef::Named(id) => type_from_decl(id, types),
        // v0.20a: a function type. Effectfulness is structural (ret is
        // Effect[_]); nothing extra to record.
        TypeRef::Fn(params, ret, _) => {
            let params: Option<Vec<Ty>> =
                params.iter().map(|p| resolve_type_ref(p, types)).collect();
            Some(Ty::Fn {
                params: params?,
                ret: Box::new(resolve_type_ref(ret, types)?),
            })
        }
        TypeRef::Result(t, e, _) => {
            let t = resolve_type_ref(t, types)?;
            let e = resolve_type_ref(e, types)?;
            Some(Ty::Result(Box::new(t), Box::new(e)))
        }
        TypeRef::Option(t, _) => {
            let t = resolve_type_ref(t, types)?;
            Some(Ty::Option(Box::new(t)))
        }
        TypeRef::Effect(t, _) => {
            let t = resolve_type_ref(t, types)?;
            Some(Ty::Effect(Box::new(t)))
        }
        TypeRef::HttpResult(t, _) => {
            let t = resolve_type_ref(t, types)?;
            Some(Ty::HttpResult(Box::new(t)))
        }
        TypeRef::List(t, _) => {
            let t = resolve_type_ref(t, types)?;
            Some(Ty::List(Box::new(t)))
        }
        TypeRef::Map(k, v, _) => {
            let k = resolve_type_ref(k, types)?;
            let v = resolve_type_ref(v, types)?;
            Some(Ty::Map(Box::new(k), Box::new(v)))
        }
        TypeRef::ValidationError(_) => Some(Ty::ValidationError),
        TypeRef::JsonError(_) => Some(Ty::JsonError),
        TypeRef::Unit(_) => Some(Ty::Unit),
    }
}

/// `t` is usable where `u` is expected.
pub fn compatible(t: &Ty, u: &Ty) -> bool {
    match (t, u) {
        (Ty::Base(a), Ty::Base(b)) => a == b,
        (Ty::Named { name: a, kind: ka }, Ty::Named { name: b, kind: kb }) => a == b && ka == kb,
        // Refined → base (widening).
        (
            Ty::Named {
                kind: NamedKind::Refined(b),
                ..
            },
            Ty::Base(target),
        ) => b == target,
        (Ty::Base(_), Ty::Named { .. }) => false,
        (Ty::Result(t1, e1), Ty::Result(t2, e2)) => compatible(t1, t2) && compatible(e1, e2),
        (Ty::Option(a), Ty::Option(b)) => compatible(a, b),
        (Ty::Effect(a), Ty::Effect(b)) => compatible(a, b),
        (Ty::HttpResult(a), Ty::HttpResult(b)) => compatible(a, b),
        // v0.20b: collections are covariant in their element/value types;
        // Map keys must match exactly — key-position widening would split a
        // map's keys across refined/base identities at lookup time.
        (Ty::List(a), Ty::List(b)) => compatible(a, b),
        (Ty::Map(k1, v1), Ty::Map(k2, v2)) => k1 == k2 && compatible(v1, v2),
        (Ty::ValidationError, Ty::ValidationError) => true,
        (Ty::JsonError, Ty::JsonError) => true,
        (Ty::Unit, Ty::Unit) => true,
        // v0.20a: function types — **contravariant** in parameters, covariant
        // in the return type. `compatible(t, u)` is "t usable where u is
        // expected" and is already asymmetric (refined → base widening), so
        // the per-position argument order flips for params: a function
        // expecting the *wider* param type is usable where one expecting the
        // narrower is required — and crucially, the covariant direction would
        // let unvalidated base values flow into a refined-typed body.
        (Ty::Fn { params: p, ret: r }, Ty::Fn { params: q, ret: s }) => {
            p.len() == q.len() && p.iter().zip(q).all(|(a, b)| compatible(b, a)) && compatible(r, s)
        }
        // v0.20a: rigid type variables (a generic fn's own body) match by
        // name. Flexible vars never reach `compatible` — they are eliminated
        // by substitution during call-site instantiation.
        (Ty::Var(a), Ty::Var(b)) => a == b,
        _ => false,
    }
}

/// v0.11: type-check an agent state-field initialiser (`field: T = init`). The
/// initialiser must be a *static* value of the field type — it is checked in an
/// empty, pure scope (so `self`, parameters, capabilities, and effects are all
/// out of reach) with the field type as the expected type, so refined literals
/// admit (v0.9.4) and sum variants resolve. The init's expression types are
/// recorded into `expr_types` for emission; a single
/// `karn.agents.bad_state_initialiser` is pushed on any failure.
pub fn check_state_initialiser(
    init: &Expr,
    field_type: &TypeRef,
    input: &ResolvedCommons,
    expr_types: &mut HashMap<Span, Ty>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
    hints: &mut HintSink,
) {
    let Some(field_ty) = resolve_type_ref(field_type, &input.types) else {
        return; // an unresolved field type is reported elsewhere
    };
    let mut local_errors: Vec<CompileError> = Vec::new();
    let result = {
        let mut ctx = Ctx {
            input,
            expr_types,
            errors: &mut local_errors,
            refs,
            hints,
            scopes: vec![HashMap::new()],
            return_ty: field_ty.clone(),
            return_ty_span: init.span,
            effectful: false,
            agent_state_ty: None,
            commit_seen: false,
            capabilities: HashMap::new(),
            declared_capabilities: HashMap::new(),
            given_remaining: HashSet::new(),
            given_used: HashSet::new(),
            given_entries: Vec::new(),
            given_anchor: None,
            in_test_body: false,
            test_services: HashSet::new(),
            type_vars: HashSet::new(),
        };
        type_of(init, Some(&field_ty), &mut ctx)
    };
    let compatible_result = matches!(&result, Some(t) if compatible(t, &field_ty));
    if !compatible_result || !local_errors.is_empty() {
        let got = result
            .as_ref()
            .map(|t| t.display())
            .unwrap_or_else(|| "an invalid expression".to_string());
        errors.push(
            CompileError::new(
                "karn.agents.bad_state_initialiser",
                init.span,
                format!(
                    "state field initialiser must be a static value of type `{}` (got `{got}`)",
                    field_ty.display(),
                ),
            )
            .with_note(
                "an initialiser is a compile-time value — a literal, a sum variant, \
                 `Some`/`None`/`Ok`/`Err`, a record, or `T.unsafe(lit)` — with no reference to \
                 `self`, parameters, or capabilities",
            ),
        );
    }
}

pub fn type_of_block(block: &Block, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    ctx.push_scope();
    for stmt in &block.statements {
        match stmt {
            Statement::Let(l) => {
                let annot_ty = l.type_annot.as_ref().and_then(|a| {
                    // v0.20b: the enclosing fn's type parameters are legal
                    // in body annotations (`let init: List[B] = …`).
                    let r = resolve_type_ref_in(a, &ctx.input.types, &ctx.type_vars);
                    if r.is_none() {
                        ctx.errors.push(CompileError::new(
                            "karn.resolve.unknown_type",
                            a.span(),
                            "type in `let` annotation does not resolve",
                        ));
                    } else {
                        record_type_refs(a, &ctx.input.types, &ctx.type_vars, ctx.refs);
                    }
                    r
                });
                let rhs_ty = type_of(&l.value, annot_ty.as_ref(), ctx);
                let final_ty = match (annot_ty, rhs_ty) {
                    (Some(annot), Some(rhs)) => {
                        if !compatible(&rhs, &annot) {
                            ctx.errors.push(
                                CompileError::new(
                                    "karn.types.let_annotation_mismatch",
                                    l.value.span,
                                    format!(
                                        "let binding's value has type `{}`, but the annotation declares `{}`",
                                        rhs.display(),
                                        annot.display()
                                    ),
                                )
                                .with_label(
                                    l.type_annot.as_ref().unwrap().span(),
                                    "declared type annotation",
                                ),
                            );
                        }
                        annot
                    }
                    (Some(annot), None) => annot,
                    (None, Some(rhs)) => rhs,
                    (None, None) => continue,
                };
                if l.name.name != "_" {
                    // v0.27 (ADR 0056): an annotation-absent binding gets an
                    // inferred-type inlay hint at the binding name.
                    if l.type_annot.is_none() {
                        ctx.hints
                            .record(l.name.span, format!(": {}", final_ty.display()));
                    }
                    ctx.bind(l.name.name.clone(), final_ty);
                }
            }
            Statement::EffectLet(l) => {
                if !ctx.effectful {
                    ctx.errors.push(
                        CompileError::new(
                            "karn.effect.bind_in_pure_context",
                            l.span,
                            "the `<-` operator can only be used inside an effectful body (one returning `Effect[T]`)",
                        )
                        .with_label(
                            ctx.return_ty_span,
                            format!("enclosing return type is `{}`", ctx.return_ty.display()),
                        )
                        .with_note(
                            "change the enclosing function/handler's return type to `Effect[...]`, or use `let ... =` for a pure binding",
                        ),
                    );
                }
                // Determine the inner Effect[T] payload type for the binding.
                let annot_ty = l.type_annot.as_ref().and_then(|a| {
                    // v0.20b: the enclosing fn's type parameters are legal
                    // in body annotations (`let init: List[B] = …`).
                    let r = resolve_type_ref_in(a, &ctx.input.types, &ctx.type_vars);
                    if r.is_none() {
                        ctx.errors.push(CompileError::new(
                            "karn.resolve.unknown_type",
                            a.span(),
                            "type in `let` annotation does not resolve",
                        ));
                    } else {
                        record_type_refs(a, &ctx.input.types, &ctx.type_vars, ctx.refs);
                    }
                    r
                });
                // The expected type for the RHS is `Effect[annot]` if annot present.
                let rhs_expected = annot_ty.as_ref().map(|t| Ty::Effect(Box::new(t.clone())));
                let rhs_ty = type_of(&l.value, rhs_expected.as_ref(), ctx);
                let inner_ty = match rhs_ty {
                    Some(Ty::Effect(t)) => Some((*t).clone()),
                    Some(other) => {
                        ctx.errors.push(
                            CompileError::new(
                                "karn.effect.bind_on_non_effect",
                                l.value.span,
                                format!(
                                    "the `<-` operator requires an `Effect[T]` value, but got `{}`",
                                    other.display()
                                ),
                            )
                            .with_note(
                                "use `let ... =` for a pure binding, or wrap the value with `Effect.pure(...)`",
                            ),
                        );
                        None
                    }
                    None => None,
                };
                let final_ty = match (annot_ty, inner_ty) {
                    (Some(annot), Some(rhs)) => {
                        if !compatible(&rhs, &annot) {
                            ctx.errors.push(CompileError::new(
                                "karn.types.let_annotation_mismatch",
                                l.value.span,
                                format!(
                                    "let-binding's value has type `Effect[{}]`, but the annotation declares `Effect[{}]`",
                                    rhs.display(),
                                    annot.display()
                                ),
                            ));
                        }
                        annot
                    }
                    (Some(annot), None) => annot,
                    (None, Some(rhs)) => rhs,
                    (None, None) => continue,
                };
                if l.name.name != "_" {
                    // v0.27 (ADR 0056): as for `let =`, but `final_ty` here
                    // is the peeled `Effect[T]` payload — the binding's
                    // actual type, which is what the hint must show.
                    if l.type_annot.is_none() {
                        ctx.hints
                            .record(l.name.span, format!(": {}", final_ty.display()));
                    }
                    ctx.bind(l.name.name.clone(), final_ty);
                }
            }
            Statement::Commit(c) => {
                let Some(state_ty) = ctx.agent_state_ty.clone() else {
                    ctx.errors.push(
                        CompileError::new(
                            "karn.commit.outside_agent",
                            c.span,
                            "`commit` is only valid inside an agent handler",
                        )
                        .with_note(
                            "agent handlers update persistent state via `commit newState`; \
                             services and free functions do not have state",
                        ),
                    );
                    let _ = type_of(&c.value, None, ctx);
                    continue;
                };
                if ctx.commit_seen {
                    ctx.errors.push(
                        CompileError::new(
                            "karn.commit.two_reachable_commits",
                            c.span,
                            "two `commit` statements are reachable on the same execution path",
                        )
                        .with_note(
                            "an agent handler may commit at most once per invocation; use branches to commit different values conditionally",
                        ),
                    );
                }
                let val_ty = type_of(&c.value, Some(&state_ty), ctx);
                if let Some(actual) = val_ty
                    && !compatible(&actual, &state_ty)
                {
                    ctx.errors.push(CompileError::new(
                        "karn.commit.wrong_state_type",
                        c.value.span,
                        format!(
                            "`commit` expression has type `{}`, but the agent's state type is `{}`",
                            actual.display(),
                            state_ty.display()
                        ),
                    ));
                }
                ctx.commit_seen = true;
            }
            Statement::Assert(a) => {
                if !ctx.in_test_body {
                    ctx.errors.push(
                        CompileError::new(
                            "karn.assert.outside_test",
                            a.span,
                            "`assert` is only valid inside a test case body",
                        )
                        .with_note(
                            "assertion statements verify conditions at test runtime; use them only inside `test \"...\" { ... }` blocks",
                        ),
                    );
                }
                let val_ty = type_of(&a.value, Some(&Ty::Base(BaseType::Bool)), ctx);
                if let Some(actual) = val_ty
                    && !compatible(&actual, &Ty::Base(BaseType::Bool))
                {
                    ctx.errors.push(CompileError::new(
                        "karn.assert.non_bool",
                        a.value.span,
                        format!(
                            "`assert` expression has type `{}`, but a `Bool` is required",
                            actual.display(),
                        ),
                    ));
                }
            }
        }
    }
    let ty = type_of(&block.tail, expected, ctx);
    let ty = maybe_auto_lift(ty, expected);
    if let Some(ty) = &ty {
        ctx.expr_types.insert(block.span, ty.clone());
    }
    ctx.pop_scope();
    ty
}

/// v0.7.1 tail-position auto-lift. If the expected type is `Effect[T]` and
/// the computed type is `T` (not itself an `Effect[_]`), lift it to
/// `Effect[T]`. Otherwise leave the type alone — the surrounding compatibility
/// check will report any genuine mismatch.
fn maybe_auto_lift(ty: Option<Ty>, expected: Option<&Ty>) -> Option<Ty> {
    if let Some(actual) = &ty
        && let Some(Ty::Effect(et)) = expected
        && !actual.is_effect()
        && compatible(actual, et)
    {
        return Some(Ty::Effect(Box::new(actual.clone())));
    }
    ty
}

pub fn type_of(expr: &Expr, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    let ty = match &expr.kind {
        // v0.9.4: a literal in a refined-expected position takes the refined
        // type (validated now); otherwise it keeps its base type.
        // v0.20a: a lambda. With an expected function type, params type
        // contextually and the body checks against the expected return; in an
        // unconstrained position, every param must be annotated and
        // effectfulness is inferred bottom-up by a syntactic pre-scan.
        ExprKind::Lambda(lambda) => check_lambda(lambda, expected, ctx),
        ExprKind::IntLit(_) => {
            admit_refined_literal(expr, expected, ctx).or(Some(Ty::Base(BaseType::Int)))
        }
        ExprKind::FloatLit { .. } => {
            admit_refined_literal(expr, expected, ctx).or(Some(Ty::Base(BaseType::Float)))
        }
        ExprKind::StrLit(_) => {
            admit_refined_literal(expr, expected, ctx).or(Some(Ty::Base(BaseType::String)))
        }
        ExprKind::BoolLit(_) => Some(Ty::Base(BaseType::Bool)),
        // v0.20b: a list literal. Elements check against the expected
        // element type when one is supplied (so refined literals admit,
        // v0.9.4); an empty `[]` has no inferable element type without one.
        ExprKind::ListLit(elems) => {
            let expected_elem = expected.and_then(peel_to_list);
            if elems.is_empty() {
                match expected_elem {
                    Some(t) => Some(Ty::List(Box::new(t))),
                    None => {
                        ctx.errors.push(
                            CompileError::new(
                                "karn.types.uninferable_element_type",
                                expr.span,
                                "an empty `[]` has no inferable element type",
                            )
                            .with_note(
                                "annotate the binding (`let xs: List[T] = []`) or use the empty list where a `List[T]` is expected",
                            ),
                        );
                        None
                    }
                }
            } else {
                let mut elem_ty: Option<Ty> = expected_elem;
                for e in elems {
                    let Some(t) = type_of(e, elem_ty.as_ref(), ctx) else {
                        continue;
                    };
                    match &elem_ty {
                        Some(et) => {
                            if !compatible(&t, et) {
                                ctx.errors.push(CompileError::new(
                                    "karn.types.list_element_mismatch",
                                    e.span,
                                    format!(
                                        "list element has type `{}`, but the list's element type is `{}`",
                                        t.display(),
                                        et.display()
                                    ),
                                ));
                            }
                        }
                        None => elem_ty = Some(t),
                    }
                }
                elem_ty.map(|t| Ty::List(Box::new(t)))
            }
        }
        ExprKind::Ident(id) => {
            // v0.9: a bare ident may name an HttpResult variant. Resolve to
            // HttpResult only when (a) the surrounding type implies it, or
            // (b) no user sum-type variant of the same name exists. This
            // keeps `NotFound` resolving to a user `StockError` variant
            // when the caller expects a domain Result.
            if ctx.lookup(id.name.as_str()).is_none()
                && let Some(v) = http_variant(&id.name)
            {
                let user_owns = ctx.input.types.values().any(|t| {
                    matches!(&t.body, TypeBody::Sum(s)
                        if s.variants.iter().any(|var| var.name.name == id.name))
                });
                let http_implied = expected
                    .map(|t| peel_to_http_result(t).is_some())
                    .unwrap_or(false)
                    || peel_to_http_result(&ctx.return_ty).is_some();
                if http_implied || !user_owns {
                    check_http_variant(id.span, v, &[], expected, ctx)
                } else {
                    check_ident(id, expected, ctx)
                }
            } else {
                check_ident(id, expected, ctx)
            }
        }
        ExprKind::Paren(inner) => type_of(inner, expected, ctx),
        ExprKind::Call {
            name,
            type_args,
            args,
        } => {
            // v0.9: HttpResult variant call. Prefer HttpResult when the
            // surrounding type implies it; otherwise defer to fn/user-variant
            // resolution and only fall back to HttpResult when nothing else
            // owns the name.
            let user_owners: usize = ctx
                .input
                .types
                .values()
                .filter(|t| {
                    matches!(&t.body, TypeBody::Sum(s)
                        if s.variants.iter().any(|v| v.name.name == name.name))
                })
                .count();
            let http_implied = expected
                .map(|t| peel_to_http_result(t).is_some())
                .unwrap_or(false)
                || peel_to_http_result(&ctx.return_ty).is_some();
            let unowned = !ctx.input.fns.contains_key(&name.name) && user_owners == 0;
            if let Some(v) = http_variant(&name.name)
                && (http_implied || unowned)
            {
                check_http_variant(expr.span, v, args, expected, ctx)
            } else {
                check_call(name, type_args, args, expr.span, ctx)
            }
        }
        ExprKind::UnaryOp(op, inner) => check_unary(*op, inner, expr.span, ctx),
        ExprKind::BinOp(op, lhs, rhs) => check_binop(*op, lhs, rhs, ctx),
        ExprKind::Block(b) => type_of_block(b, expected, ctx),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => check_if(cond, then_block, else_block, expr.span, expected, ctx),
        ExprKind::Ok(inner) => check_ok(inner, expr.span, expected, ctx),
        ExprKind::Err(inner) => check_err(inner, expr.span, expected, ctx),
        ExprKind::Some(inner) => check_some(inner, expr.span, expected, ctx),
        ExprKind::None => check_none(expr.span, expected, ctx),
        ExprKind::Question(inner) => check_question(inner, expr.span, ctx),
        ExprKind::ConstructorCall {
            type_name,
            method,
            args,
        } => {
            if type_name.name == HTTP_RESULT {
                if let Some(v) = http_variant(&method.name) {
                    check_http_variant(expr.span, v, args, expected, ctx)
                } else {
                    ctx.errors.push(CompileError::new(
                        "karn.types.unknown_static_member",
                        method.span,
                        format!("`HttpResult` has no variant named `{}`", method.name),
                    ));
                    None
                }
            } else {
                check_static_call(type_name, method, args, expr.span, ctx)
            }
        }
        ExprKind::RecordConstruction { type_name, fields } => {
            check_record_construction(type_name, fields, expr.span, ctx)
        }
        ExprKind::FieldAccess { receiver, field } => {
            // v0.9: `HttpResult.Variant` qualified nullary variant access.
            if let ExprKind::Ident(id) = &receiver.kind
                && ctx.lookup(id.name.as_str()).is_none()
                && id.name == HTTP_RESULT
            {
                if let Some(v) = http_variant(&field.name) {
                    if !matches!(v.payload, HttpVariantPayload::None) {
                        ctx.errors.push(CompileError::new(
                            "karn.types.variant_missing_payload",
                            field.span,
                            format!(
                                "`HttpResult.{}` has a payload — call it with an argument",
                                v.name
                            ),
                        ));
                        return None;
                    }
                    check_http_variant(field.span, v, &[], expected, ctx)
                } else {
                    ctx.errors.push(CompileError::new(
                        "karn.types.unknown_static_member",
                        field.span,
                        format!("`HttpResult` has no variant named `{}`", field.name),
                    ));
                    None
                }
            } else {
                check_field_access(receiver, field, ctx)
            }
        }
        ExprKind::MethodCall {
            receiver,
            method,
            type_args,
            args,
        } => {
            // v0.9: `HttpResult.Variant(args)` — explicit HttpResult construction.
            if let ExprKind::Ident(id) = &receiver.kind
                && ctx.lookup(id.name.as_str()).is_none()
                && id.name == HTTP_RESULT
            {
                if let Some(v) = http_variant(&method.name) {
                    check_http_variant(expr.span, v, args, expected, ctx)
                } else {
                    ctx.errors.push(CompileError::new(
                        "karn.types.unknown_static_member",
                        method.span,
                        format!("`HttpResult` has no variant named `{}`", method.name),
                    ));
                    None
                }
            } else {
                check_method_call(receiver, method, type_args, args, expr.span, expected, ctx)
            }
        }
        ExprKind::Match { discriminant, arms } => {
            check_match(discriminant, arms, expr.span, expected, ctx)
        }
        ExprKind::Is { value, pattern } => check_is(value, pattern, expr.span, ctx),
        ExprKind::UnitLit => Some(Ty::Unit),
        ExprKind::EffectPure(inner) => {
            let expected_inner = match expected {
                Some(Ty::Effect(t)) => Some((**t).clone()),
                _ => None,
            };
            let inner_ty = type_of(inner, expected_inner.as_ref(), ctx)?;
            Some(Ty::Effect(Box::new(inner_ty)))
        }
        ExprKind::RecordSpread {
            type_name,
            base,
            overrides,
        } => check_record_spread(
            type_name.as_ref(),
            base,
            overrides,
            expr.span,
            expected,
            ctx,
        ),
        ExprKind::Assert(inner) => check_assert(inner, expr.span, ctx),
        ExprKind::Mock { type_ref, args } => check_mock(type_ref, args, expr.span, ctx),
    };
    if let Some(ty) = &ty {
        ctx.expr_types.insert(expr.span, ty.clone());
    }
    ty
}

fn check_ident(id: &Ident, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    if let Some(ty) = ctx.lookup(id.name.as_str()) {
        return Some(ty);
    }
    // v0.20a: a named function referenced as a *value* where a function type
    // is expected (the contextual relaxation of `karn.resolve.fn_without_call`,
    // relocated here from the resolver). A Var-bearing expected (generic
    // instantiation, pass 1) counts as a function-type expectation.
    if let Some(fn_decl) = ctx.input.fns.get(&id.name).cloned() {
        let fn_expected = matches!(expected, Some(Ty::Fn { .. }));
        if fn_expected {
            ctx.refs.record(id.span, SymbolKind::Fn, &id.name);
            if !fn_decl.type_params.is_empty() {
                ctx.errors.push(
                    CompileError::new(
                        "karn.generics.uninferable_type_arg",
                        id.span,
                        format!(
                            "generic function `{}` cannot be passed as a value in v0.20a — its type parameters cannot be instantiated here",
                            id.name
                        ),
                    )
                    .with_note("wrap it in a lambda, or call it directly"),
                );
                return None;
            }
            let params: Option<Vec<Ty>> = fn_decl
                .params
                .iter()
                .map(|p| resolve_type_ref(&p.type_ref, &ctx.input.types))
                .collect();
            let ret = resolve_type_ref(&fn_decl.return_type, &ctx.input.types)?;
            return Some(Ty::Fn {
                params: params?,
                ret: Box::new(ret),
            });
        }
        // Bare reference outside a function-typed position: the original
        // rule, with the checker's type knowledge behind it.
        ctx.errors.push(
            CompileError::new(
                "karn.resolve.fn_without_call",
                id.span,
                format!(
                    "`{}` is a function — call it (`{}(…)`), or pass it where a function type is expected",
                    id.name, id.name
                ),
            )
            .with_note(
                "a bare function reference is only a value in a function-typed position (v0.20a)",
            ),
        );
        return None;
    }
    // Bare variant of a unique-owner sum type (nullary variants).
    let owners: Vec<&TypeDecl> = ctx
        .input
        .types
        .values()
        .filter(|t| matches!(&t.body, TypeBody::Sum(s) if s.variants.iter().any(|v| v.name.name == id.name)))
        .collect();
    if owners.len() == 1 {
        let owner = owners[0];
        if let TypeBody::Sum(s) = &owner.body
            && let Some(variant) = s.variants.iter().find(|v| v.name.name == id.name)
        {
            if !variant.payload.is_empty() {
                ctx.errors.push(
                    CompileError::new(
                        "karn.types.variant_missing_payload",
                        id.span,
                        format!(
                            "variant `{}` of `{}` has a payload — call it with arguments: `{}(...)`",
                            id.name, owner.name.name, id.name
                        ),
                    )
                    .with_label(variant.span, "variant declared here"),
                );
                return None;
            }
            return Some(named_ty(owner));
        }
    }
    None
}

/// v0.9.1: `assert e` as an expression. Test-privileged. Requires `e : Bool`.
/// Always yields type `()`.
/// True if a refinement cannot be satisfied by a generated default value — i.e.
/// it contains a `Matches` predicate, where bare `Mock[T]` must be given an
/// explicit pin instead.
fn refinement_needs_pin(refinement: &Refinement) -> bool {
    refinement
        .predicates
        .iter()
        .any(|p| matches!(p.kind, PredKind::Matches(_)))
}

/// v0.9.4 Part B (slice 1): `Mock[T]` / `Mock[T](literal)` for refined types,
/// valid only in test bodies. Sum/record/opaque types are not yet supported.
fn check_mock(type_ref: &TypeRef, args: &[Expr], span: Span, ctx: &mut Ctx) -> Option<Ty> {
    if !ctx.in_test_body {
        ctx.errors.push(
            CompileError::new(
                "karn.mock.outside_test",
                span,
                "`Mock[T]` is only valid inside a test case body",
            )
            .with_note(
                "Mock values are test-time construction; use them only inside `test \"...\" { ... }` blocks",
            ),
        );
    }
    let ty = match resolve_type_ref(type_ref, &ctx.input.types) {
        Some(t) => {
            // v0.25: `Mock[T]` names the type.
            record_type_refs(type_ref, &ctx.input.types, &HashSet::new(), ctx.refs);
            t
        }
        None => {
            ctx.errors.push(CompileError::new(
                "karn.mock.unknown_type",
                span,
                "`Mock[T]` refers to a type that does not resolve",
            ));
            return None;
        }
    };
    match &ty {
        // Refined types: bare (generate a default) or a single literal pin.
        Ty::Named {
            name,
            kind: NamedKind::Refined(base),
        } => {
            let name = name.clone();
            let base = *base;
            let decl = match ctx.input.types.get(&name) {
                Some(d) => d.clone(),
                // Unreachable: the type already resolved above.
                None => return None,
            };
            let refinement = type_decl_refinement(&decl);
            match args {
                [] => {
                    if refinement.is_some_and(refinement_needs_pin) {
                        ctx.errors.push(
                            CompileError::new(
                                "karn.mock.needs_pin",
                                span,
                                format!(
                                    "bare `Mock[{name}]` cannot generate a value for a `Matches` refinement"
                                ),
                            )
                            .with_note("provide an explicit value, e.g. `Mock[T](\"...\")`"),
                        );
                    }
                }
                [arg] => {
                    type_of(arg, Some(&Ty::Base(base)), ctx);
                    match const_literal(arg) {
                        Some(lit) if literal_matches_base(&lit, base) => {
                            if let Some(r) = refinement
                                && let Some(failed) = first_failed_predicate(r, &lit)
                            {
                                ctx.errors.push(CompileError::new(
                                    "karn.mock.literal_violates",
                                    arg.span,
                                    format!(
                                        "literal {} does not satisfy `{}` required by type `{}`",
                                        lit.display(),
                                        failed.name(),
                                        name
                                    ),
                                ));
                            }
                        }
                        _ => {
                            ctx.errors.push(CompileError::new(
                                "karn.mock.pin_not_literal",
                                arg.span,
                                format!(
                                    "`Mock[{name}](...)` requires a literal `{}` value",
                                    base.name()
                                ),
                            ));
                        }
                    }
                }
                _ => {
                    ctx.errors.push(CompileError::new(
                        "karn.mock.arity",
                        span,
                        format!(
                            "`Mock[{name}]` takes at most one pin argument, but {} were given",
                            args.len()
                        ),
                    ));
                }
            }
        }
        // v0.9.4 slice 2: opaque / sum / record — bare generation only. Pins for
        // these kinds (variant pins, record overrides) are a later increment.
        Ty::Named {
            name,
            kind: NamedKind::Opaque(_) | NamedKind::Sum | NamedKind::Record,
        } => {
            let name = name.clone();
            if !args.is_empty() {
                ctx.errors.push(
                    CompileError::new(
                        "karn.mock.pin_unsupported",
                        span,
                        format!(
                            "pinned `Mock[{name}](...)` is not yet supported for this kind of type — use bare `Mock[{name}]`"
                        ),
                    )
                    .with_note("literal pins are currently supported for refined types only"),
                );
            } else if !can_mock_bare(&ty, &ctx.input.types, MOCK_DEPTH) {
                ctx.errors.push(
                    CompileError::new(
                        "karn.mock.needs_pin",
                        span,
                        format!(
                            "bare `Mock[{name}]` cannot generate a value — it (transitively) needs a `Matches` refinement or is recursively unbounded"
                        ),
                    )
                    .with_note("provide an explicit value in the test instead"),
                );
            }
        }
        _ => {
            ctx.errors.push(CompileError::new(
                "karn.mock.unsupported_kind",
                span,
                format!("`Mock` is not a value type: `{}`", ty.display()),
            ));
        }
    }
    Some(ty)
}

/// v0.9.4 slice 2 recursion depth cap for bare `Mock` generation — guards
/// against recursively-unbounded types (a sum whose first variant re-enters the
/// type). Beyond it, bare generation is refused.
const MOCK_DEPTH: u32 = 12;

/// Whether a bare `Mock[T]` can generate a value for `ty`: refined types must
/// not carry a `Matches` predicate (no default), and sums/records must have
/// every (first-variant / field) component recursively mockable within the
/// depth cap.
fn can_mock_bare(ty: &Ty, types: &HashMap<String, TypeDecl>, depth: u32) -> bool {
    if depth == 0 {
        return false;
    }
    match ty {
        Ty::Base(_) => true,
        Ty::Named { name, .. } => {
            let Some(decl) = types.get(name) else {
                return false;
            };
            match &decl.body {
                TypeBody::Refined { refinement, .. } => {
                    !refinement.as_ref().is_some_and(refinement_needs_pin)
                }
                TypeBody::Opaque { .. } => true,
                TypeBody::Sum(s) => s.variants.first().is_some_and(|v| {
                    v.payload.iter().all(|f| {
                        resolve_type_ref(&f.type_ref, types)
                            .is_some_and(|t| can_mock_bare(&t, types, depth - 1))
                    })
                }),
                TypeBody::Record(r) => r.fields.iter().all(|f| {
                    resolve_type_ref(&f.type_ref, types)
                        .is_some_and(|t| can_mock_bare(&t, types, depth - 1))
                }),
            }
        }
        _ => false,
    }
}

fn check_assert(inner: &Expr, span: Span, ctx: &mut Ctx) -> Option<Ty> {
    if !ctx.in_test_body {
        ctx.errors.push(
            CompileError::new(
                "karn.assert.outside_test",
                span,
                "`assert` is only valid inside a test case body",
            )
            .with_note(
                "assertion expressions verify conditions at test runtime; use them only inside `test \"...\" { ... }` blocks",
            ),
        );
    }
    let val_ty = type_of(inner, Some(&Ty::Base(BaseType::Bool)), ctx);
    if let Some(actual) = val_ty
        && !compatible(&actual, &Ty::Base(BaseType::Bool))
    {
        ctx.errors.push(CompileError::new(
            "karn.assert.non_bool",
            inner.span,
            format!(
                "`assert` expression has type `{}`, but a `Bool` is required",
                actual.display(),
            ),
        ));
    }
    Some(Ty::Unit)
}

fn check_unary(op: UnaryOp, inner: &Expr, op_span: Span, ctx: &mut Ctx) -> Option<Ty> {
    let t = type_of(inner, None, ctx)?;
    match op {
        UnaryOp::Neg => {
            if t.base() == Some(BaseType::Int) {
                Some(Ty::Base(BaseType::Int))
            } else {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    op_span,
                    format!(
                        "unary `-` requires `Int`, but the operand has type `{}`",
                        t.display()
                    ),
                ));
                None
            }
        }
        UnaryOp::Not => {
            if t.base() == Some(BaseType::Bool) {
                Some(Ty::Base(BaseType::Bool))
            } else {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    op_span,
                    format!(
                        "unary `!` requires `Bool`, but the operand has type `{}`",
                        t.display()
                    ),
                ));
                None
            }
        }
    }
}

/// v0.21: whether one operand is `Int` and the other `Float` — the mix the
/// no-coercion rule (ADR 0041) rejects with its own diagnostic.
fn numeric_mix(a: Option<BaseType>, b: Option<BaseType>) -> bool {
    matches!(
        (a, b),
        (Some(BaseType::Int), Some(BaseType::Float)) | (Some(BaseType::Float), Some(BaseType::Int))
    )
}

fn push_no_numeric_coercion(op: BinOp, span: Span, lt: &Ty, rt: &Ty, ctx: &mut Ctx) {
    ctx.errors.push(
        CompileError::new(
            "karn.types.no_numeric_coercion",
            span,
            format!(
                "operator `{}` cannot mix `Int` and `Float` operands; got `{}` and `{}`",
                op.name(),
                lt.display(),
                rt.display()
            ),
        )
        .with_note(
            "there is no implicit numeric coercion — convert explicitly with \
             `.toFloat()` on the `Int`, or `.round()`/`.floor()`/`.ceil()`/`.truncate()` \
             on the `Float`",
        ),
    );
}

fn check_binop(op: BinOp, lhs: &Expr, rhs: &Expr, ctx: &mut Ctx) -> Option<Ty> {
    // For `&&`, if the lhs is or contains an `is` test, propagate the
    // bindings into the rhs scope (so `r is Ok(n) && n > 0` works).
    if op == BinOp::And {
        let lt = type_of(lhs, Some(&Ty::Base(BaseType::Bool)), ctx);
        let bindings = collect_is_bindings(lhs, ctx);
        ctx.push_scope();
        for (name, ty) in bindings {
            ctx.bind(name, ty);
        }
        let rt = type_of(rhs, Some(&Ty::Base(BaseType::Bool)), ctx);
        ctx.pop_scope();
        let (lt, rt) = (lt?, rt?);
        if lt.base() != Some(BaseType::Bool) {
            ctx.errors.push(CompileError::new(
                "karn.types.type_mismatch",
                lhs.span,
                format!(
                    "operator `&&` requires `Bool` operands; left operand has type `{}`",
                    lt.display()
                ),
            ));
            return None;
        }
        if rt.base() != Some(BaseType::Bool) {
            ctx.errors.push(CompileError::new(
                "karn.types.type_mismatch",
                rhs.span,
                format!(
                    "operator `&&` requires `Bool` operands; right operand has type `{}`",
                    rt.display()
                ),
            ));
            return None;
        }
        return Some(Ty::Base(BaseType::Bool));
    }

    let lt = type_of(lhs, None, ctx);
    let rt = type_of(rhs, None, ctx);
    let (lt, rt) = (lt?, rt?);
    let span = lhs.span.merge(rhs.span);
    let lt_base = lt.base();
    let rt_base = rt.base();
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
            // v0.21: arithmetic is defined on `Int` and `Float`, never mixed
            // — there is no implicit numeric coercion (ADR 0041).
            match (lt_base, rt_base) {
                (Some(BaseType::Int), Some(BaseType::Int)) => Some(Ty::Base(BaseType::Int)),
                (Some(BaseType::Float), Some(BaseType::Float)) => Some(Ty::Base(BaseType::Float)),
                (Some(BaseType::Int), Some(BaseType::Float))
                | (Some(BaseType::Float), Some(BaseType::Int)) => {
                    push_no_numeric_coercion(op, span, &lt, &rt, ctx);
                    None
                }
                _ => {
                    let (side, side_span, ty) =
                        if !matches!(lt_base, Some(BaseType::Int) | Some(BaseType::Float)) {
                            ("left", lhs.span, &lt)
                        } else {
                            ("right", rhs.span, &rt)
                        };
                    ctx.errors.push(CompileError::new(
                        "karn.types.type_mismatch",
                        side_span,
                        format!(
                            "operator `{}` requires `Int` or `Float` operands; {side} operand has type `{}`",
                            op.name(),
                            ty.display()
                        ),
                    ));
                    None
                }
            }
        }
        BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
            if lt_base != rt_base || lt_base.is_none() {
                if numeric_mix(lt_base, rt_base) {
                    push_no_numeric_coercion(op, span, &lt, &rt, ctx);
                    return None;
                }
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    span,
                    format!(
                        "operator `{}` requires both operands to have the same base type; got `{}` and `{}`",
                        op.name(),
                        lt.display(),
                        rt.display()
                    ),
                ));
                return None;
            }
            if !matches!(
                lt_base,
                Some(BaseType::Int) | Some(BaseType::String) | Some(BaseType::Float)
            ) {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    span,
                    format!(
                        "operator `{}` is only defined on `Int`, `Float`, and `String`, not `{}`",
                        op.name(),
                        lt.display()
                    ),
                ));
                return None;
            }
            Some(Ty::Base(BaseType::Bool))
        }
        BinOp::Eq | BinOp::NotEq => {
            if lt_base.is_some() && rt_base.is_some() {
                if lt_base != rt_base {
                    if numeric_mix(lt_base, rt_base) {
                        push_no_numeric_coercion(op, span, &lt, &rt, ctx);
                        return None;
                    }
                    ctx.errors.push(CompileError::new(
                        "karn.types.type_mismatch",
                        span,
                        format!(
                            "operator `{}` requires both operands to have the same base type; got `{}` and `{}`",
                            op.name(),
                            lt.display(),
                            rt.display()
                        ),
                    ));
                    return None;
                }
            } else if !compatible(&lt, &rt) && !compatible(&rt, &lt) {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    span,
                    format!(
                        "operator `{}` requires both operands to have the same type; got `{}` and `{}`",
                        op.name(),
                        lt.display(),
                        rt.display()
                    ),
                ));
                return None;
            }
            Some(Ty::Base(BaseType::Bool))
        }
        BinOp::And | BinOp::Or => {
            if lt.base() != Some(BaseType::Bool) {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    lhs.span,
                    format!(
                        "operator `{}` requires `Bool` operands; left operand has type `{}`",
                        op.name(),
                        lt.display()
                    ),
                ));
                return None;
            }
            if rt.base() != Some(BaseType::Bool) {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    rhs.span,
                    format!(
                        "operator `{}` requires `Bool` operands; right operand has type `{}`",
                        op.name(),
                        rt.display()
                    ),
                ));
                return None;
            }
            Some(Ty::Base(BaseType::Bool))
        }
    }
}

fn check_call(
    name: &Ident,
    type_args: &[TypeRef],
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    if let Some(fn_decl) = ctx.input.fns.get(&name.name).cloned() {
        ctx.refs.record(name.span, SymbolKind::Fn, &name.name);
        return check_call_against_fn(name, &fn_decl, type_args, args, ctx);
    }
    // v0.20a: explicit type arguments only apply to (generic) functions.
    if !type_args.is_empty() {
        ctx.errors.push(CompileError::new(
            "karn.generics.type_arg_mismatch",
            span,
            format!(
                "`{}` is not a generic function — it takes no type arguments",
                name.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    // Could be a bare variant constructor with payload.
    let owners: Vec<TypeDecl> = ctx
        .input
        .types
        .values()
        .filter(|t| matches!(&t.body, TypeBody::Sum(s) if s.variants.iter().any(|v| v.name.name == name.name)))
        .cloned()
        .collect();
    if owners.len() == 1 {
        let owner = owners.into_iter().next().unwrap();
        return check_variant_construction(&owner, &name.name, args, span, ctx);
    }
    // Agent instantiation: `AgentName(key)` constructs an instance keyed by
    // `key`. The result type carries the agent's name so subsequent
    // `agent_instance.method(args)` lookups can find the agent's handler set.
    if let Some(agent) = ctx.input.agents.get(&name.name).cloned() {
        ctx.refs.record(name.span, SymbolKind::Agent, &name.name);
        let key_ty = resolve_type_ref(&agent.key_type, &ctx.input.types);
        if args.len() != 1 {
            ctx.errors.push(CompileError::new(
                "karn.agent.construction_arity",
                span,
                format!(
                    "agent `{}` is constructed with one key argument, but {} were given",
                    name.name,
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return None;
        }
        let arg_ty = type_of(&args[0], key_ty.as_ref(), ctx);
        if let (Some(a), Some(k)) = (arg_ty.as_ref(), key_ty.as_ref())
            && !compatible(a, k)
        {
            ctx.errors.push(CompileError::new(
                "karn.agent.key_mismatch",
                args[0].span,
                format!(
                    "agent `{}` key is `{}`, but a value of type `{}` was given",
                    name.name,
                    k.display(),
                    a.display()
                ),
            ));
        }
        return Some(Ty::Named {
            name: name.name.clone(),
            kind: NamedKind::Record,
        });
    }
    // v0.20a: value application — calling a scope binding (param/local) of
    // function type. Placed AFTER fns/variants/agents: putting scope first
    // would change the meaning of currently-passing programs (the additive
    // guard); the resulting ident/call precedence asymmetry is pre-existing
    // and documented in §5.
    if let Some(ty) = ctx.lookup(&name.name) {
        return match ty {
            Ty::Fn { params, ret } => check_value_application(name, &params, &ret, args, span, ctx),
            other => {
                // Relocated from the resolver (which has no type info): a
                // non-function-typed value called as a function.
                ctx.errors.push(
                    CompileError::new(
                        "karn.resolve.param_as_function",
                        span,
                        format!(
                            "`{}` has type `{}` and is not callable",
                            name.name,
                            other.display()
                        ),
                    )
                    .with_note("only values of function type can be applied"),
                );
                for a in args {
                    let _ = type_of(a, None, ctx);
                }
                None
            }
        };
    }
    let _ = span;
    None
}

/// v0.20a: type-check the application of a function-typed value (`f(x)`
/// where `f` is a param or local of type `A -> B`). Reuses the ordinary
/// argument rules; an effectful result (`ret` is `Effect[_]`) is an effect
/// operation, legal only in an effectful context — the same confinement a
/// capability call obeys.
/// v0.20a: type-check a lambda (`(params) => body`). Two paths:
///
/// - **Expected function type** (ground — guaranteed by the generic
///   instantiation order): params type contextually (an annotation must be
///   compatible with the expected param), the body checks against the
///   expected return with `effectful` derived from it, and the result is the
///   expected type (checking-mode bidirectionality).
/// - **Unconstrained**: every param must be annotated
///   (`karn.lambda.unannotated_param`); effectfulness is decided by a
///   syntactic pre-scan of the body (`<-`, capability calls, effectful named
///   or value calls), and the result type wraps in `Effect` when it fired.
///
/// The enclosing handler's capability map and `given` tracking stay shared —
/// a lambda may close over and call a `given` capability (ADR 0033). The
/// frame swap forbids `commit` inside a lambda (`agent_state_ty = None` →
/// the existing `karn.commit.outside_agent`).
fn check_lambda(lambda: &LambdaExpr, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    let expected_fn = match expected {
        Some(Ty::Fn { params, ret }) => Some((params.clone(), (**ret).clone())),
        _ => None,
    };

    // Establish param types.
    let mut param_tys: Vec<Ty> = Vec::new();
    let mut scope: HashMap<String, Ty> = HashMap::new();
    if let Some((eps, _)) = &expected_fn {
        if eps.len() != lambda.params.len() {
            ctx.errors.push(CompileError::new(
                "karn.types.lambda_mismatch",
                lambda.span,
                format!(
                    "this lambda takes {} parameter(s), but a function of {} parameter(s) is expected",
                    lambda.params.len(),
                    eps.len()
                ),
            ));
            return None;
        }
        for (p, ep) in lambda.params.iter().zip(eps) {
            let ty = match &p.type_ref {
                Some(tr) => {
                    let annotated = resolve_type_ref(tr, &ctx.input.types)?;
                    if !compatible(ep, &annotated) {
                        ctx.errors.push(CompileError::new(
                            "karn.types.lambda_mismatch",
                            p.span,
                            format!(
                                "lambda parameter `{}` is annotated `{}`, but `{}` is expected here",
                                p.name.name,
                                annotated.display(),
                                ep.display()
                            ),
                        ));
                    }
                    annotated
                }
                None => {
                    // v0.27 (ADR 0056): a param typed from the expected fn
                    // type gets an inferred-type inlay hint at its name.
                    if p.name.name != "_" {
                        ctx.hints.record(p.name.span, format!(": {}", ep.display()));
                    }
                    ep.clone()
                }
            };
            scope.insert(p.name.name.clone(), ty.clone());
            param_tys.push(ty);
        }
    } else {
        let mut missing = false;
        for p in &lambda.params {
            match &p.type_ref {
                Some(tr) => {
                    let ty = resolve_type_ref(tr, &ctx.input.types)?;
                    scope.insert(p.name.name.clone(), ty.clone());
                    param_tys.push(ty);
                }
                None => {
                    ctx.errors.push(
                        CompileError::new(
                            "karn.lambda.unannotated_param",
                            p.span,
                            format!(
                                "lambda parameter `{}` needs a type annotation — no function type is expected here to infer it from",
                                p.name.name
                            ),
                        )
                        .with_note("annotate the parameter (e.g. `(x: Int) => …`), or pass the lambda where a function type is expected"),
                    );
                    missing = true;
                }
            }
        }
        if missing {
            return None;
        }
    }

    ctx.scopes.push(scope);

    // v0.20a generics: an expected return that still carries a *flexible*
    // type variable (pass 2 of inference — the lambda's result is what binds
    // it) is treated as unconstrained: the body types bottom-up and the
    // caller's unify captures the variable. v0.20b: the enclosing generic
    // fn's own type parameters are *rigid* — an expected return of
    // `Option[A]` inside `find[A]`'s body is fully constrained, and the
    // body's `None`/`[]`/`Map.empty()` may infer from it.
    let ret_constrained = expected_fn
        .as_ref()
        .is_some_and(|(_, er)| !contains_flexible_var(er, &ctx.type_vars));

    // Decide the body's effectfulness BEFORE typing it: the effect gates
    // (`bind_in_pure_context`, `capability_in_pure_context`, the fn-value
    // gate) fire during typing off `ctx.effectful`.
    let body_effectful = match &expected_fn {
        Some((_, er)) if ret_constrained => er.is_effect(),
        _ => body_performs_effects(&lambda.body, ctx),
    };

    // Frame swap (save/restore — the capability map and given-tracking stay
    // shared so closures over capabilities work and count as uses).
    let saved_effectful = ctx.effectful;
    let saved_return_ty = ctx.return_ty.clone();
    let saved_return_ty_span = ctx.return_ty_span;
    let saved_agent_state_ty = ctx.agent_state_ty.take();
    let saved_commit_seen = ctx.commit_seen;
    ctx.effectful = body_effectful;
    ctx.return_ty = match &expected_fn {
        Some((_, er)) if ret_constrained => er.clone(),
        // Placeholder: no diagnostic path can consult it — the pre-scan sets
        // `effectful` whenever a `<-` exists, so `bind_in_pure_context`'s
        // return-type label is unreachable here.
        _ => Ty::Unit,
    };
    ctx.return_ty_span = lambda.span;
    ctx.commit_seen = false;

    let body_expected = match &expected_fn {
        Some((_, er)) if ret_constrained => Some(er.clone()),
        _ => None,
    };
    let body_ty = type_of(&lambda.body, body_expected.as_ref(), ctx);

    ctx.effectful = saved_effectful;
    ctx.return_ty = saved_return_ty;
    ctx.return_ty_span = saved_return_ty_span;
    ctx.agent_state_ty = saved_agent_state_ty;
    ctx.commit_seen = saved_commit_seen;
    ctx.scopes.pop();

    match expected_fn {
        // Var-bearing expected return: report the actual function type and
        // let the caller's unify bind the variable.
        Some((eps, _)) if !ret_constrained => {
            let bt = body_ty?;
            let ret = if body_effectful && !bt.is_effect() {
                Ty::Effect(Box::new(bt))
            } else {
                bt
            };
            Some(Ty::Fn {
                params: eps,
                ret: Box::new(ret),
            })
        }
        Some((eps, er)) => {
            if let Some(bt) = body_ty.as_ref() {
                // A pure body against an effectful expected return auto-lifts
                // (the emitter's async arrow realises the lifted Promise).
                let lifted =
                    maybe_auto_lift(Some(bt.clone()), Some(&er)).unwrap_or_else(|| bt.clone());
                if !compatible(&lifted, &er) {
                    ctx.errors.push(CompileError::new(
                        "karn.types.lambda_mismatch",
                        lambda.body.span,
                        format!(
                            "lambda body has type `{}`, but `{}` is expected",
                            bt.display(),
                            er.display()
                        ),
                    ));
                    return None;
                }
            }
            Some(Ty::Fn {
                params: eps,
                ret: Box::new(er),
            })
        }
        None => {
            let bt = body_ty?;
            let ret = if body_effectful && !bt.is_effect() {
                Ty::Effect(Box::new(bt))
            } else {
                bt
            };
            Some(Ty::Fn {
                params: param_tys,
                ret: Box::new(ret),
            })
        }
    }
}

/// v0.20a: the syntactic pre-scan deciding a lambda's effectfulness in an
/// unconstrained position, run after the lambda's params are in scope and
/// before typing. True on: an `<-` bind; a capability static-call; a call on
/// a scope binding or named function whose type/signature returns `Effect`;
/// `Effect.pure`. Does **not** descend into nested lambdas — an inner
/// lambda's effects are its own.
fn body_performs_effects(e: &Expr, ctx: &Ctx) -> bool {
    fn block_performs(b: &Block, ctx: &Ctx) -> bool {
        for s in &b.statements {
            match s {
                Statement::EffectLet(_) => return true,
                Statement::Let(l) => {
                    if body_performs_effects(&l.value, ctx) {
                        return true;
                    }
                }
                Statement::Commit(c) => {
                    if body_performs_effects(&c.value, ctx) {
                        return true;
                    }
                }
                Statement::Assert(a) => {
                    if body_performs_effects(&a.value, ctx) {
                        return true;
                    }
                }
            }
        }
        body_performs_effects(&b.tail, ctx)
    }
    match &e.kind {
        ExprKind::Lambda(_) => false,
        ExprKind::Block(b) => block_performs(b, ctx),
        ExprKind::EffectPure(_) => true,
        // A capability operation call (`Cap.op(…)`) or `Effect.pure` shape.
        ExprKind::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            if let ExprKind::Ident(id) = &receiver.kind
                && ctx.capabilities.contains_key(&id.name)
            {
                return true;
            }
            // v0.20b: the effectful kernel fold returns `Effect[Acc]`.
            // Detected by name (the pre-scan is syntactic); a false positive
            // only *permits* effect syntax — a pure body still types pure.
            if method.name == FOLD_EFF {
                return true;
            }
            body_performs_effects(receiver, ctx)
                || args.iter().any(|a| body_performs_effects(a, ctx))
        }
        ExprKind::Call { name, args, .. } => {
            if let Some(Ty::Fn { ret, .. }) = ctx.lookup(&name.name)
                && ret.is_effect()
            {
                return true;
            }
            if let Some(f) = ctx.input.fns.get(&name.name)
                && matches!(f.return_type, TypeRef::Effect(..))
            {
                return true;
            }
            args.iter().any(|a| body_performs_effects(a, ctx))
        }
        ExprKind::ConstructorCall { args, .. } => {
            args.iter().any(|a| body_performs_effects(a, ctx))
        }
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            body_performs_effects(cond, ctx)
                || block_performs(then_block, ctx)
                || block_performs(else_block, ctx)
        }
        ExprKind::Match { discriminant, arms } => {
            body_performs_effects(discriminant, ctx)
                || arms.iter().any(|a| match &a.body {
                    MatchBody::Expr(e) => body_performs_effects(e, ctx),
                    MatchBody::Block(b) => block_performs(b, ctx),
                })
        }
        ExprKind::BinOp(_, l, r) => body_performs_effects(l, ctx) || body_performs_effects(r, ctx),
        ExprKind::UnaryOp(_, i)
        | ExprKind::Paren(i)
        | ExprKind::Ok(i)
        | ExprKind::Err(i)
        | ExprKind::Some(i)
        | ExprKind::Question(i)
        | ExprKind::Assert(i) => body_performs_effects(i, ctx),
        ExprKind::RecordConstruction { fields, .. } => fields.iter().any(|f| {
            f.value
                .as_ref()
                .is_some_and(|v| body_performs_effects(v, ctx))
        }),
        ExprKind::RecordSpread {
            base, overrides, ..
        } => {
            body_performs_effects(base, ctx)
                || overrides.iter().any(|f| {
                    f.value
                        .as_ref()
                        .is_some_and(|v| body_performs_effects(v, ctx))
                })
        }
        ExprKind::FieldAccess { receiver, .. } => body_performs_effects(receiver, ctx),
        ExprKind::Is { value, .. } => body_performs_effects(value, ctx),
        ExprKind::Mock { args, .. } => args.iter().any(|a| body_performs_effects(a, ctx)),
        ExprKind::ListLit(elems) => elems.iter().any(|e| body_performs_effects(e, ctx)),
        ExprKind::Ident(_)
        | ExprKind::IntLit(_)
        | ExprKind::FloatLit { .. }
        | ExprKind::StrLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::None
        | ExprKind::UnitLit => false,
    }
}

fn check_value_application(
    name: &Ident,
    params: &[Ty],
    ret: &Ty,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    if ret.is_effect() && !ctx.effectful {
        ctx.errors.push(
            CompileError::new(
                "karn.effect.fn_value_in_pure_context",
                span,
                format!(
                    "`{}` is an effectful function (`{}`) and cannot be called in a pure context",
                    name.name,
                    Ty::Fn {
                        params: params.to_vec(),
                        ret: Box::new(ret.clone())
                    }
                    .display()
                ),
            )
            .with_note(
                "effectful function values may only be called where the enclosing body is effectful (its return type is an Effect)",
            ),
        );
    }
    if params.len() != args.len() {
        ctx.errors.push(CompileError::new(
            "karn.types.call_arity",
            span,
            format!(
                "`{}` takes {} argument(s), but {} were given",
                name.name,
                params.len(),
                args.len()
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    for (arg, param_ty) in args.iter().zip(params) {
        let arg_ty = type_of(arg, Some(param_ty), ctx);
        if let Some(a) = arg_ty.as_ref()
            && !compatible(a, param_ty)
        {
            ctx.errors.push(CompileError::new(
                "karn.types.argument_mismatch",
                arg.span,
                format!(
                    "argument has type `{}`, but `{}` expects `{}`",
                    a.display(),
                    name.name,
                    param_ty.display()
                ),
            ));
        }
    }
    Some(ret.clone())
}

/// v0.20a: instantiate and check a call to a generic function. Two-pass
/// argument-directed inference: pass 1 types every **non-lambda** argument
/// left-to-right against the (possibly still Var-bearing) expected — safe
/// because every expected-driven feature in `type_of` matches concrete
/// variants, so a Var falls through benignly (pinned by a unit test) — and
/// unifies; pass 2 types **lambda** arguments against the now-substituted
/// expecteds (a lambda whose expected params are still Var-bearing is
/// uninferable) and unifies the result, capturing return-position variables.
/// Conflicts demand exact equality (`karn.generics.type_arg_mismatch`); the
/// explicit `name[T](…)` form builds the substitution directly.
fn check_generic_call(
    name: &Ident,
    fn_decl: &FnDecl,
    type_args: &[TypeRef],
    args: &[Expr],
    ctx: &mut Ctx,
) -> Option<Ty> {
    let vars: HashSet<String> = fn_decl
        .type_params
        .iter()
        .map(|tp| tp.name.name.clone())
        .collect();
    if fn_decl.params.len() != args.len() {
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    let var_params: Vec<Option<Ty>> = fn_decl
        .params
        .iter()
        .map(|p| resolve_type_ref_in(&p.type_ref, &ctx.input.types, &vars))
        .collect();
    let ret_pattern = resolve_type_ref_in(&fn_decl.return_type, &ctx.input.types, &vars)?;

    let mut subst: HashMap<String, Ty> = HashMap::new();
    if !type_args.is_empty() {
        if type_args.len() != fn_decl.type_params.len() {
            ctx.errors.push(CompileError::new(
                "karn.generics.type_arg_mismatch",
                name.span,
                format!(
                    "`{}` takes {} type argument(s), but {} were given",
                    name.name,
                    fn_decl.type_params.len(),
                    type_args.len()
                ),
            ));
            return None;
        }
        for (tp, ta) in fn_decl.type_params.iter().zip(type_args) {
            // Resolve explicit type args with the *enclosing* fn's type
            // params in scope, so `identity[A](x)` inside a generic body
            // works.
            let ty = resolve_type_ref_in(ta, &ctx.input.types, &ctx.type_vars)?;
            subst.insert(tp.name.name.clone(), ty);
        }
    }

    let mut arg_tys: Vec<Option<Ty>> = vec![None; args.len()];
    // Pass 1 — non-lambda arguments.
    for (i, arg) in args.iter().enumerate() {
        if matches!(arg.kind, ExprKind::Lambda(_)) {
            continue;
        }
        let expected = var_params[i].as_ref().map(|p| substitute(p, &subst));
        let ty = type_of(arg, expected.as_ref(), ctx);
        if let (Some(pattern), Some(actual)) = (var_params[i].as_ref(), ty.as_ref())
            && !unify(pattern, actual, &mut subst)
        {
            ctx.errors.push(CompileError::new(
                "karn.generics.type_arg_mismatch",
                arg.span,
                format!(
                    "argument {} infers a type for `{}`'s type parameter that conflicts with an earlier argument — annotate with `{}[T](…)`",
                    i + 1,
                    name.name,
                    name.name
                ),
            ));
            return None;
        }
        arg_tys[i] = ty;
    }
    // Pass 2 — lambda arguments, against substituted expecteds.
    for (i, arg) in args.iter().enumerate() {
        if !matches!(arg.kind, ExprKind::Lambda(_)) {
            continue;
        }
        let expected = var_params[i].as_ref().map(|p| substitute(p, &subst));
        let params_unconstrained = matches!(
            expected.as_ref(),
            Some(Ty::Fn { params, .. }) if params.iter().any(contains_var)
        );
        let fully_annotated = matches!(
            &arg.kind,
            ExprKind::Lambda(l) if l.params.iter().all(|p| p.type_ref.is_some())
        );
        if params_unconstrained && !fully_annotated {
            ctx.errors.push(
                CompileError::new(
                    "karn.generics.uninferable_type_arg",
                    arg.span,
                    format!(
                        "the lambda's parameter types depend on `{}`'s type parameters, which the other arguments do not determine",
                        name.name
                    ),
                )
                .with_note("annotate the lambda's parameters, or give explicit type arguments: `name[T](…)`"),
            );
            return None;
        }
        // A fully-annotated lambda grounds the variables itself: type it
        // bottom-up and let unify capture them.
        let ty = if params_unconstrained {
            type_of(arg, None, ctx)
        } else {
            type_of(arg, expected.as_ref(), ctx)
        };
        if let (Some(pattern), Some(actual)) = (var_params[i].as_ref(), ty.as_ref())
            && !unify(pattern, actual, &mut subst)
        {
            ctx.errors.push(CompileError::new(
                "karn.generics.type_arg_mismatch",
                arg.span,
                format!(
                    "the lambda's type conflicts with `{}`'s inferred type arguments",
                    name.name
                ),
            ));
            return None;
        }
        arg_tys[i] = ty;
    }
    // Every type parameter must now be determined.
    for tp in &fn_decl.type_params {
        if !subst.contains_key(&tp.name.name) {
            ctx.errors.push(
                CompileError::new(
                    "karn.generics.uninferable_type_arg",
                    name.span,
                    format!(
                        "type parameter `{}` of `{}` is neither inferable from the arguments nor given explicitly",
                        tp.name.name, name.name
                    ),
                )
                .with_label(tp.span, "declared here")
                .with_note("give explicit type arguments: `name[T](…)`"),
            );
            return None;
        }
    }
    // Final compatibility over the fully-ground parameter types.
    let mut ok = true;
    for (i, (pattern, arg)) in var_params.iter().zip(args).enumerate() {
        let (Some(pattern), Some(arg_ty)) = (pattern.as_ref(), arg_tys[i].as_ref()) else {
            continue;
        };
        let ground = substitute(pattern, &subst);
        if !compatible(arg_ty, &ground) {
            ctx.errors.push(CompileError::new(
                "karn.types.argument_mismatch",
                arg.span,
                format!(
                    "argument {} to `{}` has type `{}`, but `{}` is expected",
                    i + 1,
                    name.name,
                    arg_ty.display(),
                    ground.display()
                ),
            ));
            ok = false;
        }
    }
    if !ok {
        return None;
    }
    let ret = substitute(&ret_pattern, &subst);
    // v0.20b: the return is ground *up to the caller's rigid type
    // parameters* — a generic fn calling another generic fn (karn.list's
    // `map` calling `reverse`) legitimately instantiates the callee at its
    // own rigid vars, which flow through `compatible` by name-equality.
    Some(ret)
}

fn check_call_against_fn(
    name: &Ident,
    fn_decl: &FnDecl,
    type_args: &[TypeRef],
    args: &[Expr],
    ctx: &mut Ctx,
) -> Option<Ty> {
    // v0.20a: generic functions take the instantiation path; the
    // non-generic path below runs byte-identically to v0.19 (the additive
    // guard). Explicit type args on a non-generic fn are rejected.
    if !fn_decl.type_params.is_empty() {
        return check_generic_call(name, fn_decl, type_args, args, ctx);
    }
    if !type_args.is_empty() {
        ctx.errors.push(CompileError::new(
            "karn.generics.type_arg_mismatch",
            name.span,
            format!(
                "`{}` is not a generic function — it takes no type arguments",
                name.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    if fn_decl.params.len() != args.len() {
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    let resolved_params: Vec<(Option<Ty>, &Param)> = fn_decl
        .params
        .iter()
        .map(|p| (resolve_type_ref(&p.type_ref, &ctx.input.types), p))
        .collect();
    let mut ok = true;
    for (i, ((param_ty, param), arg)) in resolved_params.iter().zip(args.iter()).enumerate() {
        let arg_ty = type_of(arg, param_ty.as_ref(), ctx);
        let (Some(arg_ty), Some(param_ty)) = (arg_ty, param_ty.as_ref()) else {
            ok = false;
            continue;
        };
        if !compatible(&arg_ty, param_ty) {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.argument_mismatch",
                    arg.span,
                    format!(
                        "argument {} to `{}` has type `{}`, but parameter `{}` expects `{}`",
                        i + 1,
                        name.name,
                        arg_ty.display(),
                        param.name.name,
                        param_ty.display()
                    ),
                )
                .with_label(param.span, "parameter declared here"),
            );
            ok = false;
        }
    }
    if !ok {
        return None;
    }
    resolve_type_ref(&fn_decl.return_type, &ctx.input.types)
}

fn check_variant_construction(
    owner: &TypeDecl,
    variant_name: &str,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let TypeBody::Sum(s) = &owner.body else {
        return None;
    };
    let variant = s.variants.iter().find(|v| v.name.name == variant_name)?;
    if variant.payload.len() != args.len() {
        ctx.errors.push(
            CompileError::new(
                "karn.types.variant_arity",
                span,
                format!(
                    "variant `{}` of `{}` expects {} argument(s), but {} were given",
                    variant_name,
                    owner.name.name,
                    variant.payload.len(),
                    args.len()
                ),
            )
            .with_label(variant.span, "variant declared here"),
        );
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    let mut ok = true;
    for (i, (field, arg)) in variant.payload.iter().zip(args.iter()).enumerate() {
        let expected = resolve_type_ref(&field.type_ref, &ctx.input.types);
        let actual = type_of(arg, expected.as_ref(), ctx);
        let (Some(actual), Some(expected)) = (actual, expected) else {
            ok = false;
            continue;
        };
        if !compatible(&actual, &expected) {
            ctx.errors.push(CompileError::new(
                "karn.types.variant_payload_mismatch",
                arg.span,
                format!(
                    "argument {} to variant `{}` has type `{}`, but field `{}` expects `{}`",
                    i + 1,
                    variant_name,
                    actual.display(),
                    field.name.name,
                    expected.display()
                ),
            ));
            ok = false;
        }
    }
    if !ok {
        return None;
    }
    Some(named_ty(owner))
}

fn check_if(
    cond: &Expr,
    then_block: &Block,
    else_block: &Block,
    if_span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let cond_ty = type_of(cond, Some(&Ty::Base(BaseType::Bool)), ctx);
    if let Some(c) = &cond_ty
        && c.base() != Some(BaseType::Bool)
    {
        ctx.errors.push(CompileError::new(
            "karn.types.if_non_bool_cond",
            cond.span,
            format!(
                "`if` condition must have type `Bool`, but has type `{}`",
                c.display()
            ),
        ));
    }
    // `is` bindings in the condition flow into the then-branch.
    let bindings = collect_is_bindings(cond, ctx);
    ctx.push_scope();
    for (name, ty) in bindings {
        ctx.bind(name, ty);
    }
    let then_ty = type_of_block(then_block, expected, ctx);
    ctx.pop_scope();
    let else_ty = type_of_block(else_block, expected, ctx);
    match (then_ty, else_ty) {
        (Some(t), Some(e)) => {
            if t == e {
                Some(t)
            } else {
                ctx.errors.push(
                    CompileError::new(
                        "karn.types.if_branch_mismatch",
                        if_span,
                        format!(
                            "`if` branches produce different types: `{}` and `{}`",
                            t.display(),
                            e.display()
                        ),
                    )
                    .with_label(
                        then_block.tail.span,
                        format!("then-branch has type `{}`", t.display()),
                    )
                    .with_label(
                        else_block.tail.span,
                        format!("else-branch has type `{}`", e.display()),
                    )
                    .with_note("both branches of an `if` expression must produce the same type"),
                );
                None
            }
        }
        _ => None,
    }
}

fn check_ok(inner: &Expr, span: Span, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    // v0.9: `Ok` is now overloaded between `Result.Ok` and `HttpResult.Ok`.
    // First consult the expected type (propagated from let-annotations, match
    // arms, and the enclosing return type via tail-position auto-lift).
    let in_result = surrounding_result(expected, &ctx.return_ty);
    let in_http = expected
        .and_then(peel_to_http_result)
        .or_else(|| peel_to_http_result(&ctx.return_ty));
    match (in_result.clone(), in_http.clone()) {
        (Some(_), Some(_)) => {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.ambiguous_constructor",
                    span,
                    "ambiguous constructor `Ok`: could be `Result.Ok` or `HttpResult.Ok`",
                )
                .with_note("qualify it as `Result.Ok(...)` or `HttpResult.Ok(...)`"),
            );
            // Best-effort: still type the inner.
            let _ = type_of(inner, None, ctx);
            None
        }
        (None, Some(t_ty)) => {
            let inner_ty = type_of(inner, Some(&t_ty), ctx)?;
            if !compatible(&inner_ty, &t_ty) {
                ctx.errors.push(CompileError::new(
                    "karn.types.ok_value_mismatch",
                    inner.span,
                    format!(
                        "`Ok(...)` value has type `{}`, but the surrounding context expects `HttpResult[{}]`",
                        inner_ty.display(),
                        t_ty.display(),
                    ),
                ));
                return None;
            }
            Some(Ty::HttpResult(Box::new(t_ty)))
        }
        (Some((t_ty, e_ty)), None) => {
            let inner_ty = type_of(inner, Some(&t_ty), ctx)?;
            if !compatible(&inner_ty, &t_ty) {
                ctx.errors.push(
                    CompileError::new(
                        "karn.types.ok_value_mismatch",
                        inner.span,
                        format!(
                            "`Ok(...)` value has type `{}`, but the surrounding context expects `Result[{}, {}]`",
                            inner_ty.display(),
                            t_ty.display(),
                            e_ty.display()
                        ),
                    )
                    .with_label(ctx.return_ty_span, "context's expected `Result` type"),
                );
                return None;
            }
            Some(Ty::Result(Box::new(t_ty), Box::new(e_ty)))
        }
        (None, None) => {
            let _ = type_of(inner, None, ctx);
            ctx.errors.push(
                CompileError::new(
                    "karn.types.cannot_infer_result_type_params",
                    span,
                    "cannot infer the type parameter of `Ok(...)`",
                )
                .with_note(
                    "add a `let` annotation (`let x: Result[T, E] = Ok(...)`) \
                     or declare the enclosing function's return type as `Result[T, E]` or `HttpResult[T]`",
                ),
            );
            None
        }
    }
}

/// Peel one optional `Effect[_]` wrapper to expose an underlying `HttpResult[T]`.
fn peel_to_http_result(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::HttpResult(inner) => Some((**inner).clone()),
        Ty::Effect(inner) => peel_to_http_result(inner),
        _ => None,
    }
}

/// Type-check construction of an `HttpResult[T]` variant (v0.9 §4.3).
///
/// Variants come in three payload shapes:
/// - `Value` (`Ok`, `Created`) — argument's type is `T`. `T` is taken from
///   the expected type if available; otherwise reported as ambiguous.
/// - `Message` (`BadRequest`, `Conflict`, `UnprocessableEntity`,
///   `ServerError`) — argument must be `String`.
/// - `None` (`NoContent`, `Unauthorized`, `Forbidden`, `NotFound`) — no
///   argument permitted; `T` is taken from the expected type or left
///   inferred.
fn check_http_variant(
    span: Span,
    variant: HttpVariant,
    args: &[Expr],
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let expected_t = expected
        .and_then(peel_to_http_result)
        .or_else(|| peel_to_http_result(&ctx.return_ty));
    match variant.payload {
        HttpVariantPayload::Value => {
            if args.len() != 1 {
                ctx.errors.push(CompileError::new(
                    "karn.types.variant_arity",
                    span,
                    format!(
                        "`HttpResult.{}` expects 1 argument, but {} were given",
                        variant.name,
                        args.len(),
                    ),
                ));
                return None;
            }
            let arg_ty = type_of(&args[0], expected_t.as_ref(), ctx)?;
            let t_ty = match (expected_t, arg_ty.clone()) {
                (Some(t), _) => {
                    if !compatible(&arg_ty, &t) {
                        ctx.errors.push(CompileError::new(
                            "karn.types.ok_value_mismatch",
                            args[0].span,
                            format!(
                                "`HttpResult.{}` value has type `{}`, but the surrounding context expects `HttpResult[{}]`",
                                variant.name,
                                arg_ty.display(),
                                t.display(),
                            ),
                        ));
                        return None;
                    }
                    t
                }
                (None, t) => t,
            };
            Some(Ty::HttpResult(Box::new(t_ty)))
        }
        HttpVariantPayload::Message => {
            if args.len() != 1 {
                ctx.errors.push(CompileError::new(
                    "karn.types.variant_arity",
                    span,
                    format!(
                        "`HttpResult.{}` expects 1 `String` argument, but {} were given",
                        variant.name,
                        args.len(),
                    ),
                ));
                return None;
            }
            let arg_ty = type_of(&args[0], Some(&Ty::Base(BaseType::String)), ctx)?;
            if !compatible(&arg_ty, &Ty::Base(BaseType::String)) {
                ctx.errors.push(CompileError::new(
                    "karn.types.argument_mismatch",
                    args[0].span,
                    format!(
                        "`HttpResult.{}` expects a `String` message, but got `{}`",
                        variant.name,
                        arg_ty.display(),
                    ),
                ));
                return None;
            }
            // Inner T is irrelevant for message variants but the type needs
            // a concrete payload. Pick `()` when nothing is known; otherwise
            // use the propagated expected type.
            let t_ty = expected_t.unwrap_or(Ty::Unit);
            Some(Ty::HttpResult(Box::new(t_ty)))
        }
        HttpVariantPayload::None => {
            if !args.is_empty() {
                ctx.errors.push(CompileError::new(
                    "karn.types.variant_arity",
                    span,
                    format!(
                        "`HttpResult.{}` takes no arguments, but {} were given",
                        variant.name,
                        args.len(),
                    ),
                ));
                return None;
            }
            let t_ty = expected_t.unwrap_or(Ty::Unit);
            Some(Ty::HttpResult(Box::new(t_ty)))
        }
    }
}

fn check_err(inner: &Expr, span: Span, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    let surrounding = surrounding_result(expected, &ctx.return_ty);
    let expected_e = surrounding.as_ref().map(|(_, e)| e.clone());
    let inner_ty = type_of(inner, expected_e.as_ref(), ctx)?;
    match surrounding {
        Some((t_ty, e_ty)) => {
            if !compatible(&inner_ty, &e_ty) {
                ctx.errors.push(
                    CompileError::new(
                        "karn.types.err_value_mismatch",
                        inner.span,
                        format!(
                            "`Err(...)` value has type `{}`, but the surrounding context expects `Result[{}, {}]`",
                            inner_ty.display(),
                            t_ty.display(),
                            e_ty.display()
                        ),
                    )
                    .with_label(ctx.return_ty_span, "context's expected `Result` type"),
                );
                return None;
            }
            Some(Ty::Result(Box::new(t_ty), Box::new(e_ty)))
        }
        None => {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.cannot_infer_result_type_params",
                    span,
                    "cannot infer the value type parameter of `Err(...)`",
                )
                .with_note(
                    "add a `let` annotation or declare the enclosing function's return type as `Result[T, E]`",
                ),
            );
            None
        }
    }
}

fn check_some(inner: &Expr, _span: Span, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    let expected_inner = expected
        .and_then(peel_to_option)
        .or_else(|| peel_to_option(&ctx.return_ty));
    let inner_ty = type_of(inner, expected_inner.as_ref(), ctx)?;
    if let Some(exp) = &expected_inner
        && !compatible(&inner_ty, exp)
    {
        ctx.errors.push(CompileError::new(
            "karn.types.some_value_mismatch",
            inner.span,
            format!(
                "`Some(...)` value has type `{}`, but the surrounding context expects `Option[{}]`",
                inner_ty.display(),
                exp.display()
            ),
        ));
        return None;
    }
    Some(Ty::Option(Box::new(inner_ty)))
}

fn check_none(span: Span, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    if let Some(t) = expected.and_then(peel_to_option) {
        return Some(Ty::Option(Box::new(t)));
    }
    if let Some(t) = peel_to_option(&ctx.return_ty) {
        return Some(Ty::Option(Box::new(t)));
    }
    ctx.errors.push(
        CompileError::new(
            "karn.types.cannot_infer_option_type_param",
            span,
            "cannot infer the value type of `None`",
        )
        .with_note(
            "add an annotation (`let x: Option[T] = None`) or use `None` where the context expects an `Option`",
        ),
    );
    None
}

fn surrounding_result(expected: Option<&Ty>, return_ty: &Ty) -> Option<(Ty, Ty)> {
    if let Some(t) = expected
        && let Some(pair) = peel_to_result(t)
    {
        return Some(pair);
    }
    peel_to_result(return_ty)
}

/// Peel one optional `Effect[_]` wrapper to expose an underlying `Result[T, E]`.
/// Used by `Ok` / `Err` checking in v0.7.1 so that bare constructors in
/// `Effect[Result[T, E]]` tail positions can pick up the surrounding type's
/// parameters via the auto-lift propagation.
fn peel_to_result(ty: &Ty) -> Option<(Ty, Ty)> {
    match ty {
        Ty::Result(t, e) => Some(((**t).clone(), (**e).clone())),
        Ty::Effect(inner) => peel_to_result(inner),
        _ => None,
    }
}

/// Companion to `peel_to_result` for `Option[T]`.
fn peel_to_option(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::Option(t) => Some((**t).clone()),
        Ty::Effect(inner) => peel_to_option(inner),
        _ => None,
    }
}

/// Companion to `peel_to_result` for `List[T]` (v0.20b) — the expected
/// element type of a list literal, looking through `Effect[_]` so tail
/// auto-lift positions still propagate it.
fn peel_to_list(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::List(t) => Some((**t).clone()),
        Ty::Effect(inner) => peel_to_list(inner),
        _ => None,
    }
}

/// Companion to `peel_to_list` for `Map[K, V]` (v0.20b).
fn peel_to_map(ty: &Ty) -> Option<(Ty, Ty)> {
    match ty {
        Ty::Map(k, v) => Some(((**k).clone(), (**v).clone())),
        Ty::Effect(inner) => peel_to_map(inner),
        _ => None,
    }
}

/// v0.20b: `List.empty()` / `Map.empty()` — the built-in collection statics.
/// Their element/key/value types are exactly as uninferable as an empty
/// `[]`, so they share `karn.types.uninferable_element_type`. The resolver
/// has already rejected any static other than `empty`.
fn check_collection_static(
    type_name: &Ident,
    method: &Ident,
    args: &[Expr],
    span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    for a in args {
        let _ = type_of(a, None, ctx);
    }
    if method.name != "empty" {
        // The resolver owns the unknown-static diagnostic; don't double up.
        return None;
    }
    if !args.is_empty() {
        ctx.errors.push(CompileError::new(
            "karn.types.method_arity",
            span,
            format!("`{}.empty` takes no arguments", type_name.name),
        ));
        return None;
    }
    let inferred = match type_name.name.as_str() {
        LIST => expected
            .and_then(peel_to_list)
            .map(|t| Ty::List(Box::new(t))),
        _ => expected
            .and_then(peel_to_map)
            .map(|(k, v)| Ty::Map(Box::new(k), Box::new(v))),
    };
    if inferred.is_none() {
        ctx.errors.push(
            CompileError::new(
                "karn.types.uninferable_element_type",
                span,
                format!(
                    "`{}.empty()` has no expected type to infer its type arguments from",
                    type_name.name
                ),
            )
            .with_note(
                "annotate the binding (`let xs: List[T] = List.empty()`) or use it where the collection type is expected",
            ),
        );
    }
    inferred
}

/// v0.20b: type a built-in `List[T]` kernel method. The fold accumulator is
/// inferred from the `init` argument, then the step function checks against
/// the fully-instantiated function type (params type contextually, v0.20a).
fn check_list_kernel_method(
    method: &Ident,
    args: &[Expr],
    elem: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let arity = |n: usize, ctx: &mut Ctx| {
        if args.len() != n {
            ctx.errors.push(CompileError::new(
                "karn.types.method_arity",
                span,
                format!(
                    "`List.{}` takes {n} argument{}, got {}",
                    method.name,
                    if n == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return false;
        }
        true
    };
    match method.name.as_str() {
        "length" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(Ty::Base(BaseType::Int))
        }
        "get" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(
                &args[0],
                &Ty::Base(BaseType::Int),
                "the `List.get` index",
                ctx,
            );
            Some(Ty::Option(Box::new(elem.clone())))
        }
        "prepend" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(&args[0], elem, "the `List.prepend` element", ctx);
            Some(Ty::List(Box::new(elem.clone())))
        }
        "fold" => {
            if !arity(2, ctx) {
                return None;
            }
            let acc = type_of(&args[0], None, ctx)?;
            let step = Ty::Fn {
                params: vec![acc.clone(), elem.clone()],
                ret: Box::new(acc.clone()),
            };
            check_arg(&args[1], &step, "the `List.fold` step function", ctx);
            Some(acc)
        }
        FOLD_EFF => {
            if !arity(2, ctx) {
                return None;
            }
            let acc = type_of(&args[0], None, ctx)?;
            let step = Ty::Fn {
                params: vec![acc.clone(), elem.clone()],
                ret: Box::new(Ty::Effect(Box::new(acc.clone()))),
            };
            check_arg(&args[1], &step, "the `List.foldEff` step function", ctx);
            // `foldEff` runs its effectful step function — like any
            // effectful function-value call, it is confined to effectful
            // contexts (0031).
            if !ctx.effectful {
                ctx.errors.push(
                    CompileError::new(
                        "karn.effect.fn_value_in_pure_context",
                        span,
                        "`List.foldEff` runs an effectful step function and cannot be called in a pure context",
                    )
                    .with_note(
                        "effectful function values may only be called where the enclosing body is effectful (its return type is an Effect)",
                    ),
                );
            }
            Some(Ty::Effect(Box::new(acc)))
        }
        _ => {
            ctx.errors.push(CompileError::new(
                "karn.types.method_not_found",
                method.span,
                format!(
                    "the built-in `List[{}]` type has no method `{}` — the kernel is `length`, `get`, `prepend`, `fold`, `foldEff`",
                    elem.display(),
                    method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            None
        }
    }
}

/// v0.21: type a built-in numeric kernel method (ADR 0041). Conversions are
/// value methods on the bare base types: `Int -> Float` is total
/// (`toFloat`); `Float -> Int` is one of four named, lossy roundings — there
/// is deliberately no ambiguous `toInt`. v0.22a (ADR 0048) extends the
/// kernel with `abs`/`min`/`max`/`clamp` on both numeric types and
/// `isNaN`/`isFinite` on `Float`.
fn check_numeric_kernel_method(
    method: &Ident,
    args: &[Expr],
    base: BaseType,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let b = Ty::Base(base);
    // (parameter types, return type)
    let sig: Option<(Vec<Ty>, Ty)> = match (base, method.name.as_str()) {
        (BaseType::Int, "toFloat") => Some((vec![], Ty::Base(BaseType::Float))),
        (BaseType::Float, "round" | "floor" | "ceil" | "truncate") => {
            Some((vec![], Ty::Base(BaseType::Int)))
        }
        (_, "abs") => Some((vec![], b.clone())),
        (_, "min" | "max") => Some((vec![b.clone()], b.clone())),
        (_, "clamp") => Some((vec![b.clone(), b.clone()], b.clone())),
        (BaseType::Float, "isNaN" | "isFinite") => Some((vec![], Ty::Base(BaseType::Bool))),
        _ => None,
    };
    let Some((params, ret)) = sig else {
        let kernel = match base {
            BaseType::Int => "`toFloat`, `abs`, `min`, `max`, `clamp`",
            _ => {
                "`round`, `floor`, `ceil`, `truncate`, `abs`, `min`, `max`, `clamp`, \
                 `isNaN`, `isFinite`"
            }
        };
        ctx.errors.push(CompileError::new(
            "karn.types.method_not_found",
            method.span,
            format!(
                "the built-in `{}` type has no method `{}` — the kernel is {kernel}",
                base.name(),
                method.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    };
    if args.len() != params.len() {
        ctx.errors.push(CompileError::new(
            "karn.types.method_arity",
            span,
            format!(
                "`{}.{}` takes {} argument{}, got {}",
                base.name(),
                method.name,
                params.len(),
                if params.len() == 1 { "" } else { "s" },
                args.len()
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    for (a, p) in args.iter().zip(&params) {
        check_arg(
            a,
            p,
            &format!("the `{}.{}` argument", base.name(), method.name),
            ctx,
        );
    }
    Some(ret)
}

/// v0.22a: type a built-in `String` kernel method (ADR 0046). `String` is
/// opaque (no char access), so its operations are compiler built-ins
/// lowering to TS string methods — the 0034/0037 hybrid posture. Semantics
/// are UTF-16 code units, except `chars()` (code points, normatively).
fn check_string_kernel_method(
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let s = Ty::Base(BaseType::String);
    let int = Ty::Base(BaseType::Int);
    let boolean = Ty::Base(BaseType::Bool);
    let strings = Ty::List(Box::new(s.clone()));
    // (parameter types, return type)
    let sig: Option<(Vec<Ty>, Ty)> = match method.name.as_str() {
        "length" => Some((vec![], int.clone())),
        "split" => Some((vec![s.clone()], strings.clone())),
        "trim" | "toUpper" | "toLower" => Some((vec![], s.clone())),
        "contains" | "startsWith" | "endsWith" => Some((vec![s.clone()], boolean)),
        "replace" => Some((vec![s.clone(), s.clone()], s.clone())),
        "slice" => Some((vec![int.clone(), int.clone()], s.clone())),
        "indexOf" => Some((vec![s.clone()], Ty::Option(Box::new(int)))),
        "chars" => Some((vec![], strings)),
        "concat" => Some((vec![s.clone()], s.clone())),
        _ => None,
    };
    let Some((params, ret)) = sig else {
        ctx.errors.push(CompileError::new(
            "karn.types.method_not_found",
            method.span,
            format!(
                "the built-in `String` type has no method `{}` — the kernel is \
                 `length`, `split`, `trim`, `contains`, `startsWith`, `endsWith`, \
                 `replace`, `slice`, `indexOf`, `toUpper`, `toLower`, `chars`, `concat`",
                method.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    };
    if args.len() != params.len() {
        ctx.errors.push(CompileError::new(
            "karn.types.method_arity",
            span,
            format!(
                "`String.{}` takes {} argument{}, got {}",
                method.name,
                params.len(),
                if params.len() == 1 { "" } else { "s" },
                args.len()
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    for (a, p) in args.iter().zip(&params) {
        check_arg(a, p, &format!("the `String.{}` argument", method.name), ctx);
    }
    Some(ret)
}

/// v0.22a: type a function-valued kernel argument (the 0048 combinators —
/// `map`/`andThen`/`mapErr`). Parameter types are known from the receiver;
/// the return is read from the actual: an expected return carrying a
/// flexible variable lets a lambda type bottom-up (the v0.20a pass-2 rule),
/// and `unify` captures it here. Returns the function's return type.
fn check_kernel_fn_arg(arg: &Expr, params: Vec<Ty>, label: &str, ctx: &mut Ctx) -> Option<Ty> {
    const RET_VAR: &str = "__kernel_ret";
    let expected = Ty::Fn {
        params: params.clone(),
        ret: Box::new(Ty::Var(RET_VAR.to_string())),
    };
    let actual = type_of(arg, Some(&expected), ctx)?;
    let mut subst: HashMap<String, Ty> = HashMap::new();
    if unify(&expected, &actual, &mut subst)
        && let Some(ret) = subst.get(RET_VAR).cloned()
    {
        // `unify` is permissive ground-vs-ground; re-check the whole shape.
        let want = Ty::Fn {
            params,
            ret: Box::new(ret.clone()),
        };
        if compatible(&actual, &want) {
            return Some(ret);
        }
    }
    ctx.errors.push(CompileError::new(
        "karn.types.argument_mismatch",
        arg.span,
        format!(
            "{label} expects a function over the receiver's value, but got `{}`",
            actual.display()
        ),
    ));
    None
}

/// v0.22a: type a built-in `Option[T]` kernel method (ADR 0048) — the
/// combinators as value methods on the compiler-known receiver (collision-
/// free, unlike free functions imported by bare name).
fn check_option_kernel_method(
    method: &Ident,
    args: &[Expr],
    inner: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let arity = |n: usize, ctx: &mut Ctx| {
        if args.len() != n {
            ctx.errors.push(CompileError::new(
                "karn.types.method_arity",
                span,
                format!(
                    "`Option.{}` takes {n} argument{}, got {}",
                    method.name,
                    if n == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return false;
        }
        true
    };
    match method.name.as_str() {
        "map" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret = check_kernel_fn_arg(
                &args[0],
                vec![inner.clone()],
                "the `Option.map` function",
                ctx,
            )?;
            Some(Ty::Option(Box::new(ret)))
        }
        "andThen" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret = check_kernel_fn_arg(
                &args[0],
                vec![inner.clone()],
                "the `Option.andThen` function",
                ctx,
            )?;
            match ret {
                Ty::Option(_) => Some(ret),
                other => {
                    ctx.errors.push(CompileError::new(
                        "karn.types.argument_mismatch",
                        args[0].span,
                        format!(
                            "the `Option.andThen` function must return an `Option`, but returns `{}`",
                            other.display()
                        ),
                    ));
                    None
                }
            }
        }
        "getOrElse" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(&args[0], inner, "the `Option.getOrElse` fallback", ctx);
            Some(inner.clone())
        }
        "isSome" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(Ty::Base(BaseType::Bool))
        }
        "okOr" => {
            if !arity(1, ctx) {
                return None;
            }
            let err = type_of(&args[0], None, ctx)?;
            Some(Ty::Result(Box::new(inner.clone()), Box::new(err)))
        }
        _ => {
            ctx.errors.push(CompileError::new(
                "karn.types.method_not_found",
                method.span,
                format!(
                    "the built-in `Option[{}]` type has no method `{}` — the kernel is \
                     `map`, `andThen`, `getOrElse`, `isSome`, `okOr`",
                    inner.display(),
                    method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            None
        }
    }
}

/// v0.22a: type a built-in `Result[T, E]` kernel method (ADR 0048).
fn check_result_kernel_method(
    method: &Ident,
    args: &[Expr],
    ok: &Ty,
    err: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let arity = |n: usize, ctx: &mut Ctx| {
        if args.len() != n {
            ctx.errors.push(CompileError::new(
                "karn.types.method_arity",
                span,
                format!(
                    "`Result.{}` takes {n} argument{}, got {}",
                    method.name,
                    if n == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return false;
        }
        true
    };
    match method.name.as_str() {
        "map" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret =
                check_kernel_fn_arg(&args[0], vec![ok.clone()], "the `Result.map` function", ctx)?;
            Some(Ty::Result(Box::new(ret), Box::new(err.clone())))
        }
        "andThen" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret = check_kernel_fn_arg(
                &args[0],
                vec![ok.clone()],
                "the `Result.andThen` function",
                ctx,
            )?;
            match ret {
                Ty::Result(b, e2) => {
                    if !compatible(&e2, err) && !compatible(err, &e2) {
                        ctx.errors.push(CompileError::new(
                            "karn.types.argument_mismatch",
                            args[0].span,
                            format!(
                                "the `Result.andThen` function's error type `{}` does not match the receiver's `{}`",
                                e2.display(),
                                err.display()
                            ),
                        ));
                        return None;
                    }
                    Some(Ty::Result(b, Box::new(err.clone())))
                }
                other => {
                    ctx.errors.push(CompileError::new(
                        "karn.types.argument_mismatch",
                        args[0].span,
                        format!(
                            "the `Result.andThen` function must return a `Result`, but returns `{}`",
                            other.display()
                        ),
                    ));
                    None
                }
            }
        }
        "mapErr" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret = check_kernel_fn_arg(
                &args[0],
                vec![err.clone()],
                "the `Result.mapErr` function",
                ctx,
            )?;
            Some(Ty::Result(Box::new(ok.clone()), Box::new(ret)))
        }
        "getOrElse" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(&args[0], ok, "the `Result.getOrElse` fallback", ctx);
            Some(ok.clone())
        }
        "isOk" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(Ty::Base(BaseType::Bool))
        }
        _ => {
            ctx.errors.push(CompileError::new(
                "karn.types.method_not_found",
                method.span,
                format!(
                    "the built-in `Result[{}, {}]` type has no method `{}` — the kernel is \
                     `map`, `andThen`, `mapErr`, `getOrElse`, `isOk`",
                    ok.display(),
                    err.display(),
                    method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            None
        }
    }
}

/// v0.22b: whether a type can pass through the typed JSON codec — every
/// boundary-serialisable shape: bases, named types, and the built-in
/// generic containers over them. Functions, effects, `HttpResult`, the
/// error builtins, and type variables cannot.
fn json_codable(t: &Ty) -> bool {
    match t {
        Ty::Base(_) | Ty::Named { .. } | Ty::Unit => true,
        Ty::Result(a, b) => json_codable(a) && json_codable(b),
        Ty::Option(a) | Ty::List(a) => json_codable(a),
        Ty::Map(k, v) => json_codable(k) && json_codable(v),
        Ty::Fn { .. }
        | Ty::Effect(_)
        | Ty::HttpResult(_)
        | Ty::ValidationError
        | Ty::JsonError
        | Ty::Var(_) => false,
    }
}

/// v0.22b: type the `Json` codec statics (ADR 0045). `encode(v) -> String`
/// over any codable value (it throws on a non-finite `Float`, per 0040 —
/// documented, not typed); `decode[T](s) -> Result[T, JsonError]` with `T`
/// explicit (`Json.decode[Order](s)`) or inferred from an expected
/// `Result[T, JsonError]`.
fn check_json_static(
    method: &Ident,
    type_args: &[TypeRef],
    args: &[Expr],
    span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let arity1 = |ctx: &mut Ctx| {
        if args.len() != 1 {
            ctx.errors.push(CompileError::new(
                "karn.types.method_arity",
                span,
                format!(
                    "`Json.{}` takes 1 argument, got {}",
                    method.name,
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return false;
        }
        true
    };
    match method.name.as_str() {
        "encode" => {
            if !type_args.is_empty() {
                ctx.errors.push(CompileError::new(
                    "karn.generics.type_arg_mismatch",
                    span,
                    "`Json.encode` takes no type arguments — its type comes from the value",
                ));
            }
            if !arity1(ctx) {
                return None;
            }
            let t = type_of(&args[0], None, ctx)?;
            if !json_codable(&t) {
                ctx.errors.push(
                    CompileError::new(
                        "karn.types.json_uncodable",
                        args[0].span,
                        format!("`{}` cannot be encoded as JSON", t.display()),
                    )
                    .with_note(
                        "the codec covers base types, named types, and the built-in \
                         containers over them — not functions, effects, or error builtins",
                    ),
                );
                return None;
            }
            Some(Ty::Base(BaseType::String))
        }
        "decode" => {
            if !arity1(ctx) {
                return None;
            }
            check_arg(
                &args[0],
                &Ty::Base(BaseType::String),
                "the `Json.decode` input",
                ctx,
            );
            let t = match type_args {
                [one] => resolve_type_ref(one, &ctx.input.types)?,
                [] => match expected {
                    Some(Ty::Result(t, e)) if **e == Ty::JsonError => (**t).clone(),
                    _ => {
                        ctx.errors.push(
                            CompileError::new(
                                "karn.generics.uninferable_type_arg",
                                span,
                                "cannot infer the target type of `Json.decode`",
                            )
                            .with_note(
                                "give it explicitly (`Json.decode[Order](s)`) or use the \
                                 result where a `Result[T, JsonError]` is expected",
                            ),
                        );
                        return None;
                    }
                },
                _ => {
                    ctx.errors.push(CompileError::new(
                        "karn.generics.type_arg_mismatch",
                        span,
                        format!(
                            "`Json.decode` takes exactly one type argument, got {}",
                            type_args.len()
                        ),
                    ));
                    return None;
                }
            };
            if !json_codable(&t) || t == Ty::Unit {
                ctx.errors.push(
                    CompileError::new(
                        "karn.types.json_uncodable",
                        span,
                        format!("`{}` cannot be decoded from JSON", t.display()),
                    )
                    .with_note(
                        "the codec covers base types, named types, and the built-in \
                         containers over them — not functions, effects, or error builtins",
                    ),
                );
                return None;
            }
            Some(Ty::Result(Box::new(t), Box::new(Ty::JsonError)))
        }
        _ => {
            // The resolver owns the unknown-static diagnostic.
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            None
        }
    }
}

/// v0.22a: type the numeric parse statics — `Int.parse(s)` /
/// `Float.parse(s)` (`-> Option[T]`, ADR 0048). Parsing is full-string
/// (trailing garbage is `None`, unlike `parseFloat`); an out-of-safe-range
/// `Int` or non-finite `Float` is `None`.
fn check_numeric_parse_static(
    type_name: &Ident,
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    if method.name != "parse" {
        // The resolver owns the unknown-static diagnostic; don't double up.
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    if args.len() != 1 {
        ctx.errors.push(CompileError::new(
            "karn.types.method_arity",
            span,
            format!(
                "`{}.parse` takes 1 argument, got {}",
                type_name.name,
                args.len()
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    check_arg(
        &args[0],
        &Ty::Base(BaseType::String),
        &format!("the `{}.parse` argument", type_name.name),
        ctx,
    );
    let inner = if type_name.name == INT {
        BaseType::Int
    } else {
        BaseType::Float
    };
    Some(Ty::Option(Box::new(Ty::Base(inner))))
}

/// v0.20b: type a built-in `Map[K, V]` kernel method.
fn check_map_kernel_method(
    method: &Ident,
    args: &[Expr],
    key: &Ty,
    val: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let arity = |n: usize, ctx: &mut Ctx| {
        if args.len() != n {
            ctx.errors.push(CompileError::new(
                "karn.types.method_arity",
                span,
                format!(
                    "`Map.{}` takes {n} argument{}, got {}",
                    method.name,
                    if n == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return false;
        }
        true
    };
    match method.name.as_str() {
        "length" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(Ty::Base(BaseType::Int))
        }
        "keys" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(Ty::List(Box::new(key.clone())))
        }
        "get" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(&args[0], key, "the `Map.get` key", ctx);
            Some(Ty::Option(Box::new(val.clone())))
        }
        "insert" => {
            if !arity(2, ctx) {
                return None;
            }
            check_arg(&args[0], key, "the `Map.insert` key", ctx);
            check_arg(&args[1], val, "the `Map.insert` value", ctx);
            Some(Ty::Map(Box::new(key.clone()), Box::new(val.clone())))
        }
        _ => {
            ctx.errors.push(CompileError::new(
                "karn.types.method_not_found",
                method.span,
                format!(
                    "the built-in `Map[{}, {}]` type has no method `{}` — the kernel is `length`, `keys`, `get`, `insert`",
                    key.display(),
                    val.display(),
                    method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            None
        }
    }
}

/// Type-check a kernel-method argument against its expected type, with the
/// expected type propagated in (so lambdas and literals type contextually).
fn check_arg(arg: &Expr, expected: &Ty, what: &str, ctx: &mut Ctx) {
    let Some(actual) = type_of(arg, Some(expected), ctx) else {
        return;
    };
    if !compatible(&actual, expected) {
        ctx.errors.push(CompileError::new(
            "karn.types.type_mismatch",
            arg.span,
            format!(
                "{what} has type `{}`, but `{}` is required",
                actual.display(),
                expected.display()
            ),
        ));
    }
}

fn check_question(inner: &Expr, span: Span, ctx: &mut Ctx) -> Option<Ty> {
    let inner_ty = type_of(inner, None, ctx)?;
    let Ty::Result(t, e) = &inner_ty else {
        ctx.errors.push(
            CompileError::new(
                "karn.types.question_on_non_result",
                inner.span,
                format!(
                    "the `?` operator requires a `Result[T, E]` value, but got `{}`",
                    inner_ty.display()
                ),
            )
            .with_label(span, "this `?` requires a Result"),
        );
        return None;
    };
    // v0.5: `?` is also valid inside `Effect[Result[T, E]]` — the `Err` is
    // propagated as `Effect.pure(Err(e))`.
    let effect_result = if let Ty::Effect(inner_eff) = &ctx.return_ty
        && let Ty::Result(_, eff_e) = inner_eff.as_ref()
    {
        Some(eff_e.as_ref().clone())
    } else {
        None
    };
    let Ty::Result(_ret_t, ret_e) = &ctx.return_ty else {
        if let Some(eff_e) = effect_result {
            if !compatible(e, &eff_e) {
                ctx.errors.push(CompileError::new(
                    "karn.types.question_error_mismatch",
                    span,
                    format!(
                        "the `?` operator propagates an error of type `{}`, but the enclosing function returns `Effect[Result[_, {}]]`",
                        e.display(),
                        eff_e.display()
                    ),
                ));
                return None;
            }
            return Some((**t).clone());
        }
        ctx.errors.push(
            CompileError::new(
                "karn.types.question_outside_result",
                span,
                "the `?` operator can only be used inside a function returning `Result`",
            )
            .with_label(
                ctx.return_ty_span,
                format!("function returns `{}`", ctx.return_ty.display()),
            ),
        );
        return None;
    };
    if !compatible(e, ret_e) {
        ctx.errors.push(CompileError::new(
            "karn.types.question_error_mismatch",
            span,
            format!(
                "the `?` operator propagates an error of type `{}`, but the enclosing function returns `Result[_, {}]`",
                e.display(),
                ret_e.display()
            ),
        ));
        return None;
    }
    Some((**t).clone())
}

/// Record a capability reference's binding edge, qualifying flattened bare
/// names (`consumes U { Cap }`) to their providing unit (v0.25). A bare
/// non-flattened name is the consuming unit's own declaration — qualified
/// at assembly.
fn record_capability_ref(span: Span, name: &str, ctx: &mut Ctx) {
    if let Some(unit) = ctx.input.cross_context.flattened_caps.get(name) {
        ctx.refs
            .record_in_unit(span, SymbolKind::Capability, name, unit);
    } else {
        ctx.refs.record(span, SymbolKind::Capability, name);
    }
}

fn check_static_call(
    type_name: &Ident,
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    // Capability dispatch (v0.5): if `type_name` names a capability declared
    // in the context, dispatch via the capability table. If the capability is
    // declared but not in `given`, error specifically.
    if ctx.declared_capabilities.contains_key(&type_name.name)
        && !ctx.capabilities.contains_key(&type_name.name)
    {
        record_capability_ref(type_name.span, &type_name.name, ctx);
        let mut err = CompileError::new(
            "karn.given.undeclared_capability",
            type_name.span,
            format!(
                "capability `{}` is used but not listed in the handler's `given` clause",
                type_name.name
            ),
        )
        .with_note(format!(
            "add `{}` to the handler's `given` clause so the dependency surface is visible at the declaration site",
            type_name.name
        ));
        // v0.26 (ADR 0054): the one-click counterpart of the note.
        if let Some((span, insert)) =
            given_insertion_edit(&ctx.given_entries, ctx.given_anchor, &type_name.name)
        {
            err = err.with_suggestion(
                format!("add `{}` to the `given` clause", type_name.name),
                vec![(span, insert)],
                Applicability::MachineApplicable,
            );
        }
        ctx.errors.push(err);
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    if let Some(cap) = ctx.capabilities.get(&type_name.name).cloned() {
        record_capability_ref(type_name.span, &type_name.name, ctx);
        if !ctx.effectful {
            ctx.errors.push(
                CompileError::new(
                    "karn.effect.capability_in_pure_context",
                    span,
                    format!(
                        "capability `{}` can only be called inside an effectful body (one returning `Effect[T]`)",
                        type_name.name
                    ),
                ),
            );
        }
        ctx.given_used.insert(type_name.name.clone());
        let Some(op) = cap.ops.iter().find(|o| o.name == method.name) else {
            ctx.errors.push(CompileError::new(
                "karn.capability.unknown_operation",
                method.span,
                format!(
                    "capability `{}` has no operation named `{}`",
                    type_name.name, method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return None;
        };
        if op.params.len() != args.len() {
            ctx.errors.push(CompileError::new(
                "karn.capability.op_arity",
                span,
                format!(
                    "capability operation `{}.{}` expects {} argument(s), but {} were given",
                    type_name.name,
                    method.name,
                    op.params.len(),
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return None;
        }
        let op_clone = op.clone();
        for (i, (param_ty, arg)) in op_clone.params.iter().zip(args.iter()).enumerate() {
            let arg_ty = type_of(arg, Some(param_ty), ctx);
            if let Some(actual) = arg_ty
                && !compatible(&actual, param_ty)
            {
                ctx.errors.push(CompileError::new(
                    "karn.types.argument_mismatch",
                    arg.span,
                    format!(
                        "argument {} to capability `{}.{}` has type `{}`, but parameter expects `{}`",
                        i + 1,
                        type_name.name,
                        method.name,
                        actual.display(),
                        param_ty.display()
                    ),
                ));
            }
        }
        return Some(op_clone.return_ty);
    }
    let decl = ctx.input.types.get(&type_name.name)?.clone();
    ctx.refs
        .record(type_name.span, SymbolKind::Type, &type_name.name);
    let table = ctx
        .input
        .methods
        .get(&type_name.name)
        .cloned()
        .unwrap_or_default();

    // 1) User-declared static method.
    if let Some(method_decl) = table.statics.get(&method.name).cloned() {
        return check_method_args(&method_decl, args, ctx, type_name, method);
    }

    // 2) Built-in `of` constructor on refined or opaque types.
    if method.name == OF
        && let Some(base) = type_decl_base(&decl)
    {
        if args.len() != 1 {
            ctx.errors.push(CompileError::new(
                "karn.types.constructor_arity",
                span,
                format!(
                    "constructor `{}.of` expects 1 argument, but {} were given",
                    type_name.name,
                    args.len()
                ),
            ));
            return None;
        }
        let arg = &args[0];
        let expected = Ty::Base(base);
        let arg_ty = type_of(arg, Some(&expected), ctx)?;
        if !compatible(&arg_ty, &expected) {
            ctx.errors.push(CompileError::new(
                "karn.types.constructor_base_mismatch",
                arg.span,
                format!(
                    "constructor `{}.of` expects a `{}` argument, but got `{}`",
                    type_name.name,
                    base.name(),
                    arg_ty.display()
                ),
            ));
            return None;
        }
        // `.of` is always the runtime constructor: it returns
        // `Result[T, ValidationError]`. Compile-time literal admission (v0.9.4)
        // happens instead wherever an expected refined type is known — see
        // `admit_refined_literal`, used by `type_of` — so `.of`'s type never
        // depends on the form of its argument.
        return Some(Ty::Result(
            Box::new(named_ty(&decl)),
            Box::new(Ty::ValidationError),
        ));
    }

    // 2b) Built-in `unsafe` constructor on opaque types — only available
    // inside the defining commons.
    if method.name == UNSAFE
        && let TypeBody::Opaque { base, .. } = &decl.body
    {
        if !ctx.input.is_local_type(&decl.name.name) {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.opaque_unsafe_outside",
                    method.span,
                    format!(
                        "`{}.unsafe(...)` is only available within the commons that defines the opaque type `{}`",
                        type_name.name, type_name.name
                    ),
                )
                .with_note(
                    "outside the defining commons, opaque values are constructed via `T.of(value)`",
                ),
            );
            return None;
        }
        if args.len() != 1 {
            ctx.errors.push(CompileError::new(
                "karn.types.constructor_arity",
                span,
                format!(
                    "`{}.unsafe` expects 1 argument, but {} were given",
                    type_name.name,
                    args.len()
                ),
            ));
            return None;
        }
        let arg = &args[0];
        let expected = Ty::Base(*base);
        let arg_ty = type_of(arg, Some(&expected), ctx)?;
        if !compatible(&arg_ty, &expected) {
            ctx.errors.push(CompileError::new(
                "karn.types.constructor_base_mismatch",
                arg.span,
                format!(
                    "`{}.unsafe` expects a `{}` argument, but got `{}`",
                    type_name.name,
                    base.name(),
                    arg_ty.display()
                ),
            ));
            return None;
        }
        return Some(named_ty(&decl));
    }

    // 3) Qualified variant construction `TypeName.Variant(args)`.
    if let TypeBody::Sum(_) = &decl.body {
        return check_variant_construction(&decl, &method.name, args, span, ctx);
    }

    ctx.errors.push(
        CompileError::new(
            "karn.types.unknown_static_member",
            method.span,
            format!(
                "type `{}` has no static method or variant named `{}`",
                type_name.name, method.name
            ),
        )
        .with_label(decl.name.span, "type declared here"),
    );
    None
}

fn check_method_args(
    method_decl: &FnDecl,
    args: &[Expr],
    ctx: &mut Ctx,
    type_name: &Ident,
    method: &Ident,
) -> Option<Ty> {
    if method_decl.params.len() != args.len() {
        ctx.errors.push(
            CompileError::new(
                "karn.types.method_arity",
                method.span,
                format!(
                    "static method `{}.{}` expects {} argument(s), but {} were given",
                    type_name.name,
                    method.name,
                    method_decl.params.len(),
                    args.len()
                ),
            )
            .with_label(method_decl.name.ident().span, "method declared here"),
        );
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    let mut ok = true;
    for (i, (param, arg)) in method_decl.params.iter().zip(args.iter()).enumerate() {
        let expected = resolve_type_ref(&param.type_ref, &ctx.input.types);
        let actual = type_of(arg, expected.as_ref(), ctx);
        let (Some(actual), Some(expected)) = (actual, expected) else {
            ok = false;
            continue;
        };
        if !compatible(&actual, &expected) {
            ctx.errors.push(CompileError::new(
                "karn.types.argument_mismatch",
                arg.span,
                format!(
                    "argument {} to `{}.{}` has type `{}`, but parameter `{}` expects `{}`",
                    i + 1,
                    type_name.name,
                    method.name,
                    actual.display(),
                    param.name.name,
                    expected.display()
                ),
            ));
            ok = false;
        }
    }
    if !ok {
        return None;
    }
    resolve_type_ref(&method_decl.return_type, &ctx.input.types)
}

fn check_record_spread(
    type_name: Option<&Ident>,
    base: &Expr,
    overrides: &[FieldInit],
    span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    // 1) Determine the record type.
    let base_ty = type_of(base, expected, ctx)?;
    let record_name = match &base_ty {
        Ty::Named {
            name,
            kind: NamedKind::Record,
        } => name.clone(),
        _ => {
            ctx.errors.push(CompileError::new(
                "karn.record_spread.non_record_base",
                base.span,
                format!(
                    "record spread requires a record-typed base, but got `{}`",
                    base_ty.display()
                ),
            ));
            return None;
        }
    };
    if let Some(tn) = type_name
        && tn.name != record_name
    {
        ctx.errors.push(CompileError::new(
            "karn.record_spread.type_mismatch",
            tn.span,
            format!(
                "spread type prefix `{}` does not match the base's type `{}`",
                tn.name, record_name
            ),
        ));
    }
    let decl = ctx.input.types.get(&record_name)?.clone();
    let TypeBody::Record(r) = &decl.body else {
        return None;
    };
    let declared: HashMap<&str, &RecordField> =
        r.fields.iter().map(|f| (f.name.name.as_str(), f)).collect();
    let _ = span;
    for f in overrides {
        let Some(declared_field) = declared.get(f.name.name.as_str()) else {
            ctx.errors.push(CompileError::new(
                "karn.record_spread.unknown_field",
                f.name.span,
                format!(
                    "record type `{}` has no field `{}`",
                    record_name, f.name.name
                ),
            ));
            continue;
        };
        let expected_ty = resolve_type_ref(&declared_field.type_ref, &ctx.input.types);
        let value_ty = match &f.value {
            Some(v) => type_of(v, expected_ty.as_ref(), ctx),
            None => ctx.lookup(&f.name.name),
        };
        if let (Some(actual), Some(expected_ty)) = (value_ty, expected_ty)
            && !compatible(&actual, &expected_ty)
        {
            ctx.errors.push(CompileError::new(
                "karn.record_spread.field_type_mismatch",
                f.value.as_ref().map(|v| v.span).unwrap_or(f.name.span),
                format!(
                    "spread override of field `{}` has type `{}`, but the declared type is `{}`",
                    f.name.name,
                    actual.display(),
                    expected_ty.display()
                ),
            ));
        }
    }
    Some(base_ty)
}

fn check_record_construction(
    type_name: &Ident,
    fields: &[FieldInit],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let decl = ctx.input.types.get(&type_name.name)?.clone();
    ctx.refs
        .record(type_name.span, SymbolKind::Type, &type_name.name);
    if matches!(decl.body, TypeBody::Opaque { .. }) {
        ctx.errors.push(
            CompileError::new(
                "karn.types.opaque_record_construction",
                type_name.span,
                format!(
                    "opaque type `{}` cannot be constructed with record-literal syntax",
                    type_name.name
                ),
            )
            .with_note(
                "construct opaque values via `T.of(value)` (validated) or `T.unsafe(value)` (inside the defining commons)",
            ),
        );
        return None;
    }
    let TypeBody::Record(r) = &decl.body else {
        return None;
    };
    // Collect declared fields.
    let declared: HashMap<&str, &RecordField> =
        r.fields.iter().map(|f| (f.name.name.as_str(), f)).collect();
    let _ = span;
    for f in fields {
        if let Some(declared_field) = declared.get(f.name.name.as_str()) {
            let expected = resolve_type_ref(&declared_field.type_ref, &ctx.input.types);
            let value_ty = match &f.value {
                Some(v) => type_of(v, expected.as_ref(), ctx),
                None => ctx.lookup(&f.name.name),
            };
            if let (Some(actual), Some(expected)) = (value_ty, expected)
                && !compatible(&actual, &expected)
            {
                ctx.errors.push(
                    CompileError::new(
                        "karn.types.field_value_mismatch",
                        f.value.as_ref().map(|v| v.span).unwrap_or(f.name.span),
                        format!(
                            "field `{}` expects `{}`, but the value has type `{}`",
                            f.name.name,
                            expected.display(),
                            actual.display()
                        ),
                    )
                    .with_label(declared_field.name.span, "field declared here"),
                );
            }
        }
    }
    Some(named_ty(&decl))
}

fn check_field_access(receiver: &Expr, field: &Ident, ctx: &mut Ctx) -> Option<Ty> {
    // Qualified nullary variant: `TypeName.Variant` where TypeName is a
    // declared sum type and Variant is one of its payload-less variants.
    if let ExprKind::Ident(id) = &receiver.kind
        && ctx.lookup(id.name.as_str()).is_none()
        && let Some(decl) = ctx.input.types.get(&id.name)
        && let TypeBody::Sum(s) = &decl.body
        && let Some(variant) = s.variants.iter().find(|v| v.name.name == field.name)
    {
        if !variant.payload.is_empty() {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.variant_missing_payload",
                    field.span,
                    format!(
                        "variant `{}.{}` has a payload — call it with arguments",
                        id.name, field.name
                    ),
                )
                .with_label(variant.span, "variant declared here"),
            );
            return None;
        }
        return Some(named_ty(decl));
    }
    let recv_ty = type_of(receiver, None, ctx)?;
    // `.raw` on an opaque value: only available within the defining commons.
    // Returns the base type. The emitter compiles this to a `value as base`
    // type assertion (see emitter::lower_expr for FieldAccess).
    if field.name == RAW
        && let Ty::Named {
            kind: NamedKind::Opaque(base),
            name,
        } = &recv_ty
    {
        if !ctx.input.is_local_type(name) {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.opaque_raw_outside",
                    field.span,
                    format!(
                        "`.raw` on opaque type `{}` is only available within its defining commons",
                        name
                    ),
                )
                .with_note(
                    "the base representation of an opaque type is hidden from importers; \
                     define a method on the type or use a public accessor",
                ),
            );
            return None;
        }
        return Some(Ty::Base(*base));
    }
    // v0.22b: `JsonError` is a compiler-known record (ADR 0047) — uniform
    // `String` fields so a decode failure is inspectable in Karn.
    if recv_ty == Ty::JsonError {
        return match field.name.as_str() {
            "kind" | "path" | "message" => Some(Ty::Base(BaseType::String)),
            other => {
                ctx.errors.push(CompileError::new(
                    "karn.types.unknown_field",
                    field.span,
                    format!(
                        "`JsonError` has no field `{other}` — its fields are `kind`, `path`, `message`"
                    ),
                ));
                None
            }
        };
    }
    let Ty::Named {
        name,
        kind: NamedKind::Record,
    } = &recv_ty
    else {
        ctx.errors.push(CompileError::new(
            "karn.types.field_access_on_non_record",
            field.span,
            format!(
                "field access requires a record type, but the receiver has type `{}`",
                recv_ty.display()
            ),
        ));
        return None;
    };
    let decl = ctx.input.types.get(name)?;
    let TypeBody::Record(r) = &decl.body else {
        return None;
    };
    let Some(field_decl) = r.fields.iter().find(|f| f.name.name == field.name) else {
        ctx.errors.push(
            CompileError::new(
                "karn.types.unknown_field",
                field.span,
                format!("record type `{}` has no field `{}`", name, field.name),
            )
            .with_label(decl.name.span, "type declared here"),
        );
        return None;
    };
    resolve_type_ref(&field_decl.type_ref, &ctx.input.types)
}

fn check_method_call(
    receiver: &Expr,
    method: &Ident,
    type_args: &[TypeRef],
    args: &[Expr],
    span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    // v0.22b: explicit type arguments apply only to the `Json.decode[T]`
    // static — every other method/static takes none (the 0039/0045 rule;
    // generic *user* methods remain deferred). A user-declared type named
    // `Json` shadows the codec module and takes no type arguments.
    if !type_args.is_empty()
        && !matches!(&receiver.kind, ExprKind::Ident(id) if id.name == JSON
            && !ctx.input.types.contains_key(JSON))
    {
        ctx.errors.push(CompileError::new(
            "karn.generics.type_arg_mismatch",
            span,
            format!(
                "`{}` is not a generic method — it takes no type arguments",
                method.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    // v0.25: a test body invokes the target's service as `svc.call(args)`.
    // The emitter wires it from the same service set; the checker types it
    // loosely (the runner recovers outcomes at runtime), but the binding
    // edge is real — record it so test-file references index.
    if let ExprKind::Ident(id) = &receiver.kind
        && method.name == "call"
        && ctx.lookup(id.name.as_str()).is_none()
        && ctx.test_services.contains(&id.name)
        && let Some(unit) = ctx.input.cross_context.self_context.clone()
    {
        ctx.refs
            .record_in_unit(id.span, SymbolKind::Service, &id.name, &unit);
    }
    // v0.6: cross-context service call. Two shapes:
    //   - `Alias.service(args)`           where Alias is from `consumes X as Alias`
    //   - `prefix.tail.service(args)`     where `prefix.tail` is a consumed context's
    //                                     qualified name (parsed as nested FieldAccess).
    // The full-qualified-name form must be checked before the bare-ident form
    // (the prefix's first segment doesn't resolve as anything local).
    if ctx.lookup_root_ident(receiver).is_none() {
        // v0.15: cross-context capability call — `B.Cap.op(args)` /
        // `Alias.Cap.op(args)`. Checked before the service-call shape because
        // the receiver carries an extra (capability) segment.
        if let Some(chain) = flatten_ident_chain(receiver)
            && let Some((consumed, cap)) = ctx.input.cross_context.resolve_cross_capability(&chain)
        {
            // v0.25: the capability name-segment (`Cap` in `B.Cap` /
            // `Alias.Cap`) is the outermost field of the receiver chain.
            if let ExprKind::FieldAccess { field, .. } = &receiver.kind {
                ctx.refs
                    .record_in_unit(field.span, SymbolKind::Capability, &cap, &consumed);
            }
            return check_cross_context_capability_call(
                receiver, &consumed, &cap, method, args, span, ctx,
            );
        }
        if let Some(consumed) = cross_context_prefix(receiver, ctx) {
            return check_cross_context_call(receiver, &consumed, method, args, span, ctx);
        }
        // Looks like a dotted prefix (no local binding for the root). If the
        // chain matches the shape of a consumed-context call but the prefix
        // isn't actually consumed, surface an explicit diagnostic so the user
        // can fix the missing `consumes` clause rather than seeing a silent
        // "no methods" error later.
        if let ExprKind::FieldAccess { .. } = &receiver.kind
            && let Some(chain) = flatten_ident_chain(receiver)
            && chain.contains('.')
        {
            let info = &ctx.input.cross_context;
            let in_context = info.self_context.is_some();
            if in_context && info.resolve_prefix(&chain).is_none() {
                ctx.errors.push(
                    CompileError::new(
                        "karn.resolve.unconsumed_context",
                        receiver.span,
                        format!(
                            "`{chain}.{}` looks like a cross-context service call, but `{chain}` is not in this context's `consumes` clauses",
                            method.name
                        ),
                    )
                    .with_note(
                        "add a `consumes {chain}` clause at the top of the context, or use an alias and call it through the alias",
                    ),
                );
                for a in args {
                    let _ = type_of(a, None, ctx);
                }
                return None;
            }
        }
    }
    // Detect capability call (v0.5): receiver is a bare Ident naming a
    // capability declared in the context (in scope via `given`, or declared
    // but undeclared in `given` — the static-call path emits the error).
    if let ExprKind::Ident(id) = &receiver.kind
        && ctx.lookup(id.name.as_str()).is_none()
        && (ctx.capabilities.contains_key(&id.name)
            || ctx.declared_capabilities.contains_key(&id.name))
    {
        return check_static_call(id, method, args, span, ctx);
    }
    // Detect static-call shape: receiver is a bare Ident naming a declared
    // type (not a local/param). Dispatch to check_static_call.
    if let ExprKind::Ident(id) = &receiver.kind
        && ctx.lookup(id.name.as_str()).is_none()
        && ctx.input.types.contains_key(&id.name)
    {
        return check_static_call(id, method, args, span, ctx);
    }
    // v0.20b: qualified statics on the built-in collection types —
    // `List.empty()` / `Map.empty()`. Like an empty `[]`, they need an
    // expected type to pin their element/key/value types.
    if let ExprKind::Ident(id) = &receiver.kind
        && ctx.lookup(id.name.as_str()).is_none()
        && !ctx.input.types.contains_key(&id.name)
        && (id.name == LIST || id.name == MAP)
    {
        return check_collection_static(id, method, args, span, expected, ctx);
    }
    // v0.22a: the numeric parse statics — `Int.parse(s)` / `Float.parse(s)`.
    // The parser only admits these keywords in receiver position when
    // followed by `.`, so the Ident shape here is exactly the static form.
    if let ExprKind::Ident(id) = &receiver.kind
        && (id.name == INT || id.name == FLOAT)
    {
        return check_numeric_parse_static(id, method, args, span, ctx);
    }
    // v0.22b: the typed JSON codec statics (ADR 0045).
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == JSON
        && ctx.lookup(JSON).is_none()
        && !ctx.input.types.contains_key(JSON)
    {
        return check_json_static(method, type_args, args, span, expected, ctx);
    }
    // v0.20b: `insert`/`prepend` return their receiver's collection type —
    // propagate an expected collection type down the chain so
    // `let m: Map[String, Int] = Map.empty().insert("a", 1)` infers.
    let recv_expected = match (expected, method.name.as_str()) {
        (Some(t), "insert") => peel_to_map(t).map(|(k, v)| Ty::Map(Box::new(k), Box::new(v))),
        (Some(t), "prepend") => peel_to_list(t).map(|e| Ty::List(Box::new(e))),
        _ => None,
    };
    let recv_ty = type_of(receiver, recv_expected.as_ref(), ctx)?;
    // v0.20b: built-in kernel methods on the collection types. These are
    // compiler-known special forms typed directly here — generic in their
    // accumulator without the (deferred) declared-generic-methods feature;
    // the deferral bites only on declared methods (ADR 0037).
    match recv_ty.clone() {
        Ty::List(elem) => {
            return check_list_kernel_method(method, args, &elem, span, ctx);
        }
        Ty::Map(key, val) => {
            return check_map_kernel_method(method, args, &key, &val, span, ctx);
        }
        // v0.21: the numeric kernel — conversions as value methods on the
        // bare base types (a refined value reaches them via `.raw`).
        Ty::Base(base @ (BaseType::Int | BaseType::Float)) => {
            return check_numeric_kernel_method(method, args, base, span, ctx);
        }
        // v0.22a: the string kernel (ADR 0046).
        Ty::Base(BaseType::String) => {
            return check_string_kernel_method(method, args, span, ctx);
        }
        // v0.22a: the Option/Result combinators as kernel methods (ADR 0048).
        Ty::Option(inner) => {
            return check_option_kernel_method(method, args, &inner, span, ctx);
        }
        Ty::Result(ok, err) => {
            return check_result_kernel_method(method, args, &ok, &err, span, ctx);
        }
        _ => {}
    }
    // Find a named type for the receiver, then look up its instance methods.
    let type_name = match &recv_ty {
        Ty::Named { name, .. } => name.clone(),
        _ => {
            ctx.errors.push(CompileError::new(
                "karn.types.method_on_non_named_type",
                method.span,
                format!(
                    "type `{}` has no methods — only user-declared types support method calls",
                    recv_ty.display()
                ),
            ));
            return None;
        }
    };
    // Agent handler dispatch: when the receiver is an agent instance, look
    // up the method against the agent's declared `on call` handlers and
    // resolve to the handler's return type.
    if let Some(agent) = ctx.input.agents.get(&type_name).cloned() {
        let Some(handler) = agent.handlers.iter().find(|h| {
            h.method_name
                .as_ref()
                .is_some_and(|n| n.name == method.name)
        }) else {
            ctx.errors.push(CompileError::new(
                "karn.agent.handler_not_found",
                method.span,
                format!(
                    "agent `{}` has no handler named `{}`",
                    type_name, method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return None;
        };
        if handler.params.len() != args.len() {
            ctx.errors.push(CompileError::new(
                "karn.agent.handler_arity",
                method.span,
                format!(
                    "agent handler `{}.{}` expects {} argument(s), but {} were given",
                    type_name,
                    method.name,
                    handler.params.len(),
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return None;
        }
        for (p, arg) in handler.params.iter().zip(args.iter()) {
            let pty = resolve_type_ref(&p.type_ref, &ctx.input.types);
            let _ = type_of(arg, pty.as_ref(), ctx);
        }
        return resolve_type_ref(&handler.return_type, &ctx.input.types);
    }
    let table = ctx
        .input
        .methods
        .get(&type_name)
        .cloned()
        .unwrap_or_default();
    let Some(method_decl) = table.instance.get(&method.name).cloned() else {
        ctx.errors.push(CompileError::new(
            "karn.types.method_not_found",
            method.span,
            format!(
                "type `{}` has no instance method named `{}`",
                type_name, method.name
            ),
        ));
        return None;
    };
    // Param count excludes the implicit `self`.
    if method_decl.params.len() != args.len() {
        ctx.errors.push(
            CompileError::new(
                "karn.types.method_arity",
                method.span,
                format!(
                    "method `{}.{}` expects {} argument(s), but {} were given",
                    type_name,
                    method.name,
                    method_decl.params.len(),
                    args.len()
                ),
            )
            .with_label(method_decl.name.ident().span, "method declared here"),
        );
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    let mut ok = true;
    for (i, (param, arg)) in method_decl.params.iter().zip(args.iter()).enumerate() {
        let expected = resolve_type_ref(&param.type_ref, &ctx.input.types);
        let actual = type_of(arg, expected.as_ref(), ctx);
        let (Some(actual), Some(expected)) = (actual, expected) else {
            ok = false;
            continue;
        };
        if !compatible(&actual, &expected) {
            ctx.errors.push(CompileError::new(
                "karn.types.argument_mismatch",
                arg.span,
                format!(
                    "argument {} to `{}.{}` has type `{}`, but parameter `{}` expects `{}`",
                    i + 1,
                    type_name,
                    method.name,
                    actual.display(),
                    param.name.name,
                    expected.display()
                ),
            ));
            ok = false;
        }
    }
    let _ = span;
    if !ok {
        return None;
    }
    resolve_type_ref(&method_decl.return_type, &ctx.input.types)
}

fn check_match(
    discriminant: &Expr,
    arms: &[MatchArm],
    span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let disc_ty = type_of(discriminant, None, ctx)?;
    let expected_variants = variants_of(&disc_ty, &ctx.input.types);
    let Some(expected_variants) = expected_variants else {
        ctx.errors.push(CompileError::new(
            "karn.types.match_non_sum_discriminant",
            discriminant.span,
            format!(
                "cannot match on a value of type `{}` — `match` requires a sum, `Result`, or `Option`",
                disc_ty.display()
            ),
        ));
        return None;
    };
    let mut arm_types: Vec<(Ty, Span)> = Vec::new();
    let mut covered: HashSet<String> = HashSet::new();
    let mut saw_wildcard = false;
    let mut unreachable_reported = false;
    for arm in arms {
        if saw_wildcard && !unreachable_reported {
            ctx.errors.push(CompileError::new(
                "karn.types.unreachable_arm",
                arm.span,
                "this match arm is unreachable because a wildcard arm precedes it",
            ));
            unreachable_reported = true;
        }
        ctx.push_scope();
        match &arm.pattern {
            Pattern::Wildcard(_) => {
                saw_wildcard = true;
            }
            Pattern::Variant {
                type_name,
                variant,
                bindings,
                span: pat_span,
            } => {
                // v0.25: a qualified `T.Variant` pattern references `T`.
                if let Some(tn) = type_name
                    && ctx.input.types.contains_key(&tn.name)
                {
                    ctx.refs.record(tn.span, SymbolKind::Type, &tn.name);
                }
                // Validate the variant against expected_variants.
                let variant_info = expected_variants.iter().find(|v| v.name == variant.name);
                let Some(variant_info) = variant_info else {
                    ctx.errors.push(CompileError::new(
                        "karn.types.unknown_variant_in_pattern",
                        *pat_span,
                        format!(
                            "type `{}` has no variant `{}`",
                            disc_ty.display(),
                            variant.name
                        ),
                    ));
                    ctx.pop_scope();
                    continue;
                };
                // Optional qualifier must match the discriminant type's name.
                if let Some(tn) = type_name
                    && let Ty::Named { name, .. } = &disc_ty
                    && &tn.name != name
                {
                    ctx.errors.push(CompileError::new(
                        "karn.types.pattern_type_mismatch",
                        tn.span,
                        format!(
                            "pattern qualifier `{}` does not match the discriminant type `{}`",
                            tn.name, name
                        ),
                    ));
                }
                if !covered.insert(variant.name.clone()) {
                    ctx.errors.push(CompileError::new(
                        "karn.types.duplicate_variant_arm",
                        *pat_span,
                        format!("variant `{}` is matched more than once", variant.name),
                    ));
                }
                if bindings.is_empty() && !variant_info.payload.is_empty() {
                    // Variant has payload but pattern has no bindings — allowed,
                    // means "don't bind".
                } else if !bindings.is_empty() {
                    // Resolve each binding to a payload field's type.
                    if !variant_info.payload.is_empty() {
                        let payload_map: HashMap<&str, (usize, &Ty)> = variant_info
                            .payload
                            .iter()
                            .enumerate()
                            .map(|(i, (name, ty))| (name.as_str(), (i, ty)))
                            .collect();
                        // Allow positional or named bindings, but not both.
                        let any_named = bindings
                            .iter()
                            .any(|b| matches!(b.kind, PatternBindingKind::Named { .. }));
                        if any_named {
                            for b in bindings {
                                match &b.kind {
                                    PatternBindingKind::Named { field, name } => {
                                        let Some((_, ty)) = payload_map.get(field.name.as_str())
                                        else {
                                            ctx.errors.push(CompileError::new(
                                                "karn.types.unknown_pattern_field",
                                                field.span,
                                                format!(
                                                    "variant `{}` has no payload field `{}`",
                                                    variant.name, field.name
                                                ),
                                            ));
                                            continue;
                                        };
                                        if !b.is_wildcard() {
                                            ctx.bind(name.name.clone(), (*ty).clone());
                                        }
                                    }
                                    PatternBindingKind::Positional { .. } => {
                                        ctx.errors.push(CompileError::new(
                                            "karn.types.mixed_pattern_bindings",
                                            b.span,
                                            "pattern bindings must be all named (`field: name`) or all positional",
                                        ));
                                    }
                                }
                            }
                        } else if bindings.len() != variant_info.payload.len() {
                            ctx.errors.push(CompileError::new(
                                "karn.types.pattern_arity",
                                *pat_span,
                                format!(
                                    "variant `{}` has {} payload field(s), but the pattern has {} binding(s)",
                                    variant.name,
                                    variant_info.payload.len(),
                                    bindings.len()
                                ),
                            ));
                        } else {
                            for (b, (_, ty)) in bindings
                                .iter()
                                .zip(variant_info.payload.iter().map(|p| (&p.0, &p.1)))
                            {
                                if !b.is_wildcard() {
                                    ctx.bind(b.local_name().name.clone(), ty.clone());
                                }
                            }
                        }
                    } else {
                        ctx.errors.push(CompileError::new(
                            "karn.types.pattern_arity",
                            *pat_span,
                            format!(
                                "variant `{}` has no payload, but the pattern binds fields",
                                variant.name
                            ),
                        ));
                    }
                }
            }
        }
        let body_ty = match &arm.body {
            MatchBody::Expr(e) => maybe_auto_lift(type_of(e, expected, ctx), expected),
            MatchBody::Block(b) => type_of_block(b, expected, ctx),
        };
        ctx.pop_scope();
        if let Some(t) = body_ty {
            arm_types.push((t, arm.body.span()));
        }
    }
    // Exhaustiveness.
    if !saw_wildcard {
        for v in &expected_variants {
            if !covered.contains(&v.name) {
                ctx.errors.push(
                    CompileError::new(
                        "karn.types.non_exhaustive_match",
                        span,
                        format!(
                            "non-exhaustive `match` — variant `{}` of `{}` is not covered",
                            v.name,
                            disc_ty.display()
                        ),
                    )
                    .with_note("add a match arm for this variant, or use a wildcard `_` arm"),
                );
            }
        }
    }
    // All arm bodies must agree.
    if arm_types.is_empty() {
        return None;
    }
    let first = arm_types[0].0.clone();
    for (t, span) in arm_types.iter().skip(1) {
        if *t != first {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.match_arm_mismatch",
                    *span,
                    format!(
                        "match-arm body has type `{}`, but earlier arms have type `{}`",
                        t.display(),
                        first.display()
                    ),
                )
                .with_note("every arm of a `match` must produce the same type"),
            );
            return None;
        }
    }
    Some(first)
}

fn check_is(value: &Expr, pattern: &Pattern, _span: Span, ctx: &mut Ctx) -> Option<Ty> {
    let value_ty = type_of(value, None, ctx)?;
    let variants = variants_of(&value_ty, &ctx.input.types);
    match pattern {
        Pattern::Wildcard(_) => {
            // `_` matches anything, but is only meaningful over a sum today.
            if variants.is_none() {
                ctx.errors.push(CompileError::new(
                    "karn.types.is_non_sum",
                    pattern.span(),
                    format!(
                        "the `is` operator requires a sum, `Result`, or `Option` value, but got `{}`",
                        value_ty.display()
                    ),
                ));
            }
            return Some(Ty::Base(BaseType::Bool));
        }
        Pattern::Variant {
            variant,
            bindings,
            type_name,
            ..
        } => {
            // v0.25: a qualified `T.Variant` pattern references `T`.
            if let Some(tn) = type_name
                && ctx.input.types.contains_key(&tn.name)
            {
                ctx.refs.record(tn.span, SymbolKind::Type, &tn.name);
            }
            // 1. Sum-variant interpretation: the name is a variant of `value`'s
            //    sum type. (Takes priority when `value` is that sum.)
            let info = variants
                .as_ref()
                .and_then(|vs| vs.iter().find(|v| v.name == variant.name));
            let Some(info) = info else {
                // 2. v0.13 refinement narrowing: a bare nullary name that
                //    resolves to a refined type whose base matches `value`.
                if type_name.is_none()
                    && bindings.is_empty()
                    && let Some(decl) = ctx.input.types.get(&variant.name)
                    && let TypeBody::Refined { base, .. } = &decl.body
                {
                    if compatible(&value_ty, &Ty::Base(*base)) {
                        // v0.25: `x is RefinedType` names the type.
                        ctx.refs
                            .record(variant.span, SymbolKind::Type, &variant.name);
                        return Some(Ty::Base(BaseType::Bool));
                    }
                    ctx.errors.push(CompileError::new(
                        "karn.types.is_base_mismatch",
                        pattern.span(),
                        format!(
                            "`is {}` checks an `{}` value, but got `{}`",
                            variant.name,
                            base.name(),
                            value_ty.display()
                        ),
                    ));
                    return Some(Ty::Base(BaseType::Bool));
                }
                // 3. Neither a variant nor a base-compatible refined type.
                if variants.is_none() {
                    ctx.errors.push(CompileError::new(
                        "karn.types.is_non_sum",
                        pattern.span(),
                        format!(
                            "the `is` operator requires a sum, `Result`, or `Option` value, but got `{}`",
                            value_ty.display()
                        ),
                    ));
                } else {
                    ctx.errors.push(CompileError::new(
                        "karn.types.is_unknown_variant",
                        variant.span,
                        format!(
                            "type `{}` has no variant `{}`",
                            value_ty.display(),
                            variant.name
                        ),
                    ));
                }
                return Some(Ty::Base(BaseType::Bool));
            };
            // Just validate bindings shape; binding TYPES introduced via
            // `collect_is_bindings` are handled at the consumer site.
            if !bindings.is_empty() && info.payload.is_empty() {
                ctx.errors.push(CompileError::new(
                    "karn.types.pattern_arity",
                    pattern.span(),
                    format!(
                        "variant `{}` has no payload, but the pattern binds fields",
                        variant.name
                    ),
                ));
            } else if !bindings.is_empty() {
                let any_named = bindings
                    .iter()
                    .any(|b| matches!(b.kind, PatternBindingKind::Named { .. }));
                if !any_named && bindings.len() != info.payload.len() {
                    ctx.errors.push(CompileError::new(
                        "karn.types.pattern_arity",
                        pattern.span(),
                        format!(
                            "variant `{}` has {} payload field(s), but the pattern has {} binding(s)",
                            variant.name,
                            info.payload.len(),
                            bindings.len()
                        ),
                    ));
                }
            }
        }
    }
    Some(Ty::Base(BaseType::Bool))
}

/// Collect the bindings introduced by `is` patterns inside a condition
/// expression. Currently we recognise:
///  - `expr is Pat`
///  - `lhs && rhs`        (recursive into both sides; later wins on collision)
///  - `(expr)` parens
fn collect_is_bindings(expr: &Expr, ctx: &mut Ctx) -> Vec<(String, Ty)> {
    let mut out = Vec::new();
    collect_is_bindings_into(expr, ctx, &mut out);
    out
}

fn collect_is_bindings_into(expr: &Expr, ctx: &mut Ctx, out: &mut Vec<(String, Ty)>) {
    match &expr.kind {
        ExprKind::Is { value, pattern } => {
            // Recompute value type from the expr_types side-table; this avoids
            // mutating type-checking state. If we don't have it, fall back to
            // recomputing.
            let value_ty = ctx.expr_types.get(&value.span).cloned();
            if let Some(value_ty) = value_ty {
                // v0.13 refinement narrowing: `ident is RefinedType` re-binds the
                // identifier to the refined type in the narrowed branch.
                if let (
                    ExprKind::Ident(id),
                    Pattern::Variant {
                        variant,
                        bindings,
                        type_name: None,
                        ..
                    },
                ) = (&value.kind, pattern)
                    && bindings.is_empty()
                    && variants_of(&value_ty, &ctx.input.types)
                        .is_none_or(|vs| !vs.iter().any(|v| v.name == variant.name))
                    && let Some(decl) = ctx.input.types.get(&variant.name)
                    && let TypeBody::Refined { base, .. } = &decl.body
                    && compatible(&value_ty, &Ty::Base(*base))
                {
                    out.push((
                        id.name.clone(),
                        Ty::Named {
                            name: variant.name.clone(),
                            kind: NamedKind::Refined(*base),
                        },
                    ));
                    return;
                }
                gather_pattern_bindings(&value_ty, pattern, &ctx.input.types, out);
            }
        }
        ExprKind::BinOp(BinOp::And, lhs, rhs) => {
            collect_is_bindings_into(lhs, ctx, out);
            collect_is_bindings_into(rhs, ctx, out);
        }
        ExprKind::Paren(inner) => collect_is_bindings_into(inner, ctx, out),
        _ => {}
    }
}

fn gather_pattern_bindings(
    value_ty: &Ty,
    pattern: &Pattern,
    types: &HashMap<String, TypeDecl>,
    out: &mut Vec<(String, Ty)>,
) {
    let Pattern::Variant {
        variant, bindings, ..
    } = pattern
    else {
        return;
    };
    let Some(variants) = variants_of(value_ty, types) else {
        return;
    };
    let Some(info) = variants.iter().find(|v| v.name == variant.name) else {
        return;
    };
    let any_named = bindings
        .iter()
        .any(|b| matches!(b.kind, PatternBindingKind::Named { .. }));
    if any_named {
        let payload_map: HashMap<&str, &Ty> =
            info.payload.iter().map(|(n, t)| (n.as_str(), t)).collect();
        for b in bindings {
            if let PatternBindingKind::Named { field, name } = &b.kind
                && let Some(ty) = payload_map.get(field.name.as_str())
                && name.name != "_"
            {
                out.push((name.name.clone(), (*ty).clone()));
            }
        }
    } else {
        for (b, (_, ty)) in bindings.iter().zip(info.payload.iter()) {
            if !b.is_wildcard() {
                out.push((b.local_name().name.clone(), ty.clone()));
            }
        }
    }
}

/// A flattened view of a type's variants (name + payload types).
struct VariantInfo {
    name: String,
    payload: Vec<(String, Ty)>,
}

/// If `receiver` resolves to a consumed-context prefix (an alias or a
/// dotted qualified name appearing in `consumes`), return the consumed
/// context's qualified name. Otherwise None. Local bindings, types, and
/// capabilities take precedence — those are checked at the call site.
fn cross_context_prefix(receiver: &Expr, ctx: &Ctx) -> Option<String> {
    let info = &ctx.input.cross_context;
    if info.consumed_contexts.is_empty() && info.aliases.is_empty() {
        return None;
    }
    // Walk the receiver to assemble a candidate dotted name. Supports:
    //   Ident(X)                                 -> "X"
    //   FieldAccess { Ident(A), B }              -> "A.B"
    //   FieldAccess { FieldAccess { Ident(A), B }, C } -> "A.B.C"
    let candidate = flatten_ident_chain(receiver)?;
    let head = candidate.split('.').next().unwrap_or("");
    // The head must not shadow a local binding / capability / declared type.
    if ctx.lookup(head).is_some() {
        return None;
    }
    if ctx.capabilities.contains_key(head) || ctx.declared_capabilities.contains_key(head) {
        return None;
    }
    // If the head is a known local type, only an alias whose name happens to
    // collide could redirect this; aliases conflicting with types are an
    // error in project.rs, so a clash here is impossible at this point.
    info.resolve_prefix(candidate.as_str())
}

/// Flatten an `Ident`/`FieldAccess` chain into its dotted name, or None if
/// any segment isn't a bare identifier.
fn flatten_ident_chain(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Ident(id) => Some(id.name.clone()),
        ExprKind::FieldAccess { receiver, field } => {
            let head = flatten_ident_chain(receiver)?;
            Some(format!("{head}.{}", field.name))
        }
        _ => None,
    }
}

/// Type-check a cross-context service call (v0.6 §4.2). `receiver` carries
/// the prefix's source span for diagnostics. `consumed` is the resolved
/// qualified name of the consumed context.
/// v0.15: type-check a cross-context capability call `B.Cap.op(args)` /
/// `Alias.Cap.op(args)`. The capability operation signatures are carried in
/// `consumed_capabilities` (in the providing context's namespace); the
/// capability must be listed in the handler/provider's `given` clause.
fn check_cross_context_capability_call(
    receiver: &Expr,
    consumed: &str,
    cap: &str,
    method: &Ident,
    args: &[Expr],
    _span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    // Capability calls require an effectful body (same rule as local ones).
    if !ctx.effectful {
        ctx.errors.push(CompileError::new(
            "karn.effect.capability_in_pure_context",
            method.span,
            format!(
                "capability `{consumed}.{cap}` can only be called inside an effectful body (one returning `Effect[T]`)"
            ),
        ));
    }
    // The capability must be declared in this handler/provider's `given`.
    // The local deps key is the capability's simple name.
    if !ctx.given_remaining.contains(cap) {
        let mut err = CompileError::new(
            "karn.given.undeclared_capability",
            receiver.span,
            format!("capability `{consumed}.{cap}` is used but not listed in the `given` clause"),
        )
        .with_note(format!(
            "add `{consumed}.{cap}` to the handler's `given` clause so the dependency surface is visible at the declaration site"
        ));
        // v0.26 (ADR 0054): the one-click counterpart of the note — the
        // clause entry is the qualified form the user writes (`B.Cap`).
        if let Some((span, insert)) = given_insertion_edit(
            &ctx.given_entries,
            ctx.given_anchor,
            &format!("{consumed}.{cap}"),
        ) {
            err = err.with_suggestion(
                format!("add `{consumed}.{cap}` to the `given` clause"),
                vec![(span, insert)],
                Applicability::MachineApplicable,
            );
        }
        ctx.errors.push(err);
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    ctx.given_used.insert(cap.to_string());

    let info = &ctx.input.cross_context;
    let op = info
        .consumed_capabilities
        .get(consumed)
        .and_then(|caps| caps.get(cap))
        .and_then(|c| c.ops.iter().find(|o| o.name == method.name))
        .cloned();
    let Some(op) = op else {
        ctx.errors.push(CompileError::new(
            "karn.capability.unknown_operation",
            method.span,
            format!(
                "capability `{consumed}.{cap}` has no operation named `{}`",
                method.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    };
    if op.params.len() != args.len() {
        ctx.errors.push(CompileError::new(
            "karn.capability.op_arity",
            method.span,
            format!(
                "capability operation `{consumed}.{cap}.{}` expects {} argument(s), but {} were given",
                method.name,
                op.params.len(),
                args.len()
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }

    // Resolve parameter / return types in the consumed context's namespace.
    let consumed_types = info
        .consumed_types
        .get(consumed)
        .cloned()
        .unwrap_or_default();
    let mut all_ok = true;
    for (i, ((pname, ptype_ref), arg)) in op.params.iter().zip(args.iter()).enumerate() {
        let param_ty = resolve_type_ref(ptype_ref, &consumed_types).unwrap_or(Ty::Unit);
        let Some(arg_ty) = type_of(arg, None, ctx) else {
            all_ok = false;
            continue;
        };
        if !structurally_compatible(&arg_ty, &param_ty, &ctx.input.types, &consumed_types) {
            ctx.errors.push(CompileError::new(
                "karn.boundary.structural_mismatch",
                arg.span,
                format!(
                    "cross-context argument {} to `{consumed}.{cap}.{}` has type `{}`, but parameter `{pname}` expects `{}`",
                    i + 1,
                    method.name,
                    arg_ty.display(),
                    param_ty.display(),
                ),
            ));
            all_ok = false;
        }
    }
    if !all_ok {
        return None;
    }
    let raw_ret = resolve_type_ref(&op.return_type, &consumed_types).unwrap_or(Ty::Unit);
    Some(rebrand_return_type(&raw_ret, &ctx.input.types))
}

fn check_cross_context_call(
    receiver: &Expr,
    consumed: &str,
    method: &Ident,
    args: &[Expr],
    _span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    // The consuming context must be effectful at this call site (services
    // and agent handlers are; pure free fns are not).
    if !ctx.effectful {
        ctx.errors.push(
            CompileError::new(
                "karn.effect.cross_context_in_pure_context",
                method.span,
                format!(
                    "cross-context service call `{}.{}` can only be made inside an effectful body (one returning `Effect[T]`)",
                    consumed, method.name
                ),
            )
            .with_label(receiver.span, "consumed context prefix"),
        );
    }
    let info = &ctx.input.cross_context;
    let Some(svcs) = info.consumed_services.get(consumed) else {
        ctx.errors.push(
            CompileError::new(
                "karn.consumes.unknown_context",
                receiver.span,
                format!("context `{consumed}` is not in scope here"),
            )
            .with_note(
                "add a `consumes` clause for the target context at the top of the consuming context",
            ),
        );
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    };
    let Some(service) = svcs.get(&method.name).cloned() else {
        ctx.errors.push(
            CompileError::new(
                "karn.consumes.unknown_service",
                method.span,
                format!(
                    "context `{consumed}` has no service named `{}`",
                    method.name
                ),
            )
            .with_note(
                "cross-context calls require an `on call` service handler in the consumed context",
            ),
        );
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    };
    ctx.refs
        .record_in_unit(method.span, SymbolKind::Service, &method.name, consumed);

    if service.params.len() != args.len() {
        ctx.errors.push(
            CompileError::new(
                "karn.consumes.service_arity",
                method.span,
                format!(
                    "cross-context service `{consumed}.{}` expects {} argument(s), but {} were given",
                    method.name,
                    service.params.len(),
                    args.len()
                ),
            )
            .with_label(service.span, "service declared here"),
        );
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }

    // Resolve the consumed-context types so we can describe parameter shapes.
    let consumed_types = info
        .consumed_types
        .get(consumed)
        .cloned()
        .unwrap_or_default();

    // Walk each argument, checking structural compatibility (Phase 4).
    let mut all_ok = true;
    for (i, ((pname, ptype_ref), arg)) in service.params.iter().zip(args.iter()).enumerate() {
        let param_ty = resolve_type_ref(ptype_ref, &consumed_types).unwrap_or(Ty::Unit);
        // Type-check the argument in the caller's context.
        let arg_ty = type_of(arg, None, ctx);
        let Some(arg_ty) = arg_ty else {
            all_ok = false;
            continue;
        };
        if !structurally_compatible(&arg_ty, &param_ty, &ctx.input.types, &consumed_types) {
            ctx.errors.push(
                CompileError::new(
                    "karn.boundary.structural_mismatch",
                    arg.span,
                    format!(
                        "cross-context argument {} to `{consumed}.{}` has type `{}` in `{}`, but parameter `{pname}` expects `{}` in `{}`",
                        i + 1,
                        method.name,
                        arg_ty.display(),
                        ctx.input
                            .cross_context
                            .self_context
                            .as_deref()
                            .unwrap_or("?"),
                        param_ty.display(),
                        consumed,
                    ),
                )
                .with_label(service.span, "service declared here")
                .with_note(
                    "values crossing a context boundary must have structurally compatible types (same commons-derived type, or identical record/sum shape)",
                ),
            );
            all_ok = false;
        }
    }
    if !all_ok {
        return None;
    }

    // Return type rebrand: project the consumed context's return type into
    // the calling context's namespace by renaming named types whose unqualified
    // name appears in the caller's type table (v0.6 §4.5).
    let raw_ret = resolve_type_ref(&service.return_type, &consumed_types).unwrap_or(Ty::Unit);
    let rebranded = rebrand_return_type(&raw_ret, &ctx.input.types);
    Some(rebranded)
}

/// Project a return type produced in the consumed context's namespace into
/// the caller's namespace by re-resolving named types that exist on both
/// sides. The structural shape stays the same; the brand changes.
fn rebrand_return_type(t: &Ty, caller_types: &HashMap<String, TypeDecl>) -> Ty {
    match t {
        Ty::Named { name, kind } => {
            // If the caller's namespace has the same name, prefer the caller's
            // view (it carries the caller's brand at emission time). Otherwise
            // keep the consumed-context name; the caller can hold it opaquely.
            if let Some(decl) = caller_types.get(name) {
                named_ty(decl)
            } else {
                Ty::Named {
                    name: name.clone(),
                    kind: kind.clone(),
                }
            }
        }
        Ty::Result(t, e) => Ty::Result(
            Box::new(rebrand_return_type(t, caller_types)),
            Box::new(rebrand_return_type(e, caller_types)),
        ),
        Ty::Option(t) => Ty::Option(Box::new(rebrand_return_type(t, caller_types))),
        Ty::Effect(t) => Ty::Effect(Box::new(rebrand_return_type(t, caller_types))),
        Ty::HttpResult(t) => Ty::HttpResult(Box::new(rebrand_return_type(t, caller_types))),
        Ty::List(t) => Ty::List(Box::new(rebrand_return_type(t, caller_types))),
        Ty::Map(k, v) => Ty::Map(
            Box::new(rebrand_return_type(k, caller_types)),
            Box::new(rebrand_return_type(v, caller_types)),
        ),
        Ty::Base(_) | Ty::ValidationError | Ty::JsonError | Ty::Unit => t.clone(),
        // v0.20a: function types are confined to non-boundary positions
        // (`karn.types.function_at_boundary`), so a cross-context return can
        // never carry one; Vars never escape call checking.
        Ty::Fn { .. } | Ty::Var(_) => t.clone(),
    }
}

/// Structural compatibility check for values crossing a context boundary
/// (v0.6 §4.3). The two types may be expressed in different namespaces
/// (caller-side / callee-side type tables), so we walk them in parallel
/// against their respective tables.
fn structurally_compatible(
    arg: &Ty,
    param: &Ty,
    arg_types: &HashMap<String, TypeDecl>,
    param_types: &HashMap<String, TypeDecl>,
) -> bool {
    structurally_compatible_inner(arg, param, arg_types, param_types, &mut HashSet::new())
}

fn structurally_compatible_inner(
    arg: &Ty,
    param: &Ty,
    arg_types: &HashMap<String, TypeDecl>,
    param_types: &HashMap<String, TypeDecl>,
    visited: &mut HashSet<(String, String)>,
) -> bool {
    match (arg, param) {
        (Ty::Base(a), Ty::Base(b)) => a == b,
        (Ty::ValidationError, Ty::ValidationError) => true,
        (Ty::JsonError, Ty::JsonError) => true,
        (Ty::Unit, Ty::Unit) => true,
        (Ty::Result(t1, e1), Ty::Result(t2, e2)) => {
            structurally_compatible_inner(t1, t2, arg_types, param_types, visited)
                && structurally_compatible_inner(e1, e2, arg_types, param_types, visited)
        }
        (Ty::Option(a), Ty::Option(b)) => {
            structurally_compatible_inner(a, b, arg_types, param_types, visited)
        }
        (Ty::Effect(a), Ty::Effect(b)) => {
            structurally_compatible_inner(a, b, arg_types, param_types, visited)
        }
        (Ty::HttpResult(a), Ty::HttpResult(b)) => {
            structurally_compatible_inner(a, b, arg_types, param_types, visited)
        }
        (Ty::Named { name: an, .. }, Ty::Named { name: bn, .. }) => {
            // Cycle break: once we've started comparing (an, bn) we trust
            // the recursive case to succeed.
            let key = (an.clone(), bn.clone());
            if !visited.insert(key.clone()) {
                return true;
            }
            let ok = structural_compare_named(an, bn, arg_types, param_types, visited);
            visited.remove(&key);
            ok
        }
        // Refined-named widens to its base; tolerate one-sided widening only
        // when comparing within the same nominal name (handled above) or when
        // the param accepts a plain base.
        (
            Ty::Named {
                kind: NamedKind::Refined(b),
                ..
            },
            Ty::Base(target),
        ) => b == target,
        _ => false,
    }
}

fn structural_compare_named(
    arg_name: &str,
    param_name: &str,
    arg_types: &HashMap<String, TypeDecl>,
    param_types: &HashMap<String, TypeDecl>,
    visited: &mut HashSet<(String, String)>,
) -> bool {
    // The "same nominal name" case is the most common: both sides derive
    // the same commons type. Compare their structural shapes.
    let Some(arg_decl) = arg_types.get(arg_name) else {
        return false;
    };
    let Some(param_decl) = param_types.get(param_name) else {
        return false;
    };
    match (&arg_decl.body, &param_decl.body) {
        (
            TypeBody::Refined {
                base: ab,
                refinement: ar,
                ..
            },
            TypeBody::Refined {
                base: bb,
                refinement: br,
                ..
            },
        ) => {
            if ab != bb {
                return false;
            }
            refinements_match(ar.as_ref(), br.as_ref())
        }
        (
            TypeBody::Opaque {
                base: ab,
                refinement: ar,
                ..
            },
            TypeBody::Opaque {
                base: bb,
                refinement: br,
                ..
            },
        ) => {
            // Opaque types must share a name to be compatible (a context's
            // opaque cannot be reinterpreted as a different context's opaque).
            if arg_name != param_name {
                return false;
            }
            if ab != bb {
                return false;
            }
            refinements_match(ar.as_ref(), br.as_ref())
        }
        (TypeBody::Record(a), TypeBody::Record(b)) => {
            if a.fields.len() != b.fields.len() {
                return false;
            }
            for af in &a.fields {
                let Some(bf) = b.fields.iter().find(|f| f.name.name == af.name.name) else {
                    return false;
                };
                let at = resolve_type_ref(&af.type_ref, arg_types);
                let bt = resolve_type_ref(&bf.type_ref, param_types);
                let (Some(at), Some(bt)) = (at, bt) else {
                    return false;
                };
                if !structurally_compatible_inner(&at, &bt, arg_types, param_types, visited) {
                    return false;
                }
            }
            true
        }
        (TypeBody::Sum(a), TypeBody::Sum(b)) => {
            if a.variants.len() != b.variants.len() {
                return false;
            }
            for av in &a.variants {
                let Some(bv) = b.variants.iter().find(|v| v.name.name == av.name.name) else {
                    return false;
                };
                if av.payload.len() != bv.payload.len() {
                    return false;
                }
                for (af, bf) in av.payload.iter().zip(bv.payload.iter()) {
                    if af.name.name != bf.name.name {
                        return false;
                    }
                    let at = resolve_type_ref(&af.type_ref, arg_types);
                    let bt = resolve_type_ref(&bf.type_ref, param_types);
                    let (Some(at), Some(bt)) = (at, bt) else {
                        return false;
                    };
                    if !structurally_compatible_inner(&at, &bt, arg_types, param_types, visited) {
                        return false;
                    }
                }
            }
            true
        }
        _ => false,
    }
}

fn refinements_match(a: Option<&Refinement>, b: Option<&Refinement>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(_), None) => true, // sending side is more restrictive — receiving is more permissive
        (None, Some(_)) => false,
        (Some(a), Some(b)) => {
            if a.predicates.len() != b.predicates.len() {
                return false;
            }
            // Exact match required (per spec §4.3 conservative rule).
            // Predicate order matters here; refinements are conventionally
            // written in a fixed order.
            for (pa, pb) in a.predicates.iter().zip(b.predicates.iter()) {
                if !predicate_eq(&pa.kind, &pb.kind) {
                    return false;
                }
            }
            true
        }
    }
}

fn predicate_eq(a: &PredKind, b: &PredKind) -> bool {
    match (a, b) {
        (PredKind::Matches(x), PredKind::Matches(y)) => x == y,
        (PredKind::InRange(a1, a2), PredKind::InRange(b1, b2)) => a1 == b1 && a2 == b2,
        (PredKind::InRangeF(a1, a2), PredKind::InRangeF(b1, b2)) => {
            a1.value == b1.value && a2.value == b2.value
        }
        (PredKind::MinLength(a), PredKind::MinLength(b)) => a == b,
        (PredKind::MaxLength(a), PredKind::MaxLength(b)) => a == b,
        (PredKind::Length(a), PredKind::Length(b)) => a == b,
        (PredKind::NonNegative, PredKind::NonNegative) => true,
        (PredKind::Positive, PredKind::Positive) => true,
        (PredKind::NonEmpty, PredKind::NonEmpty) => true,
        _ => false,
    }
}

fn variants_of(ty: &Ty, types: &HashMap<String, TypeDecl>) -> Option<Vec<VariantInfo>> {
    match ty {
        Ty::Named {
            kind: NamedKind::Sum,
            name,
        } => {
            let decl = types.get(name)?;
            if let TypeBody::Sum(s) = &decl.body {
                Some(
                    s.variants
                        .iter()
                        .map(|v| VariantInfo {
                            name: v.name.name.clone(),
                            payload: v
                                .payload
                                .iter()
                                .map(|f| {
                                    let t = resolve_type_ref(&f.type_ref, types)
                                        .unwrap_or(Ty::Base(BaseType::Int));
                                    (f.name.name.clone(), t)
                                })
                                .collect(),
                        })
                        .collect(),
                )
            } else {
                None
            }
        }
        Ty::Result(t, e) => Some(vec![
            VariantInfo {
                name: "Ok".to_string(),
                payload: vec![("value".to_string(), (**t).clone())],
            },
            VariantInfo {
                name: "Err".to_string(),
                payload: vec![("error".to_string(), (**e).clone())],
            },
        ]),
        Ty::Option(t) => Some(vec![
            VariantInfo {
                name: "Some".to_string(),
                payload: vec![("value".to_string(), (**t).clone())],
            },
            VariantInfo {
                name: "None".to_string(),
                payload: vec![],
            },
        ]),
        Ty::HttpResult(t) => Some(
            HTTP_VARIANTS
                .iter()
                .map(|v| VariantInfo {
                    name: v.name.to_string(),
                    payload: match v.payload {
                        HttpVariantPayload::None => vec![],
                        HttpVariantPayload::Value => vec![("value".to_string(), (**t).clone())],
                        HttpVariantPayload::Message => {
                            vec![("message".to_string(), Ty::Base(BaseType::String))]
                        }
                    },
                })
                .collect(),
        ),
        _ => None,
    }
}

// ── v0.9.2: agent state-field zeroability ──────────────────────────────────
//
// Fresh agent state is the zero-value record (finding #10): a never-seen key
// reads `0` / `false` / `""` / `None` rather than `undefined`. A type is
// *zeroable* when it has a defined zero; agent state fields must be zeroable,
// since a fresh key has no committed value to load. Non-zeroable fields (a
// non-Option sum, an opaque type, or a refined type whose refinement excludes
// the underlying zero) are a compile error until explicit-initialiser syntax
// lands.

/// The TypeScript zero-value expression for `type_ref` (with an optional
/// inline field refinement), or `None` if the type is not zeroable.
pub fn zero_value_ts(
    type_ref: &TypeRef,
    inline: Option<&Refinement>,
    types: &HashMap<String, TypeDecl>,
) -> Option<String> {
    match type_ref {
        TypeRef::Base(b, _) => {
            if refinement_admits_zero(*b, inline) {
                zero_of_base(*b)
            } else {
                None
            }
        }
        // Option's zero is None, regardless of the inner type.
        TypeRef::Option(_, _) => Some("None".to_string()),
        TypeRef::Named(id) => {
            let decl = types.get(&id.name)?;
            match &decl.body {
                TypeBody::Refined {
                    base, refinement, ..
                } => {
                    if refinement_admits_zero(*base, refinement.as_ref()) {
                        zero_of_base(*base)
                    } else {
                        None
                    }
                }
                TypeBody::Record(rec) => agent_state_zero_record(&rec.fields, types),
                // Non-Option sum types and opaque types have no defined zero.
                TypeBody::Sum(_) | TypeBody::Opaque { .. } => None,
            }
        }
        // Result / Effect / HttpResult / ValidationError / Unit are not
        // admissible state-field types and have no zero.
        _ => None,
    }
}

/// The zero record `{ f₁: z₁, …, fₙ: zₙ }` for a set of fields, or `None` if
/// any field is not zeroable.
pub fn agent_state_zero_record(
    fields: &[RecordField],
    types: &HashMap<String, TypeDecl>,
) -> Option<String> {
    let mut parts = Vec::new();
    for f in fields {
        let z = zero_value_ts(&f.type_ref, f.refinement.as_ref(), types)?;
        parts.push(format!("{}: {}", f.name.name, z));
    }
    Some(format!("{{ {} }}", parts.join(", ")))
}

fn zero_of_base(b: BaseType) -> Option<String> {
    Some(
        match b {
            BaseType::Int => "0",
            BaseType::Bool => "false",
            BaseType::String => "\"\"",
            BaseType::Float => "0",
        }
        .to_string(),
    )
}

/// Whether the zero value of `base` satisfies every predicate in `refinement`.
/// Conservative: any predicate we cannot prove admits the zero returns false,
/// surfacing the `non_zeroable_state_field` diagnostic rather than risking an
/// invalid fresh state.
fn refinement_admits_zero(base: BaseType, refinement: Option<&Refinement>) -> bool {
    let Some(r) = refinement else {
        return true;
    };
    r.predicates.iter().all(|p| pred_admits_zero(base, &p.kind))
}

fn pred_admits_zero(base: BaseType, k: &PredKind) -> bool {
    match base {
        BaseType::Int => match k {
            PredKind::NonNegative => true,
            PredKind::Positive => false,
            PredKind::InRange(lo, hi) => *lo <= 0 && 0 <= *hi,
            // Length/Matches predicates don't apply to Int; reject conservatively.
            _ => false,
        },
        BaseType::String => match k {
            PredKind::Matches(p) => regex_matches_empty(p),
            PredKind::MinLength(n) => *n <= 0,
            PredKind::MaxLength(n) => *n >= 0,
            PredKind::Length(n) => *n == 0,
            PredKind::NonEmpty => false,
            // Numeric predicates don't apply to String; reject conservatively.
            _ => false,
        },
        // The only Bool zero is `false`; no Bool refinement predicates exist.
        BaseType::Bool => true,
        BaseType::Float => match k {
            PredKind::NonNegative => true,
            PredKind::Positive => false,
            PredKind::InRangeF(lo, hi) => lo.value <= 0.0 && 0.0 <= hi.value,
            // Other predicates don't apply to Float; reject conservatively.
            _ => false,
        },
    }
}

/// Does the refinement pattern match the empty string? Anchored exactly as the
/// emitted refined-type constructor anchors it (`^(?:pattern)$`).
fn regex_matches_empty(pattern: &str) -> bool {
    match Regex::new(&format!("^(?:{pattern})$")) {
        Ok(re) => re.is_match(""),
        Err(_) => false,
    }
}

#[cfg(test)]
mod generics_tests {
    use super::*;

    fn var(n: &str) -> Ty {
        Ty::Var(n.to_string())
    }
    fn int() -> Ty {
        Ty::Base(BaseType::Int)
    }

    #[test]
    fn unify_binds_and_holds() {
        let mut s = HashMap::new();
        assert!(unify(&var("A"), &int(), &mut s));
        assert_eq!(s.get("A"), Some(&int()));
        // Same binding again: fine. A different one: conflict.
        assert!(unify(&var("A"), &int(), &mut s));
        assert!(!unify(&var("A"), &Ty::Base(BaseType::String), &mut s));
    }

    #[test]
    fn unify_walks_structure() {
        let mut s = HashMap::new();
        let pattern = Ty::Fn {
            params: vec![var("A")],
            ret: Box::new(Ty::Effect(Box::new(var("B")))),
        };
        let actual = Ty::Fn {
            params: vec![int()],
            ret: Box::new(Ty::Effect(Box::new(Ty::Base(BaseType::String)))),
        };
        assert!(unify(&pattern, &actual, &mut s));
        assert_eq!(s.get("A"), Some(&int()));
        assert_eq!(s.get("B"), Some(&Ty::Base(BaseType::String)));
    }

    #[test]
    fn substitute_grounds_fully() {
        let mut s = HashMap::new();
        s.insert("A".to_string(), int());
        let t = Ty::Option(Box::new(Ty::Fn {
            params: vec![var("A")],
            ret: Box::new(var("A")),
        }));
        let g = substitute(&t, &s);
        assert!(!contains_var(&g));
    }

    /// The §2 invariant (pinned per the plan): every expected-driven feature
    /// in `type_of` matches *concrete* `Ty` variants, so a Var-bearing
    /// expected imposes no constraint — `compatible` must simply reject
    /// Var-vs-ground pairs rather than panic or accept.
    #[test]
    fn var_bearing_expected_is_benign() {
        assert!(!compatible(&int(), &var("A")));
        assert!(!compatible(&var("A"), &int()));
        // Rigid vars: name equality only.
        assert!(compatible(&var("A"), &var("A")));
        assert!(!compatible(&var("A"), &var("B")));
    }
}

/// Characterization pins for `checker.rs`'s pure free functions (v0.29.10
/// slice 0). These pin *current* behaviour ahead of the upcoming module split
/// so the verbatim moves are verifiable. Any surprising behaviour is pinned
/// as-is, flagged with a comment — these are not specifications.
#[cfg(test)]
mod pure_helper_pins {
    use super::*;
    use crate::ast::{FloatBound, RefinementPred};

    // -- small constructors ------------------------------------------------

    fn sp() -> Span {
        Span::new(0, 0)
    }
    fn ident(n: &str) -> Ident {
        Ident {
            name: n.to_string(),
            span: sp(),
        }
    }
    fn var(n: &str) -> Ty {
        Ty::Var(n.to_string())
    }
    fn int() -> Ty {
        Ty::Base(BaseType::Int)
    }
    fn string() -> Ty {
        Ty::Base(BaseType::String)
    }
    fn expr(kind: ExprKind) -> Expr {
        Expr { kind, span: sp() }
    }
    fn pred(kind: PredKind) -> RefinementPred {
        RefinementPred { kind, span: sp() }
    }
    fn refinement(preds: Vec<PredKind>) -> Refinement {
        Refinement {
            predicates: preds.into_iter().map(pred).collect(),
            span: sp(),
        }
    }
    fn fbound(value: f64) -> FloatBound {
        FloatBound {
            value,
            lexeme: value.to_string(),
        }
    }
    fn refined_decl(name: &str, base: BaseType, refinement: Option<Refinement>) -> TypeDecl {
        TypeDecl {
            name: ident(name),
            body: TypeBody::Refined {
                base,
                base_span: sp(),
                refinement,
            },
            documentation: None,
            span: sp(),
            trivia: crate::ast::Trivia::default(),
        }
    }
    fn record_decl(name: &str) -> TypeDecl {
        TypeDecl {
            name: ident(name),
            body: TypeBody::Record(crate::ast::RecordBody {
                fields: vec![],
                span: sp(),
            }),
            documentation: None,
            span: sp(),
            trivia: crate::ast::Trivia::default(),
        }
    }

    // -- unify -------------------------------------------------------------

    #[test]
    fn unify_identical_concrete_types() {
        let mut s = HashMap::new();
        assert!(unify(&int(), &int(), &mut s));
        assert!(s.is_empty());
    }

    #[test]
    fn unify_var_binds_in_subst() {
        let mut s = HashMap::new();
        assert!(unify(&var("A"), &string(), &mut s));
        assert_eq!(s.get("A"), Some(&string()));
    }

    #[test]
    fn unify_nested_generic_binds() {
        // List[A] vs List[Int] binds A := Int.
        let mut s = HashMap::new();
        let pat = Ty::List(Box::new(var("A")));
        let act = Ty::List(Box::new(int()));
        assert!(unify(&pat, &act, &mut s));
        assert_eq!(s.get("A"), Some(&int()));
    }

    #[test]
    fn unify_surprise_concrete_mismatch_returns_true() {
        // SURPRISING (pinned as-is): `unify`'s catch-all is `_ => true`, so a
        // ground-vs-ground mismatch (Int vs String) and a constructor mismatch
        // (List vs Option) both *succeed* here — `compatible` owns those
        // diagnostics post-substitution, not `unify`.
        let mut s = HashMap::new();
        assert!(unify(&int(), &string(), &mut s));
        assert!(unify(
            &Ty::List(Box::new(int())),
            &Ty::Option(Box::new(int())),
            &mut s,
        ));
        // The only false paths: a Var rebind conflict and an Fn arity mismatch.
        let mut s2 = HashMap::new();
        assert!(unify(&var("A"), &int(), &mut s2));
        assert!(!unify(&var("A"), &string(), &mut s2));
        let mut s3 = HashMap::new();
        let f1 = Ty::Fn {
            params: vec![int()],
            ret: Box::new(int()),
        };
        let f2 = Ty::Fn {
            params: vec![int(), int()],
            ret: Box::new(int()),
        };
        assert!(!unify(&f1, &f2, &mut s3));
    }

    // -- substitute --------------------------------------------------------

    #[test]
    fn substitute_replaces_bound_var() {
        let mut s = HashMap::new();
        s.insert("A".to_string(), int());
        assert_eq!(substitute(&var("A"), &s), int());
    }

    #[test]
    fn substitute_recurses_into_nested() {
        let mut s = HashMap::new();
        s.insert("A".to_string(), string());
        let t = Ty::Map(Box::new(var("A")), Box::new(int()));
        assert_eq!(
            substitute(&t, &s),
            Ty::Map(Box::new(string()), Box::new(int())),
        );
    }

    #[test]
    fn substitute_leaves_unbound_var_alone() {
        let s = HashMap::new();
        assert_eq!(substitute(&var("Z"), &s), var("Z"));
    }

    // -- contains_var / contains_flexible_var ------------------------------

    #[test]
    fn contains_var_positive_and_negative() {
        assert!(contains_var(&Ty::Option(Box::new(var("A")))));
        assert!(!contains_var(&Ty::Option(Box::new(int()))));
        assert!(!contains_var(&int()));
    }

    #[test]
    fn contains_flexible_var_respects_rigid_set() {
        let mut rigid = HashSet::new();
        rigid.insert("A".to_string());
        // A is rigid → not flexible.
        assert!(!contains_flexible_var(&var("A"), &rigid));
        // B is not rigid → flexible.
        assert!(contains_flexible_var(&var("B"), &rigid));
        // No vars at all → not flexible.
        assert!(!contains_flexible_var(&int(), &rigid));
    }

    // -- peel_to_* ---------------------------------------------------------

    #[test]
    fn peel_to_result_matches_and_misses() {
        let r = Ty::Result(Box::new(int()), Box::new(string()));
        assert_eq!(peel_to_result(&r), Some((int(), string())));
        assert_eq!(peel_to_result(&int()), None);
        // Pinned: peels through Effect[_].
        assert_eq!(
            peel_to_result(&Ty::Effect(Box::new(r))),
            Some((int(), string()))
        );
    }

    #[test]
    fn peel_to_option_matches_and_misses() {
        assert_eq!(peel_to_option(&Ty::Option(Box::new(int()))), Some(int()));
        assert_eq!(peel_to_option(&int()), None);
    }

    #[test]
    fn peel_to_list_matches_and_misses() {
        assert_eq!(peel_to_list(&Ty::List(Box::new(string()))), Some(string()));
        assert_eq!(peel_to_list(&int()), None);
    }

    #[test]
    fn peel_to_map_matches_and_misses() {
        let m = Ty::Map(Box::new(string()), Box::new(int()));
        assert_eq!(peel_to_map(&m), Some((string(), int())));
        assert_eq!(peel_to_map(&int()), None);
    }

    #[test]
    fn peel_to_http_result_matches_and_misses() {
        assert_eq!(
            peel_to_http_result(&Ty::HttpResult(Box::new(int()))),
            Some(int()),
        );
        assert_eq!(peel_to_http_result(&int()), None);
    }

    // -- maybe_auto_lift ---------------------------------------------------

    #[test]
    fn maybe_auto_lift_lifts_into_expected_effect() {
        // T lifts to Effect[T] when expected is Effect[T] and T is not effectful.
        let expected = Ty::Effect(Box::new(int()));
        let lifted = maybe_auto_lift(Some(int()), Some(&expected));
        assert_eq!(lifted, Some(Ty::Effect(Box::new(int()))));
    }

    #[test]
    fn maybe_auto_lift_leaves_non_matching_alone() {
        // Already Effect[_]: untouched.
        let expected = Ty::Effect(Box::new(int()));
        assert_eq!(
            maybe_auto_lift(Some(Ty::Effect(Box::new(int()))), Some(&expected)),
            Some(Ty::Effect(Box::new(int()))),
        );
        // Expected not an Effect: untouched.
        assert_eq!(maybe_auto_lift(Some(int()), Some(&int())), Some(int()));
        // None type: untouched.
        assert_eq!(maybe_auto_lift(None, Some(&expected)), None);
    }

    // -- const_literal -----------------------------------------------------

    #[test]
    fn const_literal_extracts_literals() {
        assert!(matches!(
            const_literal(&expr(ExprKind::IntLit(7))),
            Some(ConstLit::Int(7)),
        ));
        assert!(matches!(
            const_literal(&expr(ExprKind::BoolLit(true))),
            Some(ConstLit::Bool(true)),
        ));
        assert!(matches!(
            const_literal(&expr(ExprKind::StrLit("hi".into()))),
            Some(ConstLit::Str(s)) if s == "hi",
        ));
        assert!(matches!(
            const_literal(&expr(ExprKind::FloatLit {
                value: 1.5,
                lexeme: "1.5".into(),
            })),
            Some(ConstLit::Float(_)),
        ));
        // Unary-neg on an int literal folds.
        let neg = expr(ExprKind::UnaryOp(
            UnaryOp::Neg,
            Box::new(expr(ExprKind::IntLit(3))),
        ));
        assert!(matches!(const_literal(&neg), Some(ConstLit::Int(-3))));
    }

    #[test]
    fn const_literal_rejects_non_literals() {
        assert!(const_literal(&expr(ExprKind::Ident(ident("x")))).is_none());
    }

    // -- eval_predicate ----------------------------------------------------

    #[test]
    fn eval_predicate_int_and_float() {
        assert!(eval_predicate(&PredKind::NonNegative, &ConstLit::Int(0)));
        assert!(!eval_predicate(&PredKind::NonNegative, &ConstLit::Int(-1)));
        assert!(eval_predicate(&PredKind::Positive, &ConstLit::Int(1)));
        assert!(!eval_predicate(&PredKind::Positive, &ConstLit::Int(0)));
        assert!(eval_predicate(&PredKind::InRange(1, 10), &ConstLit::Int(5),));
        assert!(!eval_predicate(
            &PredKind::InRange(1, 10),
            &ConstLit::Int(11),
        ));
    }

    #[test]
    fn eval_predicate_string() {
        assert!(eval_predicate(
            &PredKind::MinLength(2),
            &ConstLit::Str("ab".into()),
        ));
        assert!(!eval_predicate(
            &PredKind::MinLength(3),
            &ConstLit::Str("ab".into()),
        ));
        assert!(eval_predicate(
            &PredKind::NonEmpty,
            &ConstLit::Str("x".into()),
        ));
        assert!(!eval_predicate(
            &PredKind::NonEmpty,
            &ConstLit::Str(String::new()),
        ));
        assert!(eval_predicate(
            &PredKind::Matches("[a-z]+".into()),
            &ConstLit::Str("abc".into()),
        ));
        assert!(!eval_predicate(
            &PredKind::Matches("[a-z]+".into()),
            &ConstLit::Str("ABC".into()),
        ));
    }

    #[test]
    fn eval_predicate_base_mismatch_is_vacuously_true() {
        // SURPRISING (pinned as-is): a predicate/literal base mismatch returns
        // `true` — base/predicate mismatch is a declaration-time error reported
        // elsewhere, not by construction-time eval.
        assert!(eval_predicate(&PredKind::MinLength(5), &ConstLit::Int(0),));
    }

    // -- literal_matches_base ----------------------------------------------

    #[test]
    fn literal_matches_base_pairs() {
        assert!(literal_matches_base(&ConstLit::Int(1), BaseType::Int));
        assert!(literal_matches_base(
            &ConstLit::Str("x".into()),
            BaseType::String,
        ));
        assert!(!literal_matches_base(&ConstLit::Int(1), BaseType::String));
        assert!(!literal_matches_base(&ConstLit::Unit, BaseType::Int));
    }

    // -- type_decl_base / type_decl_refinement -----------------------------

    #[test]
    fn type_decl_base_refined_vs_record() {
        let refined = refined_decl("Age", BaseType::Int, None);
        assert_eq!(type_decl_base(&refined), Some(BaseType::Int));
        assert_eq!(type_decl_base(&record_decl("Pt")), None);
    }

    #[test]
    fn type_decl_refinement_present_vs_absent() {
        let with = refined_decl(
            "Age",
            BaseType::Int,
            Some(refinement(vec![PredKind::Positive])),
        );
        assert!(type_decl_refinement(&with).is_some());
        let without = refined_decl("Raw", BaseType::Int, None);
        assert!(type_decl_refinement(&without).is_none());
        assert!(type_decl_refinement(&record_decl("Pt")).is_none());
    }

    // -- check_*_refinement_consistency ------------------------------------

    #[test]
    fn int_refinement_consistency() {
        // Consistent: 1..=10 with Positive — no error.
        let mut errs = vec![];
        check_int_refinement_consistency(
            &refinement(vec![PredKind::Positive, PredKind::InRange(1, 10)]),
            &mut errs,
        );
        assert!(errs.is_empty());
        // Inconsistent: InRange(10, 1) is empty → exactly one error.
        let mut errs = vec![];
        check_int_refinement_consistency(&refinement(vec![PredKind::InRange(10, 1)]), &mut errs);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].category, "karn.types.empty_refinement");
    }

    #[test]
    fn float_refinement_consistency() {
        // Consistent range.
        let mut errs = vec![];
        check_float_refinement_consistency(
            &refinement(vec![PredKind::InRangeF(fbound(0.0), fbound(1.0))]),
            &mut errs,
        );
        assert!(errs.is_empty());
        // Empty: 5.0..=1.0 → one error.
        let mut errs = vec![];
        check_float_refinement_consistency(
            &refinement(vec![PredKind::InRangeF(fbound(5.0), fbound(1.0))]),
            &mut errs,
        );
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].category, "karn.types.empty_refinement");
        // Degenerate-but-exclusive: Positive with InRangeF(0.0, 0.0) → lo==hi
        // and lo_exclusive → one error.
        let mut errs = vec![];
        check_float_refinement_consistency(
            &refinement(vec![
                PredKind::Positive,
                PredKind::InRangeF(fbound(0.0), fbound(0.0)),
            ]),
            &mut errs,
        );
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn string_refinement_consistency() {
        // Consistent: MinLength(1), MaxLength(10).
        let mut errs = vec![];
        check_string_refinement_consistency(
            &refinement(vec![PredKind::MinLength(1), PredKind::MaxLength(10)]),
            &mut errs,
        );
        assert!(errs.is_empty());
        // min > max → one error.
        let mut errs = vec![];
        check_string_refinement_consistency(
            &refinement(vec![PredKind::MinLength(10), PredKind::MaxLength(2)]),
            &mut errs,
        );
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].category, "karn.types.empty_refinement");
        // Conflicting exact lengths → TWO errors (pinned as-is): the explicit
        // `Length(3)`/`Length(5)` conflict push, *plus* the subsequent
        // min_len(5) > max_len(3) empty-range push (each `Length` clamps both
        // bounds to itself).
        let mut errs = vec![];
        check_string_refinement_consistency(
            &refinement(vec![PredKind::Length(3), PredKind::Length(5)]),
            &mut errs,
        );
        assert_eq!(errs.len(), 2);
        assert!(
            errs.iter()
                .all(|e| e.category == "karn.types.empty_refinement")
        );
    }

    // -- numeric_mix -------------------------------------------------------

    #[test]
    fn numeric_mix_int_float_pairs() {
        assert!(numeric_mix(Some(BaseType::Int), Some(BaseType::Float)));
        assert!(numeric_mix(Some(BaseType::Float), Some(BaseType::Int)));
        assert!(!numeric_mix(Some(BaseType::Int), Some(BaseType::Int)));
        assert!(!numeric_mix(Some(BaseType::Float), Some(BaseType::Float)));
        assert!(!numeric_mix(None, Some(BaseType::Int)));
    }
}
