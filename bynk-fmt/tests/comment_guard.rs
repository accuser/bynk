//! #523: the comment-loss guard. Trivia attaches at declaration/statement
//! granularity, so a comment inside an expression subtree can vanish from the
//! formatted output. `format_source` must refuse (a visible no-op with a
//! `bynk.fmt.comment_loss` diagnostic) rather than write output that lost
//! user text.

use bynk_fmt::{FormatOptions, format_source};
use bynk_syntax::lexer::tokenize;
use bynk_syntax::parser::parse_unit_with_warnings;

fn expect_refusal(name: &str, source: &str) {
    match format_source(source, &FormatOptions::default()) {
        Ok(out) => {
            // If the formatter has since learned to preserve this placement,
            // the guard must not have fired — but the comment must be there.
            assert!(
                out.contains("keep me"),
                "{name}: formatted without refusing, but the comment vanished:\n{out}"
            );
        }
        Err(e) => {
            assert_eq!(e.errors.len(), 1, "{name}: expected one diagnostic");
            assert_eq!(
                e.errors[0].category, "bynk.fmt.comment_loss",
                "{name}: wrong category: {}",
                e.errors[0].category
            );
        }
    }
}

#[test]
fn refuses_to_drop_a_comment_between_binop_operands() {
    expect_refusal(
        "mid-binop",
        "commons demo\n\nfn f(x: Int) -> Int {\n  x +\n  -- keep me\n  1\n}\n",
    );
}

#[test]
fn refuses_to_drop_a_comment_inside_a_record_literal() {
    expect_refusal(
        "in-record",
        "commons demo\n\ntype P = { x: Int, y: Int }\n\nfn f() -> P {\n  P {\n    x: 1,\n    -- keep me\n    y: 2,\n  }\n}\n",
    );
}

#[test]
fn refuses_to_drop_a_comment_inside_a_match() {
    expect_refusal(
        "in-match",
        "commons demo\n\nfn f(o: Option[Int]) -> Int {\n  match o {\n    -- keep me\n    Some(n) => n,\n    None => 0,\n  }\n}\n",
    );
}

#[test]
fn refuses_to_drop_a_comment_inside_a_list_literal() {
    expect_refusal(
        "in-list",
        "commons demo\n\nfn f() -> List[Int] {\n  [\n    1,\n    -- keep me\n    2,\n  ]\n}\n",
    );
}

#[test]
fn refuses_to_drop_a_comment_after_the_tail_expression() {
    expect_refusal(
        "after-tail",
        "commons demo\n\nfn f() -> Int {\n  let x = 1\n  x\n  -- keep me\n}\n",
    );
}

/// Statement-level comments are the supported placement — the guard must not
/// fire on a file the formatter handles correctly, including when formatting
/// moves the comment (leading-trivia normalisation) without losing it.
#[test]
fn statement_level_comments_format_without_refusal() {
    let source = "commons demo\n\n-- module comment\nfn f(x: Int) -> Int {\n  -- leading comment\n  let y = x -- trailing comment\n  y\n}\n";
    let out = format_source(source, &FormatOptions::default()).expect("must format");
    for needle in ["module comment", "leading comment", "trailing comment"] {
        assert!(out.contains(needle), "lost `{needle}`:\n{out}");
    }
    // And the guard's contract holds transitively: formatting the output
    // again is loss-free and stable.
    let again = format_source(&out, &FormatOptions::default()).expect("must reformat");
    assert_eq!(out, again, "formatting must be idempotent");
}

/// #1664, #1756: a `---` block separated from the next declaration by a blank
/// line, or with nothing after it, is an orphan. The formatter once deleted it,
/// then (#1664) refused the file; it now keeps the block where it was. The
/// output must reformat to itself and still hold the orphan: formatting must
/// not attach it to the declaration below.
fn expect_kept(name: &str, source: &str) {
    let opts = FormatOptions::default();
    let out = format_source(source, &opts)
        .unwrap_or_else(|e| panic!("{name}: refused: {}", e.errors[0].message));
    assert!(out.contains("keep me"), "{name}: lost the block:\n{out}");
    let again = format_source(&out, &opts).expect("reformats");
    assert_eq!(out, again, "{name}: not idempotent");
    assert_eq!(
        orphan_warnings(&out),
        orphan_warnings(source),
        "{name}: formatting changed which blocks are orphans:\n{out}"
    );
}

fn orphan_warnings(source: &str) -> usize {
    let tokens = tokenize(source).expect("tokenises");
    let (_, warnings) = parse_unit_with_warnings(&tokens, source).expect("parses");
    warnings
        .iter()
        .filter(|w| w.category == "bynk.parse.orphan_doc_block")
        .count()
}

#[test]
fn keeps_an_orphan_doc_block_before_a_declaration() {
    expect_kept(
        "orphan before decl",
        "commons d\n\n---\nkeep me\n---\n\nfn f() -> Int { 1 }\n",
    );
}

#[test]
fn keeps_an_orphan_doc_block_between_declarations() {
    expect_kept(
        "orphan between decls",
        "commons d\n\nfn g() -> Int { 2 }\n\n---\nkeep me\n---\n\nfn f() -> Int { 1 }\n",
    );
}

#[test]
fn keeps_an_orphan_doc_block_at_end_of_file() {
    expect_kept("orphan at eof", "commons d\n\n---\nkeep me\n---\n");
}

/// In a brace-form body the block keeps its place and takes the body's indent,
/// before a declaration and before the closing brace.
#[test]
fn keeps_an_orphan_doc_block_in_a_brace_body() {
    expect_kept(
        "orphan in a brace body",
        "commons d {\n  ---\n  keep me\n  ---\n\n  fn f() -> Int { 1 }\n\n  ---\n  keep me too\n  ---\n}\n",
    );
}

/// Line comments on either side of an orphan stay on their side of it.
#[test]
fn keeps_an_orphan_between_line_comments() {
    let source = "commons d\n\n-- above\n---\nkeep me\n---\n\n-- below\nfn f() -> Int { 1 }\n";
    expect_kept("orphan between comments", source);
    let out = format_source(source, &FormatOptions::default()).unwrap();
    let above = out.find("-- above").unwrap();
    let block = out.find("keep me").unwrap();
    let below = out.find("-- below").unwrap();
    assert!(above < block && block < below, "{out}");
}

/// A `--` line directly under a doc block's closing `---` was binned as the
/// block's trailing comment, which nothing collects, so the file was refused
/// (#1756, found by `doc_blocks_are_always_kept`). It is a leading comment of
/// the declaration below, and the formatter moves it above the attached doc.
#[test]
fn keeps_a_comment_directly_under_a_doc_block() {
    let source = "commons d\n\n---\nattached\n---\n-- keep me\nfn f() -> Int { 1 }\n";
    let out = format_source(source, &FormatOptions::default()).expect("formats");
    assert_eq!(
        out,
        "commons d\n\n-- keep me\n---\nattached\n---\nfn f() -> Int { 1 }\n"
    );
}

/// The block before the unit header is kept too.
#[test]
fn keeps_an_orphan_doc_block_before_the_header() {
    expect_kept(
        "orphan before header",
        "---\nkeep me\n---\n\ncommons d\n\nfn f() -> Int { 1 }\n",
    );
}

/// The doc-block guard runs whether or not any `--` comment needed a slot, and
/// an attached block, re-indented inside a brace body, is not mistaken for a
/// lost one.
#[test]
fn an_attached_doc_block_formats() {
    let src = "context c {\n---\n  keep me\n---\nfn f() -> Int { 1 }\n}\n";
    let out = format_source(src, &FormatOptions::default()).expect("an attached doc formats");
    assert!(out.contains("keep me"), "{out}");
}
