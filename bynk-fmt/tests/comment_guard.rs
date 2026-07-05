//! #523: the comment-loss guard. Trivia attaches at declaration/statement
//! granularity, so a comment inside an expression subtree can vanish from the
//! formatted output. `format_source` must refuse (a visible no-op with a
//! `bynk.fmt.comment_loss` diagnostic) rather than write output that lost
//! user text.

use bynk_fmt::{FormatOptions, format_source};

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
