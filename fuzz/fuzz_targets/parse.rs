//! Fuzz the lexer + parser (#516): the front half every surface shares —
//! the CLI, the LSP (where a panic kills a request), and the in-browser
//! playground (where a panic is a crash).
//!
//! Invariants:
//! - `tokenize` never panics; every token span is in-bounds and char-aligned.
//! - `parse_units` (the bailing CLI path) and `parse_unit_with_recovery`
//!   (the LSP path) never panic; every diagnostic span is in-bounds and
//!   char-aligned.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let check_span = |span: bynk_syntax::span::Span| {
        assert!(
            span.start <= span.end && span.end <= source.len(),
            "span out of bounds: {span:?} in {} bytes",
            source.len()
        );
        assert!(
            source.is_char_boundary(span.start) && source.is_char_boundary(span.end),
            "span splits a codepoint: {span:?}"
        );
    };
    match bynk_syntax::lexer::tokenize(source) {
        Err(e) => check_span(e.span),
        Ok(tokens) => {
            for t in &tokens {
                check_span(t.span);
            }
            match bynk_syntax::parser::parse_units(&tokens, source) {
                Ok(_) => {}
                Err(errors) => {
                    for e in errors {
                        check_span(e.span);
                    }
                }
            }
            let (_unit, errors) = bynk_syntax::parser::parse_unit_with_recovery(&tokens, source);
            for e in errors {
                check_span(e.span);
            }
        }
    }
});
