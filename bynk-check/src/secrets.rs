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

    // `secret_reads_of`'s own walk (`walk_block`/`walk_expr`) has no direct
    // unit coverage here, mirroring the original `bynk-emit` module it moved
    // from — that file's own tests exercised only `json_string`/`render`,
    // the manifest-rendering half that stayed behind. Behaviour parity for
    // the walk itself is what `bynk-check/tests/differential_analysis.rs`
    // and `bynk-lsp/tests/analysis_residual_gap.rs` are for.

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
