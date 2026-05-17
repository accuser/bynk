//! Name resolution (spec §5.1).
//!
//! Builds a symbol table for the commons and validates that:
//! - No two top-level items share a name.
//! - Every `TypeRef::Named` resolves to a type declaration.
//! - Every function call resolves to a function declaration (with the right arity).
//! - Every identifier in expression position resolves to a parameter.
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
                    if let TypeRef::Named(ref id) = p.type_ref
                        && !types.contains_key(&id.name)
                    {
                        errors.push(unknown_type_error(id));
                    }
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
                if let TypeRef::Named(ref id) = f.return_type
                    && !types.contains_key(&id.name)
                {
                    errors.push(unknown_type_error(id));
                }
                // Walk body expression.
                let params: HashMap<&str, &TypeRef> = f
                    .params
                    .iter()
                    .map(|p| (p.name.name.as_str(), &p.type_ref))
                    .collect();
                check_expr_references(&f.body, &params, &types, &fns, &mut errors);
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
        "only base types (Int, String, Bool) and types declared in this commons are in scope",
    )
}

fn check_expr_references(
    expr: &Expr,
    params: &HashMap<&str, &TypeRef>,
    types: &HashMap<String, TypeDecl>,
    fns: &HashMap<String, FnDecl>,
    errors: &mut Vec<CompileError>,
) {
    match &expr.kind {
        ExprKind::IntLit(_) | ExprKind::StrLit(_) | ExprKind::BoolLit(_) => {}
        ExprKind::Ident(id) => {
            if params.contains_key(id.name.as_str()) {
                // OK.
            } else if types.contains_key(&id.name) {
                errors.push(
                    CompileError::new(
                        "karn.resolve.type_in_expr",
                        id.span,
                        format!("`{}` is a type, not a value", id.name),
                    )
                    .with_note("types cannot appear in expression position in v0"),
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
                        "only parameters and functions declared in this commons are in scope",
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
                                "`{}` is a type, not a function — types cannot be called in v0",
                                name.name
                            ),
                        ));
                    } else if params.contains_key(name.name.as_str()) {
                        errors.push(CompileError::new(
                            "karn.resolve.param_as_function",
                            name.span,
                            format!("`{}` is a parameter, not a function", name.name),
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
                check_expr_references(a, params, types, fns, errors);
            }
        }
        ExprKind::BinOp(_, lhs, rhs) => {
            check_expr_references(lhs, params, types, fns, errors);
            check_expr_references(rhs, params, types, fns, errors);
        }
        ExprKind::UnaryOp(_, e) => check_expr_references(e, params, types, fns, errors),
        ExprKind::Paren(e) => check_expr_references(e, params, types, fns, errors),
    }
}
