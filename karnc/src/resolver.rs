//! Name resolution (spec §5.1, v0.1 §4.1).
//!
//! Builds a symbol table for the commons and validates that:
//! - No two top-level items share a name.
//! - Every `TypeRef::Named` resolves to a type declaration.
//! - Every function call resolves to a function declaration (with the right arity).
//! - Every identifier in expression position resolves to a parameter or a
//!   `let` binding in scope.
//! - In v0.1, `let` block-scopes are managed; `let` bindings cannot collide
//!   with type or function names.
//! - Constructor calls (`TypeName.of(args)`) resolve to a declared refined
//!   type with the recognised `of` method.
//!
//! On success returns a [`ResolvedCommons`] — the original AST plus a symbol
//! table the type checker consumes.

use std::collections::HashMap;

use crate::ast::*;
use crate::error::CompileError;

/// Output of resolution: the AST plus the symbol tables the checker needs.
pub struct ResolvedCommons {
    pub commons: Commons,
    pub types: HashMap<String, TypeDecl>,
    pub fns: HashMap<String, FnDecl>,
}

/// Resolve names in a commons. Accumulates all errors before returning so
/// the user sees as much feedback as possible per compile.
pub fn resolve(commons: Commons) -> Result<ResolvedCommons, Vec<CompileError>> {
    let mut errors = Vec::new();
    let mut types: HashMap<String, TypeDecl> = HashMap::new();
    let mut fns: HashMap<String, FnDecl> = HashMap::new();

    // First pass: collect declarations and detect duplicates / name overlap.
    for item in &commons.items {
        let name = item.name();
        match item {
            CommonsItem::Type(t) => {
                if let Some(prev) = types.get(&name.name) {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.duplicate_type",
                            name.span,
                            format!("type `{}` is already declared", name.name),
                        )
                        .with_label(prev.name.span, "previously declared here"),
                    );
                } else if let Some(prev) = fns.get(&name.name) {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.name_conflict",
                            name.span,
                            format!(
                                "type `{}` conflicts with a function of the same name",
                                name.name
                            ),
                        )
                        .with_label(prev.name.span, "function declared here"),
                    );
                } else {
                    types.insert(name.name.clone(), t.clone());
                }
            }
            CommonsItem::Fn(f) => {
                if let Some(prev) = fns.get(&name.name) {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.duplicate_fn",
                            name.span,
                            format!("function `{}` is already declared", name.name),
                        )
                        .with_label(prev.name.span, "previously declared here"),
                    );
                } else if let Some(prev) = types.get(&name.name) {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.name_conflict",
                            name.span,
                            format!(
                                "function `{}` conflicts with a type of the same name",
                                name.name
                            ),
                        )
                        .with_label(prev.name.span, "type declared here"),
                    );
                } else {
                    fns.insert(name.name.clone(), f.clone());
                }
            }
        }
    }

    // Second pass: validate references inside type-refs and function bodies.
    for item in &commons.items {
        match item {
            CommonsItem::Type(_) => {
                // Type bodies only reference base types (not named types) in v0.
            }
            CommonsItem::Fn(f) => {
                // Check parameter types resolve.
                let mut seen_params: HashMap<&str, &Ident> = HashMap::new();
                for p in &f.params {
                    check_type_ref_resolves(&p.type_ref, &types, &mut errors);
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
                // Check return type resolves.
                check_type_ref_resolves(&f.return_type, &types, &mut errors);
                // Walk the body block.
                let params: HashMap<String, ()> =
                    f.params.iter().map(|p| (p.name.name.clone(), ())).collect();
                let mut scopes: Vec<HashMap<String, ()>> = Vec::new();
                check_block_references(&f.body, &params, &mut scopes, &types, &fns, &mut errors);
            }
        }
    }

    if errors.is_empty() {
        Ok(ResolvedCommons {
            commons,
            types,
            fns,
        })
    } else {
        Err(errors)
    }
}

fn unknown_type_error(id: &Ident) -> CompileError {
    CompileError::new(
        "karn.resolve.unknown_type",
        id.span,
        format!("unknown type `{}`", id.name),
    )
    .with_note(
        "only base types (Int, String, Bool), types declared in this commons, \
         `Result[T, E]`, and `ValidationError` are in scope",
    )
}

/// Recursively check that every type reference resolves to a declared type
/// (or is a base / built-in type). `Result[T, E]` is handled inline; both
/// type arguments are checked.
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
        TypeRef::ValidationError(_) => {}
    }
}

/// Check that names referenced inside a block all resolve. Each statement
/// extends the locals scope for the rest of the block.
fn check_block_references(
    block: &Block,
    params: &HashMap<String, ()>,
    scopes: &mut Vec<HashMap<String, ()>>,
    types: &HashMap<String, TypeDecl>,
    fns: &HashMap<String, FnDecl>,
    errors: &mut Vec<CompileError>,
) {
    scopes.push(HashMap::new());
    for stmt in &block.statements {
        match stmt {
            Statement::Let(l) => {
                // Check the RHS first (the binding is not yet in scope).
                check_expr_references(&l.value, params, scopes, types, fns, errors);
                // Optional type annotation.
                if let Some(annot) = &l.type_annot {
                    check_type_ref_resolves(annot, types, errors);
                }
                // A let cannot shadow a type or function name.
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
                        .with_label(prev.name.span, "function declared here")
                        .with_note("choose a different name for the let binding"),
                    );
                } else {
                    // Shadowing a parameter or earlier let is permitted
                    // silently in v0.1 (the spec says warn-but-compile; the
                    // v0 diagnostic infrastructure does not carry warnings,
                    // so we accept the binding and move on).
                    scopes.last_mut().unwrap().insert(l.name.name.clone(), ());
                }
            }
        }
    }
    check_expr_references(&block.tail, params, scopes, types, fns, errors);
    scopes.pop();
}

fn name_in_scope(name: &str, params: &HashMap<String, ()>, scopes: &[HashMap<String, ()>]) -> bool {
    if params.contains_key(name) {
        return true;
    }
    scopes.iter().rev().any(|s| s.contains_key(name))
}

fn check_expr_references(
    expr: &Expr,
    params: &HashMap<String, ()>,
    scopes: &mut Vec<HashMap<String, ()>>,
    types: &HashMap<String, TypeDecl>,
    fns: &HashMap<String, FnDecl>,
    errors: &mut Vec<CompileError>,
) {
    match &expr.kind {
        ExprKind::IntLit(_) | ExprKind::StrLit(_) | ExprKind::BoolLit(_) => {}
        ExprKind::Ident(id) => {
            if name_in_scope(&id.name, params, scopes) {
                // OK.
            } else if types.contains_key(&id.name) {
                errors.push(
                    CompileError::new(
                        "karn.resolve.type_in_expr",
                        id.span,
                        format!("`{}` is a type, not a value", id.name),
                    )
                    .with_note(
                        "types cannot appear in expression position; \
                         use `TypeName.of(value)` to construct a refined value",
                    ),
                );
            } else if fns.contains_key(&id.name) {
                errors.push(
                    CompileError::new(
                        "karn.resolve.fn_without_call",
                        id.span,
                        format!(
                            "`{}` is a function and must be called — first-class functions are not in v0",
                            id.name
                        ),
                    )
                    .with_note("add an argument list, e.g. `f(x)`"),
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
                            .with_label(decl.name.span, "function declared here"),
                        );
                    }
                }
                None => {
                    if types.contains_key(&name.name) {
                        errors.push(CompileError::new(
                            "karn.resolve.type_as_function",
                            name.span,
                            format!(
                                "`{}` is a type, not a function — types cannot be called directly; \
                                 use `{}.of(value)` to construct a refined value",
                                name.name, name.name
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
                check_expr_references(a, params, scopes, types, fns, errors);
            }
        }
        ExprKind::BinOp(_, lhs, rhs) => {
            check_expr_references(lhs, params, scopes, types, fns, errors);
            check_expr_references(rhs, params, scopes, types, fns, errors);
        }
        ExprKind::UnaryOp(_, e) => check_expr_references(e, params, scopes, types, fns, errors),
        ExprKind::Paren(e) => check_expr_references(e, params, scopes, types, fns, errors),
        ExprKind::Block(b) => check_block_references(b, params, scopes, types, fns, errors),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            check_expr_references(cond, params, scopes, types, fns, errors);
            check_block_references(then_block, params, scopes, types, fns, errors);
            check_block_references(else_block, params, scopes, types, fns, errors);
        }
        ExprKind::Ok(inner) | ExprKind::Err(inner) | ExprKind::Question(inner) => {
            check_expr_references(inner, params, scopes, types, fns, errors);
        }
        ExprKind::ConstructorCall {
            type_name,
            method,
            args,
        } => {
            // Resolve the type name.
            if !types.contains_key(&type_name.name) {
                if fns.contains_key(&type_name.name) {
                    errors.push(CompileError::new(
                        "karn.resolve.constructor_target_not_type",
                        type_name.span,
                        format!(
                            "`{}` is a function, not a type — only types have constructor methods like `.of`",
                            type_name.name
                        ),
                    ));
                } else if name_in_scope(&type_name.name, params, scopes) {
                    errors.push(CompileError::new(
                        "karn.resolve.constructor_target_not_type",
                        type_name.span,
                        format!(
                            "`{}` is a value, not a type — only types have constructor methods like `.of`",
                            type_name.name
                        ),
                    ));
                } else {
                    errors.push(unknown_type_error(type_name));
                }
            }
            // v0.1 only recognises the `of` constructor method.
            if method.name != "of" {
                errors.push(
                    CompileError::new(
                        "karn.resolve.unknown_constructor",
                        method.span,
                        format!(
                            "unknown constructor method `{}` on type `{}`",
                            method.name, type_name.name
                        ),
                    )
                    .with_note("v0.1 supports only the `of` constructor method"),
                );
            }
            for a in args {
                check_expr_references(a, params, scopes, types, fns, errors);
            }
        }
    }
}
