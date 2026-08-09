//! `bynk.Secrets` read-name checking (v0.173, ADR 0196 D1).
//!
//! `Secrets.get` takes an ordinary `String` expression, so a computed name is
//! invisible to any pass. Where one is seen, this module warns
//! (`bynk.secrets.computed_name`) — a non-failing diagnostic (ADR 0117): the
//! program is correct, `bynk deploy` simply cannot know which secret the
//! context reads, and cannot list it in the deploy plan.
//!
//! P5.5 (`design/tracks/semantics-in-the-checker.md` §6, §9): relocated here
//! from `bynk-emit/src/emitter/secrets.rs` — a real, `CompileError::new`-
//! constructed diagnostic, previously raised only from `bynk-emit::project`'s
//! `run_checks`, and (per that call site's own now-stale comment) never
//! reachable from [`crate::analysis::analyse_project`] at all, since
//! `bynk-check` cannot depend on `bynk-emit`. §9 named this an open risk
//! rather than a scoped relocation; this module closes it, per R3.5. The
//! manifest-only half — `declared_secrets`/`emit_secrets_manifest`/`render`,
//! which describe `bynk-secrets.json` rather than diagnose anything — stays
//! in `bynk-emit`, an emission concern this crate must not depend on. Its
//! caller there now reaches [`secret_reads_of`] here, qualified, rather than
//! duplicating the walk (the same dual-use pattern P5.4 used for
//! `bynk-check::test_suites`).

use bynk_syntax::ast::{Block, Expr, ExprKind};
use bynk_syntax::error::CompileError;
use bynk_syntax::span::Span;

/// The capability whose `get` names a platform secret. Matched against the
/// capability a context actually resolved, never against the spelling — see
/// [`reads_secrets_of_bynk`].
const SECRETS_CAPABILITY: &str = "Secrets";

/// What a context's handlers read through `bynk.Secrets`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SecretReads {
    /// Literal names, sorted. A census only while `complete` holds.
    pub names: std::collections::BTreeSet<String>,
    /// False when at least one `Secrets.get` argument was not a literal, so no
    /// pass could know the name.
    pub complete: bool,
}

impl SecretReads {
    /// A context that reads nothing: complete by vacuity, and the shape a
    /// non-`bynk.Secrets` context gets without walking a single expression.
    fn none() -> Self {
        Self {
            names: std::collections::BTreeSet::new(),
            complete: true,
        }
    }
}

/// Does this context's `Secrets` resolve to **`bynk`**'s?
///
/// `flattened` maps a context's in-scope capability name to the unit
/// providing it, so this asks the question that matters — *whose* `Secrets`
/// is this? — rather than matching the identifier. A context with no
/// `Secrets` at all answers `false` here and never gets walked.
fn reads_secrets_of_bynk(flattened: &std::collections::HashMap<String, String>) -> bool {
    flattened
        .get(SECRETS_CAPABILITY)
        .is_some_and(|unit| unit == crate::firstparty::BYNK_UNIT)
}

/// The literal `bynk.Secrets` names this context's handlers read, and whether
/// that list is everything.
///
/// Split so callers can scope it differently without the *rule* differing:
/// `bynk-emit`'s manifest wants a context's whole handler set (its names span
/// every file), while this warning wants one file's handlers at a time,
/// because a diagnostic sink attributes a diagnostic to a path and a merged
/// handler set has thrown that away.
pub fn secret_reads_of<'a>(
    handlers: impl Iterator<Item = &'a bynk_syntax::ast::Handler>,
    flattened: &std::collections::HashMap<String, String>,
) -> (SecretReads, Vec<CompileError>) {
    if !reads_secrets_of_bynk(flattened) {
        return (SecretReads::none(), Vec::new());
    }
    let mut reads = SecretReads::none();
    let mut warnings = Vec::new();
    for handler in handlers {
        walk_block(&handler.body, &mut reads, &mut warnings);
    }
    (reads, warnings)
}

/// Every expression in a handler body.
///
/// Reuses `statement_exprs` rather than matching `Statement` here: a
/// `Secrets.get` in a `let`, an `expect`, a `~>` send or a bare `do` is still a
/// read, and re-enumerating the statement kinds would be a second place to
/// forget one the day a new kind lands.
fn walk_block(block: &Block, reads: &mut SecretReads, warnings: &mut Vec<CompileError>) {
    let mut exprs: Vec<&Expr> = Vec::new();
    for statement in &block.statements {
        bynk_syntax::ast::statement_exprs(statement, &mut exprs);
    }
    exprs.push(&block.tail);
    for e in exprs {
        walk_expr(e, reads, warnings);
    }
}

/// Visit `e` and everything under it, recording each `Secrets.get` call.
///
/// Recurses through [`bynk_syntax::ast::expr_children`] rather than re-matching
/// every `ExprKind`: a `Secrets.get` inside a `match` arm, a lambda, or an
/// interpolation hole is still a read, and a hand-rolled visitor would be a
/// second place to forget that.
fn walk_expr(e: &Expr, reads: &mut SecretReads, warnings: &mut Vec<CompileError>) {
    if let ExprKind::MethodCall {
        receiver,
        method,
        args,
        ..
    } = &e.kind
        && method.name == "get"
        && matches!(&receiver.kind, ExprKind::Ident(name) if name.name == SECRETS_CAPABILITY)
    {
        // Arity is the checker's (`bynk.capability.op_arity`); this reads the
        // one argument when it is there and stays quiet when it is not, rather
        // than reporting a second diagnostic about the same call.
        match args.first().map(|a| &a.kind) {
            Some(ExprKind::StrLit(name)) => {
                reads.names.insert(name.clone());
            }
            Some(_) => {
                reads.complete = false;
                warnings.push(computed_name_warning(e.span));
            }
            None => {}
        }
    }
    for child in bynk_syntax::ast::expr_children(e) {
        walk_expr(child, reads, warnings);
    }
}

/// The one thing that can tell an author `deploy` has lost sight of a secret.
///
/// A warning, not an error ([DECISION A]): `Secrets.get(pickName())` is legal
/// and sometimes reasonable, and making it a compile failure to serve a driver's
/// convenience would be the language spending expressiveness it does not need to
/// spend. The severity is carried by the code — `Severity::for_error` classifies
/// it, and the diagnostic sink routes on that.
fn computed_name_warning(span: Span) -> CompileError {
    CompileError::new(
        "bynk.secrets.computed_name",
        span,
        "`Secrets.get` is called with a computed name, so `bynk deploy` cannot know which secret this context reads"
            .to_string(),
    )
    .with_note(
        "the deploy plan will not list it, and will say its list of read secrets is incomplete; \
         pass a string literal if you want it planned",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // `secret_reads_of`'s own walk (`walk_block`/`walk_expr`) is pinned
    // end-to-end by `bynkc/tests/fixtures/positive/374_secrets_computed_name`
    // (`target.txt` = `workers`) — an earlier version of this comment pointed
    // at `differential_analysis.rs`/`analysis_residual_gap.rs` instead, which
    // was wrong: this PR's own additions to both establish that neither can
    // observe this walk (both new sites are gap-in-name-only under the
    // hardcoded `BuildTarget::Bundle`/`SchemaLock::Off` both analysis paths
    // share — see their own doc comments). The tests below cover the same
    // branches directly, now that `secret_reads_of` is `bynk-check`'s public
    // API, by parsing real source rather than hand-building a `Handler`.

    fn parse_context_handlers(src: &str) -> Vec<bynk_syntax::ast::Handler> {
        let tokens = bynk_syntax::lexer::tokenize(src).expect("lex");
        let unit = bynk_syntax::parser::parse_unit(&tokens, src).expect("parse");
        let bynk_syntax::ast::SourceUnit::Context(ctx) = unit else {
            panic!("expected a context unit");
        };
        ctx.items
            .into_iter()
            .filter_map(|item| match item {
                bynk_syntax::ast::CommonsItem::Service(s) => Some(s.handlers.into_iter()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    fn flattened_to_bynk() -> std::collections::HashMap<String, String> {
        [(
            SECRETS_CAPABILITY.to_string(),
            crate::firstparty::BYNK_UNIT.to_string(),
        )]
        .into_iter()
        .collect()
    }

    const LITERAL_READ: &str = "context net.probe\n\nconsumes bynk { Secrets }\n\nservice probe {\n  on call() -> Effect[Option[String]] given Secrets {\n    Secrets.get(\"API_KEY\")\n  }\n}\n";

    #[test]
    fn a_literal_argument_is_collected_and_warns_nothing() {
        let handlers = parse_context_handlers(LITERAL_READ);
        let flattened = flattened_to_bynk();
        let (reads, warnings) = secret_reads_of(handlers.iter(), &flattened);
        assert_eq!(reads.names, ["API_KEY".to_string()].into());
        assert!(reads.complete);
        assert!(warnings.is_empty());
    }

    const COMPUTED_READ: &str = "context net.probe\n\nconsumes bynk { Secrets }\n\nservice probe {\n  on call(key: String) -> Effect[Option[String]] given Secrets {\n    Secrets.get(key)\n  }\n}\n";

    #[test]
    fn a_computed_argument_warns_once_and_marks_the_read_set_incomplete() {
        let handlers = parse_context_handlers(COMPUTED_READ);
        let flattened = flattened_to_bynk();
        let (reads, warnings) = secret_reads_of(handlers.iter(), &flattened);
        assert!(reads.names.is_empty());
        assert!(!reads.complete);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].category, "bynk.secrets.computed_name");
    }

    const NESTED_IN_MATCH_ARM: &str = "context net.probe\n\nconsumes bynk { Secrets }\n\ntype Choice = enum { A, B }\n\nservice probe {\n  on call(key: String, choice: Choice) -> Effect[Option[String]] given Secrets {\n    match choice {\n      A => Secrets.get(key)\n      B => Secrets.get(\"FIXED\")\n    }\n  }\n}\n";

    #[test]
    fn a_call_nested_inside_a_match_arm_is_still_found() {
        // `expr_children` is trusted to recurse into constructs `walk_expr`
        // never names explicitly — a `match` arm's body is the case this test
        // pins, since `walk_expr` only ever matches `MethodCall` directly.
        let handlers = parse_context_handlers(NESTED_IN_MATCH_ARM);
        let flattened = flattened_to_bynk();
        let (reads, warnings) = secret_reads_of(handlers.iter(), &flattened);
        assert_eq!(reads.names, ["FIXED".to_string()].into());
        assert!(
            !reads.complete,
            "the other arm's computed argument still counts"
        );
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn a_context_whose_secrets_is_not_bynks_is_never_walked() {
        // Same computed-argument source as above — if this resolved to
        // `bynk`'s `Secrets`, it would warn; walking nothing is the point.
        let handlers = parse_context_handlers(COMPUTED_READ);
        let flattened: std::collections::HashMap<String, String> =
            [(SECRETS_CAPABILITY.to_string(), "acme.vault".to_string())]
                .into_iter()
                .collect();
        let (reads, warnings) = secret_reads_of(handlers.iter(), &flattened);
        assert_eq!(reads, SecretReads::none());
        assert!(warnings.is_empty());
    }

    #[test]
    fn reads_secrets_of_bynk_matches_on_the_resolved_unit_not_the_spelling() {
        let to_bynk: std::collections::HashMap<String, String> = [(
            SECRETS_CAPABILITY.to_string(),
            crate::firstparty::BYNK_UNIT.to_string(),
        )]
        .into_iter()
        .collect();
        assert!(reads_secrets_of_bynk(&to_bynk));

        let to_someone_else: std::collections::HashMap<String, String> =
            [(SECRETS_CAPABILITY.to_string(), "acme.vault".to_string())]
                .into_iter()
                .collect();
        assert!(!reads_secrets_of_bynk(&to_someone_else));

        assert!(!reads_secrets_of_bynk(&std::collections::HashMap::new()));
    }

    #[test]
    fn computed_name_warning_carries_the_registered_code() {
        let w = computed_name_warning(Span::default());
        assert_eq!(w.category, "bynk.secrets.computed_name");
    }
}
