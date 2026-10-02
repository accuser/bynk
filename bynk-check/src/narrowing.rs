//! Which `is` tests an expression proves. #1654 (runtime-semantics track S5,
//! Decision A): the one structural rule the resolver, the checker and the
//! emitter all read, so their views of an `is` binding's scope cannot drift.
//!
//! An `e is P` test with bindings brings those bindings into scope wherever the
//! test is **known to have matched**. Given a Boolean expression and whether it
//! evaluated to `true` or `false`, [`matched_is_tests`] returns the `is` tests
//! that must then have matched:
//!
//! | expression    | when true                     | when false                    |
//! |---------------|-------------------------------|-------------------------------|
//! | `e is P`      | the test                      | —                             |
//! | `a && b`      | `a` when true, `b` when true  | — (either may have failed)    |
//! | `a \|\| b`    | — (either may have held)      | `a` when false, `b` when false |
//! | `!e`          | `e` when false                | `e` when true                 |
//! | `a implies b` | — (`!a \|\| b`)               | `a` when true, `b` when false |
//! | `(e)`         | `e`                           | `e`                           |
//!
//! The consumers apply it at these scopes:
//! - an `if`'s then-branch: its condition when true;
//! - an `if`'s else-branch: its condition when false (`if !(o is Some(v))
//!   { … } else { v }`, type-system §2.3.6);
//! - the right operand of `&&` and `implies`: the left operand when true, since
//!   the right is evaluated only then.
//!
//! The right operand of `||` (the left operand when false) is not scoped in
//! this slice: nothing lowers it with bindings yet, and a scope the checker
//! accepts must be one the emitter lowers.

use bynk_syntax::ast::{BinOp, Expr, ExprKind, UnaryOp};

/// The `is` tests (`ExprKind::Is` nodes) that `expr` proves matched when it
/// evaluates to `when_true`, in source order.
pub fn matched_is_tests(expr: &Expr, when_true: bool) -> Vec<&Expr> {
    let mut out = Vec::new();
    collect(expr, when_true, &mut out);
    out
}

fn collect<'e>(expr: &'e Expr, when_true: bool, out: &mut Vec<&'e Expr>) {
    match (&expr.kind, when_true) {
        (ExprKind::Is { .. }, true) => out.push(expr),
        (ExprKind::Paren(inner), _) => collect(inner, when_true, out),
        (ExprKind::UnaryOp(UnaryOp::Not, inner), _) => collect(inner, !when_true, out),
        (ExprKind::BinOp(BinOp::And, a, b), true) | (ExprKind::BinOp(BinOp::Or, a, b), false) => {
            collect(a, when_true, out);
            collect(b, when_true, out);
        }
        (ExprKind::BinOp(BinOp::Implies, a, b), false) => {
            collect(a, true, out);
            collect(b, false, out);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::matched_is_tests;
    use bynk_syntax::{lexer, parser};

    /// The `is` tests' receiver names `cond` proves when `when_true`.
    fn proved(cond: &str, when_true: bool) -> Vec<String> {
        let src = format!(
            "commons m\n\nfn f(a: Option[Int], b: Option[Int], c: Bool) -> Bool {{ {cond} }}\n"
        );
        let tokens = lexer::tokenize(&src).expect("lex");
        let unit = parser::parse_unit(&tokens, &src).expect("parse");
        let bynk_syntax::ast::SourceUnit::Commons(commons) = unit else {
            panic!("commons")
        };
        let bynk_syntax::ast::CommonsItem::Fn(f) = &commons.items[0] else {
            panic!("fn")
        };
        matched_is_tests(&f.body.tail, when_true)
            .into_iter()
            .map(|e| match &e.kind {
                bynk_syntax::ast::ExprKind::Is { value, .. } => match &value.kind {
                    bynk_syntax::ast::ExprKind::Ident(id) => id.name.clone(),
                    _ => "?".to_string(),
                },
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn a_test_proves_itself_only_when_true() {
        assert_eq!(proved("a is Some(x)", true), ["a"]);
        assert!(proved("a is Some(x)", false).is_empty());
        assert_eq!(proved("(a is Some(x))", true), ["a"]);
    }

    #[test]
    fn and_proves_both_when_true_and_nothing_when_false() {
        assert_eq!(proved("a is Some(x) && b is Some(y)", true), ["a", "b"]);
        assert!(proved("a is Some(x) && b is Some(y)", false).is_empty());
    }

    #[test]
    fn or_proves_nothing_when_true_and_both_negations_when_false() {
        assert!(proved("a is Some(x) || b is Some(y)", true).is_empty());
        assert_eq!(
            proved("!(a is Some(x)) || !(b is Some(y))", false),
            ["a", "b"]
        );
    }

    #[test]
    fn not_flips_the_polarity() {
        assert!(proved("!(a is Some(x))", true).is_empty());
        assert_eq!(proved("!(a is Some(x))", false), ["a"]);
        assert_eq!(proved("!!(a is Some(x))", true), ["a"]);
        // `!(a && b)` false means `a && b` true.
        assert_eq!(proved("!(a is Some(x) && b is Some(y))", false), ["a", "b"]);
        // `!(a && b)` true proves nothing.
        assert!(proved("!(a is Some(x) && b is Some(y))", true).is_empty());
    }

    #[test]
    fn implies_proves_its_antecedent_and_negated_consequent_when_false() {
        assert!(proved("a is Some(x) implies c", true).is_empty());
        assert_eq!(
            proved("a is Some(x) implies !(b is Some(y))", false),
            ["a", "b"]
        );
    }
}
