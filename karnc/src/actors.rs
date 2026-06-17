//! v0.45 actor contracts (the actors-foundations slice).
//!
//! An `actor` declaration is a nominal *boundary contract* (ADR Q1): a closed,
//! compiler-known authentication `Scheme` plus an optional sealed identity. A
//! handler consumes an actor on its `by` clause; the boundary verifies the
//! scheme and mints the identity before the body runs (two-phase, fail-closed —
//! ADR Q5/Q2).
//!
//! This module holds the compiler-known parts: the closed scheme set, the
//! prelude actors, the per-protocol default actors, and the admissible-scheme
//! sets. Foundations admits only the two zero-crypto schemes (`None`,
//! `Internal`); `Bearer`/`Signature` are reserved-and-rejected.

use std::collections::HashMap;

use crate::ast::{
    ActorDecl, BinOp, Expr, ExprKind, Handler, HandlerKind, ServiceProtocol, TypeRef, UnaryOp,
};
use crate::span::Span;

/// The authentication scheme — a closed, compiler-known set (ADR Q1). Sealed
/// now, openable later by widening this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Anonymous — no verification; identity is `()`. (`Visitor`.)
    None,
    /// In-system / platform trust — the channel itself is the assertion
    /// (service-binding / platform dispatch). Admitted in Foundations.
    Internal,
    /// Bearer token — reserved; not admitted in Foundations.
    Bearer,
    /// Request signature — reserved; not admitted in Foundations.
    Signature,
}

impl Scheme {
    /// Classify a scheme name written in `auth = <Scheme>`. `None` means the
    /// name is not one of the four compiler-known schemes.
    pub fn from_name(s: &str) -> Option<Scheme> {
        Some(match s {
            "None" => Scheme::None,
            "Internal" => Scheme::Internal,
            "Bearer" => Scheme::Bearer,
            "Signature" => Scheme::Signature,
            _ => return None,
        })
    }

    /// The schemes the compiler can emit verification for. v0.45 admitted the
    /// two zero-crypto schemes (`None`/`Internal`); v0.47 added `Bearer`
    /// (JWT/HS256); v0.51 adds `Signature` (HMAC over the body). All four
    /// schemes are now admitted.
    pub fn admitted(self) -> bool {
        matches!(
            self,
            Scheme::None | Scheme::Internal | Scheme::Bearer | Scheme::Signature
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::None => "None",
            Scheme::Internal => "Internal",
            Scheme::Bearer => "Bearer",
            Scheme::Signature => "Signature",
        }
    }
}

/// The identity a verified actor yields (ADR Q2). In Foundations this is `()`
/// for trivial actors, the built-in sealed `CallerId` for the cross-context
/// `Internal` channel (Q7, folded in), or a context-owned declared type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// `()` — `None` actors and platform-tag `Internal` actors.
    Unit,
    /// The built-in sealed calling-context identity (Q7). Minted at the
    /// service-binding seam; read-only and never re-checked.
    CallerId,
    /// A context-owned declared type named in `identity = <T>`.
    Declared(String),
}

/// The built-in sealed identity type for the cross-context calling principal.
pub const CALLER_ID: &str = "CallerId";

/// A resolved actor contract: its scheme and the identity it yields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    pub scheme: Scheme,
    pub identity: Identity,
}

/// The prelude actors — compiler-known boundary contracts available without a
/// declaration. They back the per-protocol defaults and let public HTTP routes
/// write `by v: Visitor` without ceremony.
pub fn prelude_actor(name: &str) -> Option<Contract> {
    Some(match name {
        // Anonymous public surface — the only safe HTTP actor in Foundations.
        "Visitor" => Contract {
            scheme: Scheme::None,
            identity: Identity::Unit,
        },
        // Platform schedulers / producers — Internal, carrying no useful
        // identity payload (a bare tag).
        "Scheduler" | "Producer" => Contract {
            scheme: Scheme::Internal,
            identity: Identity::Unit,
        },
        // The cross-context calling principal — Internal, yielding the sealed
        // `CallerId` (Q7).
        "Caller" => Contract {
            scheme: Scheme::Internal,
            identity: Identity::CallerId,
        },
        _ => return None,
    })
}

/// The default actor a handler inherits when it omits `by`, by protocol (ADR
/// Q5). HTTP has no safe default — `by` is required there.
pub fn default_actor(protocol: &ServiceProtocol) -> Option<&'static str> {
    match protocol {
        ServiceProtocol::Call => Some("Caller"),
        ServiceProtocol::Cron => Some("Scheduler"),
        ServiceProtocol::Queue { .. } => Some("Producer"),
        ServiceProtocol::Http => None,
    }
}

/// v0.47: the data the emitter needs to lower a Bearer verification seam for a
/// handler — the `by` binder (v0.50: `None` for the binder-less verify-and-
/// discard form), the signing-secret env name, and the identity type to
/// construct from the JWT `sub` claim. Resolved only for a handler whose `by`
/// clause names a local Bearer actor; the checker guarantees the secret is
/// present and the identity is a string-constructible local type.
#[derive(Debug, Clone)]
pub struct BearerSeam {
    /// The identity binder, or `None` for `by <BearerActor>` (verify the token,
    /// don't capture the identity). When `None` the seam still verifies fail-
    /// closed but mints no identity and threads nothing into `deps`.
    pub binder: Option<String>,
    pub secret: String,
    pub identity_type: String,
    /// v0.53: the authorisation invariant when the `by` actor is a refinement
    /// (`actor Admin = User where <pred>`). The seam verifies the scheme (401),
    /// then checks this predicate against the verified claims (403 fail-closed),
    /// then mints the (base) identity. `None` for a plain Bearer actor.
    pub authorization: Option<ClaimPredicate>,
}

/// Resolve a handler's Bearer seam, if its `by` clause names a local Bearer
/// actor — or a **refinement** of one (v0.53), following the refinement to its
/// base for the scheme/secret/identity and carrying the authorisation
/// predicate. Returns `None` for non-Bearer handlers (prelude actors are never
/// Bearer) — those emit unchanged.
pub fn bearer_seam_for(
    handler: &Handler,
    actors: &HashMap<String, ActorDecl>,
) -> Option<BearerSeam> {
    let by = handler.by_clause.as_ref()?;
    let named = actors.get(&by.primary().name)?;
    // Follow a refinement to its base; carry the authorisation predicate. The
    // checker guarantees a refinement's base is Bearer and its predicate parses.
    let (base, authorization) = match &named.refinement {
        Some(r) => (
            actors.get(&r.base.name)?,
            parse_claim_predicate(&r.predicate).ok(),
        ),
        None => (named, None),
    };
    if Scheme::from_name(base.auth.as_ref()?.name.as_str()) != Some(Scheme::Bearer) {
        return None;
    }
    let secret = base.scheme_arg("secret")?.value.as_str()?.to_string();
    let TypeRef::Named(id) = base.identity.as_ref()? else {
        return None;
    };
    Some(BearerSeam {
        binder: by.binder.as_ref().map(|b| b.name.clone()),
        secret,
        identity_type: id.name.clone(),
        authorization,
    })
}

/// v0.54: the binder of a cross-context `on call … by c: Caller` handler that
/// captures a live `CallerId` (the calling context's name, Q7). `None` unless
/// the handler binds an identity whose contract is `CallerId` — i.e. the
/// `Caller` prelude actor (the only source of `CallerId`). A binder-less
/// `on call` (or one inheriting the `Caller` default) captures nothing and is
/// unaffected.
pub fn caller_binder_for(handler: &Handler, actors: &HashMap<String, ActorDecl>) -> Option<String> {
    // `CallerId` is a cross-context `on call` concept; the checker rejects a
    // `Caller` actor on other protocols (`scheme_not_admissible`), but guard here
    // too so the caller seam is never emitted off the call path.
    if !matches!(handler.kind, HandlerKind::Call) {
        return None;
    }
    let by = handler.by_clause.as_ref()?;
    let binder = by.binder.as_ref()?;
    let name = &by.primary().name;
    // `CallerId` is yielded only by the `Caller` prelude actor; a local actor
    // never declares it. A binder that collides with a param is suppressed
    // upstream, mirroring the other seams.
    let is_caller = !actors.contains_key(name)
        && prelude_actor(name).map(|c| c.identity) == Some(Identity::CallerId)
        && !handler.params.iter().any(|p| p.name.name == binder.name);
    is_caller.then(|| binder.name.clone())
}

/// v0.51: the data the emitter needs to lower a Signature verification seam —
/// the signing-secret env name, the signature header, and an optional
/// timestamp header + tolerance window for replay defence. Resolved only for a
/// handler whose `by` clause names a local Signature actor.
#[derive(Debug, Clone)]
pub struct SignatureSeam {
    pub secret: String,
    pub header: String,
    pub timestamp_header: Option<String>,
    pub tolerance_secs: Option<i64>,
}

/// Resolve a handler's Signature seam, if its `by` clause names a local
/// Signature actor. The checker guarantees `secret` and `header` are present.
pub fn signature_seam_for(
    handler: &Handler,
    actors: &HashMap<String, ActorDecl>,
) -> Option<SignatureSeam> {
    let by = handler.by_clause.as_ref()?;
    let actor = actors.get(&by.primary().name)?;
    if Scheme::from_name(actor.auth.as_ref()?.name.as_str()) != Some(Scheme::Signature) {
        return None;
    }
    signature_seam_from_decl(actor)
}

/// The Signature seam data carried by an actor declaration (its keyed config).
/// Shared by the single-actor `signature_seam_for` and the multi-actor
/// `sum_members_for`.
fn signature_seam_from_decl(actor: &ActorDecl) -> Option<SignatureSeam> {
    Some(SignatureSeam {
        secret: actor.scheme_arg("secret")?.value.as_str()?.to_string(),
        header: actor.scheme_arg("header")?.value.as_str()?.to_string(),
        timestamp_header: actor
            .scheme_arg("timestamp")
            .and_then(|a| a.value.as_str())
            .map(str::to_string),
        tolerance_secs: actor.scheme_arg("tolerance").and_then(|a| a.value.as_int()),
    })
}

/// v0.52: one resolved member of a multi-actor sum — the seam the emitter tries
/// at that position in the first-wins order. `actor_name` is the variant tag the
/// body matches on.
#[derive(Debug, Clone)]
pub struct SumMember {
    pub actor_name: String,
    pub seam: SumMemberSeam,
}

/// The verification a sum member contributes. `None` (a catch-all such as
/// `Visitor`) always resolves, so it terminates the order.
#[derive(Debug, Clone)]
pub enum SumMemberSeam {
    None,
    Bearer {
        secret: String,
        identity_type: String,
    },
    Signature(SignatureSeam),
}

impl SumMember {
    /// Whether resolving this member needs the raw request body read.
    pub fn needs_body(&self) -> bool {
        matches!(self.seam, SumMemberSeam::Signature(_))
    }
    /// The member's identity type name, if it mints one (Bearer). `None`/
    /// Signature members carry a unit identity.
    pub fn identity_type(&self) -> Option<&str> {
        match &self.seam {
            SumMemberSeam::Bearer { identity_type, .. } => Some(identity_type),
            _ => None,
        }
    }
}

/// v0.52: resolve a handler's `by` clause into ordered sum members, if it names
/// more than one actor. `None` for a single-actor handler (those keep the
/// existing seam paths). The checker has already validated peer/scheme/
/// reachability rules; this lowers the verified members for emission.
pub fn sum_members_for(
    handler: &Handler,
    actors: &HashMap<String, ActorDecl>,
) -> Option<Vec<SumMember>> {
    let by = handler.by_clause.as_ref()?;
    if !by.is_sum() {
        return None;
    }
    let mut members = Vec::new();
    for actor_ref in &by.actors {
        let seam = if let Some(decl) = actors.get(&actor_ref.name) {
            match Scheme::from_name(decl.auth.as_ref()?.name.as_str())? {
                Scheme::None => SumMemberSeam::None,
                Scheme::Bearer => {
                    let secret = decl.scheme_arg("secret")?.value.as_str()?.to_string();
                    let TypeRef::Named(id) = decl.identity.as_ref()? else {
                        return None;
                    };
                    SumMemberSeam::Bearer {
                        secret,
                        identity_type: id.name.clone(),
                    }
                }
                Scheme::Signature => SumMemberSeam::Signature(signature_seam_from_decl(decl)?),
                Scheme::Internal => return None,
            }
        } else {
            // A prelude actor: only `Visitor` (scheme `None`) is an HTTP peer.
            match prelude_actor(&actor_ref.name) {
                Some(c) if c.scheme == Scheme::None => SumMemberSeam::None,
                _ => return None,
            }
        };
        members.push(SumMember {
            actor_name: actor_ref.name.clone(),
            seam,
        });
    }
    Some(members)
}

/// Whether `scheme` is admissible on `protocol` (the admissible-scheme-per-
/// protocol check). HTTP admits `None` (public routes) and `Bearer` (an
/// `Authorization` header is an HTTP concept); the internal protocols
/// (call/cron/queue) admit `Internal`. `Signature` is still reserved.
pub fn scheme_admissible(protocol: &ServiceProtocol, scheme: Scheme) -> bool {
    match protocol {
        ServiceProtocol::Http => {
            matches!(scheme, Scheme::None | Scheme::Bearer | Scheme::Signature)
        }
        ServiceProtocol::Call | ServiceProtocol::Cron | ServiceProtocol::Queue { .. } => {
            matches!(scheme, Scheme::Internal)
        }
    }
}

/// v0.53: the closed claim-predicate vocabulary for a refinement actor's `where`
/// clause (`actor Admin = User where hasClaim("admin")`). Claims are untyped
/// JSON, so the predicate is a closed set — `hasClaim`/`claimEquals` composed
/// with `&&`/`||`/`!` — checked against the *verified* JWT claims at the
/// boundary. A general typed-claims expression surface is a later slice.
#[derive(Debug, Clone)]
pub enum ClaimPredicate {
    /// `hasClaim("name")` — the claim is present and truthy.
    HasClaim(String),
    /// `claimEquals("name", "value")` — the claim string-equals `value`.
    ClaimEquals(String, String),
    And(Box<ClaimPredicate>, Box<ClaimPredicate>),
    Or(Box<ClaimPredicate>, Box<ClaimPredicate>),
    Not(Box<ClaimPredicate>),
}

fn claim_str_lit(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::StrLit(s) => Some(s.clone()),
        _ => None,
    }
}

/// Recognise the closed claim-predicate vocabulary in a refinement `where`
/// expression. `Err(span)` points at the first sub-expression outside the set
/// (for `karn.actor.refinement_predicate_unsupported`).
pub fn parse_claim_predicate(e: &Expr) -> Result<ClaimPredicate, Span> {
    match &e.kind {
        ExprKind::Paren(inner) => parse_claim_predicate(inner),
        ExprKind::BinOp(BinOp::And, l, r) => Ok(ClaimPredicate::And(
            Box::new(parse_claim_predicate(l)?),
            Box::new(parse_claim_predicate(r)?),
        )),
        ExprKind::BinOp(BinOp::Or, l, r) => Ok(ClaimPredicate::Or(
            Box::new(parse_claim_predicate(l)?),
            Box::new(parse_claim_predicate(r)?),
        )),
        ExprKind::UnaryOp(UnaryOp::Not, inner) => {
            Ok(ClaimPredicate::Not(Box::new(parse_claim_predicate(inner)?)))
        }
        ExprKind::Call {
            name,
            type_args,
            args,
        } if type_args.is_empty() => match (name.name.as_str(), args.as_slice()) {
            ("hasClaim", [a]) => claim_str_lit(a).map(ClaimPredicate::HasClaim).ok_or(a.span),
            ("claimEquals", [a, b]) => match (claim_str_lit(a), claim_str_lit(b)) {
                (Some(n), Some(v)) => Ok(ClaimPredicate::ClaimEquals(n, v)),
                (None, _) => Err(a.span),
                (_, None) => Err(b.span),
            },
            _ => Err(name.span),
        },
        _ => Err(e.span),
    }
}

/// Lower a claim predicate to a JavaScript boolean expression over `claims_var`
/// (the verified claims object, `Record<string, unknown>`). Used by the emitter
/// for the refinement seam's 403 check.
pub fn claim_predicate_to_js(pred: &ClaimPredicate, claims_var: &str) -> String {
    match pred {
        ClaimPredicate::HasClaim(name) => {
            format!("Boolean({claims_var}[\"{}\"])", js_str_escape(name))
        }
        ClaimPredicate::ClaimEquals(name, value) => format!(
            "({claims_var}[\"{}\"] === \"{}\")",
            js_str_escape(name),
            js_str_escape(value)
        ),
        ClaimPredicate::And(l, r) => format!(
            "({} && {})",
            claim_predicate_to_js(l, claims_var),
            claim_predicate_to_js(r, claims_var)
        ),
        ClaimPredicate::Or(l, r) => format!(
            "({} || {})",
            claim_predicate_to_js(l, claims_var),
            claim_predicate_to_js(r, claims_var)
        ),
        ClaimPredicate::Not(inner) => {
            format!("(!{})", claim_predicate_to_js(inner, claims_var))
        }
    }
}

fn js_str_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
