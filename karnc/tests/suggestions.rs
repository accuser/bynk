//! v0.26 (ADR 0054): the seed quick-fixes carry machine-applicable
//! [`Suggestion`]s whose `given`-clause edits are list-aware. The position
//! matrix below pins the **exact emitted text** for first / middle / last /
//! only positions (and add-to-existing / synthesise-absent) — fix
//! *correctness* is pinned here in `karnc`, with no LSP involved.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use karnc::error::{Applicability, Suggestion};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/suggestions/src")
}

/// The handlers context with a parameterised `given` clause (pass a leading
/// space, e.g. `" given Alpha, Beta"`, or `""` for no clause) and body.
fn handlers_file(given: &str, body: &str) -> String {
    format!(
        "\
context app.handlers

capability Alpha {{
  fn ping() -> Effect[Int]
}}

capability Beta {{
  fn ping() -> Effect[Int]
}}

capability Gamma {{
  fn ping() -> Effect[Int]
}}

provides Alpha = AlphaImpl {{
  fn ping() -> Effect[Int] {{
    1
  }}
}}

provides Beta = BetaImpl {{
  fn ping() -> Effect[Int] {{
    2
  }}
}}

provides Gamma = GammaImpl {{
  fn ping() -> Effect[Int] {{
    3
  }}
}}

service handlers {{
  on call(x: Int) -> Effect[Int]{given} {{
{body}
  }}
}}
"
    )
}

/// Diagnose the project with `text` overlaid on `file`, returning that
/// file's diagnostics.
fn diagnose_with(file: &str, text: &str) -> Vec<karnc::Diagnostic> {
    let abs = fixture_root().join(file);
    let canonical = abs.canonicalize().unwrap_or(abs);
    let mut overlay = HashMap::new();
    overlay.insert(canonical, text.to_string());
    let result = karnc::diagnose_project(&fixture_root(), &overlay);
    result
        .files
        .iter()
        .find(|f| f.source_path.to_string_lossy().replace('\\', "/") == file)
        .map(|f| f.diagnostics.clone())
        .unwrap_or_default()
}

/// Exactly one diagnostic of `category`, carrying exactly one
/// `MachineApplicable` suggestion — returned for application.
fn sole_suggestion(diags: &[karnc::Diagnostic], category: &str) -> Suggestion {
    let matching: Vec<_> = diags
        .iter()
        .filter(|d| d.error.category == category)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one `{category}`; got: {:?}",
        diags.iter().map(|d| d.error.category).collect::<Vec<_>>()
    );
    let suggestions = &matching[0].error.suggestions;
    assert_eq!(suggestions.len(), 1, "expected exactly one suggestion");
    let s = suggestions[0].clone();
    assert_eq!(s.applicability, Applicability::MachineApplicable);
    s
}

/// The applied fix must itself diagnose clean — a suggestion that leaves
/// any diagnostic behind is not machine-applicable.
fn assert_clean(file: &str, text: &str) {
    let diags = diagnose_with(file, text);
    assert!(
        diags.is_empty(),
        "applied fix left diagnostics: {:?}",
        diags.iter().map(|d| d.error.category).collect::<Vec<_>>()
    );
}

/// Apply a suggestion's edits (span → replacement) to the source text.
fn apply(text: &str, s: &Suggestion) -> String {
    let mut edits = s.edits.clone();
    edits.sort_by_key(|(span, _)| span.start);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (span, replacement) in &edits {
        out.push_str(&text[last..span.start]);
        out.push_str(replacement);
        last = span.end;
    }
    out.push_str(&text[last..]);
    out
}

// -- remove unused capability: the position matrix --

const USES_BETA_GAMMA: &str = "    let b <- Beta.ping()\n    let g <- Gamma.ping()\n    b + g";
const USES_ALPHA_GAMMA: &str = "    let a <- Alpha.ping()\n    let g <- Gamma.ping()\n    a + g";
const USES_ALPHA_BETA: &str = "    let a <- Alpha.ping()\n    let b <- Beta.ping()\n    a + b";
const USES_ALPHA: &str = "    let a <- Alpha.ping()\n    a";
const USES_NONE: &str = "    42";

#[test]
fn remove_first_capability() {
    let text = handlers_file(" given Alpha, Beta, Gamma", USES_BETA_GAMMA);
    let s = sole_suggestion(
        &diagnose_with("app/handlers.karn", &text),
        "karn.given.unused_capability",
    );
    assert_eq!(s.message, "remove `Alpha` from the `given` clause");
    let fixed = apply(&text, &s);
    assert_eq!(fixed, handlers_file(" given Beta, Gamma", USES_BETA_GAMMA));
    assert_clean("app/handlers.karn", &fixed);
}

#[test]
fn remove_middle_capability() {
    let text = handlers_file(" given Alpha, Beta, Gamma", USES_ALPHA_GAMMA);
    let s = sole_suggestion(
        &diagnose_with("app/handlers.karn", &text),
        "karn.given.unused_capability",
    );
    let fixed = apply(&text, &s);
    assert_eq!(
        fixed,
        handlers_file(" given Alpha, Gamma", USES_ALPHA_GAMMA)
    );
    assert_clean("app/handlers.karn", &fixed);
}

#[test]
fn remove_last_capability() {
    let text = handlers_file(" given Alpha, Beta, Gamma", USES_ALPHA_BETA);
    let s = sole_suggestion(
        &diagnose_with("app/handlers.karn", &text),
        "karn.given.unused_capability",
    );
    let fixed = apply(&text, &s);
    assert_eq!(fixed, handlers_file(" given Alpha, Beta", USES_ALPHA_BETA));
    assert_clean("app/handlers.karn", &fixed);
}

#[test]
fn remove_only_capability_drops_the_given_keyword() {
    let text = handlers_file(" given Alpha", USES_NONE);
    let s = sole_suggestion(
        &diagnose_with("app/handlers.karn", &text),
        "karn.given.unused_capability",
    );
    let fixed = apply(&text, &s);
    assert_eq!(fixed, handlers_file("", USES_NONE));
    assert_clean("app/handlers.karn", &fixed);
}

// -- add capability to `given` --

#[test]
fn add_capability_after_existing_entries() {
    let text = handlers_file(" given Alpha", USES_ALPHA_BETA);
    let s = sole_suggestion(
        &diagnose_with("app/handlers.karn", &text),
        "karn.given.undeclared_capability",
    );
    assert_eq!(s.message, "add `Beta` to the `given` clause");
    let fixed = apply(&text, &s);
    assert_eq!(fixed, handlers_file(" given Alpha, Beta", USES_ALPHA_BETA));
    assert_clean("app/handlers.karn", &fixed);
}

#[test]
fn add_capability_synthesises_an_absent_clause() {
    let text = handlers_file("", USES_ALPHA);
    let s = sole_suggestion(
        &diagnose_with("app/handlers.karn", &text),
        "karn.given.undeclared_capability",
    );
    let fixed = apply(&text, &s);
    assert_eq!(fixed, handlers_file(" given Alpha", USES_ALPHA));
    assert_clean("app/handlers.karn", &fixed);
}

// -- cross-context (`B.Cap`) add --

fn crossuse_file(given: &str) -> String {
    format!(
        "\
context app.crossuse

consumes platform.time as Time

service crossuse {{
  on call() -> Effect[Int]{given} {{
    let t <- Time.Clock.now()
    t
  }}
}}
"
    )
}

#[test]
fn add_cross_context_capability_synthesises_an_absent_clause() {
    let text = crossuse_file("");
    let s = sole_suggestion(
        &diagnose_with("app/crossuse.karn", &text),
        "karn.given.undeclared_capability",
    );
    // The clause entry is the *canonical* context path (the diagnosis site
    // sees the resolved name, not the `as Time` alias spelling) — valid
    // alongside alias-style calls, as the clean re-diagnosis below proves.
    assert_eq!(s.message, "add `platform.time.Clock` to the `given` clause");
    let fixed = apply(&text, &s);
    assert_eq!(fixed, crossuse_file(" given platform.time.Clock"));
    assert_clean("app/crossuse.karn", &fixed);
}

// -- the baseline fixtures themselves are clean --

#[test]
fn baseline_fixtures_carry_no_diagnostics() {
    let result = karnc::diagnose_project(&fixture_root(), &HashMap::new());
    for f in &result.files {
        assert!(
            f.diagnostics.is_empty(),
            "{} unexpectedly has diagnostics: {:?}",
            f.source_path.display(),
            f.diagnostics
                .iter()
                .map(|d| d.error.category)
                .collect::<Vec<_>>()
        );
    }
}
