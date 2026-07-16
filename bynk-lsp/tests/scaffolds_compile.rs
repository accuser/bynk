//! v0.121 (ADR 0157): the scaffold-compiles test.
//!
//! Each editor scaffold catalogue — the LSP's `completion::SNIPPETS` and VS
//! Code's static `vscode-bynk/snippets/bynk.json` — is lexed and parsed
//! against the current grammar, independently, after stripping snippet
//! tab-stop syntax to a compilable skeleton. A scaffold that no longer
//! compiles fails the build; this is the test that would have caught the
//! retired `test "…" { }` snippet. There is deliberately no cross-catalogue
//! parity assertion (DECISION A, ADR 0157) — each set is checked against the
//! grammar on its own.
//!
//! This file is `exclude`d from the published `bynk-lsp` crate (Cargo.toml) —
//! it reads `../vscode-bynk/snippets/bynk.json`, which is not in the crate
//! tarball, so a standalone `cargo test` on the published crate must not see it.

mod support;

// bynk-lsp is a binary crate: include the pure module directly (the pattern
// `legend_drift.rs` established). `symbols` satisfies completion's one
// `crate::symbols` reference.

use std::path::Path;

use bynk_lsp::completion;
use bynk_syntax::lexer::tokenize;
use bynk_syntax::parser::parse_unit_with_recovery;

fn assert_scaffold_parses(name: &str, source: &str) {
    let tokens = tokenize(source).unwrap_or_else(|e| panic!("{name}: lex error: {e:?}"));
    let (unit, errs) = parse_unit_with_recovery(&tokens, source);
    assert!(
        unit.is_some() && errs.is_empty(),
        "{name}: scaffold no longer parses against the current grammar\n\
         --- skeleton ---\n{source}\n--- errors ---\n{errs:?}"
    );
}

#[test]
fn lsp_snippets_compile() {
    for &(name, body) in completion::SNIPPETS {
        let stripped = support::strip_snippet_placeholders(body);
        let source = support::wrap_for_parse(name, &stripped);
        assert_scaffold_parses(name, &source);
    }
}

/// The VS Code catalogue's `(name, joined-body)` pairs, read from disk.
fn vscode_snippets() -> Vec<(String, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../vscode-bynk/snippets/bynk.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&text).expect("bynk.json is valid JSON");
    let entries = json.as_object().expect("bynk.json is an object");
    assert!(!entries.is_empty(), "bynk.json has no snippets");

    entries
        .iter()
        .map(|(name, entry)| {
            let lines = entry["body"]
                .as_array()
                .unwrap_or_else(|| panic!("{name}: body is not an array"));
            let body = lines
                .iter()
                .map(|l| {
                    l.as_str()
                        .unwrap_or_else(|| panic!("{name}: body line is not a string"))
                })
                .collect::<Vec<_>>()
                .join("\n");
            (name.clone(), body)
        })
        .collect()
}

#[test]
fn vscode_snippets_compile() {
    for (name, body) in vscode_snippets() {
        let stripped = support::strip_snippet_placeholders(&body);
        let source = support::wrap_for_parse(&name, &stripped);
        assert_scaffold_parses(&name, &source);
    }
}

/// v0.125 (slice 4): the scaffold refresh's own coverage floor. Every language
/// construct the slice set out to de-stale must appear in at least one scaffold
/// across the *union* of the two catalogues — this is not the per-set parity
/// DECISION A forbids, only proof the fill happened and cannot silently regress.
#[test]
fn catalogues_cover_the_refreshed_constructs() {
    // Distinctive keyword tokens introduced by slice 4's fill (the refined-type
    // fill is witnessed by `where`, the opaque fill by `opaque`).
    const CONSTRUCTS: &[&str] = &[
        "suite",
        "case",
        "property",
        "expect",
        "for",
        "actor",
        "invariant",
        "transition",
        "requires",
        "ensures",
        "given",
        "opaque",
        "where",
        "uses",
        "consumes",
        "adapter",
    ];

    // The union of every scaffold body's word tokens, both catalogues.
    let mut tokens = std::collections::HashSet::new();
    let mut add = |body: &str| {
        for tok in body.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if !tok.is_empty() {
                tokens.insert(tok.to_string());
            }
        }
    };
    for &(_, body) in completion::SNIPPETS {
        add(body);
    }
    for (_, body) in vscode_snippets() {
        add(&body);
    }

    for kw in CONSTRUCTS {
        assert!(
            tokens.contains(*kw),
            "no scaffold in either catalogue exercises `{kw}` — the slice-4 refresh regressed"
        );
    }
}
