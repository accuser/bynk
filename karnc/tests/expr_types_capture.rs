//! v0.30.2 (ADR 0063): the per-file expression-type capture that backs
//! `.`-member completion's receiver typing. ADR 0094 (slice 4) lifts the clean-file
//! ceiling: in Analyse mode the checker's best-effort partial types are recorded
//! even when the file has an error elsewhere, so completion works mid-edit.

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
fn an_erroring_file_still_records_its_well_typed_expressions() {
    // ADR 0094 (lifting ADR 0063's ceiling): the broken fixture's `bad` fn has a
    // type error, so `check_record` returns `Err` — but the well-typed
    // expressions elsewhere (`good`'s `n * 2`) are still captured in Analyse mode,
    // so `.`-member completion / signature help work on a buffer with an unrelated
    // error. The diagnostic is still reported.
    let result = karnc::diagnose_project(&fixture_root("broken"), &HashMap::new());
    assert!(
        result
            .files
            .iter()
            .find(|f| f.source_path.to_string_lossy().replace('\\', "/") == "billing/charge.karn")
            .is_some_and(|f| !f.diagnostics.is_empty()),
        "the broken fixture still carries its error"
    );
    let (entries, text) = types_for(&result, "billing/charge.karn")
        .expect("ADR 0094: an erroring file now records its partial types");
    // A well-typed receiver away from the error (`n` in `good`'s `n * 2`) types as
    // `Int` — so typing `n.` there would complete despite `bad` failing to check.
    let off = text.find("n * 2").expect("fixture has `n * 2` in `good`");
    assert_eq!(
        type_at_offset(&entries, off),
        Some(&Ty::Base(BaseType::Int)),
        "`good`'s expressions are typed even though `bad` errors"
    );
}

#[test]
fn an_erroring_handler_body_records_its_well_typed_receivers() {
    // ADR 0094: handler bodies are typed in `check_context_declarations`, a later
    // exit than `check_record`. Here `check_record` is clean (no top-level fns)
    // but the handler errors (`cents + true`); the receiver `cents` still types as
    // `Int`, recorded at the declaration-check exit.
    let result = karnc::diagnose_project(&fixture_root("broken_handler"), &HashMap::new());
    assert!(
        result
            .files
            .iter()
            .find(|f| f.source_path.to_string_lossy().replace('\\', "/") == "billing/handler.karn")
            .is_some_and(|f| !f.diagnostics.is_empty()),
        "the handler fixture carries its error"
    );
    let (entries, text) = types_for(&result, "billing/handler.karn")
        .expect("ADR 0094: an erroring handler body still records its partial types");
    let off = text
        .find("cents + true")
        .expect("fixture has `cents + true`");
    assert_eq!(
        type_at_offset(&entries, off),
        Some(&Ty::Base(BaseType::Int)),
        "the receiver `cents` types as Int even though the handler body errors"
    );
}
