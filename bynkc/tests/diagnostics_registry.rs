//! Keeps the diagnostic registry, the compiler source, and the generated
//! reference page in lock-step.
//!
//! 1. Every `bynk.*` code used as a string literal in the compiler source must
//!    appear in `bynkc::diagnostics::REGISTRY`, and vice versa. "Compiler
//!    source" now spans two crates: `bynkc/src` and the `bynk-syntax/src` leaf
//!    the syntax foundation (lexer/parser/diagnostics) was extracted into
//!    (crate-decomposition slice 1) — the registry lives in `bynk-syntax`, but
//!    emit sites are split across both crates, so both trees are scanned.
//! 2. `site/src/content/docs/book/reference/diagnostics.md` must match what the
//!    registry renders.
//!
//! Regenerate the docs page after changing the registry with:
//!     BYNK_BLESS=1 cargo test -p bynkc --test diagnostics_registry

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use bynkc::diagnostics::{EXPLANATIONS, REGISTRY, render_markdown};

fn grammar_json() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tree-sitter-bynk/src/grammar.json");
    fs::read_to_string(path).expect("read grammar.json")
}

/// Collect every `"bynk.x.y"` string literal across the compiler source,
/// excluding the registry module itself. Scans every compiler crate, since the
/// decomposition split the emit sites across crate boundaries: `bynkc` (CLI +
/// glue), the `bynk-syntax` leaf (lexer/parser), the `bynk-check` layer
/// (resolver/checker/actors), and `bynk-emit` (emitter/project/validate).
fn codes_used_in_source() -> BTreeSet<String> {
    let re = regex::Regex::new(r#""(bynk\.[a-z_]+\.[a-z_]+)""#).unwrap();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut codes = BTreeSet::new();
    collect(&manifest.join("src"), &re, &mut codes);
    collect(&manifest.join("../bynk-syntax/src"), &re, &mut codes);
    collect(&manifest.join("../bynk-check/src"), &re, &mut codes);
    collect(&manifest.join("../bynk-emit/src"), &re, &mut codes);
    codes
}

fn collect(dir: &Path, re: &regex::Regex, out: &mut BTreeSet<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, re, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            // The registry module deliberately lists every code; skip it so the
            // comparison reflects actual emit sites. `firstparty.rs` is also
            // skipped: its `LOCALE_TYPES_UNIT` constant ("bynk.locale.types")
            // is a firstparty commons name, not a diagnostic code, but
            // happens to share the two-dot "bynk.x.y" shape this regex uses
            // as its heuristic for one.
            if path
                .file_name()
                .is_some_and(|n| n == "diagnostics.rs" || n == "firstparty.rs")
            {
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
fn generated_diagnostics_page_is_up_to_date() {
    let page = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../site/src/content/docs/book/reference/diagnostics.md");
    let rendered = common::to_site_page(&render_markdown(), "reference");

    if std::env::var_os("BYNK_BLESS").is_some() {
        fs::write(&page, &rendered).unwrap();
        return;
    }

    let current = fs::read_to_string(&page).unwrap_or_default();
    assert_eq!(
        current, rendered,
        "site/src/content/docs/book/reference/diagnostics.md is out of date with the registry.\n\
         Regenerate with: BYNK_BLESS=1 cargo test -p bynkc --test diagnostics_registry"
    );
}

// --- #853: `bynk explain` / `codeDescription` explanations ------------------

/// A GitHub-style heading slug: lowercase, non-alphanumerics (bar spaces and
/// hyphens) dropped, spaces collapsed to single hyphens. Sufficient for the
/// simple headings the curated explanations anchor at (the guard fails loudly
/// if a chosen anchor ever needs richer slugging).
fn slug(heading: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in heading.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if (ch == ' ' || ch == '-' || ch == '_') && !out.is_empty() && !last_dash {
            out.push('-');
            last_dash = true;
        }
        // Everything else (backticks, parens, punctuation) is dropped.
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// The `##`-level (and deeper) heading slugs present in a Markdown page.
/// Content inside fenced code blocks is skipped, so a `#`-led line in an example
/// (a shell comment, say) is never mistaken for a heading and can't mask a
/// stale anchor by slugging to the same value.
fn heading_slugs(markdown: &str) -> BTreeSet<String> {
    let mut slugs = BTreeSet::new();
    let mut in_fence = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed.starts_with('#') {
            let s = slug(trimmed.trim_start_matches('#'));
            if !s.is_empty() {
                slugs.insert(s);
            }
        }
    }
    slugs
}

#[test]
fn explanations_reference_real_codes_and_are_sorted() {
    let registered: BTreeSet<&str> = REGISTRY.iter().map(|d| d.code).collect();
    for e in EXPLANATIONS {
        assert!(
            registered.contains(e.code),
            "EXPLANATIONS entry `{}` is not a real diagnostic code in REGISTRY \
             (bynk-syntax/src/diagnostics.rs).",
            e.code
        );
    }
    let codes: Vec<&str> = EXPLANATIONS.iter().map(|e| e.code).collect();
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    assert_eq!(codes, sorted, "EXPLANATIONS must be sorted by code");

    let unique: BTreeSet<&str> = codes.iter().copied().collect();
    assert_eq!(
        unique.len(),
        codes.len(),
        "EXPLANATIONS has a duplicate code"
    );
}

/// The code→page mapping is guarded against Book-page moves and anchor renames:
/// each explanation's target page must exist on disk and, when an anchor is
/// given, a heading with that slug must be present. (The generated diagnostics
/// page also links these in-site, so `astro build`'s link checker is a second,
/// independent guard.)
#[test]
fn explanation_pages_and_anchors_exist() {
    let docs_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../site/src/content/docs");
    for e in EXPLANATIONS {
        assert!(
            e.page.starts_with('/') && !e.page.ends_with('/'),
            "EXPLANATIONS `{}` page `{}` must be a root-relative path with no trailing slash",
            e.code,
            e.page
        );
        // `/book/reference/types` → `<docs>/book/reference/types.md`
        let file = docs_root.join(format!("{}.md", e.page.trim_start_matches('/')));
        let markdown = fs::read_to_string(&file).unwrap_or_else(|_| {
            panic!(
                "EXPLANATIONS `{}` points at Book page `{}`, but {} does not exist. \
                 Update the mapping in bynk-syntax/src/diagnostics.rs if the page moved.",
                e.code,
                e.page,
                file.display()
            )
        });
        if !e.anchor.is_empty() {
            let slugs = heading_slugs(&markdown);
            assert!(
                slugs.contains(e.anchor),
                "EXPLANATIONS `{}` anchors at `#{}`, but {} has no heading with that slug. \
                 Present slugs: {:?}",
                e.code,
                e.anchor,
                file.display(),
                slugs
            );
        }
    }
}
