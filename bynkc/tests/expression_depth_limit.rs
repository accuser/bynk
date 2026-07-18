//! Regression coverage for the front end's operator-chain depth bound (#714).
//!
//! The parser assembles associative operator chains (`+`, `*`, `&&`, `||`)
//! *iteratively*, so a flat `1 + 1 + … + 1` never overflows the parser and —
//! unlike parenthesised/nested source (#713) — never re-enters `parse_expr`, so
//! it slips past that recursion guard. But it produces an arbitrarily deep
//! left-nested tree that overflows every recursive consumer downstream (the
//! checker's `type_of`, the formatter, the emitter, and the AST's own recursive
//! `Drop`). A `bynkc check` on a 20 000-term chain used to abort with `fatal
//! runtime error: stack overflow` (exit 134) on a valid program.
//!
//! The fix counts each chain fold against the same nesting budget the parser
//! uses for recursive descent (`bynk.parse.nesting_too_deep`), so the tree is
//! never built past the bound and the program is rejected with a clean
//! diagnostic instead. These tests run on an ordinary (small, ~2 MiB) test
//! thread: a chain that was *not* bounded would build the deep tree and overflow
//! this thread, so a passing run is itself the evidence that the tree stays
//! bounded.

/// A single-file commons whose `f` returns an `Int` built from a `+` chain of
/// `terms` ones.
fn chain_source(terms: usize) -> String {
    let chain = vec!["1"; terms].join(" + ");
    format!("commons demo.deep\nfn f() -> Int {{\n  {chain}\n}}\n")
}

#[test]
fn chain_within_the_limit_compiles() {
    // A short chain is a perfectly valid program and must still compile — the
    // bound rejects only genuinely pathological depth.
    let src = chain_source(50);
    assert!(
        bynkc::compile(&src, "within.bynk").is_ok(),
        "a chain shorter than the depth bound should compile",
    );
}

#[test]
fn chain_past_the_limit_is_rejected_not_crashed() {
    // The exact reproduction from #714: a chain far past the bound. The process
    // must survive (no stack overflow) and report the nesting diagnostic.
    let src = chain_source(20_000);
    let errors =
        bynkc::compile(&src, "deep.bynk").expect_err("an over-deep chain must be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.category == "bynk.parse.nesting_too_deep"),
        "expected a bynk.parse.nesting_too_deep diagnostic, got: {:?}",
        errors.iter().map(|e| &e.category).collect::<Vec<_>>(),
    );
}
