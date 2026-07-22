//! `bynk-secrets.json` generation per Worker (v0.172, ADR 0195).
//!
//! The **declared** secret names a context's handlers will read from `env` at
//! runtime — an `actor`'s `auth = Bearer(secret = "…")` / `Signature(secret =
//! "…")`, including the members of a multi-actor sum. `deploy` reads this file
//! to know which secrets it must see set before it pushes.
//!
//! The file carries **two kinds of knowledge, which are not equally strong**
//! (ADR 0196):
//!
//! - `declared` — an `actor`'s auth secret. A literal fixed at parse time,
//!   required at compile time, and **fail-closed**: unset, the Worker answers
//!   401 to every request. `deploy` refuses to ship without a value.
//! - `read` — a literal `bynk.Secrets` name (`Secrets.get("X")`). `get` returns
//!   `Option`, so absence is a legitimate handled outcome — these are
//!   **advisory**, and `deploy` warns rather than failing.
//!
//! And `read_complete`, which is the honesty. `Secrets.get` takes an ordinary
//! `String` expression, so a computed name is invisible to any pass: where one
//! is seen, the context warns (`bynk.secrets.computed_name`) and this flag goes
//! false. `declared` is a **floor, not a census** (ADR 0195 D2); `read` is a
//! census only while `read_complete` holds, and says so when it does not.
//!
//! Why a file rather than an API: the driver has two compile paths, and under a
//! `bynkc` override the compiler is a child process handing back an exit status
//! — there is no in-memory model to consult. A name the compiler knows must
//! reach the driver in the build output, or not at all (ADR 0195 D5).

use std::collections::BTreeSet;

use bynk_check::actors::SumMemberSeam;
use bynk_syntax::CompileError;
use bynk_syntax::ast::{Block, Expr, ExprKind};
use bynk_syntax::span::Span;

use crate::project::UnitTable;

/// The file the driver reads, beside each Worker's `wrangler.toml`.
pub const SECRETS_MANIFEST: &str = "bynk-secrets.json";

/// The manifest schema version. Bumped only by a breaking shape change; the
/// driver refuses a version it does not know rather than guessing, as the
/// deploy ledger does.
///
/// **2** (v0.173, ADR 0196) added `read` and `read_complete`. The bump is
/// deliberate rather than a default-on-absence read: a v1 manifest carries no
/// evidence either way about computed names, and defaulting `read_complete` to
/// `true` for it would be the manifest's one claim that could be silently wrong.
const MANIFEST_VERSION: u32 = 2;

/// The capability whose `get` names a platform secret. Matched against the
/// capability a context actually resolved, never against the spelling — see
/// [`reads_secrets_of_bynk`].
const SECRETS_CAPABILITY: &str = "Secrets";

/// What a context's handlers read through `bynk.Secrets`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SecretReads {
    /// Literal names, sorted. A census only while `complete` holds.
    pub names: BTreeSet<String>,
    /// False when at least one `Secrets.get` argument was not a literal, so no
    /// pass could know the name.
    pub complete: bool,
}

impl SecretReads {
    /// A context that reads nothing: complete by vacuity, and the shape a
    /// non-`bynk.Secrets` context gets without walking a single expression.
    fn none() -> Self {
        Self {
            names: BTreeSet::new(),
            complete: true,
        }
    }
}

/// Every secret name this context's handlers will read from `env`.
///
/// Enumerated over exactly the handlers the entry emitter lowers seams for
/// (`table.services`, which is where a `from websocket` service lives too), and
/// resolved with exactly the same `bynk_check::actors` functions — so the
/// manifest cannot describe a Worker other than the one emitted beside it. An
/// actor that is declared but named by no handler's `by` clause resolves no
/// seam and contributes nothing: the Worker never reads it.
///
/// `Oidc` names no secret — its trust root is the provider's published JWKS, not
/// a shared value — and a sum's `None` member (a catch-all such as `Visitor`)
/// verifies nothing. Both are skipped rather than defaulted: inventing a name
/// for them would ask the user to set a secret that nothing reads.
pub fn declared_secrets(table: &UnitTable) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for handler in table.services.values().flat_map(|s| s.handlers.iter()) {
        if let Some(seam) = bynk_check::actors::bearer_seam_for(handler, &table.actors) {
            names.insert(seam.secret);
        }
        if let Some(seam) = bynk_check::actors::signature_seam_for(handler, &table.actors) {
            names.insert(seam.secret);
        }
        for member in bynk_check::actors::sum_members_for(handler, &table.actors)
            .into_iter()
            .flatten()
        {
            match member.seam {
                SumMemberSeam::Bearer { secret, .. } => {
                    names.insert(secret);
                }
                SumMemberSeam::Signature(seam) => {
                    names.insert(seam.secret);
                }
                SumMemberSeam::None => {}
            }
        }
    }
    names
}

/// Does this context's `Secrets` resolve to **`bynk`**'s?
///
/// The whole of [DECISION D]. `flattened` maps a context's in-scope capability
/// name to the unit providing it, so this asks the question that matters —
/// *whose* `Secrets` is this? — rather than matching the identifier.
///
/// An author may declare their own capability (an adapter's
/// `capability Jwt { … }` is an ordinary thing to write), so nothing stops one
/// being named `Secrets`. Collecting from *that* would put a name in the
/// manifest that `deploy` then sets on Cloudflare — a real secret written to a
/// real account for a store that was never Cloudflare's. A context with no
/// `Secrets` at all answers `false` here and never gets walked.
fn reads_secrets_of_bynk(flattened: &std::collections::HashMap<String, String>) -> bool {
    flattened
        .get(SECRETS_CAPABILITY)
        .is_some_and(|unit| unit == bynk_check::firstparty::BYNK_UNIT)
}

/// The literal `bynk.Secrets` names this context's handlers read, and whether
/// that list is everything.
///
/// Walks the same handlers [`declared_secrets`] does, so the two classes cannot
/// come to describe different Workers. Emits `bynk.secrets.computed_name` — a
/// non-failing warning ([ADR 0117](../../../design/decisions)) — at each call
/// site whose argument is not a literal, which is the only place an author can
/// be told that they have stepped outside what `deploy` can see.
/// Returns the reads **and** the warnings, rather than taking a sink, so the two
/// callers can each take the half they need from one pure walk: `run_checks`
/// raises the warnings (which is what puts them in the editor, via the same
/// analyse path `bynk check` uses), and `build_output` writes the names into the
/// manifest. Called twice over the same inputs rather than threaded through
/// `RunChecks` — it is a cheap AST walk beside emitting TypeScript, and one
/// function with one rule cannot disagree with itself.
pub fn secret_reads(
    table: &UnitTable,
    flattened: &std::collections::HashMap<String, String>,
) -> (SecretReads, Vec<CompileError>) {
    secret_reads_of(
        table.services.values().flat_map(|s| s.handlers.iter()),
        flattened,
    )
}

/// The walk itself, over any handler set.
///
/// Split so the two callers can scope it differently without the *rule*
/// differing: the manifest wants a context's whole handler set (its names span
/// every file), while the warning wants one file's handlers at a time, because
/// `extend_for` attributes a diagnostic to a path and a `UnitTable` has merged
/// that away.
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

/// Render the manifest for a context, or `None` when there is nothing to say.
///
/// Emitted when **anything** is known ([DECISION E]): a declared secret, a read
/// name, or the fact that a name is computed. Slice 3's rule — emit only for a
/// non-empty `declared` — would have stayed silent for a context that reads
/// secrets but declares none, which is exactly the context this file now exists
/// to describe.
pub fn emit_secrets_manifest(table: &UnitTable, reads: &SecretReads) -> Option<String> {
    render(&declared_secrets(table), reads)
}

/// Render a resolved name set. Split from the derivation so the file's shape is
/// tested without building a project model.
///
/// Absent rather than empty: a project with no declared secret must not grow a
/// file into every worker directory for a feature it does not use — and "no
/// file" is the same answer as "an empty list" to a driver that must tolerate a
/// build tree from a compiler predating this file anyway.
fn render(declared: &BTreeSet<String>, reads: &SecretReads) -> Option<String> {
    // `complete` is the third thing worth saying: a context that reads one
    // computed name and nothing else knows something — that it does not know —
    // and the file has to carry it or the driver cannot.
    if declared.is_empty() && reads.names.is_empty() && reads.complete {
        return None;
    }
    // Hand-rendered rather than via serde: this crate does not depend on
    // serde_json, and the shape is three fields.
    Some(format!(
        "{{\n  \"version\": {MANIFEST_VERSION},\n  \"declared\": {},\n  \"read\": {},\n  \"read_complete\": {}\n}}\n",
        json_array(declared),
        json_array(&reads.names),
        reads.complete,
    ))
}

/// A JSON array of names, one per line, or `[]`.
fn json_array(names: &BTreeSet<String>) -> String {
    if names.is_empty() {
        return "[]".to_string();
    }
    let rendered: Vec<String> = names
        .iter()
        .map(|n| format!("    {}", json_string(n)))
        .collect();
    format!("[\n{}\n  ]", rendered.join(",\n"))
}

/// Escape a secret name as a JSON string literal.
///
/// A name reaches here from a Bynk `StrLit`, so as far as this file is concerned
/// it is arbitrary text. Escaping the two structural characters plus the control
/// range is what keeps the manifest parseable rather than merely
/// usually-parseable.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn a_name_is_escaped_as_a_json_string() {
        assert_eq!(json_string("AUTH_JWT_SECRET"), "\"AUTH_JWT_SECRET\"");
        // A secret name is a Bynk string literal, so it is arbitrary text; the
        // manifest must stay parseable rather than usually-parseable.
        assert_eq!(json_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(json_string("a\nb"), "\"a\\nb\"");
        assert_eq!(json_string("a\u{1}b"), "\"a\\u0001b\"");
    }

    fn reads(names: &[&str], complete: bool) -> SecretReads {
        SecretReads {
            names: set(names),
            complete,
        }
    }

    /// [DECISION E]: the file appears when **anything** is known, and only then.
    #[test]
    fn a_manifest_appears_when_anything_is_known_and_not_otherwise() {
        // Nothing at all — no file. A project with no secrets must not grow one
        // into every worker directory.
        assert_eq!(render(&set(&[]), &reads(&[], true)), None);

        // A read with no declared secret is exactly the context slice 3's rule
        // would have stayed silent about — and exactly the one worth describing.
        assert!(render(&set(&[]), &reads(&["API_KEY"], true)).is_some());

        // Knowing that you *don't* know is knowledge too: a context whose only
        // secret is computed still emits, or the driver cannot say its list is
        // incomplete.
        assert!(render(&set(&[]), &reads(&[], false)).is_some());
    }

    /// The committed shape, byte for byte — it is a file a reviewer reads in a
    /// fixture diff and a driver parses. Asserted here rather than round-tripped
    /// through a parser because this crate deliberately carries three
    /// dependencies and `serde_json` is not among them; that the bytes *parse*
    /// is asserted driver-side, where the reader lives.
    #[test]
    fn the_manifest_is_sorted_and_pinned() {
        // A `BTreeSet` orders the names, so the file is byte-stable for a given
        // context rather than dependent on handler iteration order.
        assert_eq!(
            render(&set(&["B_SECRET", "A_SECRET"]), &reads(&["R"], true))
                .expect("a non-empty set renders"),
            "{\n  \"version\": 2,\n  \"declared\": [\n    \"A_SECRET\",\n    \"B_SECRET\"\n  ],\n  \
             \"read\": [\n    \"R\"\n  ],\n  \"read_complete\": true\n}\n",
        );
        // The empty-list and false-flag shapes, which the fixtures also carry.
        assert_eq!(
            render(&set(&["ONLY"]), &reads(&[], false)).expect("renders"),
            "{\n  \"version\": 2,\n  \"declared\": [\n    \"ONLY\"\n  ],\n  \
             \"read\": [],\n  \"read_complete\": false\n}\n",
        );
    }
}
