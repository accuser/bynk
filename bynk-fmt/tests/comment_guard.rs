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

/// #1756 review: a service, capability or agent body had no slot for what
/// comes before its closing `}`, so a `--` comment there was deleted with no
/// refusal, and an orphaned block made `fmt` refuse. Both are kept now.
#[test]
fn keeps_comments_and_an_orphan_at_the_end_of_a_service() {
    expect_kept(
        "end of service",
        "context c\n\nservice s {\n  on call() -> Effect[()] { Effect.pure(()) }\n\n  -- keep me\n  ---\n  keep me too\n  ---\n}\n",
    );
    let out = format_source(
        "context c\n\nservice s {\n  on call() -> Effect[()] { Effect.pure(()) }\n\n  -- keep me\n}\n",
        &FormatOptions::default(),
    )
    .unwrap();
    assert!(out.contains("-- keep me"), "{out}");
}

#[test]
fn keeps_comments_and_an_orphan_at_the_end_of_a_capability() {
    expect_kept(
        "end of capability",
        "context c\n\ncapability K {\n  fn op() -> Effect[Int]\n\n  ---\n  keep me\n  ---\n  -- and me\n}\n",
    );
}

#[test]
fn keeps_comments_and_an_orphan_at_the_end_of_an_agent() {
    expect_kept(
        "end of agent",
        "context c\n\nagent A {\n  key id: String\n  store n: Cell[Int] = 0\n  on call peek() -> Effect[Int] { Effect.pure(n) }\n\n  ---\n  keep me\n  ---\n}\n",
    );
}

/// A doc block above a service policy documents nothing: it warns as an
/// orphan whether or not a blank line separates it, and is kept.
#[test]
fn keeps_an_orphan_before_a_service_policy() {
    expect_kept(
        "before a policy",
        "context api\n\nservice api from http {\n  ---\n  keep me\n  ---\n  cors {\n    origins: [\"https://a.example.com\"],\n  }\n\n  on GET(\"/ping\") () -> Effect[HttpResult[String]] by v: Visitor {\n    Ok(\"pong\")\n  }\n}\n",
    );
}

/// A doc block above an adapter's `uses` was dropped with no warning; it is an
/// orphan like any other now.
#[test]
fn keeps_an_orphan_before_an_adapter_clause() {
    let source = "adapter a\n\n---\nkeep me\n---\nuses bynk\n\ncapability K {\n  fn op() -> Effect[Int]\n}\n";
    assert_eq!(orphan_warnings(source), 1);
    expect_kept("before adapter uses", source);
}

/// #1786: a comment inside a `cors`/`security`/`limits` policy had no slot. It
/// was deleted with no refusal, then (#1785) refused. It is kept now.
#[test]
fn keeps_a_comment_inside_a_service_policy() {
    expect_kept(
        "in-policy",
        "context api\n\nservice api from http {\n  cors {\n    -- keep me\n    origins: [\"https://a.example.com\"],\n  }\n\n  on GET(\"/ping\") () -> Effect[HttpResult[String]] by v: Visitor {\n    Ok(\"pong\")\n  }\n}\n",
    );
}

/// #1786: every place a comment can sit in a policy keeps it there: above a
/// field, at the end of a field's line, before `}`, after `}`, and an orphaned
/// doc block between fields, which also warns.
#[test]
fn keeps_comments_in_every_policy_position() {
    let source = "context api\n\nservice api from http {\n  cors {\n    -- above\n    origins: [\"https://a.example.com\"], -- same line\n    ---\n    keep me\n    ---\n\n    credentials: false\n    -- before close\n  } -- after close\n  security {\n    hsts: 180.days, -- on security\n  }\n  limits {\n    -- on limits\n    maxBody: 1_048_576,\n  }\n\n  on GET(\"/ping\") () -> Effect[HttpResult[String]] by v: Visitor {\n    Ok(\"pong\")\n  }\n}\n";
    assert_eq!(orphan_warnings(source), 1);
    expect_kept("policy positions", source);
    let out = format_source(source, &FormatOptions::default()).unwrap();
    for (before, after) in [
        ("-- above", "origins:"),
        ("origins:", "-- same line"),
        ("-- same line", "keep me"),
        ("keep me", "credentials:"),
        ("credentials:", "-- before close"),
        ("-- before close", "-- after close"),
        ("hsts:", "-- on security"),
        ("-- on limits", "maxBody:"),
    ] {
        assert!(
            out.find(before).unwrap() < out.find(after).unwrap(),
            "`{before}` should precede `{after}`:\n{out}"
        );
    }
    // The whole line, so a comment hoisted onto a line of its own fails.
    for line in [
        "origins: [\"https://a.example.com\"],  -- same line\n",
        "hsts: 180.days,  -- on security\n",
    ] {
        assert!(
            out.contains(line),
            "`{line}` stays on its field's line:\n{out}"
        );
    }
}

/// #1786 review: an orphan as the last thing before a policy's `}` (it ends the
/// policy's `trailing_comments`, so no blank line follows it), and a policy
/// holding nothing but a comment.
#[test]
fn keeps_an_orphan_closing_a_policy_and_a_comment_only_policy() {
    let wrap = |cors: &str| {
        format!(
            "context api\n\nservice api from http {{\n  cors {{\n{cors}  }}\n\n  on GET(\"/ping\") () -> Effect[HttpResult[String]] by v: Visitor {{\n    Ok(\"pong\")\n  }}\n}}\n"
        )
    };
    let closing =
        wrap("    origins: [\"https://a.example.com\"],\n    ---\n    keep me\n    ---\n");
    assert_eq!(orphan_warnings(&closing), 1);
    expect_kept("orphan closing a policy", &closing);
    let out = format_source(&closing, &FormatOptions::default()).unwrap();
    assert!(
        out.contains("keep me\n\t\t---\n\t}"),
        "no blank line before `}}`:\n{out}"
    );
    expect_kept("comment-only policy", &wrap("    -- keep me\n"));
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

/// #1788: a comment on the same line as an opening `{` was filed as the `{`'s
/// trailing trivia, which no parser collected, so the file was refused. It now
/// leads whatever follows the brace: the first item, or the `}` of an empty
/// body. `fmt` moves it onto its own line under the brace.
#[test]
fn keeps_a_comment_on_the_line_of_an_opening_brace() {
    let policy = |name: &str, field: &str| {
        format!(
            "context api\n\nservice api from http {{\n  {name} {{ -- keep me\n    {field}\n  }}\n\n  on GET(\"/ping\") () -> Effect[HttpResult[String]] by v: Visitor {{\n    Ok(\"pong\")\n  }}\n}}\n"
        )
    };
    for (name, source) in [
        (
            "commons",
            "commons d { -- keep me\n  fn f() -> Int { 1 }\n}\n".to_string(),
        ),
        (
            "context",
            "context c { -- keep me\n  fn f() -> Int { 1 }\n}\n".to_string(),
        ),
        (
            "suite",
            "suite demo.wallet { -- keep me\n  property \"p\" {\n    for all n: Int { expect n == n }\n  }\n}\n"
                .to_string(),
        ),
        (
            "service",
            "context c\n\nservice s { -- keep me\n  on call() -> Effect[()] { Effect.pure(()) }\n}\n"
                .to_string(),
        ),
        (
            "agent",
            "context c\n\nagent A { -- keep me\n  key id: String\n  store n: Cell[Int]\n\n  on call get() -> Effect[Int] {\n    n\n  }\n}\n"
                .to_string(),
        ),
        (
            "capability",
            "context c\n\ncapability K { -- keep me\n  fn op() -> Effect[Int]\n}\n".to_string(),
        ),
        ("cors", policy("cors", "origins: [\"https://a.example.com\"],")),
        ("security", policy("security", "hsts: 180.days,")),
        ("limits", policy("limits", "maxBody: 1_048_576,")),
        (
            "fn",
            "commons d\n\nfn f() -> Int { -- keep me\n  let x = 1\n  x\n}\n".to_string(),
        ),
        (
            "fn tail",
            "commons d\n\nfn f() -> Int { -- keep me\n  1\n}\n".to_string(),
        ),
        (
            "record",
            "commons d\n\ntype P = { -- keep me\n  x: Int,\n}\n".to_string(),
        ),
        (
            "doc block after the brace",
            "context c\n\nservice s { -- keep me\n  ---\n  doc\n  ---\n  on call() -> Effect[()] { Effect.pure(()) }\n}\n"
                .to_string(),
        ),
        ("empty commons", "commons d { -- keep me\n}\n".to_string()),
        (
            "empty record",
            "commons d\n\ntype P = { -- keep me\n}\n".to_string(),
        ),
        (
            "empty policy",
            policy("cors", ""),
        ),
    ] {
        expect_kept(name, &source);
        let out = format_source(&source, &FormatOptions::default()).unwrap();
        let line = out.lines().find(|l| l.contains("keep me")).unwrap();
        assert_eq!(line.trim(), "-- keep me", "{name}: moved onto its own line:\n{out}");
    }
}

/// #1788: the brace-line comment of a record type and of an agent had nowhere
/// to go, because a record field and an agent's `key` had no comment slot.
/// Now every position in either keeps its comment there.
#[test]
fn keeps_comments_in_a_record_type_and_on_an_agent_key() {
    let record = "commons d\n\ntype P = {\n  -- above\n  x: Int, -- same line\n  y: Int -- last\n  -- before close\n} -- after close\n";
    let agent = "context c\n\nagent A {\n  -- above key\n  key id: String -- on key\n  store n: Cell[Int]\n\n  on call get() -> Effect[Int] {\n    n\n  }\n}\n";
    for (name, source, lines) in [
        (
            "record",
            record,
            &[
                "\t-- above\n\tx: Int,  -- same line\n",
                "\ty: Int,  -- last\n\t-- before close\n}  -- after close\n",
            ][..],
        ),
        (
            "agent key",
            agent,
            &["\t-- above key\n\tkey id: String  -- on key\n"][..],
        ),
    ] {
        let out = format_source(source, &FormatOptions::default())
            .unwrap_or_else(|e| panic!("{name}: refused: {}", e.errors[0].message));
        for line in lines {
            assert!(out.contains(line), "{name}: `{line}` not in place:\n{out}");
        }
        let again = format_source(&out, &FormatOptions::default()).expect("reformats");
        assert_eq!(out, again, "{name}: not idempotent");
    }
}
