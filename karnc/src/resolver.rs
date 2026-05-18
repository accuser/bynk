//! Name resolution (spec §5.1, v0.1 §4.1, v0.2 §4.1).
//!
//! Builds symbol tables for the commons and validates that:
//! - No two top-level items share a name (types, fns, methods are all named).
//! - Every `TypeRef::Named` resolves to a declared type.
//! - Every free function call resolves to a function declaration.
//! - Every identifier in expression position resolves to a parameter, a
//!   `let` binding, or `self` (inside a method).
//! - Constructor / static calls (`TypeName.method(args)`) resolve either to
//!   the built-in `T.of` of a refined type, a static method on `T`, or a
//!   variant constructor when `T` is a sum type.
//! - Record construction targets a declared record type and uses only
//!   declared fields.
//! - Method calls resolve via the receiver's nominal type (the actual type
//!   check happens in the type checker).
//!
//! On success returns a [`ResolvedCommons`] — the original AST plus
//! symbol tables the type checker consumes.

use std::collections::HashMap;

use crate::ast::*;
use crate::error::CompileError;

/// Per-type method table built during resolution: keyed by method name,
/// values are clones of the [`FnDecl`] for that method.
#[derive(Debug, Default, Clone)]
pub struct MethodTable {
    pub instance: HashMap<String, FnDecl>,
    pub statics: HashMap<String, FnDecl>,
}

/// Output of resolution: the AST plus the symbol tables the checker needs.
pub struct ResolvedCommons {
    pub commons: Commons,
    pub types: HashMap<String, TypeDecl>,
    pub fns: HashMap<String, FnDecl>,
    /// Per-type method tables (instance + static).
    pub methods: HashMap<String, MethodTable>,
}

/// Resolve names in a commons. Accumulates all errors before returning so
/// the user sees as much feedback as possible per compile.
pub fn resolve(commons: Commons) -> Result<ResolvedCommons, Vec<CompileError>> {
    let mut errors = Vec::new();
    let mut types: HashMap<String, TypeDecl> = HashMap::new();
    let mut fns: HashMap<String, FnDecl> = HashMap::new();
    let mut methods: HashMap<String, MethodTable> = HashMap::new();

    // First pass: collect declarations and detect duplicates / name overlap.
    for item in &commons.items {
        match item {
            CommonsItem::Type(t) => {
                if let Some(prev) = types.get(&t.name.name) {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.duplicate_type",
                            t.name.span,
                            format!("type `{}` is already declared", t.name.name),
                        )
                        .with_label(prev.name.span, "previously declared here"),
                    );
                } else if let Some(prev) = fns.get(&t.name.name) {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.name_conflict",
                            t.name.span,
                            format!(
                                "type `{}` conflicts with a function of the same name",
                                t.name.name
                            ),
                        )
                        .with_label(prev.name.ident().span, "function declared here"),
                    );
                } else {
                    types.insert(t.name.name.clone(), t.clone());
                    methods.insert(t.name.name.clone(), MethodTable::default());
                }
            }
            CommonsItem::Fn(f) => match &f.name {
                FnName::Free(id) => {
                    if let Some(prev) = fns.get(&id.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_fn",
                                id.span,
                                format!("function `{}` is already declared", id.name),
                            )
                            .with_label(prev.name.ident().span, "previously declared here"),
                        );
                    } else if let Some(prev) = types.get(&id.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.name_conflict",
                                id.span,
                                format!(
                                    "function `{}` conflicts with a type of the same name",
                                    id.name
                                ),
                            )
                            .with_label(prev.name.span, "type declared here"),
                        );
                    } else {
                        fns.insert(id.name.clone(), f.clone());
                    }
                }
                FnName::Method {
                    type_name,
                    method_name,
                } => {
                    // The type the method is attached to must be declared.
                    if !types.contains_key(&type_name.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.method_unknown_type",
                                type_name.span,
                                format!(
                                    "method `{}.{}` attached to an unknown type `{}`",
                                    type_name.name, method_name.name, type_name.name
                                ),
                            )
                            .with_note(
                                "methods can only be declared on types defined in the same commons",
                            ),
                        );
                        continue;
                    }
                    let table = methods.entry(type_name.name.clone()).or_default();
                    let bucket = if f.has_self {
                        &mut table.instance
                    } else {
                        &mut table.statics
                    };
                    if let Some(prev) = bucket.get(&method_name.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_method",
                                method_name.span,
                                format!(
                                    "method `{}.{}` is already declared",
                                    type_name.name, method_name.name
                                ),
                            )
                            .with_label(prev.name.ident().span, "previously declared here"),
                        );
                    } else {
                        bucket.insert(method_name.name.clone(), f.clone());
                    }
                }
            },
        }
    }

    // Second pass: validate references inside type-refs and function bodies.
    for item in &commons.items {
        match item {
            CommonsItem::Type(t) => {
                check_type_decl_refs(t, &types, &mut errors);
            }
            CommonsItem::Fn(f) => {
                check_fn_refs(f, &types, &fns, &methods, &mut errors);
            }
        }
    }

    if errors.is_empty() {
        Ok(ResolvedCommons {
            commons,
            types,
            fns,
            methods,
        })
    } else {
        Err(errors)
    }
}

/// Recursively walk a type declaration to check that every type reference
/// inside it resolves.
fn check_type_decl_refs(
    t: &TypeDecl,
    types: &HashMap<String, TypeDecl>,
    errors: &mut Vec<CompileError>,
) {
    match &t.body {
        TypeBody::Refined { .. } => {
            // Refined-type bodies only reference base types directly.
        }
        TypeBody::Record(r) => {
            let mut seen = HashMap::new();
            for f in &r.fields {
                if let Some(prev_span) = seen.get(&f.name.name) {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.duplicate_field",
                            f.name.span,
                            format!("field `{}` is declared more than once", f.name.name),
                        )
                        .with_label(*prev_span, "previously declared here"),
                    );
                } else {
                    seen.insert(f.name.name.clone(), f.name.span);
                }
                // Detect direct self-reference: `type A = { f: A }`.
                // v0.2 has no indirection (no `Option[A]`, no records-of-records
                // wrapping the same type) for this check to defeat; a direct
                // `Named(A)` field where A is the enclosing type is forbidden.
                if let TypeRef::Named(id) = &f.type_ref
                    && id.name == t.name.name
                {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.recursive_record_field",
                            f.name.span,
                            format!(
                                "record `{}` cannot directly contain a field of its own type",
                                t.name.name
                            ),
                        )
                        .with_label(t.name.span, "type declared here")
                        .with_note(
                            "wrap the recursive reference in `Option[...]` to break the cycle",
                        ),
                    );
                }
                check_type_ref_resolves(&f.type_ref, types, errors);
            }
        }
        TypeBody::Sum(s) => {
            let mut seen = HashMap::new();
            for v in &s.variants {
                if let Some(prev_span) = seen.get(&v.name.name) {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.duplicate_variant",
                            v.name.span,
                            format!("variant `{}` is declared more than once", v.name.name),
                        )
                        .with_label(*prev_span, "previously declared here"),
                    );
                } else {
                    seen.insert(v.name.name.clone(), v.name.span);
                }
                let mut payload_seen = HashMap::new();
                for f in &v.payload {
                    if let Some(prev) = payload_seen.get(&f.name.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_field",
                                f.name.span,
                                format!(
                                    "payload field `{}` is declared more than once in variant `{}`",
                                    f.name.name, v.name.name
                                ),
                            )
                            .with_label(*prev, "previously declared here"),
                        );
                    } else {
                        payload_seen.insert(f.name.name.clone(), f.name.span);
                    }
                    check_type_ref_resolves(&f.type_ref, types, errors);
                }
            }
        }
    }
}

fn check_fn_refs(
    f: &FnDecl,
    types: &HashMap<String, TypeDecl>,
    fns: &HashMap<String, FnDecl>,
    methods: &HashMap<String, MethodTable>,
    errors: &mut Vec<CompileError>,
) {
    // Parameter types resolve.
    let mut seen_params: HashMap<&str, &Ident> = HashMap::new();
    for p in &f.params {
        check_type_ref_resolves(&p.type_ref, types, errors);
        if let Some(prev) = seen_params.get(p.name.name.as_str()) {
            errors.push(
                CompileError::new(
                    "karn.resolve.duplicate_param",
                    p.name.span,
                    format!("parameter `{}` is declared more than once", p.name.name),
                )
                .with_label(prev.span, "previously declared here"),
            );
        } else {
            seen_params.insert(p.name.name.as_str(), &p.name);
        }
    }
    check_type_ref_resolves(&f.return_type, types, errors);

    // Build the initial scope: parameters plus `self` (for instance methods).
    let mut params: HashMap<String, ()> =
        f.params.iter().map(|p| (p.name.name.clone(), ())).collect();
    if f.has_self {
        params.insert("self".to_string(), ());
    }
    let in_method = matches!(f.name, FnName::Method { .. });
    let mut scopes: Vec<HashMap<String, ()>> = Vec::new();
    check_block_references(
        &f.body,
        &params,
        in_method,
        &mut scopes,
        types,
        fns,
        methods,
        errors,
    );
}

fn unknown_type_error(id: &Ident) -> CompileError {
    CompileError::new(
        "karn.resolve.unknown_type",
        id.span,
        format!("unknown type `{}`", id.name),
    )
    .with_note(
        "only base types (Int, String, Bool), types declared in this commons, \
         `Result[T, E]`, `Option[T]`, and `ValidationError` are in scope",
    )
}

/// Recursively check that every type reference resolves.
fn check_type_ref_resolves(
    r: &TypeRef,
    types: &HashMap<String, TypeDecl>,
    errors: &mut Vec<CompileError>,
) {
    match r {
        TypeRef::Base(_, _) => {}
        TypeRef::Named(id) => {
            if !types.contains_key(&id.name) {
                errors.push(unknown_type_error(id));
            }
        }
        TypeRef::Result(t, e, _) => {
            check_type_ref_resolves(t, types, errors);
            check_type_ref_resolves(e, types, errors);
        }
        TypeRef::Option(t, _) => {
            check_type_ref_resolves(t, types, errors);
        }
        TypeRef::ValidationError(_) => {}
    }
}

/// Lookup a name across scopes. Returns true if it's bound somewhere
/// (param, self, or any let-scope).
fn name_in_scope(name: &str, params: &HashMap<String, ()>, scopes: &[HashMap<String, ()>]) -> bool {
    if params.contains_key(name) {
        return true;
    }
    scopes.iter().rev().any(|s| s.contains_key(name))
}

#[allow(clippy::too_many_arguments)]
fn check_block_references(
    block: &Block,
    params: &HashMap<String, ()>,
    in_method: bool,
    scopes: &mut Vec<HashMap<String, ()>>,
    types: &HashMap<String, TypeDecl>,
    fns: &HashMap<String, FnDecl>,
    methods: &HashMap<String, MethodTable>,
    errors: &mut Vec<CompileError>,
) {
    scopes.push(HashMap::new());
    for stmt in &block.statements {
        match stmt {
            Statement::Let(l) => {
                check_expr_references(
                    &l.value, params, in_method, scopes, types, fns, methods, errors,
                );
                if let Some(annot) = &l.type_annot {
                    check_type_ref_resolves(annot, types, errors);
                }
                if let Some(prev) = types.get(&l.name.name) {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.let_shadows_type",
                            l.name.span,
                            format!(
                                "`let {}` shadows the declared type `{}`",
                                l.name.name, l.name.name
                            ),
                        )
                        .with_label(prev.name.span, "type declared here")
                        .with_note("choose a different name for the let binding"),
                    );
                } else if let Some(prev) = fns.get(&l.name.name) {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.let_shadows_fn",
                            l.name.span,
                            format!(
                                "`let {}` shadows the declared function `{}`",
                                l.name.name, l.name.name
                            ),
                        )
                        .with_label(prev.name.ident().span, "function declared here")
                        .with_note("choose a different name for the let binding"),
                    );
                } else {
                    scopes.last_mut().unwrap().insert(l.name.name.clone(), ());
                }
            }
        }
    }
    check_expr_references(
        &block.tail,
        params,
        in_method,
        scopes,
        types,
        fns,
        methods,
        errors,
    );
    scopes.pop();
}

#[allow(clippy::too_many_arguments)]
fn check_expr_references(
    expr: &Expr,
    params: &HashMap<String, ()>,
    in_method: bool,
    scopes: &mut Vec<HashMap<String, ()>>,
    types: &HashMap<String, TypeDecl>,
    fns: &HashMap<String, FnDecl>,
    methods: &HashMap<String, MethodTable>,
    errors: &mut Vec<CompileError>,
) {
    match &expr.kind {
        ExprKind::IntLit(_) | ExprKind::StrLit(_) | ExprKind::BoolLit(_) | ExprKind::None => {}
        ExprKind::Ident(id) => {
            if id.name == "self" {
                if !in_method {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.self_outside_method",
                            id.span,
                            "`self` can only be used inside a method body",
                        )
                        .with_note(
                            "declare the function as `fn TypeName.method(self, ...)` if you intended a method",
                        ),
                    );
                }
                return;
            }
            if name_in_scope(&id.name, params, scopes) {
                // OK.
            } else if let Some(sum_owner) = find_unique_variant_owner(&id.name, types) {
                // It's a bare variant reference. We treat it as a valid
                // expression in resolver — the type checker will assign
                // the correct sum type. Mark with no error.
                let _ = sum_owner;
            } else if types.contains_key(&id.name) {
                errors.push(
                    CompileError::new(
                        "karn.resolve.type_in_expr",
                        id.span,
                        format!("`{}` is a type, not a value", id.name),
                    )
                    .with_note(
                        "types cannot appear in expression position; \
                         use `TypeName.of(value)` or `TypeName { ... }` to construct values",
                    ),
                );
            } else if fns.contains_key(&id.name) {
                errors.push(
                    CompileError::new(
                        "karn.resolve.fn_without_call",
                        id.span,
                        format!(
                            "`{}` is a function and must be called — first-class functions are not in v0.2",
                            id.name
                        ),
                    )
                    .with_note("add an argument list, e.g. `f(x)`"),
                );
            } else if find_ambiguous_variant_owners(&id.name, types).len() > 1 {
                errors.push(
                    CompileError::new(
                        "karn.resolve.ambiguous_variant",
                        id.span,
                        format!(
                            "the variant name `{}` is declared on multiple sum types — qualify it as `TypeName.{}`",
                            id.name, id.name
                        ),
                    ),
                );
            } else {
                errors.push(
                    CompileError::new(
                        "karn.resolve.unknown_name",
                        id.span,
                        format!("unknown name `{}`", id.name),
                    )
                    .with_note(
                        "only parameters, `let` bindings, and functions declared \
                         in this commons are in scope",
                    ),
                );
            }
        }
        ExprKind::Call(name, args) => {
            match fns.get(&name.name) {
                Some(decl) => {
                    if decl.params.len() != args.len() {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.arity_mismatch",
                                name.span,
                                format!(
                                    "function `{}` expects {} argument(s), but {} were given",
                                    name.name,
                                    decl.params.len(),
                                    args.len()
                                ),
                            )
                            .with_label(decl.name.ident().span, "function declared here"),
                        );
                    }
                }
                None => {
                    // Maybe it's a variant constructor with a payload (e.g., `Placed(at, total)`).
                    let owners = find_ambiguous_variant_owners(&name.name, types);
                    if owners.len() == 1 {
                        // Single owner — treat as variant construction. Type
                        // checker validates arg count and types.
                    } else if owners.len() > 1 {
                        errors.push(CompileError::new(
                            "karn.resolve.ambiguous_variant",
                            name.span,
                            format!(
                                "the variant name `{}` is declared on multiple sum types — qualify it as `TypeName.{}(...)`",
                                name.name, name.name
                            ),
                        ));
                    } else if types.contains_key(&name.name) {
                        errors.push(CompileError::new(
                            "karn.resolve.type_as_function",
                            name.span,
                            format!(
                                "`{}` is a type, not a function — use `{}.of(value)` or `{} {{ ... }}` instead",
                                name.name, name.name, name.name
                            ),
                        ));
                    } else if name_in_scope(&name.name, params, scopes) {
                        errors.push(CompileError::new(
                            "karn.resolve.param_as_function",
                            name.span,
                            format!("`{}` is a value, not a function", name.name),
                        ));
                    } else {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.unknown_function",
                                name.span,
                                format!("unknown function `{}`", name.name),
                            )
                            .with_note("only functions declared in this commons are callable"),
                        );
                    }
                }
            }
            for a in args {
                check_expr_references(a, params, in_method, scopes, types, fns, methods, errors);
            }
        }
        ExprKind::BinOp(_, lhs, rhs) => {
            check_expr_references(lhs, params, in_method, scopes, types, fns, methods, errors);
            check_expr_references(rhs, params, in_method, scopes, types, fns, methods, errors);
        }
        ExprKind::UnaryOp(_, e) => {
            check_expr_references(e, params, in_method, scopes, types, fns, methods, errors)
        }
        ExprKind::Paren(e) => {
            check_expr_references(e, params, in_method, scopes, types, fns, methods, errors)
        }
        ExprKind::Block(b) => {
            check_block_references(b, params, in_method, scopes, types, fns, methods, errors)
        }
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            check_expr_references(cond, params, in_method, scopes, types, fns, methods, errors);
            // `is`-pattern bindings inside the condition flow into the
            // then-branch's scope (v0.2 §3.9).
            let mut then_extra: HashMap<String, ()> = HashMap::new();
            collect_is_binding_names(cond, &mut then_extra);
            scopes.push(then_extra);
            check_block_references(
                then_block, params, in_method, scopes, types, fns, methods, errors,
            );
            scopes.pop();
            check_block_references(
                else_block, params, in_method, scopes, types, fns, methods, errors,
            );
        }
        ExprKind::Ok(inner) | ExprKind::Err(inner) | ExprKind::Question(inner) => {
            check_expr_references(
                inner, params, in_method, scopes, types, fns, methods, errors,
            );
        }
        ExprKind::Some(inner) => {
            check_expr_references(
                inner, params, in_method, scopes, types, fns, methods, errors,
            );
        }
        ExprKind::ConstructorCall {
            type_name,
            method,
            args,
        } => {
            // The expression `T.name(args)` may be:
            //   - a static method call (or refined-type `of`),
            //   - a qualified variant constructor on a sum.
            // The resolver only needs to ensure that *something* matches.
            if let Some(decl) = types.get(&type_name.name) {
                let table = methods.get(&type_name.name).cloned().unwrap_or_default();
                let is_static_method = table.statics.contains_key(&method.name);
                let is_of_constructor =
                    method.name == "of" && matches!(decl.body, TypeBody::Refined { .. });
                let is_variant = match &decl.body {
                    TypeBody::Sum(s) => s.variants.iter().any(|v| v.name.name == method.name),
                    _ => false,
                };
                if !(is_static_method || is_of_constructor || is_variant) {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.unknown_static_member",
                            method.span,
                            format!(
                                "type `{}` has no static method or variant named `{}`",
                                type_name.name, method.name
                            ),
                        )
                        .with_label(decl.name.span, "type declared here"),
                    );
                }
            } else {
                errors.push(unknown_type_error(type_name));
            }
            for a in args {
                check_expr_references(a, params, in_method, scopes, types, fns, methods, errors);
            }
        }
        ExprKind::RecordConstruction { type_name, fields } => {
            match types.get(&type_name.name) {
                Some(decl) => match &decl.body {
                    TypeBody::Record(r) => {
                        let declared: HashMap<&str, &RecordField> =
                            r.fields.iter().map(|f| (f.name.name.as_str(), f)).collect();
                        let mut provided: HashMap<&str, &Ident> = HashMap::new();
                        for f in fields {
                            if !declared.contains_key(f.name.name.as_str()) {
                                errors.push(
                                    CompileError::new(
                                        "karn.resolve.unknown_field",
                                        f.name.span,
                                        format!(
                                            "record type `{}` has no field `{}`",
                                            type_name.name, f.name.name
                                        ),
                                    )
                                    .with_label(decl.name.span, "type declared here"),
                                );
                            }
                            if let Some(prev) = provided.get(f.name.name.as_str()) {
                                errors.push(
                                    CompileError::new(
                                        "karn.resolve.duplicate_field_init",
                                        f.name.span,
                                        format!(
                                            "field `{}` is initialised more than once",
                                            f.name.name
                                        ),
                                    )
                                    .with_label(prev.span, "previously initialised here"),
                                );
                            } else {
                                provided.insert(f.name.name.as_str(), &f.name);
                            }
                            // Shorthand `name` — must be in scope.
                            match &f.value {
                                Some(v) => check_expr_references(
                                    v, params, in_method, scopes, types, fns, methods, errors,
                                ),
                                None => {
                                    if !name_in_scope(&f.name.name, params, scopes) {
                                        errors.push(
                                            CompileError::new(
                                                "karn.resolve.unknown_name",
                                                f.name.span,
                                                format!(
                                                    "shorthand field initialiser `{}` requires a binding of that name in scope",
                                                    f.name.name
                                                ),
                                            )
                                            .with_note(
                                                "either bring `{name}` into scope or use the full `field: value` form",
                                            ),
                                        );
                                    }
                                }
                            }
                        }
                        for decl_field in &r.fields {
                            if !provided.contains_key(decl_field.name.name.as_str()) {
                                errors.push(
                                    CompileError::new(
                                        "karn.resolve.missing_field",
                                        type_name.span,
                                        format!(
                                            "missing required field `{}` for record `{}`",
                                            decl_field.name.name, type_name.name
                                        ),
                                    )
                                    .with_label(decl_field.name.span, "field declared here"),
                                );
                            }
                        }
                    }
                    _ => {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.not_a_record_type",
                                type_name.span,
                                format!(
                                    "`{}` is not a record type — only record types can be constructed with `{{ ... }}`",
                                    type_name.name
                                ),
                            )
                            .with_label(decl.name.span, "type declared here"),
                        );
                    }
                },
                None => errors.push(unknown_type_error(type_name)),
            }
        }
        ExprKind::FieldAccess { receiver, field } => {
            // `TypeName.Variant` — qualified nullary variant reference.
            if let ExprKind::Ident(id) = &receiver.kind
                && !name_in_scope(&id.name, params, scopes)
                && let Some(decl) = types.get(&id.name)
            {
                let known_variant = match &decl.body {
                    TypeBody::Sum(s) => s.variants.iter().any(|v| v.name.name == field.name),
                    _ => false,
                };
                if !known_variant {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.unknown_static_member",
                            field.span,
                            format!(
                                "type `{}` has no static method or variant named `{}`",
                                id.name, field.name
                            ),
                        )
                        .with_label(decl.name.span, "type declared here"),
                    );
                }
            } else {
                check_expr_references(
                    receiver, params, in_method, scopes, types, fns, methods, errors,
                );
            }
        }
        ExprKind::MethodCall {
            receiver,
            method,
            args,
        } => {
            // If the receiver is a bare ident of a declared type (and not a
            // local binding), this is a static call: `T.method(args)`.
            // Validate the type/method/variant resolution here, mirroring
            // ConstructorCall's resolver path. Otherwise recurse into the
            // receiver as a value expression.
            if let ExprKind::Ident(id) = &receiver.kind
                && !name_in_scope(&id.name, params, scopes)
                && let Some(decl) = types.get(&id.name)
            {
                let table = methods.get(&id.name).cloned().unwrap_or_default();
                let is_static_method = table.statics.contains_key(&method.name);
                let is_of_constructor =
                    method.name == "of" && matches!(decl.body, TypeBody::Refined { .. });
                let is_variant = match &decl.body {
                    TypeBody::Sum(s) => s.variants.iter().any(|v| v.name.name == method.name),
                    _ => false,
                };
                if !(is_static_method || is_of_constructor || is_variant) {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.unknown_static_member",
                            method.span,
                            format!(
                                "type `{}` has no static method or variant named `{}`",
                                id.name, method.name
                            ),
                        )
                        .with_label(decl.name.span, "type declared here"),
                    );
                }
            } else {
                check_expr_references(
                    receiver, params, in_method, scopes, types, fns, methods, errors,
                );
            }
            for a in args {
                check_expr_references(a, params, in_method, scopes, types, fns, methods, errors);
            }
        }
        ExprKind::Match { discriminant, arms } => {
            check_expr_references(
                discriminant,
                params,
                in_method,
                scopes,
                types,
                fns,
                methods,
                errors,
            );
            for arm in arms {
                // Pattern bindings introduce names in the arm body. The
                // type checker validates the pattern against the discriminant
                // type. Resolver pushes a scope with those binding names so
                // body references resolve.
                let mut arm_scope = HashMap::new();
                collect_pattern_bindings(&arm.pattern, &mut arm_scope);
                scopes.push(arm_scope);
                match &arm.body {
                    MatchBody::Expr(e) => check_expr_references(
                        e, params, in_method, scopes, types, fns, methods, errors,
                    ),
                    MatchBody::Block(b) => check_block_references(
                        b, params, in_method, scopes, types, fns, methods, errors,
                    ),
                }
                scopes.pop();
            }
        }
        ExprKind::Is { value, pattern } => {
            check_expr_references(
                value, params, in_method, scopes, types, fns, methods, errors,
            );
            // `is` pattern bindings flow through to the truthy branch of
            // an enclosing context; binding scope is handled by the type
            // checker. Resolver doesn't introduce anything here.
            let _ = pattern;
        }
    }
}

/// Walk an expression collecting names introduced by `is` patterns inside
/// it, when applied as a Boolean test. Mirrors the binding-flow rule from
/// v0.2 §3.9 — bindings from `expr is Pat`, `lhs && (expr is Pat)`, or
/// `(expr is Pat)` flow into the surrounding truthy branch.
fn collect_is_binding_names(expr: &Expr, into: &mut HashMap<String, ()>) {
    match &expr.kind {
        ExprKind::Is {
            pattern: Pattern::Variant { bindings, .. },
            ..
        } => {
            for b in bindings {
                if !b.is_wildcard() {
                    into.insert(b.local_name().name.clone(), ());
                }
            }
        }
        ExprKind::BinOp(BinOp::And, l, r) => {
            collect_is_binding_names(l, into);
            collect_is_binding_names(r, into);
        }
        ExprKind::Paren(inner) => collect_is_binding_names(inner, into),
        _ => {}
    }
}

/// Walk a pattern collecting the names it would bind.
fn collect_pattern_bindings(pattern: &Pattern, into: &mut HashMap<String, ()>) {
    match pattern {
        Pattern::Wildcard(_) => {}
        Pattern::Variant { bindings, .. } => {
            for b in bindings {
                if !b.is_wildcard() {
                    into.insert(b.local_name().name.clone(), ());
                }
            }
        }
    }
}

/// Find the unique sum type that owns a given variant name. Returns None
/// if no type owns it; ignores cases of multiple owners (those are
/// reported via `find_ambiguous_variant_owners`).
fn find_unique_variant_owner<'a>(
    name: &str,
    types: &'a HashMap<String, TypeDecl>,
) -> Option<&'a TypeDecl> {
    let owners = find_ambiguous_variant_owners(name, types);
    if owners.len() == 1 {
        Some(owners[0])
    } else {
        None
    }
}

fn find_ambiguous_variant_owners<'a>(
    name: &str,
    types: &'a HashMap<String, TypeDecl>,
) -> Vec<&'a TypeDecl> {
    let mut out = Vec::new();
    for t in types.values() {
        if let TypeBody::Sum(s) = &t.body
            && s.variants.iter().any(|v| v.name.name == name)
        {
            out.push(t);
        }
    }
    out
}
