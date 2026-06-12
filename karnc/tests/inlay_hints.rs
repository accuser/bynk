//! v0.27 (ADR 0056): the harvested inlay-hint set — inferred-type hints for
//! annotation-absent `let` / `let <-` bindings and lambda parameters,
//! per-file, labels in Karn surface syntax, surviving a transient type
//! error at the sites the checker still reaches.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn fixture_root(which: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/inlay")
        .join(which)
        .join("src")
}

/// The fixture file's hints alongside its analysed text.
fn hints_for(result: &karnc::ProjectDiagnostics, file: &str) -> (Vec<(usize, String)>, String) {
    let text = result
        .files
        .iter()
        .find(|f| f.source_path.to_string_lossy().replace('\\', "/") == file)
        .unwrap_or_else(|| panic!("{file} analysed"))
        .text
        .clone();
    let hints = result
        .hints
        .iter()
        .find(|(p, _)| p.to_string_lossy().replace('\\', "/") == file)
        .map(|(_, hs)| hs.iter().map(|(s, l)| (s.start, l.clone())).collect())
        .unwrap_or_default();
    (hints, text)
}

/// The hint label at binding `name` (matched at the byte offset of
/// `pattern`'s first occurrence plus `pattern.find(name)`).
fn label_at<'a>(
    hints: &'a [(usize, String)],
    text: &str,
    pattern: &str,
    name: &str,
) -> Option<&'a str> {
    let line = text
        .find(pattern)
        .unwrap_or_else(|| panic!("fixture contains `{pattern}`"));
    let offset = line
        + pattern
            .find(name)
            .unwrap_or_else(|| panic!("`{pattern}` contains `{name}`"));
    hints
        .iter()
        .find(|(start, _)| *start == offset)
        .map(|(_, l)| l.as_str())
}

#[test]
fn let_bindings_and_lambda_params_get_inferred_type_hints() {
    let result = karnc::diagnose_project(&fixture_root("clean"), &HashMap::new());
    let (hints, text) = hints_for(&result, "shop/util.karn");

    // `let =` with an inferred type — the headline.
    assert_eq!(
        label_at(&hints, &text, "let total = ", "total"),
        Some(": Int")
    );
    // Lambda params typed from `fold`'s expected fn type.
    assert_eq!(
        label_at(&hints, &text, "(acc, x) => ", "acc"),
        Some(": Int")
    );
    assert_eq!(label_at(&hints, &text, "(acc, x) => ", "x"), Some(": Int"));
}

#[test]
fn labels_read_in_karn_surface_syntax() {
    let result = karnc::diagnose_project(&fixture_root("clean"), &HashMap::new());
    let (hints, text) = hints_for(&result, "shop/util.karn");

    // Display fidelity: generic source syntax, not an internal rendering.
    assert_eq!(
        label_at(&hints, &text, "let xs = ", "xs"),
        Some(": List[Int]")
    );
    assert_eq!(
        label_at(&hints, &text, "let first = ", "first"),
        Some(": Option[Int]")
    );
    // The lambda-typed binding pins the `Fn` rendering.
    assert_eq!(
        label_at(&hints, &text, "let f = ", "f"),
        Some(": Int -> Int")
    );
}

#[test]
fn effect_let_hints_show_the_peeled_payload() {
    let result = karnc::diagnose_project(&fixture_root("clean"), &HashMap::new());
    let (hints, text) = hints_for(&result, "billing/charge.karn");

    // `let stamp <- Clock.now()` binds the Effect payload — `Int`, never
    // `Effect[Int]`.
    assert_eq!(
        label_at(&hints, &text, "let stamp <- ", "stamp"),
        Some(": Int")
    );
}

#[test]
fn annotated_and_underscore_bindings_get_no_hint() {
    let result = karnc::diagnose_project(&fixture_root("clean"), &HashMap::new());
    let (hints, text) = hints_for(&result, "shop/util.karn");

    // An explicit annotation needs no hint; `_` binds nothing.
    assert_eq!(label_at(&hints, &text, "let twice: Int = ", "twice"), None);
    assert_eq!(label_at(&hints, &text, "let _ = ", "_"), None);
    // An annotated lambda param gets no hint either.
    assert_eq!(label_at(&hints, &text, "(x: Int) => ", "x"), None);
}

#[test]
fn clean_fixture_has_no_diagnostics() {
    // The hint fixtures must be diagnostically clean, or the other tests
    // assert against a half-checked project.
    let result = karnc::diagnose_project(&fixture_root("clean"), &HashMap::new());
    for f in &result.files {
        assert!(
            f.diagnostics.is_empty(),
            "{}: {:?}",
            f.source_path.display(),
            f.diagnostics
                .iter()
                .map(|d| d.error.category)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn hints_survive_a_transient_error_at_reached_sites() {
    let result = karnc::diagnose_project(&fixture_root("broken"), &HashMap::new());
    let (hints, text) = hints_for(&result, "billing/charge.karn");

    // The file has one fn-body type error (`n + true` in `bad`)...
    assert!(
        result
            .files
            .iter()
            .find(|f| f.source_path.to_string_lossy().replace('\\', "/") == "billing/charge.karn")
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.error.category.starts_with("karn.types.")),
        "the broken fixture carries its type error"
    );
    // ...but the erroring binding's sibling fn still hints: the sink is a
    // `&mut` parameter, not part of the Ok payload `check_record` drops.
    assert_eq!(label_at(&hints, &text, "let m = ", "m"), Some(": Int"));
    // The erroring binding itself has no computed type, so no hint.
    assert_eq!(label_at(&hints, &text, "let s = ", "s"), None);
    // Bounded guarantee (settled): a `check_record` Err short-circuits the
    // v0.5 pass, so the handler-body `let <-` hint is suppressed until the
    // fn-body error clears — "sites the checker still reaches", not
    // file-total.
    assert_eq!(label_at(&hints, &text, "let stamp <- ", "stamp"), None);
}
