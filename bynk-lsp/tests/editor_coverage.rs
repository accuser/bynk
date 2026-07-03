//! v0.121 (ADR 0156): the editor-currency coverage test.
//!
//! Every lowercase-initial `bynk-syntax::keywords::KEYWORDS` entry must have
//! a completion doc *and* a hover path. This is the tooth that would have
//! caught the testing-track keywords (`requires`/`ensures`/`transition`/
//! `expect`/…) landing with no hover coverage — a new keyword with no hover
//! path now fails a test, not a silent gap. The semantic-token legend clause
//! of ADR 0156 is already covered by `legend_drift.rs`; not duplicated here.

// bynk-lsp is a binary crate: include the pure modules directly (the pattern
// `legend_drift.rs` established). `symbols` satisfies completion's one
// `crate::symbols` reference.
#[allow(dead_code)]
#[path = "../src/completion.rs"]
mod completion;
#[allow(dead_code)]
#[path = "../src/symbols.rs"]
mod symbols;

use bynk_syntax::keywords::KEYWORDS;

fn lowercase_keywords() -> impl Iterator<Item = &'static str> {
    KEYWORDS
        .iter()
        .map(|k| k.word)
        .filter(|w| w.chars().next().is_some_and(char::is_lowercase))
}

#[test]
fn every_lowercase_keyword_has_a_completion_doc() {
    for word in lowercase_keywords() {
        assert!(
            completion::keyword_doc(word).is_some(),
            "{word}: no completion doc in the `keywords` registry"
        );
    }
}

#[test]
fn every_lowercase_keyword_has_a_hover_path() {
    for word in lowercase_keywords() {
        // A minimal fixture with the keyword as a bare token — enough to
        // exercise `describe_keyword_at`'s lexical fallback without needing a
        // syntactically valid surrounding construct (hover works mid-edit).
        let source = format!("context Scaffold\n\n{word}");
        let offset = source.rfind(word).expect("word is in the fixture");
        assert!(
            symbols::describe_keyword_at(&source, offset).is_some(),
            "{word}: no hover path — describe_keyword_at returned None"
        );
    }
}
