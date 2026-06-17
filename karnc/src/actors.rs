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

use crate::ast::{ActorDecl, Handler, ServiceProtocol, TypeRef};

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
    /// two zero-crypto schemes (`None`/`Internal`); v0.47 adds `Bearer`
    /// (JWT/HS256). `Signature` remains reserved.
    pub fn admitted(self) -> bool {
        matches!(self, Scheme::None | Scheme::Internal | Scheme::Bearer)
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
/// handler — the `by` binder, the signing-secret env name, and the identity
/// type to construct from the JWT `sub` claim. Resolved only for a handler whose
/// `by` clause names a local Bearer actor; the checker guarantees the secret is
/// present and the identity is a string-constructible local type.
#[derive(Debug, Clone)]
pub struct BearerSeam {
    pub binder: String,
    pub secret: String,
    pub identity_type: String,
}

/// Resolve a handler's Bearer seam, if its `by` clause names a local Bearer
/// actor. Returns `None` for non-Bearer handlers (prelude actors are never
/// Bearer) — those emit unchanged.
pub fn bearer_seam_for(
    handler: &Handler,
    actors: &HashMap<String, ActorDecl>,
) -> Option<BearerSeam> {
    let by = handler.by_clause.as_ref()?;
    let actor = actors.get(&by.actor.name)?;
    if Scheme::from_name(actor.auth.as_ref()?.name.as_str()) != Some(Scheme::Bearer) {
        return None;
    }
    let secret = actor.auth_secret.as_ref()?.0.clone();
    let TypeRef::Named(id) = actor.identity.as_ref()? else {
        return None;
    };
    Some(BearerSeam {
        binder: by.binder.name.clone(),
        secret,
        identity_type: id.name.clone(),
    })
}

/// Whether `scheme` is admissible on `protocol` (the admissible-scheme-per-
/// protocol check). HTTP admits `None` (public routes) and `Bearer` (an
/// `Authorization` header is an HTTP concept); the internal protocols
/// (call/cron/queue) admit `Internal`. `Signature` is still reserved.
pub fn scheme_admissible(protocol: &ServiceProtocol, scheme: Scheme) -> bool {
    match protocol {
        ServiceProtocol::Http => matches!(scheme, Scheme::None | Scheme::Bearer),
        ServiceProtocol::Call | ServiceProtocol::Cron | ServiceProtocol::Queue { .. } => {
            matches!(scheme, Scheme::Internal)
        }
    }
}
