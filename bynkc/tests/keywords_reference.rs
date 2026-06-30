//! Keeps the keyword registry, the lexer, and the generated reference page in
//! lock-step.
//!
//! 1. The alphabetic `#[token("…")]` keywords in `lexer.rs` must match exactly
//!    `bynkc::keywords::KEYWORDS`.
//! 2. `site/src/content/docs/book/reference/keywords.md` must match what the
//!    registry renders.
//!
//! Regenerate the docs page with:
//!     BYNK_BLESS=1 cargo test -p bynkc --test keywords_reference

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use bynkc::keywords::{KEYWORDS, render_markdown};

/// Alphabetic keyword tokens declared in the lexer via `#[token("…")]`. The
/// lexer now lives in the `bynk-syntax` leaf (crate-decomposition slice 1), so
/// this reads across the crate boundary.
fn keywords_in_lexer() -> BTreeSet<String> {
    let lexer = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../bynk-syntax/src/lexer.rs");
    let text = fs::read_to_string(&lexer).unwrap();
    let re = regex::Regex::new(r#"#\[token\("([a-zA-Z][a-zA-Z_]*)"\)\]"#).unwrap();
    re.captures_iter(&text).map(|c| c[1].to_string()).collect()
}

fn registry_keywords() -> BTreeSet<String> {
    KEYWORDS.iter().map(|k| k.word.to_string()).collect()
}

#[test]
fn registry_is_sorted_with_no_duplicates() {
    let words: Vec<&str> = KEYWORDS.iter().map(|k| k.word).collect();
    let mut sorted = words.clone();
    sorted.sort_unstable();
    assert_eq!(words, sorted, "KEYWORDS must be sorted by word");

    let unique: BTreeSet<&str> = words.iter().copied().collect();
    assert_eq!(unique.len(), words.len(), "KEYWORDS contains duplicates");
}

#[test]
fn registry_matches_lexer_tokens() {
    let lexed = keywords_in_lexer();
    let registered = registry_keywords();

    let missing: Vec<&String> = lexed.difference(&registered).collect();
    let extra: Vec<&String> = registered.difference(&lexed).collect();

    assert!(
        missing.is_empty(),
        "keywords in lexer.rs but missing from bynkc::keywords::KEYWORDS: {missing:#?}"
    );
    assert!(
        extra.is_empty(),
        "keywords in KEYWORDS no longer declared in lexer.rs: {extra:#?}"
    );
}

#[test]
fn generated_keywords_page_is_up_to_date() {
    let page = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../site/src/content/docs/book/reference/keywords.md");
    let rendered = common::to_site_page(&render_markdown(), "reference");

    if std::env::var_os("BYNK_BLESS").is_some() {
        fs::write(&page, &rendered).unwrap();
        return;
    }

    let current = fs::read_to_string(&page).unwrap_or_default();
    assert_eq!(
        current, rendered,
        "site/src/content/docs/book/reference/keywords.md is out of date.\n\
         Regenerate with: BYNK_BLESS=1 cargo test -p bynkc --test keywords_reference"
    );
}
