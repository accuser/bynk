//! `bynk-secrets.json` generation per Worker (v0.172, ADR 0195).
//!
//! The **declared** secret names a context's handlers will read from `env` at
//! runtime — an `actor`'s `auth = Bearer(secret = "…")` / `Signature(secret =
//! "…")`, including the members of a multi-actor sum. `deploy` reads this file
//! to know which secrets it must see set before it pushes.
//!
//! The file carries **two kinds of knowledge, which are not equally strong**
//! (ADR 0196):
//!
//! - `declared` — an `actor`'s auth secret. A literal fixed at parse time,
//!   required at compile time, and **fail-closed**: unset, the Worker answers
//!   401 to every request. `deploy` refuses to ship without a value.
//! - `read` — a literal `bynk.Secrets` name (`Secrets.get("X")`). `get` returns
//!   `Option`, so absence is a legitimate handled outcome — these are
//!   **advisory**, and `deploy` warns rather than failing.
//!
//! And `read_complete`, which is the honesty. `Secrets.get` takes an ordinary
//! `String` expression, so a computed name is invisible to any pass: where one
//! is seen, the context warns (`bynk.secrets.computed_name`) and this flag goes
//! false. `declared` is a **floor, not a census** (ADR 0195 D2); `read` is a
//! census only while `read_complete` holds, and says so when it does not.
//!
//! Why a file rather than an API: the driver has two compile paths, and under a
//! `bynkc` override the compiler is a child process handing back an exit status
//! — there is no in-memory model to consult. A name the compiler knows must
//! reach the driver in the build output, or not at all (ADR 0195 D5).
//!
//! P5.5 (`design/tracks/semantics-in-the-checker.md` §6, §9): the checking
//! half — `SecretReads`, the `Secrets.get` AST walk, and the
//! `bynk.secrets.computed_name` diagnostic itself — moved to
//! `bynk_check::secrets`, a real gap this track's settling pass had not
//! scoped (see that module's own doc). What stays here is emission-only:
//! rendering the manifest `bynk deploy` reads, which is not this crate's
//! business to ask `bynk-check` to do.

use std::collections::BTreeSet;

use bynk_project::json_string;

use bynk_check::actors::SumMemberSeam;
use bynk_check::secrets::SecretReads;

use crate::project::UnitTable;

/// The file the driver reads, beside each Worker's `wrangler.toml`.
pub const SECRETS_MANIFEST: &str = "bynk-secrets.json";

/// The manifest schema version. Bumped only by a breaking shape change; the
/// driver refuses a version it does not know rather than guessing, as the
/// deploy ledger does.
///
/// **2** (v0.173, ADR 0196) added `read` and `read_complete`. The bump is
/// deliberate rather than a default-on-absence read: a v1 manifest carries no
/// evidence either way about computed names, and defaulting `read_complete` to
/// `true` for it would be the manifest's one claim that could be silently wrong.
const MANIFEST_VERSION: u32 = 2;

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
pub(crate) fn declared_secrets(table: &UnitTable) -> BTreeSet<String> {
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

/// The reads half of what a context knows about its own `bynk.Secrets` use —
/// `declared_secrets`'s counterpart, read from the AST rather than derived
/// from actor bindings. The walk itself, its `bynk.secrets.computed_name`
/// diagnostic, and the `reads_secrets_of_bynk` capability-resolution guard
/// now live in [`bynk_check::secrets::secret_reads_of`] (P5.5) — this wrapper
/// calls it qualified rather than duplicating it, discarding the warnings
/// `build_output` has no use for (`run_checks` raises them, via
/// `bynk_check::project_model::phase_secrets_computed_name`, its own caller
/// of the same function).
pub(crate) fn secret_reads(
    table: &UnitTable,
    flattened: &std::collections::HashMap<String, String>,
) -> (SecretReads, Vec<bynk_syntax::CompileError>) {
    bynk_check::secrets::secret_reads_of(
        table.services.values().flat_map(|s| s.handlers.iter()),
        flattened,
    )
}

/// Render the manifest for a context, or `None` when there is nothing to say.
///
/// Emitted when **anything** is known ([DECISION E]): a declared secret, a read
/// name, or the fact that a name is computed. Slice 3's rule — emit only for a
/// non-empty `declared` — would have stayed silent for a context that reads
/// secrets but declares none, which is exactly the context this file now exists
/// to describe.
pub(crate) fn emit_secrets_manifest(table: &UnitTable, reads: &SecretReads) -> Option<String> {
    render(&declared_secrets(table), reads)
}

/// Render a resolved name set. Split from the derivation so the file's shape is
/// tested without building a project model.
///
/// Absent rather than empty: a project with no declared secret must not grow a
/// file into every worker directory for a feature it does not use — and "no
/// file" is the same answer as "an empty list" to a driver that must tolerate a
/// build tree from a compiler predating this file anyway.
fn render(declared: &BTreeSet<String>, reads: &SecretReads) -> Option<String> {
    // `complete` is the third thing worth saying: a context that reads one
    // computed name and nothing else knows something — that it does not know —
    // and the file has to carry it or the driver cannot.
    if declared.is_empty() && reads.names.is_empty() && reads.complete {
        return None;
    }
    // Hand-rendered rather than via serde: this crate does not depend on
    // serde_json, and the shape is three fields.
    Some(format!(
        "{{\n  \"version\": {MANIFEST_VERSION},\n  \"declared\": {},\n  \"read\": {},\n  \"read_complete\": {}\n}}\n",
        json_array(declared),
        json_array(&reads.names),
        reads.complete,
    ))
}

/// A JSON array of names, one per line, or `[]`.
fn json_array(names: &BTreeSet<String>) -> String {
    if names.is_empty() {
        return "[]".to_string();
    }
    let rendered: Vec<String> = names
        .iter()
        .map(|n| format!("    {}", json_string(n)))
        .collect();
    format!("[\n{}\n  ]", rendered.join(",\n"))
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

    fn reads(names: &[&str], complete: bool) -> SecretReads {
        SecretReads {
            names: set(names),
            complete,
        }
    }

    /// [DECISION E]: the file appears when **anything** is known, and only then.
    #[test]
    fn a_manifest_appears_when_anything_is_known_and_not_otherwise() {
        // Nothing at all — no file. A project with no secrets must not grow one
        // into every worker directory.
        assert_eq!(render(&set(&[]), &reads(&[], true)), None);

        // A read with no declared secret is exactly the context slice 3's rule
        // would have stayed silent about — and exactly the one worth describing.
        assert!(render(&set(&[]), &reads(&["API_KEY"], true)).is_some());

        // Knowing that you *don't* know is knowledge too: a context whose only
        // secret is computed still emits, or the driver cannot say its list is
        // incomplete.
        assert!(render(&set(&[]), &reads(&[], false)).is_some());
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
            render(&set(&["B_SECRET", "A_SECRET"]), &reads(&["R"], true))
                .expect("a non-empty set renders"),
            "{\n  \"version\": 2,\n  \"declared\": [\n    \"A_SECRET\",\n    \"B_SECRET\"\n  ],\n  \
             \"read\": [\n    \"R\"\n  ],\n  \"read_complete\": true\n}\n",
        );
        // The empty-list and false-flag shapes, which the fixtures also carry.
        assert_eq!(
            render(&set(&["ONLY"]), &reads(&[], false)).expect("renders"),
            "{\n  \"version\": 2,\n  \"declared\": [\n    \"ONLY\"\n  ],\n  \
             \"read\": [],\n  \"read_complete\": false\n}\n",
        );
    }
}
