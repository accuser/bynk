//! `bynk-secrets.json` generation per Worker (v0.172, ADR 0195).
//!
//! The **declared** secret names a context's handlers will read from `env` at
//! runtime — an `actor`'s `auth = Bearer(secret = "…")` / `Signature(secret =
//! "…")`, including the members of a multi-actor sum. `deploy` reads this file
//! to know which secrets it must see set before it pushes.
//!
//! This is a **floor, not a census** (ADR 0195 D2). It carries exactly the names
//! the compiler can *prove* a Worker reads, and nothing else: `bynk.Secrets`
//! reads its name from a runtime `String` expression, so those names are not
//! derivable and are the user's to supply. A reader must not take an absent name
//! for a secret the context does not need.
//!
//! Why a file rather than an API: the driver has two compile paths, and under a
//! `bynkc` override the compiler is a child process handing back an exit status
//! — there is no in-memory model to consult. A name the compiler knows must
//! reach the driver in the build output, or not at all (ADR 0195 D5).

use std::collections::BTreeSet;

use bynk_check::actors::SumMemberSeam;

use crate::project::UnitTable;

/// The file the driver reads, beside each Worker's `wrangler.toml`.
pub const SECRETS_MANIFEST: &str = "bynk-secrets.json";

/// The manifest schema version. Bumped only by a breaking shape change; the
/// driver refuses a version it does not know rather than guessing, as the
/// deploy ledger does.
const MANIFEST_VERSION: u32 = 1;

/// Every secret name this context's handlers will read from `env`.
///
/// Enumerated over exactly the handlers the entry emitter lowers seams for
/// (`table.services`, which is where a `from websocket` service lives too), and
/// resolved with exactly the same `bynk_check::actors` functions — so the
/// manifest cannot describe a Worker other than the one emitted beside it. An
/// actor that is declared but named by no handler's `by` clause resolves no
/// seam and contributes nothing: the Worker never reads it.
///
/// `Oidc` names no secret — its trust root is the provider's published JWKS, not
/// a shared value — and a sum's `None` member (a catch-all such as `Visitor`)
/// verifies nothing. Both are skipped rather than defaulted: inventing a name
/// for them would ask the user to set a secret that nothing reads.
pub fn declared_secrets(table: &UnitTable) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for handler in table.services.values().flat_map(|s| s.handlers.iter()) {
        if let Some(seam) = bynk_check::actors::bearer_seam_for(handler, &table.actors) {
            names.insert(seam.secret);
        }
        if let Some(seam) = bynk_check::actors::signature_seam_for(handler, &table.actors) {
            names.insert(seam.secret);
        }
        for member in bynk_check::actors::sum_members_for(handler, &table.actors)
            .into_iter()
            .flatten()
        {
            match member.seam {
                SumMemberSeam::Bearer { secret, .. } => {
                    names.insert(secret);
                }
                SumMemberSeam::Signature(seam) => {
                    names.insert(seam.secret);
                }
                SumMemberSeam::None => {}
            }
        }
    }
    names
}

/// Render the manifest for a context, or `None` when it declares no secret.
pub fn emit_secrets_manifest(table: &UnitTable) -> Option<String> {
    render(&declared_secrets(table))
}

/// Render a resolved name set. Split from the derivation so the file's shape is
/// tested without building a project model.
///
/// Absent rather than empty: a project with no declared secret must not grow a
/// file into every worker directory for a feature it does not use — and "no
/// file" is the same answer as "an empty list" to a driver that must tolerate a
/// build tree from a compiler predating this file anyway.
fn render(declared: &BTreeSet<String>) -> Option<String> {
    if declared.is_empty() {
        return None;
    }
    // Hand-rendered rather than via serde: this crate does not depend on
    // serde_json, and the shape is two fields.
    let names: Vec<String> = declared.iter().map(|n| json_string(n)).collect();
    Some(format!(
        "{{\n  \"version\": {MANIFEST_VERSION},\n  \"declared\": [\n    {}\n  ]\n}}\n",
        names.join(",\n    ")
    ))
}

/// Escape a secret name as a JSON string literal.
///
/// A name reaches here from a Bynk `StrLit`, so as far as this file is concerned
/// it is arbitrary text. Escaping the two structural characters plus the control
/// range is what keeps the manifest parseable rather than merely
/// usually-parseable.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn a_name_is_escaped_as_a_json_string() {
        assert_eq!(json_string("AUTH_JWT_SECRET"), "\"AUTH_JWT_SECRET\"");
        // A secret name is a Bynk string literal, so it is arbitrary text; the
        // manifest must stay parseable rather than usually-parseable.
        assert_eq!(json_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(json_string("a\nb"), "\"a\\nb\"");
        assert_eq!(json_string("a\u{1}b"), "\"a\\u0001b\"");
    }

    #[test]
    fn a_context_declaring_nothing_gets_no_file_rather_than_an_empty_one() {
        assert_eq!(render(&set(&[])), None);
    }

    /// The committed shape, byte for byte — it is a file a reviewer reads in a
    /// fixture diff and a driver parses. Asserted here rather than round-tripped
    /// through a parser because this crate deliberately carries three
    /// dependencies and `serde_json` is not among them; that the bytes *parse*
    /// is asserted driver-side, where the reader lives.
    #[test]
    fn the_manifest_is_sorted_and_pinned() {
        // A `BTreeSet` orders the names, so the file is byte-stable for a given
        // context rather than dependent on handler iteration order.
        assert_eq!(
            render(&set(&["B_SECRET", "A_SECRET"])).expect("a non-empty set renders"),
            "{\n  \"version\": 1,\n  \"declared\": [\n    \"A_SECRET\",\n    \"B_SECRET\"\n  ]\n}\n",
        );
        assert_eq!(
            render(&set(&["ONLY"])).expect("renders"),
            "{\n  \"version\": 1,\n  \"declared\": [\n    \"ONLY\"\n  ]\n}\n",
        );
    }
}
