//! #1663: diagnostic recovery on the editor path (`bynk_ide::diagnose`).
//!
//! A declaration that fails to parse is skipped, but its name is still known:
//! references to it must not echo as unknown names or types (Decision B),
//! while genuine errors elsewhere in the file are still reported.

fn diagnostics(source: &str) -> Vec<String> {
    bynk_ide::diagnose(source)
        .into_iter()
        .map(|d| {
            let (line, col) = bynk_syntax::span::line_col(source, d.error.span.start);
            format!("{} @ {line}:{col}", d.error.category)
        })
        .collect()
}

/// Fixture `49_full_money_commons` with one missing comma in `Money`: once 21
/// diagnostics (the syntax error, then 20 `unknown_type`/`method_unknown_type`
/// echoes), now just the syntax error.
#[test]
fn a_broken_record_does_not_cascade() {
    let src = include_str!("../../bynkc/tests/fixtures/positive/49_full_money_commons/input.bynk")
        .replacen(
            "minorUnits: Int where NonNegative,",
            "minorUnits: Int where NonNegative",
            1,
        );
    assert_eq!(diagnostics(&src), ["bynk.parse.expected_token @ 6:5"]);
}

/// A broken `fn` and a broken method are known too; a genuinely unknown name
/// and an unrelated type error in other declarations are still reported.
#[test]
fn broken_functions_and_methods_are_known_names() {
    let src = r#"commons demo

type Point = { x: Int, y: Int }

fn double(n: Int) -> Int { n * }

fn Point.norm(self) -> Int { self.x + }

fn uses_them(p: Point) -> Int { double(p.norm()) }

fn really_unknown() -> Int { missing(1) }

fn mistyped() -> Int { "s" }
"#;
    assert_eq!(
        diagnostics(src),
        [
            "bynk.parse.expected_expression @ 5:32",
            "bynk.parse.expected_expression @ 7:39",
            "bynk.resolve.unknown_function @ 11:30",
            "bynk.types.return_mismatch @ 13:24",
        ]
    );
}
