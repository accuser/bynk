//! v0.121 (ADR 0157): the scaffold-compiles test.
//!
//! Each editor scaffold catalogue — the LSP's `completion::SNIPPETS` and VS
//! Code's static `vscode-bynk/snippets/bynk.json` — is lexed and parsed
//! against the current grammar, independently, after stripping snippet
//! tab-stop syntax to a compilable skeleton. A scaffold that no longer
//! compiles fails the build; this is the test that would have caught the
//! retired `test "…" { }` snippet. There is deliberately no cross-catalogue
//! parity assertion (DECISION A, `design/tracks/editor-currency.md`) — each
//! set is checked against the grammar on its own.

mod support;

// bynk-lsp is a binary crate: include the pure module directly (the pattern
// `legend_drift.rs` established). `symbols` satisfies completion's one
// `crate::symbols` reference.
#[allow(dead_code)]
#[path = "../src/completion.rs"]
mod completion;
#[allow(dead_code)]
#[path = "../src/symbols.rs"]
mod symbols;

use std::path::Path;

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

#[test]
fn vscode_snippets_compile() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../vscode-bynk/snippets/bynk.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&text).expect("bynk.json is valid JSON");
    let entries = json.as_object().expect("bynk.json is an object");
    assert!(!entries.is_empty(), "bynk.json has no snippets");

    for (name, entry) in entries {
        let lines = entry["body"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: body is not an array"));
        let body: String = lines
            .iter()
            .map(|l| {
                l.as_str()
                    .unwrap_or_else(|| panic!("{name}: body line is not a string"))
            })
            .collect::<Vec<_>>()
            .join("\n");
        let stripped = support::strip_snippet_placeholders(&body);
        let source = support::wrap_for_parse(name, &stripped);
        assert_scaffold_parses(name, &source);
    }
}
