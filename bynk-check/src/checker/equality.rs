//! Which types support `==`/`!=`. #1652 (runtime-semantics track #1648, slice
//! S3; settled in `design/tracks/runtime-semantics.md` §3.1).
//!
//! `==` is **structural**: two values are equal when they have the same shape and
//! their parts compare equal, recursively (type-system §2.3.5). That only means
//! something for types made of values. Some types are not values in that sense:
//!
//! - a function (`(A) -> B`), an `Effect[T]`, a `Query[T]`: a computation, not
//!   data (ADRs 0030/0031/0115);
//! - a `Stream[T]`: a live value-over-time source (real-time track slice 0);
//! - a held `Connection[F]`: it has identity, not value equality (§2.9.3).
//!
//! §2.3.5 makes the rule recursive: a type containing one of these **anywhere**
//! (inside an `Option`, a `List`, a `Map`, a record field, a sum payload, or a
//! generic argument) is not equality-supporting, and the compiler rejects
//! `==` on it. Before #1652 the checker only looked at the *top-level* type, so
//! `Option[Connection[F]]`, `List[(Int) -> Int]`, a record with an `Effect`
//! field, and `Box[(Int) -> Int]` were all accepted.
//!
//! Two gaps remain, both permissive (the runtime walker then compares the
//! offending part by identity):
//!
//! - a type variable is equality-supporting, so `==` on `T` inside a generic
//!   function is accepted, and nothing re-checks the type argument a caller
//!   instantiates `T` with (`same(f, h)` on two functions compiles). Closing it
//!   needs equality bounds inferred per generic function, transitively;
//! - a record imported from another unit is absent from `ctx.input.types`, so
//!   `walk_decl` cannot see its fields.
//!
//! Records and sums are walked through their declared field types
//! (`TypeRef`s), because the interned `Ty::Named` carries only the name and the
//! applied arguments. A declared field that names one of the type's own
//! parameters is covered by walking the corresponding applied argument. A
//! visited set keeps recursive types (`Node = { next: Option[Node] }`) finite.

use std::collections::HashSet;

use bynk_syntax::ast::{TypeBody, TypeRef};

use super::{Ctx, Ty, TyId};

/// Why a type is not equality-supporting: the first offending part found.
pub(crate) enum NotComparable {
    /// A `Stream[T]` (`bynk.types.stream_not_comparable`).
    Stream,
    /// A held `Connection[F]` (`bynk.types.held_not_comparable`). The string is
    /// the held type's display form.
    Held(String),
    /// A function, `Effect` or `Query` (`bynk.types.not_comparable`). The string
    /// names the offending part for the diagnostic.
    Computation(String),
}

/// The first part of `ty` that has no value equality, or `None` when `ty` is
/// equality-supporting all the way down.
pub(crate) fn not_comparable(ty: TyId, ctx: &Ctx) -> Option<NotComparable> {
    let mut visited = HashSet::new();
    walk_ty(ty, ctx, &mut visited)
}

fn walk_ty(ty: TyId, ctx: &Ctx, visited: &mut HashSet<String>) -> Option<NotComparable> {
    let tys = ctx.tys;
    match &*tys.get(ty) {
        Ty::Stream(_) => Some(NotComparable::Stream),
        Ty::Connection(_) => Some(NotComparable::Held(tys.display(ty))),
        Ty::Fn { .. } | Ty::Effect(_) | Ty::Query(_) => {
            Some(NotComparable::Computation(tys.display(ty)))
        }
        Ty::Option(t) | Ty::List(t) | Ty::HttpResult(t) => walk_ty(*t, ctx, visited),
        Ty::Result(a, b) | Ty::Map(a, b) => {
            walk_ty(*a, ctx, visited).or_else(|| walk_ty(*b, ctx, visited))
        }
        Ty::Named { name, args, .. } => {
            let args = args.clone();
            let name = name.clone();
            for a in &args {
                if let Some(found) = walk_ty(*a, ctx, visited) {
                    return Some(found);
                }
            }
            walk_decl(&name, ctx, visited)
        }
        // Base types, `()`, the built-in error and verdict types, actor
        // bindings, and type variables (`T` inside a generic body) compare as
        // values: base types by `===`, everything else by the structural
        // runtime walker.
        Ty::Error
        | Ty::Base(_)
        | Ty::QueueResult
        | Ty::ValidationError
        | Ty::JsonError
        | Ty::Unit
        | Ty::Actor(_)
        | Ty::ActorSum(_)
        | Ty::Var(_) => None,
    }
}

/// Walk a named type's declared fields. Each name is walked once per check,
/// which keeps recursive types finite and makes the walk linear in the number of
/// types reachable from the operand.
fn walk_decl(name: &str, ctx: &Ctx, visited: &mut HashSet<String>) -> Option<NotComparable> {
    if !visited.insert(name.to_string()) {
        return None;
    }
    let decl = ctx.input.types.get(name)?;
    let params: Vec<&str> = decl
        .type_params
        .iter()
        .map(|p| p.name.name.as_str())
        .collect();
    let fields: Vec<&TypeRef> = match &decl.body {
        TypeBody::Record(r) => r.fields.iter().map(|f| &f.type_ref).collect(),
        TypeBody::Sum(s) => s
            .variants
            .iter()
            .flat_map(|v| v.payload.iter().map(|f| &f.type_ref))
            .collect(),
        TypeBody::Refined { .. } | TypeBody::Opaque { .. } => Vec::new(),
    };
    fields
        .into_iter()
        .find_map(|t| walk_ref(t, &params, ctx, visited))
}

fn walk_ref(
    t: &TypeRef,
    params: &[&str],
    ctx: &Ctx,
    visited: &mut HashSet<String>,
) -> Option<NotComparable> {
    let display = || render(t);
    match t {
        TypeRef::Stream(..) => Some(NotComparable::Stream),
        TypeRef::Connection(..) => Some(NotComparable::Held(display())),
        TypeRef::Fn(..) | TypeRef::Effect(..) | TypeRef::Query(..) => {
            Some(NotComparable::Computation(display()))
        }
        TypeRef::Option(a, _)
        | TypeRef::List(a, _)
        | TypeRef::HttpResult(a, _)
        | TypeRef::History(a, _) => walk_ref(a, params, ctx, visited),
        TypeRef::Result(a, b, _) | TypeRef::Map(a, b, _) => {
            walk_ref(a, params, ctx, visited).or_else(|| walk_ref(b, params, ctx, visited))
        }
        // A type parameter is checked through the applied argument at the use
        // site (`walk_ty`'s `Named` arm), not here.
        TypeRef::Named(id) if params.contains(&id.name.as_str()) => None,
        TypeRef::Named(id) => walk_decl(&id.name, ctx, visited),
        TypeRef::App { name, args, .. } => args
            .iter()
            .find_map(|a| walk_ref(a, params, ctx, visited))
            .or_else(|| walk_decl(&name.name, ctx, visited)),
        TypeRef::Base(..)
        | TypeRef::QueueResult(_)
        | TypeRef::ValidationError(_)
        | TypeRef::JsonError(_)
        | TypeRef::Unit(_) => None,
    }
}

/// Render a declared field type as the author wrote it, for the diagnostic.
/// (`context_checks::type_ref_to_display` covers only the shapes its own callers
/// meet; this walk needs every `TypeRef`.)
fn render(t: &TypeRef) -> String {
    let one = |name: &str, a: &TypeRef| format!("{name}[{}]", render(a));
    match t {
        TypeRef::Base(b, _) => b.name().to_string(),
        TypeRef::Named(id) => id.name.clone(),
        TypeRef::Result(a, b, _) => format!("Result[{}, {}]", render(a), render(b)),
        TypeRef::Map(a, b, _) => format!("Map[{}, {}]", render(a), render(b)),
        TypeRef::Option(a, _) => one("Option", a),
        TypeRef::Effect(a, _) => one("Effect", a),
        TypeRef::HttpResult(a, _) => one("HttpResult", a),
        TypeRef::List(a, _) => one("List", a),
        TypeRef::Query(a, _) => one("Query", a),
        TypeRef::Stream(a, _) => one("Stream", a),
        TypeRef::Connection(a, _) => one("Connection", a),
        TypeRef::History(a, _) => one("History", a),
        TypeRef::QueueResult(_) => "QueueResult".to_string(),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::JsonError(_) => "JsonError".to_string(),
        TypeRef::Unit(_) => "()".to_string(),
        TypeRef::Fn(params, ret, _) => {
            let ps: Vec<String> = params.iter().map(render).collect();
            format!("({}) -> {}", ps.join(", "), render(ret))
        }
        TypeRef::App { name, args, .. } => {
            let a: Vec<String> = args.iter().map(render).collect();
            format!("{}[{}]", name.name, a.join(", "))
        }
    }
}
