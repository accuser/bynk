//! Keeps the diagnostic registry, the compiler source, and the generated
//! reference page in lock-step.
//!
//! 1. Every `bynk.*` code used as a string literal in the compiler source must
//!    appear in `bynkc::diagnostics::REGISTRY`, and vice versa. "Compiler
//!    source" now spans two crates: `bynkc/src` and the `bynk-syntax/src` leaf
//!    the syntax foundation (lexer/parser/diagnostics) was extracted into
//!    (crate-decomposition slice 1) — the registry lives in `bynk-syntax`, but
//!    emit sites are split across both crates, so both trees are scanned.
//! 2. `docs/src/reference/diagnostics.md` must match what the registry renders.
//!
//! Regenerate the docs page after changing the registry with:
//!     BYNK_BLESS=1 cargo test -p bynkc --test diagnostics_registry

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use bynkc::diagnostics::{REGISTRY, render_grammar_semantics_json, render_markdown};

fn grammar_json() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tree-sitter-bynk/src/grammar.json");
    fs::read_to_string(path).expect("read grammar.json")
}

/// Collect every `"bynk.x.y"` string literal across the compiler source,
/// excluding the registry module itself. Scans all three compiler crates, since
/// the decomposition split the emit sites across crate boundaries: `bynkc`
/// (emitter/project emit sites), the `bynk-syntax` leaf (lexer/parser), and the
/// `bynk-check` layer (resolver/checker/actors emit sites).
fn codes_used_in_source() -> BTreeSet<String> {
    let re = regex::Regex::new(r#""(bynk\.[a-z_]+\.[a-z_]+)""#).unwrap();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut codes = BTreeSet::new();
    collect(&manifest.join("src"), &re, &mut codes);
    collect(&manifest.join("../bynk-syntax/src"), &re, &mut codes);
    collect(&manifest.join("../bynk-check/src"), &re, &mut codes);
    codes
}

fn collect(dir: &Path, re: &regex::Regex, out: &mut BTreeSet<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, re, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            // The registry module deliberately lists every code; skip it so the
            // comparison reflects actual emit sites.
            if path.file_name().is_some_and(|n| n == "diagnostics.rs") {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap();
            for caps in re.captures_iter(&text) {
                out.insert(caps[1].to_string());
            }
        }
    }
}

fn registry_codes() -> BTreeSet<String> {
    REGISTRY.iter().map(|d| d.code.to_string()).collect()
}

#[test]
fn registry_has_no_duplicates_and_is_sorted() {
    let codes: Vec<&str> = REGISTRY.iter().map(|d| d.code).collect();
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    assert_eq!(codes, sorted, "REGISTRY must be sorted by code");

    let unique: BTreeSet<&str> = codes.iter().copied().collect();
    assert_eq!(
        unique.len(),
        codes.len(),
        "REGISTRY contains duplicate codes"
    );
}

#[test]
fn registry_matches_codes_used_in_source() {
    let used = codes_used_in_source();
    let registered = registry_codes();

    let missing: Vec<&String> = used.difference(&registered).collect();
    let extra: Vec<&String> = registered.difference(&used).collect();

    assert!(
        missing.is_empty(),
        "codes emitted in source but missing from bynkc::diagnostics::REGISTRY: {missing:#?}\n\
         Add an entry for each in bynk-syntax/src/diagnostics.rs."
    );
    assert!(
        extra.is_empty(),
        "codes in REGISTRY that are no longer used in source: {extra:#?}\n\
         Remove them from bynk-syntax/src/diagnostics.rs."
    );
}

#[test]
fn grammar_symbols_are_embeddable_rules() {
    let grammar = grammar_json();
    // An *embeddable* rule has a `{{#grammar}}` entry (and `#rule-<raw>` anchor)
    // in grammar.md, so the diagnostics `Construct` deep-link resolves. This is
    // stricter than "a real rule": a collapsed trivial wrapper has no entry.
    let embeddable: BTreeSet<String> = bynk_grammar::embeddable_rules(&grammar)
        .into_iter()
        .collect();
    for info in REGISTRY {
        for sym in info.grammar_symbol {
            assert!(
                embeddable.contains(*sym),
                "diagnostic `{}` maps to `{sym}`, which is not an embeddable grammar rule \
                 (it needs a `{{#grammar {sym}}}` entry/anchor in grammar.md; a collapsed \
                 trivial wrapper has none). Fix the grammar_symbol in bynk-syntax/src/diagnostics.rs.",
                info.code
            );
        }
    }
}

#[test]
fn generated_grammar_semantics_json_is_up_to_date() {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/grammar-semantics.json");
    let rendered = render_grammar_semantics_json();

    if std::env::var_os("BYNK_BLESS").is_some() {
        fs::write(&file, &rendered).unwrap();
        return;
    }

    let current = fs::read_to_string(&file).unwrap_or_default();
    assert_eq!(
        current, rendered,
        "docs/grammar-semantics.json is out of date with the registry.\n\
         Regenerate with: BYNK_BLESS=1 cargo test -p bynkc --test diagnostics_registry"
    );
}

#[test]
fn generated_diagnostics_page_is_up_to_date() {
    let page =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/src/reference/diagnostics.md");
    let rendered = render_markdown();

    if std::env::var_os("BYNK_BLESS").is_some() {
        fs::write(&page, &rendered).unwrap();
        return;
    }

    let current = fs::read_to_string(&page).unwrap_or_default();
    assert_eq!(
        current, rendered,
        "docs/src/reference/diagnostics.md is out of date with the registry.\n\
         Regenerate with: BYNK_BLESS=1 cargo test -p bynkc --test diagnostics_registry"
    );
}
