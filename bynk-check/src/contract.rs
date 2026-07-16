//! v0.177 (#643): the canonical normal form of a cross-context contract, and
//! its hash.
//!
//! A `workers` build compiles context A against context B's contract, and
//! nothing at runtime checks that the *deployed* B still matches what A was
//! compiled against — `deploy --context NAME` institutionalises the skew. The
//! fix is to stamp a hash of the compiled contract beside `X-Bynk-Caller` and
//! fail closed on mismatch (ADR 0092's pattern: a compile-time constant in a
//! reserved header, metadata beside the payload, no crypto).
//!
//! The hash is only as good as the form it hashes. Two rules make it usable:
//!
//! 1. **Semantically-equal contracts must hash equal**, or a working deployment
//!    409s spuriously — which is worse than no check at all, because it breaks
//!    what worked and destroys trust in the mechanism. This is why the form is
//!    canonical (predicates as a sorted set, record fields sorted by name)
//!    rather than a rendering of source order.
//! 2. **Both sides must canonicalise the *same* thing.** The callee's contract
//!    is canonicalised **in the callee's own namespace**, from the callee's own
//!    type table, on *both* sides — never in the caller's. The caller reaches
//!    that table through `consumed_types[callee]` and the callee through its own
//!    combined table; both are produced by the same `combined_types_for`, so the
//!    two views cannot diverge by construction. A caller never canonicalises a
//!    consumed type in its own namespace, where its rebranding would make the
//!    same type render differently.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use bynk_syntax::ast::{PredKind, Refinement, TypeBody, TypeDecl, TypeRef};

use crate::resolver::CrossContextService;

/// The canonical normal form of one `on call` service contract.
///
/// Shape: `<service>(<param>: <type>, …) -> <type>`. Parameter **names** and
/// **order** are both included, and both are load-bearing rather than cosmetic:
/// a multi-argument call sends an object keyed by parameter name, and a
/// single-argument call sends the bare value — so a rename or a reorder is a
/// genuine wire change, not a refactor.
pub fn service_normal_form(svc: &CrossContextService, types: &HashMap<String, TypeDecl>) -> String {
    let mut out = String::new();
    let _ = write!(out, "{}(", svc.name);
    for (i, (pname, pty)) in svc.params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let _ = write!(
            out,
            "{pname}: {}",
            canon_type(pty, types, &mut HashSet::new())
        );
    }
    let _ = write!(
        out,
        ") -> {}",
        canon_type(&svc.return_type, types, &mut HashSet::new())
    );
    out
}

/// The canonical form of a type *as it appears on the wire*.
///
/// A named type expands **structurally**, not by name alone: the wire carries
/// the fields, so renaming a record field or changing a variant's payload is a
/// contract change that a name-only form would miss entirely. The name is kept
/// alongside the structure because Bynk's types are nominal — swapping
/// `AuthId` for a structurally identical `SessionId` changes the contract even
/// though the bytes are unchanged. Keeping the name costs nothing in false
/// positives: a rename already breaks the consumer's *compile*, so it cannot
/// reach a deploy without the consumer being rebuilt too.
fn canon_type(
    t: &TypeRef,
    types: &HashMap<String, TypeDecl>,
    seen: &mut HashSet<String>,
) -> String {
    canon_type_in(t, types, seen, &HashMap::new())
}

/// `subst` binds a generic declaration's type-parameter **names** to the
/// canonical form of the concrete argument supplied at the use site.
///
/// A generic body MUST expand with its parameters substituted, or the
/// parameter's *name* leaks into the form: `type Page[T] = { items: List[T] }`
/// and the same declaration spelled with `U` are the same type with the same
/// wire shape, but would hash differently. Across a deploy that renames a type
/// parameter — a pure refactor with no wire consequence — every call would 409.
/// That is the same class of spurious failure the sorted fields and the
/// predicate set exist to prevent, and it is the one the module's own standard
/// ("semantically-equal contracts must hash equal") forbids.
fn canon_type_in(
    t: &TypeRef,
    types: &HashMap<String, TypeDecl>,
    seen: &mut HashSet<String>,
    subst: &HashMap<String, String>,
) -> String {
    match t {
        TypeRef::Base(b, _) => b.name().to_string(),
        TypeRef::Unit(_) => "()".to_string(),
        // An `Effect` wraps the handler, not the payload — the caller awaits the
        // promise, so it is not part of the wire contract.
        TypeRef::Effect(inner, _) => canon_type_in(inner, types, seen, subst),
        TypeRef::List(a, _) => format!("List[{}]", canon_type_in(a, types, seen, subst)),
        TypeRef::Option(a, _) => format!("Option[{}]", canon_type_in(a, types, seen, subst)),
        TypeRef::Result(a, b, _) => format!(
            "Result[{}, {}]",
            canon_type_in(a, types, seen, subst),
            canon_type_in(b, types, seen, subst)
        ),
        TypeRef::Map(k, v, _) => format!(
            "Map[{}, {}]",
            canon_type_in(k, types, seen, subst),
            canon_type_in(v, types, seen, subst)
        ),
        // Generic-record instantiation: the arguments are positional, so their
        // order *is* semantic and is preserved (unlike a record's fields).
        TypeRef::App { name, args, .. } => {
            let inner: Vec<String> = args
                .iter()
                .map(|a| canon_type_in(a, types, seen, subst))
                .collect();
            // Bind the declaration's parameters to these arguments so the body
            // expands over concrete types and the parameter's name never reaches
            // the form.
            let bound: HashMap<String, String> = types
                .get(&name.name)
                .map(|d| {
                    d.type_params
                        .iter()
                        .zip(&inner)
                        .map(|(p, a)| (p.name.name.clone(), a.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let head = canon_named_in(&name.name, types, seen, &bound);
            format!("{head}[{}]", inner.join(", "))
        }
        TypeRef::Named(id) => {
            // A bound type parameter renders as the argument it stands for.
            match subst.get(&id.name) {
                Some(bound) => bound.clone(),
                None => canon_named_in(&id.name, types, seen, subst),
            }
        }
        TypeRef::HttpResult(a, _) => {
            format!("HttpResult[{}]", canon_type_in(a, types, seen, subst))
        }
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::JsonError(_) => "JsonError".to_string(),
        TypeRef::QueueResult(_) => "QueueResult".to_string(),
        // The confined family is rejected at every boundary, so it cannot appear
        // in a contract. Render it rather than panic: the normal form is also a
        // diagnostic surface, and a compiler bug should not become a crash here.
        TypeRef::Fn(..)
        | TypeRef::Query(..)
        | TypeRef::Stream(..)
        | TypeRef::Connection(..)
        | TypeRef::History(..) => "<non-boundary>".to_string(),
    }
}

fn canon_named_in(
    name: &str,
    types: &HashMap<String, TypeDecl>,
    seen: &mut HashSet<String>,
    subst: &HashMap<String, String>,
) -> String {
    // A recursive record terminates on the data, so its codec is finite and it
    // is a legal contract — but its *expansion* is not. Emit a back-reference on
    // revisit. `type Node = { next: Option[Node] }` canonicalises as
    // `Node{next: Option[@Node]}` — the cycle is named, so two different
    // recursive shapes still differ.
    if !seen.insert(name.to_string()) {
        return format!("@{name}");
    }
    let Some(decl) = types.get(name) else {
        // Not in the callee's table: a runtime- or compiler-known name with no
        // declaration to expand. The name alone is the whole contract for it.
        seen.remove(name);
        return name.to_string();
    };
    let body = match &decl.body {
        // Record fields sort by name: a JSON object is unordered, so field
        // *order* is not wire-observable and must not perturb the hash — while
        // field *presence* and type are exactly what the hash exists to pin.
        TypeBody::Record(r) => {
            let mut fields: Vec<String> = r
                .fields
                .iter()
                .map(|f| {
                    format!(
                        "{}: {}",
                        f.name.name,
                        canon_type_in(&f.type_ref, types, seen, subst)
                    )
                })
                .collect();
            fields.sort();
            format!("{{{}}}", fields.join(", "))
        }
        // Variants sort by name for the same reason: the wire carries a `kind`
        // discriminant, so declaration order is invisible to it.
        TypeBody::Sum(s) => {
            let mut variants: Vec<String> = s
                .variants
                .iter()
                .map(|v| {
                    let payload: Vec<String> = v
                        .payload
                        .iter()
                        .map(|p| canon_type_in(&p.type_ref, types, seen, subst))
                        .collect();
                    if payload.is_empty() {
                        v.name.name.clone()
                    } else {
                        format!("{}({})", v.name.name, payload.join(", "))
                    }
                })
                .collect();
            variants.sort();
            format!("|{}", variants.join("|"))
        }
        TypeBody::Refined {
            base, refinement, ..
        } => {
            format!("{} {}", base.name(), canon_refinement(refinement.as_ref()))
        }
        // An **opaque** type's predicate is deliberately excluded — only its
        // representation is part of the contract.
        //
        // The consumer cannot see the predicate by construction (that is what
        // `exports opaque` means), so no consumer behaviour can depend on it: it
        // can hold and pass an `AuthId`, never inspect or mint one. Including the
        // predicate would therefore manufacture skew failures between two
        // contexts that cannot disagree — the owner tightening `Matches(...)`
        // would 409 every caller for a change none of them can observe. This is
        // the same position ADR 0199 took on opacity, from the same premise.
        TypeBody::Opaque { base, .. } => format!("{} opaque", base.name()),
    };
    seen.remove(name);
    format!("{name}{body}")
}

/// Predicates canonicalise as a **sorted set**.
///
/// This is not a nicety adjacent to the hash; it is a precondition for it.
/// Predicates are conjunctive and side-effect-free, so `String where NonEmpty,
/// MaxLen(10)` and `String where MaxLen(10), NonEmpty` are the *same type* — and
/// hashing them in source order would make two contexts that agree perfectly
/// fail closed against each other. The same normal form also backs the checker's
/// `refinements_match`, so the matcher and the hash cannot disagree about what
/// "the same refinement" means.
pub fn canon_refinement(r: Option<&Refinement>) -> String {
    let Some(r) = r else {
        return String::new();
    };
    let mut preds: Vec<String> = r
        .predicates
        .iter()
        .map(|p| canon_predicate(&p.kind))
        .collect();
    preds.sort();
    preds.dedup();
    format!("where {}", preds.join(", "))
}

pub fn canon_predicate(p: &PredKind) -> String {
    match p {
        PredKind::Matches(s) => format!("Matches({s:?})"),
        // Bounds keep their source lexemes elsewhere (byte-stable emission), but
        // a contract is about *values*: `1` and `01` are the same bound, so the
        // parsed value is what canonicalises.
        PredKind::InRange(a, b) => format!("InRange({}, {})", a.value, b.value),
        PredKind::InRangeF(a, b) => format!("InRangeF({}, {})", a.value, b.value),
        PredKind::MinLength(n) => format!("MinLength({n})"),
        PredKind::MaxLength(n) => format!("MaxLength({n})"),
        PredKind::Length(n) => format!("Length({n})"),
        PredKind::NonNegative => "NonNegative".to_string(),
        PredKind::Positive => "Positive".to_string(),
        PredKind::NonEmpty => "NonEmpty".to_string(),
    }
}

/// FNV-1a (64-bit) over the canonical form, rendered as 16 lowercase hex chars.
///
/// **Why not a cryptographic hash.** Trust here is static and channel-based, and
/// this increment does not change that (ADR 0092): `/_bynk/call/` is
/// platform-dispatched and not externally routable, every context in a
/// deployment is one trust domain, and a malicious first-party context is out of
/// the threat model. This is a **skew detector, not a security control** — an
/// accident detector. Forging it buys an attacker nothing they could not already
/// do, so `sha2`'s ~6-crate dependency tree would buy nothing either. A
/// collision degrades to *today's* behaviour for that one pair (an undetected
/// skew), not to something worse, and at ~1e-14 for a 1000-contract project it
/// is not the risk worth engineering against.
///
/// **Why hand-rolled.** `std::collections::hash_map::DefaultHasher` is
/// explicitly not stable across Rust releases, so it cannot back a value that
/// crosses a wire or is compared between two separately-compiled binaries. FNV-1a
/// is fully specified, so two compilers agree forever.
pub fn contract_hash(normal_form: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for b in normal_form.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(PRIME);
    }
    format!("{h:016x}")
}

/// The stamped contract hash for one consumed service.
pub fn service_contract_hash(
    svc: &CrossContextService,
    types: &HashMap<String, TypeDecl>,
) -> String {
    contract_hash(&service_normal_form(svc, types))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bynk_syntax::ast::RefinementPred;

    #[test]
    fn predicate_order_does_not_change_the_form() {
        // The precondition the whole increment rests on: `String where NonEmpty,
        // MaxLen(10)` and the same predicates reordered are the *same type*, so
        // they must canonicalise — and therefore hash — identically. Hashing
        // source order would 409 two contexts that agree perfectly.
        let a = Refinement {
            predicates: vec![
                RefinementPred {
                    kind: PredKind::NonEmpty,
                    span: sp(),
                },
                RefinementPred {
                    kind: PredKind::MaxLength(10),
                    span: sp(),
                },
            ],
            span: sp(),
        };
        let b = Refinement {
            predicates: vec![
                RefinementPred {
                    kind: PredKind::MaxLength(10),
                    span: sp(),
                },
                RefinementPred {
                    kind: PredKind::NonEmpty,
                    span: sp(),
                },
            ],
            span: sp(),
        };
        assert_eq!(canon_refinement(Some(&a)), canon_refinement(Some(&b)));
        assert_eq!(
            contract_hash(&canon_refinement(Some(&a))),
            contract_hash(&canon_refinement(Some(&b)))
        );
    }

    #[test]
    fn a_different_predicate_set_changes_the_form() {
        let a = Refinement {
            predicates: vec![RefinementPred {
                kind: PredKind::MaxLength(10),
                span: sp(),
            }],
            span: sp(),
        };
        let b = Refinement {
            predicates: vec![RefinementPred {
                kind: PredKind::MaxLength(11),
                span: sp(),
            }],
            span: sp(),
        };
        assert_ne!(canon_refinement(Some(&a)), canon_refinement(Some(&b)));
    }

    #[test]
    fn fnv1a_matches_the_published_vectors() {
        // FNV-1a 64-bit reference vectors. The point of hand-rolling a
        // *specified* hash is that two compilers agree forever; pin it.
        assert_eq!(contract_hash(""), "cbf29ce484222325");
        assert_eq!(contract_hash("a"), "af63dc4c8601ec8c");
        assert_eq!(contract_hash("foobar"), "85944171f73967e8");
    }

    /// Build a type table by parsing a `commons`, so these tests exercise real
    /// declarations rather than hand-assembled AST.
    fn types_of(src: &str) -> HashMap<String, TypeDecl> {
        let tokens = bynk_syntax::lexer::tokenize(src).expect("lex");
        let commons = bynk_syntax::parser::parse(&tokens, src).expect("parse");
        commons
            .items
            .iter()
            .filter_map(|i| match i {
                bynk_syntax::ast::CommonsItem::Type(t) => Some((t.name.name.clone(), t.clone())),
                _ => None,
            })
            .collect()
    }

    fn named(n: &str) -> TypeRef {
        TypeRef::Named(bynk_syntax::ast::Ident {
            name: n.to_string(),
            span: sp(),
        })
    }

    fn svc(param_ty: TypeRef) -> CrossContextService {
        CrossContextService {
            name: "probe".to_string(),
            params: vec![("p".to_string(), param_ty)],
            return_type: TypeRef::Base(bynk_syntax::ast::BaseType::String, sp()),
            span: sp(),
        }
    }

    /// A record's **field order** is not wire-observable — a JSON object is
    /// unordered — so it must not move the hash. This is the false-positive side,
    /// and it is the one that matters most: a spurious 409 breaks a working
    /// deployment.
    #[test]
    fn record_field_order_does_not_change_the_hash() {
        let a = types_of("commons x\n\ntype P = { one: Int, two: String }\n");
        let b = types_of("commons x\n\ntype P = { two: String, one: Int }\n");
        assert_eq!(
            service_contract_hash(&svc(named("P")), &a),
            service_contract_hash(&svc(named("P")), &b),
        );
    }

    /// Field **presence** and **type**, by contrast, are exactly what the hash
    /// exists to pin. These are the cases a co-compiled build rejects
    /// structurally — but a skewed *deploy* cannot, which is why the hash carries
    /// them.
    #[test]
    fn field_presence_name_and_type_change_the_hash() {
        let base = types_of("commons x\n\ntype P = { one: Int, two: String }\n");
        let h = service_contract_hash(&svc(named("P")), &base);

        let renamed = types_of("commons x\n\ntype P = { one: Int, three: String }\n");
        assert_ne!(
            h,
            service_contract_hash(&svc(named("P")), &renamed),
            "rename"
        );

        let dropped = types_of("commons x\n\ntype P = { two: String }\n");
        assert_ne!(h, service_contract_hash(&svc(named("P")), &dropped), "drop");

        let retyped = types_of("commons x\n\ntype P = { one: String, two: String }\n");
        assert_ne!(
            h,
            service_contract_hash(&svc(named("P")), &retyped),
            "retype"
        );
    }

    /// A sum's **variant order** is invisible to the wire (the payload carries a
    /// `kind` discriminant); its variant *set* is not.
    #[test]
    fn sum_variant_order_does_not_change_the_hash_but_the_set_does() {
        let a = types_of("commons x\n\ntype E = enum { Alpha, Beta }\n");
        let b = types_of("commons x\n\ntype E = enum { Beta, Alpha }\n");
        assert_eq!(
            service_contract_hash(&svc(named("E")), &a),
            service_contract_hash(&svc(named("E")), &b),
        );
        let c = types_of("commons x\n\ntype E = enum { Alpha, Gamma }\n");
        assert_ne!(
            service_contract_hash(&svc(named("E")), &a),
            service_contract_hash(&svc(named("E")), &c),
        );
    }

    /// An **opaque** type's predicate is excluded: the consumer cannot see it by
    /// construction, so no consumer behaviour can depend on it, and including it
    /// would manufacture skew between two contexts that cannot disagree. Its
    /// *representation* is still part of the contract.
    #[test]
    fn an_opaque_types_predicate_is_excluded_but_its_representation_is_not() {
        let loose = types_of("commons x\n\ntype Id = opaque String where NonEmpty\n");
        let tight = types_of("commons x\n\ntype Id = opaque String where MaxLength(4)\n");
        assert_eq!(
            service_contract_hash(&svc(named("Id")), &loose),
            service_contract_hash(&svc(named("Id")), &tight),
            "tightening an opaque predicate must not 409 a caller that cannot see it"
        );

        // A *transparent* refined type is the opposite: the consumer can see the
        // predicate, so it is part of the contract.
        let ra = types_of("commons x\n\ntype C = String where MaxLength(4)\n");
        let rb = types_of("commons x\n\ntype C = String where MaxLength(5)\n");
        assert_ne!(
            service_contract_hash(&svc(named("C")), &ra),
            service_contract_hash(&svc(named("C")), &rb),
        );
    }

    /// A recursive record is a legal contract (its codec terminates on the data),
    /// but its expansion is not — the walk must terminate rather than blow the
    /// stack, and two different recursive shapes must still differ.
    #[test]
    fn a_recursive_record_terminates_and_stays_distinguishable() {
        let a = types_of("commons x\n\ntype Node = { v: Int, next: Option[Node] }\n");
        let b = types_of("commons x\n\ntype Node = { v: String, next: Option[Node] }\n");
        let ha = service_contract_hash(&svc(named("Node")), &a);
        assert_ne!(ha, service_contract_hash(&svc(named("Node")), &b));
    }

    /// A generic type's **parameter name** is not wire-observable: `Page[T]` and
    /// the same declaration spelled with `U` are the same type with the same
    /// shape. Renaming one is a refactor, and must not 409 a caller.
    ///
    /// Caught in review of #658: the body used to expand with the parameter
    /// *unsubstituted*, so the name leaked into the form. Same class as record
    /// field order and predicate order — the false-positive side the module's
    /// standard exists to protect.
    #[test]
    fn a_generic_type_parameter_rename_does_not_change_the_hash() {
        let t = types_of(
            "commons x\n\ntype Order = { id: Int }\ntype Page[T] = { items: List[T], total: Int }\n",
        );
        let u = types_of(
            "commons x\n\ntype Order = { id: Int }\ntype Page[U] = { items: List[U], total: Int }\n",
        );
        let app = TypeRef::App {
            name: bynk_syntax::ast::Ident {
                name: "Page".to_string(),
                span: sp(),
            },
            args: vec![named("Order")],
            span: sp(),
        };
        assert_eq!(
            service_contract_hash(&svc(app.clone()), &t),
            service_contract_hash(&svc(app), &u),
            "renaming a generic type parameter must not change the contract hash"
        );
    }

    /// The converse: the *argument* a generic is instantiated at is entirely
    /// wire-observable, so it must still move the hash.
    #[test]
    fn a_generic_argument_change_does_change_the_hash() {
        let t = types_of(
            "commons x\n\ntype Order = { id: Int }\ntype Other = { id: String }\ntype Page[T] = { items: List[T] }\n",
        );
        let app = |arg: &str| TypeRef::App {
            name: bynk_syntax::ast::Ident {
                name: "Page".to_string(),
                span: sp(),
            },
            args: vec![named(arg)],
            span: sp(),
        };
        assert_ne!(
            service_contract_hash(&svc(app("Order")), &t),
            service_contract_hash(&svc(app("Other")), &t),
        );
    }

    /// And the parameter is genuinely *substituted*, not merely ignored: the form
    /// shows the instantiated shape rather than a dangling `T`.
    #[test]
    fn a_generic_body_expands_over_its_concrete_argument() {
        let t = types_of("commons x\n\ntype Page[T] = { items: List[T] }\n");
        let app = TypeRef::App {
            name: bynk_syntax::ast::Ident {
                name: "Page".to_string(),
                span: sp(),
            },
            args: vec![TypeRef::Base(bynk_syntax::ast::BaseType::Int, sp())],
            span: sp(),
        };
        let nf = service_normal_form(&svc(app), &t);
        assert!(nf.contains("List[Int]"), "{nf}");
        assert!(
            !nf.contains("List[T]"),
            "the parameter must not survive: {nf}"
        );
    }

    fn sp() -> bynk_syntax::span::Span {
        bynk_syntax::span::Span::new(0, 0)
    }
}
