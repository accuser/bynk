//! Cross-parser conformance: the missing guard identified in #635.
//!
//! Bynk has **two** parsers. The compiler parses with the hand-written
//! recursive-descent parser in `bynk-syntax`; the editor tooling (and the
//! normative grammar appendix rendered from it) parses with the independent
//! `tree-sitter-bynk` grammar. Every other grammar guard is intra-side —
//! appendix vs `grammar.json`, `grammar.json` vs `{{#grammar}}` directives,
//! `keywords.rs` vs `lexer.rs` — so nothing tied tree-sitter to the compiler
//! parser, and three drifts (a missing `Bytes`, unenforced variant
//! capitalisation, and over-generated built-in generic arity) lived exactly in
//! that gap.
//!
//! This test closes it: each case is parsed by **both** parsers, and their
//! accept/reject decisions must agree with each other and with the pinned
//! expectation. A drift on either side — the grammar admitting what the
//! compiler rejects, or vice versa — fails here.
//!
//! Scope is the type surface where the drifts lived (base types, sum/enum
//! variants, built-in generics). Add a case here whenever a change could move
//! the two parsers apart.

use tree_sitter::Parser;

/// Does the `tree-sitter-bynk` grammar accept `src` cleanly — no `ERROR` and no
/// `MISSING` nodes? `has_error()` covers error nodes, but a recovery that
/// inserts a *missing* node (rather than an error one) is still a reject, and
/// treating it as such keeps the guard robust across tree-sitter versions as
/// cases grow.
fn tree_sitter_accepts(src: &str) -> bool {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bynk::LANGUAGE.into())
        .expect("load tree-sitter-bynk language");
    let tree = parser.parse(src, None).expect("tree-sitter parse");
    let root = tree.root_node();
    !root.has_error() && !has_missing(root)
}

/// Is `node` — or any node beneath it — a `MISSING` node?
fn has_missing(node: tree_sitter::Node) -> bool {
    if node.is_missing() {
        return true;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor).any(has_missing)
}

/// Does the compiler's own parser (`bynk-syntax`) accept `src`?
fn bynk_syntax_accepts(src: &str) -> bool {
    match bynk_syntax::lexer::tokenize(src) {
        Ok(tokens) => bynk_syntax::parser::parse(&tokens, src).is_ok(),
        Err(_) => false,
    }
}

/// Wrap a declaration body in a minimal `commons` unit both parsers accept.
fn unit(body: &str) -> String {
    format!("commons conf\n{body}\n")
}

struct Case {
    /// Human label, shown on failure.
    what: &'static str,
    /// A declaration body, wrapped into a `commons` unit before parsing.
    body: &'static str,
    /// The decision both parsers must reach: `true` = accept, `false` = reject.
    accept: bool,
}

const CASES: &[Case] = &[
    // -- Base types, incl. `Bytes` (drift A: the parser accepts it; the grammar
    //    omitted it, so tree-sitter could not parse `type Blob = Bytes` at all). --
    Case {
        what: "base Int",
        body: "type T = Int",
        accept: true,
    },
    Case {
        what: "base String",
        body: "type T = String",
        accept: true,
    },
    Case {
        what: "base Bool",
        body: "type T = Bool",
        accept: true,
    },
    Case {
        what: "base Float",
        body: "type T = Float",
        accept: true,
    },
    Case {
        what: "base Duration",
        body: "type T = Duration",
        accept: true,
    },
    Case {
        what: "base Instant",
        body: "type T = Instant",
        accept: true,
    },
    Case {
        what: "base Bytes",
        body: "type T = Bytes",
        accept: true,
    },
    Case {
        what: "Bytes in a generic",
        body: "fn f(s: String) -> Option[Bytes] { none }",
        accept: true,
    },
    // -- Sum / enum variant capitalisation (drift B: the parser accepted a
    //    lowercase variant end-to-end; the grammar's `constant_name` rejects it). --
    Case {
        what: "pipe variant capitalised",
        body: "type S = | Active | Inactive",
        accept: true,
    },
    Case {
        what: "pipe variant lowercase",
        body: "type S = | active | Inactive",
        accept: false,
    },
    Case {
        what: "enum tag capitalised",
        body: "type S = enum { Active, Inactive }",
        accept: true,
    },
    Case {
        what: "enum tag lowercase",
        body: "type S = enum { active, Inactive }",
        accept: false,
    },
    // The `embeds … as V` target is a `constant_name` too, and is the third
    // parse-time call-site of the capitalisation rule — cover it directly.
    Case {
        what: "embeds target capitalised",
        body: "type E = | Wrapped(reason: Int) embeds Int as Wrapped",
        accept: true,
    },
    Case {
        what: "embeds target lowercase",
        body: "type E = | Wrapped(reason: Int) embeds Int as wrapped",
        accept: false,
    },
    // -- Built-in generic arity (drift C: the grammar over-generated with
    //    `sep1`; the parser rejects the wrong arity at parse time). --
    Case {
        what: "Option[T] arity 1",
        body: "fn f(x: Option[Int]) -> Int { 0 }",
        accept: true,
    },
    Case {
        what: "Option arity 2 rejected",
        body: "fn f(x: Option[Int, String]) -> Int { 0 }",
        accept: false,
    },
    Case {
        what: "List[T] arity 1",
        body: "fn f(x: List[Int]) -> Int { 0 }",
        accept: true,
    },
    Case {
        what: "Result[T, E] arity 2",
        body: "fn f(x: Result[Int, String]) -> Int { 0 }",
        accept: true,
    },
    Case {
        what: "Result arity 1 rejected",
        body: "fn f(x: Result[Int]) -> Int { 0 }",
        accept: false,
    },
    Case {
        what: "Map[K, V] arity 2",
        body: "fn f(x: Map[String, Int]) -> Int { 0 }",
        accept: true,
    },
    Case {
        what: "Map arity 1 rejected",
        body: "fn f(x: Map[String]) -> Int { 0 }",
        accept: false,
    },
    // -- #472 refined patterns: `refined_pattern` is admitted only at a match
    //    arm's top-level pattern (and, syntactically, `is`'s RHS — rejected by
    //    the checker, not the grammar), never through a nested payload
    //    position — see D4 in design/decisions/*-refined-patterns.md. A PR
    //    review caught a drift here: `where`-suffix parsing had leaked into
    //    every recursive `parse_pattern` call, so the compiler accepted (and
    //    correctly compiled) `Ok(_ where P)` / `r is Ok(_ where P)`, which
    //    tree-sitter rejects. --
    Case {
        what: "refined pattern at match-arm top level",
        body: "fn f(x: Int) -> Int { match x { _ where NonNegative => 1 _ => 0 } }",
        accept: true,
    },
    Case {
        what: "refined pattern on is's top level rejected",
        body: "fn f(x: Int) -> Bool { x is _ where NonNegative }",
        accept: false,
    },
    Case {
        what: "refined pattern nested in a match-arm payload rejected",
        body: "fn f(r: Result[Int, String]) -> Int { match r { Ok(_ where NonNegative) => 1 Ok(_) => 2 Err(_) => 3 } }",
        accept: false,
    },
    Case {
        what: "refined pattern nested under is rejected",
        body: "fn f(r: Result[Int, String]) -> Bool { r is Ok(_ where NonNegative) }",
        accept: false,
    },
];

#[test]
fn tree_sitter_and_compiler_parser_agree() {
    let mut mismatches = Vec::new();
    for case in CASES {
        let src = unit(case.body);
        let ts = tree_sitter_accepts(&src);
        let rust = bynk_syntax_accepts(&src);

        // The load-bearing assertion: the two parsers must never disagree.
        if ts != rust {
            mismatches.push(format!(
                "  [{}] parsers DISAGREE: tree-sitter accepts={ts}, bynk-syntax accepts={rust}\n      src: {:?}",
                case.what, src
            ));
            continue;
        }
        // And their shared decision must match the pinned expectation, so a
        // regression that flips both in lockstep is still caught.
        if ts != case.accept {
            mismatches.push(format!(
                "  [{}] both parsers reached accept={ts}, expected accept={}\n      src: {:?}",
                case.what, case.accept, src
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "tree-sitter and the compiler parser diverged (#635 guard):\n{}",
        mismatches.join("\n")
    );
}
