//! `bynk-contracts.json` generation per Worker (v0.177, #643).
//!
//! The contract hash a context's Worker entry compares an incoming
//! `X-Bynk-Contract` against, per `on call` service — the same constants the
//! entry stamps, written where the **driver** can read them.
//!
//! Why the driver needs them: the runtime check fails closed, but it does so in
//! *production*, on live traffic, after the skewed pair is already deployed.
//! `deploy` can know sooner. It records each context's hashes when it pushes,
//! so a later `deploy --context A` can compare A's compiled view of its
//! dependencies against what those dependencies actually have live, and refuse
//! before the push rather than let requests 409 (ADR 0193's D4 gate already
//! checks a dependency *exists*; this extends it to *matches*).
//!
//! Why a file rather than an API: the driver has two compile paths, and under a
//! `bynkc` override the compiler is a child process handing back an exit status
//! — there is no in-memory model to consult. A fact the compiler knows must
//! reach the driver in the build output, or not at all (ADR 0195 D5, the same
//! reasoning as `bynk-secrets.json`).

use std::collections::BTreeMap;

/// The file the driver reads, beside each Worker's `wrangler.toml`.
pub const CONTRACTS_MANIFEST: &str = "bynk-contracts.json";

/// The manifest schema version. Bumped only by a breaking shape change; the
/// driver refuses a version it does not know rather than guessing, as the
/// deploy ledger and the secrets manifest do.
pub const MANIFEST_VERSION: u32 = 1;

/// Render a context's contract manifest: what it **provides**, and what it
/// **expects** of each context it consumes.
///
/// Both halves are needed, and they are not the same fact:
///
/// - `provides` is this context's own constant per `on call` service — what its
///   entry enforces. `deploy` records it when the Worker is pushed, so the
///   ledger knows what is *live*.
/// - `expects` is this context's compiled view of each dependency's contract —
///   the constant it stamps at each call site. `deploy` compares it against the
///   ledger's `provides` for that dependency, which is exactly the runtime check
///   moved earlier.
///
/// A gate built on `provides` alone could not work: it would compare a context
/// against itself.
///
/// Absent rather than empty when a context neither exposes nor calls an `on
/// call` service: a project that never crosses a context boundary must not grow
/// a file into every worker directory for a feature it does not use — and "no
/// file" is the same answer as "empty" to a driver that must tolerate a build
/// tree from a compiler predating this file anyway.
pub fn emit_contracts_manifest(
    provides: &BTreeMap<String, String>,
    expects: &BTreeMap<String, BTreeMap<String, String>>,
) -> Option<String> {
    if provides.is_empty() && expects.is_empty() {
        return None;
    }
    // Hand-rendered rather than via serde: this crate does not depend on
    // serde_json, and the shape is three fields.
    let expects_body: Vec<String> = expects
        .iter()
        .map(|(ctx, svcs)| {
            let inner: Vec<String> = svcs
                .iter()
                .map(|(svc, h)| format!("      {}: {}", json_string(svc), json_string(h)))
                .collect();
            format!(
                "    {}: {{\n{}\n    }}",
                json_string(ctx),
                inner.join(",\n")
            )
        })
        .collect();
    Some(format!(
        "{{\n  \"version\": {MANIFEST_VERSION},\n  \"provides\": {},\n  \"expects\": {}\n}}\n",
        json_map(provides, 2),
        if expects_body.is_empty() {
            "{}".to_string()
        } else {
            format!("{{\n{}\n  }}", expects_body.join(",\n"))
        }
    ))
}

fn json_map(m: &BTreeMap<String, String>, indent: usize) -> String {
    if m.is_empty() {
        return "{}".to_string();
    }
    let pad = " ".repeat(indent + 2);
    let close = " ".repeat(indent);
    let entries: Vec<String> = m
        .iter()
        .map(|(k, v)| format!("{pad}{}: {}", json_string(k), json_string(v)))
        .collect();
    format!("{{\n{}\n{close}}}", entries.join(",\n"))
}

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
            c if (c as u32) < 0x20 => {
                let _ = std::fmt::Write::write_fmt(&mut out, format_args!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_when_a_context_neither_provides_nor_consumes() {
        assert_eq!(
            emit_contracts_manifest(&BTreeMap::new(), &BTreeMap::new()),
            None
        );
    }

    #[test]
    fn renders_sorted_service_hashes() {
        let mut h = BTreeMap::new();
        h.insert("whoami".to_string(), "317bdd3de84d2176".to_string());
        h.insert("ask".to_string(), "0011223344556677".to_string());
        let out = emit_contracts_manifest(&h, &BTreeMap::new()).unwrap();
        assert!(out.contains("\"version\": 1"));
        // BTreeMap ordering: the file is byte-stable across builds, which the
        // golden fixtures depend on.
        let ask = out.find("\"ask\"").unwrap();
        let whoami = out.find("\"whoami\"").unwrap();
        assert!(ask < whoami, "services render in sorted order:\n{out}");
    }

    #[test]
    fn a_consumer_records_what_it_expects_of_each_dependency() {
        let mut inner = BTreeMap::new();
        inner.insert("whoami".to_string(), "317bdd3de84d2176".to_string());
        let mut expects = BTreeMap::new();
        expects.insert("app.b".to_string(), inner);
        let out = emit_contracts_manifest(&BTreeMap::new(), &expects).unwrap();
        assert!(out.contains("\"app.b\""), "{out}");
        assert!(out.contains("\"whoami\": \"317bdd3de84d2176\""), "{out}");
        assert!(out.contains("\"provides\": {}"), "{out}");
    }
}
