//! v0.30.2 (ADR 0063): the per-file expression-type capture that backs
//! `.`-member completion's receiver typing — captured on the Ok path, so a
//! file that fails to check yields nothing (the clean-file ceiling).

use karnc::ast::BaseType;
use karnc::checker::Ty;
use karnc::expr_types::type_at_offset;
use karnc::span::Span;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn fixture_root(which: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/inlay")
        .join(which)
        .join("src")
}

/// A fixture file's `(expr span, Ty)` entries alongside its analysed text.
fn types_for(result: &karnc::ProjectDiagnostics, file: &str) -> Option<(Vec<(Span, Ty)>, String)> {
    let text = result
        .files
        .iter()
        .find(|f| f.source_path.to_string_lossy().replace('\\', "/") == file)?
        .text
        .clone();
    let entries = result
        .expr_types
        .iter()
        .find(|(p, _)| p.to_string_lossy().replace('\\', "/") == file)
        .map(|(_, e)| e.clone());
    entries.map(|e| (e, text))
}

#[test]
fn a_clean_file_records_its_receiver_types() {
    let result = karnc::diagnose_project(&fixture_root("clean"), &HashMap::new());
    let (entries, text) = types_for(&result, "shop/util.karn").expect("clean file recorded");
    assert!(!entries.is_empty(), "clean file has expression types");

    // The receiver-typing use case: `xs` in `xs.fold(…)` is `List[Int]`.
    let recv = text.find("xs.fold").expect("fixture has `xs.fold`");
    assert_eq!(
        type_at_offset(&entries, recv),
        Some(&Ty::List(Box::new(Ty::Base(BaseType::Int)))),
        "the `.fold` receiver types as List[Int]"
    );
}

#[test]
fn a_file_that_fails_to_check_records_nothing_the_ceiling() {
    // The broken fixture's only file carries a type error, so `check_record`
    // bails and no expression types are captured for it — completion offers
    // nothing there (ADR 0063's clean-file ceiling).
    let result = karnc::diagnose_project(&fixture_root("broken"), &HashMap::new());
    assert!(
        result
            .files
            .iter()
            .find(|f| f.source_path.to_string_lossy().replace('\\', "/") == "billing/charge.karn")
            .is_some_and(|f| !f.diagnostics.is_empty()),
        "the broken fixture carries its error"
    );
    assert!(
        types_for(&result, "billing/charge.karn").is_none(),
        "an erroring file records no expression types"
    );
}
