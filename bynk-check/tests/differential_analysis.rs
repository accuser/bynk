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
//! `bynk_check::analysis::analyse_project`'s own doc comment named seven
//! categories of whole-project checking it deliberately did not run.
//! `messages`-bundle validation, locale bundle ambiguity, event-subscription
//! validation (P5.0/P5.1) and function-type-boundary checks (P5.2,
//! `design/tracks/semantics-in-the-checker.md` §6) have since closed — both
//! paths now call the same relocated `bynk_check::project_model` functions,
//! so a fixture triggering any of those four no longer has a legitimate
//! divergence to avoid. The clean/broken fixtures below still don't trigger
//! any of the four (unchanged since P4.1 — nothing in `SHARED_SRC`/
//! `CONTEXT_SRC` was ever shaped to trip them), so proving parity on one of
//! them needs its own dedicated fixture rather than a tweak to the shared
//! one: `new_entry_point_matches_analyse_project_with_on_a_function_type_boundary_violation`
//! below does that for category 6, the one this slice closed. Schema-registry
//! reconciliation and platform-lock enforcement also closed, at P5.3 — both
//! were (and remain) gap-in-name-only: `analyse_project` calls
//! `crate::schema_registry::reconcile` against an always-empty registry and
//! `crate::project_model::phase_platform_lock` against the hardcoded
//! Cloudflare/Bundle pair, exactly like `analyse_project_with` did (does)
//! through `run_checks` for the same hardcoded values — unreachable on this
//! path regardless of where the checking code lives, so no fixture, dedicated
//! or shared, could observe a divergence either way; kept out of the shared
//! fixture below for clarity all the same. `test`/`test integration`
//! processing (category 7) also closed, at P5.4: both paths now call
//! `bynk_check::test_suites::phase_test_bodies`/`phase_integration_bodies`
//! for their checking half, so `new_entry_point_omits_test_body_diagnostics`
//! below now asserts parity rather than divergence — see its own doc
//! comment. The clean/broken fixture below still avoids all seven
//! categories, not because any remains a legitimate divergence to dodge, but
//! because it predates every one of them closing and there's no reason to
//! reshape it now: it has no `messages` block, no `Locale`
//! capability consumption, no `Events`/`from Events(...)` subscription,
//! nothing that reaches a platform-native capability (no `--platform` lock
//! to trip), no generic-record boundary violation (a function type in a
//! non-boundary position), and no `test`/`test integration` block.
//!
//! P5.5 (`design/tracks/semantics-in-the-checker.md` §6, §9) closed two more
//! sites outside the seven-category accounting —
//! `bynk.project.schema_registry_corrupt` (now
//! `bynk_check::schema_registry::parse_or_diagnose`) and
//! `bynk.secrets.computed_name` (now
//! `bynk_check::project_model::phase_secrets_computed_name`). Both join
//! schema-registry reconciliation and platform-lock in the gap-in-name-only
//! bucket rather than needing a dedicated parity fixture:
//! `bynk_emit::project::analyse_project_with` hardcodes `SchemaLock::Off` and
//! `BuildTarget::Bundle` (`project.rs:750-763`) exactly as
//! `bynk_check::analysis::analyse_project` does, so neither diagnostic is
//! reachable through either path — no divergence exists for a fixture to
//! prove parity on.
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

fn sorted_keys<K: Ord + Clone, V>(m: &HashMap<K, V>) -> Vec<K> {
    let mut ks: Vec<K> = m.keys().cloned().collect();
    ks.sort();
    ks
}

/// Structural parity beyond `errors`/`unit_sources`: every other
/// `ProjectAnalysis` field `bynk-ide` reads and P4.2 exists to repoint —
/// review feedback on #1117 (a differential fixture that pins two of twelve
/// fields "cannot observe" a divergence in the other ten by construction).
/// `ty_intern` is deliberately excluded: it is an opaque per-call intern
/// table (`Types` exposes no length accessor), and the two paths build
/// independent tables, so comparing them carries no signal beyond what
/// `expr_types`'s key set (compared here) already pins.
fn assert_structural_parity(legacy: &analysis::ProjectAnalysis, new: &analysis::ProjectAnalysis) {
    assert_eq!(
        legacy.snapshots.len(),
        new.snapshots.len(),
        "snapshots (files read) must agree"
    );
    assert_eq!(
        sorted_keys(&legacy.hints),
        sorted_keys(&new.hints),
        "hints' file coverage must agree"
    );
    assert_eq!(
        sorted_keys(&legacy.expr_types),
        sorted_keys(&new.expr_types),
        "expr_types' file coverage must agree"
    );
    assert_eq!(
        sorted_keys(&legacy.locals),
        sorted_keys(&new.locals),
        "locals' file coverage must agree"
    );
    assert_eq!(
        sorted_keys(&legacy.requirements),
        sorted_keys(&new.requirements),
        "requirements' file coverage must agree"
    );
    assert_eq!(
        sorted_keys(&legacy.sequence_info),
        sorted_keys(&new.sequence_info),
        "sequence_info's unit coverage must agree"
    );
    assert_eq!(
        sorted_keys(&legacy.boundary_info),
        sorted_keys(&new.boundary_info),
        "boundary_info's unit coverage must agree"
    );
    assert_eq!(
        sorted_keys(&legacy.doc_scope),
        sorted_keys(&new.doc_scope),
        "doc_scope's unit coverage must agree"
    );
    assert_eq!(
        legacy.index.symbols.len(),
        new.index.symbols.len(),
        "index.symbols must agree"
    );
    assert_eq!(
        legacy.index.foreign_refs.len(),
        new.index.foreign_refs.len(),
        "index.foreign_refs must agree"
    );
    assert_eq!(
        legacy.index.calls.len(),
        new.index.calls.len(),
        "index.calls must agree"
    );
    assert_eq!(
        legacy.index.impls.len(),
        new.index.impls.len(),
        "index.impls must agree"
    );
    assert_eq!(
        legacy.index.refinements.len(),
        new.index.refinements.len(),
        "index.refinements must agree"
    );
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

    assert_structural_parity(&legacy, &new);
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

    assert_structural_parity(&legacy, &new);
}

/// P5.2 (`design/tracks/semantics-in-the-checker.md` §6): a dedicated parity
/// fixture for category 6 (function-type-boundary checks), the category this
/// slice closed. The two clean/broken fixtures above deliberately avoid a
/// boundary violation (see this module's own doc comment), so neither one
/// exercises `phase_function_type_boundaries` at all -- this proves the two
/// paths agree on it specifically, not just that the shared fixture doesn't
/// trip it. Same source as `bynk-lsp/tests/analysis_residual_gap.rs`'s
/// `function_type_boundary_diagnostic_present` (copied from
/// `bynkc/tests/fixtures/negative/152_fn_type_in_service_sig`), the analogous
/// pin one layer up, at the `bynk-ide`/`bynk-lsp` call path.
#[test]
fn new_entry_point_matches_analyse_project_with_on_a_function_type_boundary_violation() {
    const API: &str = "context hof.api\n\nservice runner {\n  on call(f: Int -> Int) -> Effect[Int] {\n    Effect.pure(0)\n  }\n}\n";
    let (root, overlay) = setup_project("fn_boundary", &[("src/hof/api.bynk", API)]);
    let roots = Roots::Single(root);

    let legacy = bynk_emit::project::analyse_project_with(&roots, &overlay);
    let new = analysis::analyse_project(&roots, &overlay);

    let legacy_rendered = render(&legacy.errors);
    let new_rendered = render(&new.errors);
    assert_eq!(
        legacy_rendered, new_rendered,
        "the two analysis paths must report identical diagnostics for a \
         function-type boundary violation"
    );
    assert!(
        legacy_rendered
            .iter()
            .any(|e| e.category == "bynk.types.function_at_boundary"),
        "fixture must actually trigger the boundary check (regression guard \
         for the fixture itself): {legacy_rendered:?}"
    );
}

/// Category 7 (test/integration-suite processing), **closed at P5.4**
/// (`design/tracks/semantics-in-the-checker.md` §6): this pinned the
/// divergence itself, per review feedback on #1117 that a fixture shaped to
/// dodge every gap can't also catch a regression in how a gap is described —
/// its own doc comment said "once it closes, tighten this assertion (and the
/// doc comment) rather than deleting the test." Tightened here: both paths
/// now call `bynk_check::test_suites::phase_test_bodies`/
/// `phase_integration_bodies` for their checking half
/// (`bynk_check::analysis::analyse_project` directly,
/// `bynk_emit::project::analyse_project_with` through `run_checks`'s
/// `process_tests`/`process_integration_tests`), so a test body's own type
/// error and its binding refs now agree between the two paths — asserting
/// parity, not divergence. `bynk-emit`'s own
/// `check_project_reports_a_test_body_error_past_an_earlier_structural_error`
/// is the analogous legacy-side pin (unaffected — it only ever exercised the
/// legacy path). Kept, not deleted: this file's own module doc says it
/// self-deletes only once phase 5 removes `run_checks`'s `Mode::Analyse` arm
/// entirely (P5.5+), which is out of scope here.
#[test]
fn new_entry_point_omits_test_body_diagnostics() {
    const MATH_SRC: &str = "commons demo.math\n\nfn double(n: Int) -> Int { n * 2 }\n";
    const TEST_SRC: &str = "suite demo.math\n\ncase \"broken\" {\n  let x: Int = \"not an int\"\n  let y = double(1)\n  expect y == 2\n}\n";

    let (root, overlay) = setup_project(
        "test_body_gap",
        &[
            ("demo/math.bynk", MATH_SRC),
            ("tests/math_test.bynk", TEST_SRC),
        ],
    );
    let roots = Roots::Single(root);

    let legacy = bynk_emit::project::analyse_project_with(&roots, &overlay);
    let new = analysis::analyse_project(&roots, &overlay);

    let legacy_categories: Vec<&str> = legacy.errors.iter().map(|a| a.error.category).collect();
    let new_categories: Vec<&str> = new.errors.iter().map(|a| a.error.category).collect();
    assert!(
        legacy_categories.contains(&"bynk.types.let_annotation_mismatch"),
        "legacy path must report the test body's own type error: {legacy_categories:?}"
    );
    assert!(
        new_categories.contains(&"bynk.types.let_annotation_mismatch"),
        "P5.4 closed category 7 — `analyse_project` now calls \
         `bynk_check::test_suites::phase_test_bodies`, so the new path should report \
         the test body's own type error too: {new_categories:?}"
    );

    // The second consequence, now closed too: the test file's `double(1)`
    // call is a binding edge (a ref on `double`'s index entry) — both paths
    // record it now, since `phase_test_bodies` populates `&mut RefSink` the
    // same way `process_tests` always did — the concrete
    // go-to-definition-inside-a-test-file regression this test also pinned.
    let total_refs = |a: &analysis::ProjectAnalysis| {
        a.index
            .symbols
            .values()
            .map(|e| e.refs.len())
            .sum::<usize>()
    };
    assert_eq!(
        total_refs(&legacy),
        total_refs(&new),
        "both paths should now record the same reference edges — legacy={}, new={}",
        total_refs(&legacy),
        total_refs(&new)
    );
}
