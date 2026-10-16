//! #1969: the cross-context wire format, version 1, as a published surface.
//!
//! The interop track's D1 (`design/tracks/interop.md` §8) makes the format a
//! caller outside the project can speak a contract with its own version,
//! apart from the compiler's: the path, method and headers of a call; the
//! order of the callee's checks, with their status codes and error bodies; the
//! JSON encoding of each boundary type; and the contract hash.
//!
//! This module is the single source for the parts that are constants. The
//! emitter builds the `/_bynk/call/` dispatch from [`CallCheck`] and the header
//! names below, and [`reference`] renders `reference/wire-format.md` from the
//! same values plus [`crate::wire`]'s encoding model and [`crate::contract`]'s
//! hash. `bynkc/tests/wire_reference.rs` holds the page, the emitted dispatch,
//! the runtime's `callService` and the generated codecs to it.
//!
//! **Stability (version 1).** Additive only: a new status code, a new error
//! `kind` or a new optional header may be added within the version. Changing an
//! existing encoding, check, status or the hash's normal form is a new wire
//! version, which a callee accepts beside the old one for an overlap window
//! ([`ACCEPTED_WIRE_VERSIONS`]). The one exception, until the interop track's
//! slice I2 lands, is the authentication credential: see [`CALL_SECRET_HEADER`].

pub mod reference;

/// The wire version this compiler's callers speak.
pub const WIRE_VERSION: u32 = 1;

/// The wire versions this compiler's callees accept. A request without
/// [`WIRE_HEADER`] is version 1.
pub const ACCEPTED_WIRE_VERSIONS: &[u32] = &[1];

/// The path prefix a call is addressed to; the service name follows it.
pub const CALL_PATH_PREFIX: &str = "/_bynk/call/";

/// The only method a call may use.
pub const CALL_METHOD: &str = "POST";

/// The caller's compiled hash of the callee's contract.
pub const CONTRACT_HEADER: &str = "X-Bynk-Contract";

/// The calling context's qualified name.
pub const CALLER_HEADER: &str = "X-Bynk-Caller";

/// The wire version the caller speaks; absent means 1.
pub const WIRE_HEADER: &str = "X-Bynk-Wire";

/// #2004: the deployment's shared call secret.
///
/// Version 1 freezes *where* authentication sits in the check order and what a
/// refusal looks like (a bare `401`), but not the credential itself: the
/// interop track's slice I2 replaces this shared secret with a per-edge
/// signature, and that swap is not a wire-version change. No non-Bynk caller
/// can hold this secret, so nothing outside a deployment depends on it.
pub const CALL_SECRET_HEADER: &str = "X-Bynk-Call-Secret";

/// The `details` of a `MalformedJson` refusal. The HTTP routes use the same.
pub const MALFORMED_JSON_DETAILS: &str = "Invalid request body";

/// One header of a call request, as the reference documents it.
pub struct Header {
    pub name: &'static str,
    pub required: &'static str,
    pub meaning: &'static str,
}

/// Every header a call carries, in the order the reference lists them.
pub const HEADERS: &[Header] = &[
    Header {
        name: CALL_SECRET_HEADER,
        required: "yes",
        meaning: "The deployment's call secret (`BYNK_CALL_SECRET`). Compared in constant time. Its value, and this header, are replaced by a per-edge signature before non-Bynk callers are supported; see *Authentication* below.",
    },
    Header {
        name: CONTRACT_HEADER,
        required: "yes",
        meaning: "The caller's hash of the service's contract, 16 lowercase hex digits. Must equal the callee's; a missing header does not.",
    },
    Header {
        name: CALLER_HEADER,
        required: "when the handler binds `by c: Caller`",
        meaning: "The calling context's qualified name (`shop.orders`), which the handler reads as `c.identity`. Not authenticated beyond the call secret.",
    },
    Header {
        name: WIRE_HEADER,
        required: "no",
        meaning: "The wire version the caller speaks, as a decimal integer. Absent means `1`.",
    },
    Header {
        name: "content-type",
        required: "no",
        meaning: "Bynk callers send `application/json`. The callee does not check it.",
    },
];

/// One check the callee makes on a call, in the published order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallCheck {
    Method,
    Authentication,
    WireVersion,
    UnknownService,
    Contract,
    MalformedJson,
    MissingCaller,
    Arguments,
}

/// The callee's checks, in the order it makes them. The first that fails
/// answers the request.
pub const CALL_CHECKS: [CallCheck; 8] = [
    CallCheck::Method,
    CallCheck::Authentication,
    CallCheck::WireVersion,
    CallCheck::UnknownService,
    CallCheck::Contract,
    CallCheck::MalformedJson,
    CallCheck::MissingCaller,
    CallCheck::Arguments,
];

impl CallCheck {
    /// The status a failure of this check answers with.
    pub fn status(self) -> u16 {
        match self {
            CallCheck::Method => 405,
            CallCheck::Authentication => 401,
            CallCheck::WireVersion => 400,
            CallCheck::UnknownService => 404,
            CallCheck::Contract => 409,
            CallCheck::MalformedJson => 400,
            CallCheck::MissingCaller => 401,
            CallCheck::Arguments => 400,
        }
    }

    /// The `kind` of the JSON error body, for a check that answers with one.
    /// `Arguments` answers with the codec's own `StructuralMismatch` or
    /// `RefinementViolation`, so it has no single kind.
    pub fn kind(self) -> Option<&'static str> {
        match self {
            CallCheck::WireVersion => Some("UnsupportedWireVersion"),
            CallCheck::Contract => Some("ContractMismatch"),
            CallCheck::MalformedJson => Some("MalformedJson"),
            CallCheck::MissingCaller => Some("Unauthorized"),
            CallCheck::Method
            | CallCheck::Authentication
            | CallCheck::UnknownService
            | CallCheck::Arguments => None,
        }
    }

    /// The plain-text body of a check that answers with one.
    pub fn text_body(self) -> Option<&'static str> {
        match self {
            CallCheck::Authentication => Some("Unauthorized"),
            CallCheck::UnknownService => Some("Not found"),
            _ => None,
        }
    }

    /// What the check tests, for the reference.
    pub fn condition(self) -> &'static str {
        match self {
            CallCheck::Method => "The method is `POST`.",
            CallCheck::Authentication => "The call secret matches the callee's own.",
            CallCheck::WireVersion => {
                "`X-Bynk-Wire` is absent or names a version the callee accepts."
            }
            CallCheck::UnknownService => {
                "The path names a service of the callee's context with an `on call` handler."
            }
            CallCheck::Contract => "`X-Bynk-Contract` equals the callee's hash of that service.",
            CallCheck::MalformedJson => "The body parses as JSON.",
            CallCheck::MissingCaller => {
                "`X-Bynk-Caller` is present and non-empty. Checked only when the handler binds `by c: Caller`."
            }
            CallCheck::Arguments => {
                "The body decodes as the handler's parameters (see *Encoding*)."
            }
        }
    }

    /// The response a failure gives, for the reference.
    pub fn response(self) -> &'static str {
        match self {
            CallCheck::Method => "empty body, `Allow: POST`",
            CallCheck::Authentication => "text `Unauthorized`",
            CallCheck::WireVersion => r#"`{"kind":"UnsupportedWireVersion","accepted":[1]}`"#,
            CallCheck::UnknownService => "text `Not found`",
            CallCheck::Contract => {
                r#"`{"kind":"ContractMismatch","service":"…","expected":"…","actual":"…"}`"#
            }
            CallCheck::MalformedJson => {
                r#"`{"kind":"MalformedJson","details":"Invalid request body"}`"#
            }
            CallCheck::MissingCaller => {
                r#"`{"kind":"Unauthorized","details":"missing caller identity"}`"#
            }
            CallCheck::Arguments => "a `StructuralMismatch` or `RefinementViolation` body",
        }
    }
}
