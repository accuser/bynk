//! P4.1 (#1115), [DECISION D]: a differential fixture proving this crate's
//! new `bynk-check`-native analysis entry point
//! ([`bynk_check::analysis::analyse_project`]) produces the same diagnostics
//! as `bynk-emit`'s `analyse_project_with` (which drives `run_checks`'s
//! `Mode::Analyse` arm) — the two paths share almost all of their real logic
//! now (`bynk_check::project_model`/`bynk_check::check_pipeline`), but
//! nothing compared their *output* until this test.
//!
//! Self-deleting: once phase 5 (`design/tracks/project-model.md`) removes
//! `run_checks`'s `Mode::Analyse` arm entirely and repoints every remaining
//! caller at `bynk_check::analysis::analyse_project` directly, there will be
//! only one path left and this comparison becomes meaningless.
//!
//! ## Why the fixture avoids the residual-gap categories
//!
//! `bynk_check::analysis::analyse_project`'s own doc comment names six
//! categories of whole-project checking it deliberately does not run
//! (schema-registry reconciliation, `messages`-bundle validation, locale
//! bundle ambiguity, event-subscription validation, platform-lock
//! enforcement, function-type-boundary checks). A fixture that triggered any
//! of those would see the two paths *legitimately* disagree — that
//! divergence is accepted debt (ADR 0328), not a bug this test should catch.
//! So the fixture below has: no `messages` block, no `Locale` capability
//! consumption, no `Events`/`from Events(...)` subscription, nothing that
//! reaches a platform-native capability (no `--platform` lock to trip), no
//! generic-record boundary violation (a function type in a non-boundary
//! position), and no `test`/`test integration` block.
//!
//! What it *does* exercise: a commons (`uses`-imported into the context, so
//! `compose_unit_symbols`'s mixin path runs), a locally-declared capability
//! with a Bynk-bodied provider (`phase_validate_providers`'s op-matching
//! check, plus `check_context_declarations`'s provider/service wiring), and
//! an HTTP handler with `by`/`given` clauses (`resolver`/`checker`'s normal
//! per-file path plus the context-checks transitive closure moved in this
//! same slice, #1115's first scope-correction).

use std::collections::HashMap;
use std::path::PathBuf;

use bynk_check::analysis;
use bynk_project::{AttributedError, Roots};

const SHARED_SRC: &str = r#"commons demo.shared

type Widget = {
  id:    String,
  count: Int,
}
"#;

const CONTEXT_SRC: &str = r#"context demo.svc

uses demo.shared

capability Counter {
  fn next(seed: Int) -> Effect[Int]
  fn reset() -> Effect[()]
}

provides Counter = StubCounter {
  fn next(seed: Int) -> Effect[Int] {
    Effect.pure(seed + 1)
  }

  fn reset() -> Effect[()] {
    Effect.pure(())
  }
}

service api from http {
  on GET("/widgets/:id") (id: String) -> Effect[HttpResult[Widget]] by Visitor given Counter {
    let n <- Counter.next(1)
    Ok(Widget { id: id, count: n })
  }
}
"#;

/// A self-contained temp project (real files on disk — discovery walks the
/// filesystem, so an overlay alone can't stand in for file *existence*).
/// Same convention as `bynk-ide`'s own `wire_contract.rs`/`sequence.rs` test
/// helpers: a name-scoped temp dir, cleaned up on entry, never `examples/`.
///
/// Also returns a *complete* overlay for every file written — the
/// content-ownership track (#1086, slice 5) removed `read_source`'s disk-read
/// fallback, so `bynk-project`'s discovery now errors (`no overlay entry
/// for ...`) on any file the overlay doesn't cover; production callers get a
/// complete overlay from `bynk-testkit`/`bynk-ide::discover_files`, and this
/// test builds the same completeness by hand from the files it just wrote.
fn setup_project(test_name: &str, files: &[(&str, &str)]) -> (PathBuf, HashMap<PathBuf, String>) {
    let root = std::env::temp_dir().join(format!(
        "bynk-check-differential-analysis-{test_name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create test root");
    let mut overlay = HashMap::new();
    for (rel, contents) in files {
        let p = root.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(&p, contents).expect("write file");
        overlay.insert(p, (*contents).to_string());
    }
    (root, overlay)
}

/// `AttributedError` rendered to a comparable, `Eq`-able form: the
/// project-relative path (if any), the diagnostic category, and the
/// rendered message. Span/label/note/suggestion positions are intentionally
/// excluded — both paths share the exact same checker/resolver, so identical
/// category+message+path already pins the diagnostic; comparing spans too
/// would just make the assertion more brittle to reformat without adding
/// confidence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RenderedError {
    path: Option<String>,
    category: &'static str,
    message: String,
}

fn render(errors: &[AttributedError]) -> Vec<RenderedError> {
    errors
        .iter()
        .map(|a| RenderedError {
            path: a
                .source_path
                .as_ref()
                .map(|p| p.to_string_lossy().replace('\\', "/")),
            category: a.error.category,
            message: a.error.message.clone(),
        })
        .collect()
}

#[test]
fn new_entry_point_matches_analyse_project_with_on_a_clean_project() {
    let (root, overlay) = setup_project(
        "clean",
        &[
            ("demo/shared.bynk", SHARED_SRC),
            ("demo/svc.bynk", CONTEXT_SRC),
        ],
    );
    let roots = Roots::Single(root);

    let legacy = bynk_emit::project::analyse_project_with(&roots, &overlay);
    let new = analysis::analyse_project(&roots, &overlay);

    assert_eq!(
        render(&legacy.errors),
        render(&new.errors),
        "the two analysis paths must report identical diagnostics on a fixture \
         that avoids every documented residual-gap category"
    );
    // Both paths must agree the fixture is clean — a divergence here would
    // mean the fixture accidentally tripped one of the six excluded checks.
    assert!(
        legacy.errors.is_empty(),
        "fixture must be clean under the legacy path: {:?}",
        render(&legacy.errors)
    );
    assert!(
        new.errors.is_empty(),
        "fixture must be clean under the new entry point: {:?}",
        render(&new.errors)
    );

    // The unit→file map should agree too — same discovery, same grouping.
    let mut legacy_units: Vec<&String> = legacy.unit_sources.keys().collect();
    let mut new_units: Vec<&String> = new.unit_sources.keys().collect();
    legacy_units.sort();
    new_units.sort();
    assert_eq!(legacy_units, new_units);
}

/// Same fixture, but with an obvious semantic error (an unknown `given`
/// capability) — proves the two paths agree on *error* diagnostics too, not
/// just the clean case.
#[test]
fn new_entry_point_matches_analyse_project_with_on_a_broken_project() {
    let broken_context = CONTEXT_SRC.replace("given Counter", "given NotACapability");
    let (root, overlay) = setup_project(
        "broken",
        &[
            ("demo/shared.bynk", SHARED_SRC),
            ("demo/svc.bynk", &broken_context),
        ],
    );
    let roots = Roots::Single(root);

    let legacy = bynk_emit::project::analyse_project_with(&roots, &overlay);
    let new = analysis::analyse_project(&roots, &overlay);

    let legacy_rendered = render(&legacy.errors);
    let new_rendered = render(&new.errors);
    assert_eq!(
        legacy_rendered, new_rendered,
        "the two analysis paths must report identical diagnostics for the same error fixture"
    );
    assert!(
        !legacy_rendered.is_empty(),
        "fixture must actually be broken (regression guard for the fixture itself)"
    );
}
