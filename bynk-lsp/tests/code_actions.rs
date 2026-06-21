//! v0.26 (ADR 0054): the `codeAction` pipeline end-to-end on the v0.25
//! harness pattern — a real project on disk through `diagnose_project`, the
//! checker-authored suggestions riding the diagnostics, and the pure
//! quick-fix computation. No transport: this exercises exactly what the
//! `code_action` handler runs after position conversion.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::*;

// The handler-side pure modules, included directly (bynk-lsp is a binary
// crate). `code_actions` resolves `crate::position` against the include.
#[allow(dead_code)]
#[path = "../src/position.rs"]
mod position;

#[allow(dead_code)]
#[path = "../src/code_actions.rs"]
mod code_actions;

fn setup_project(test_name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "bynk-lsp-code-actions-{}-{}",
        test_name,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create test root");
    for (rel, contents) in files {
        let p = root.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(&p, contents).expect("write file");
    }
    root.canonicalize().unwrap_or(root)
}

const UNUSED_CAP: &str = "\
context billing.charge

capability Clock {
  fn now() -> Effect[Int]
}

provides Clock = FixedClock {
  fn now() -> Effect[Int] {
    42
  }
}

service charge {
  on call(cents: Int) -> Effect[Int] given Clock {
    cents
  }
}
";

/// Apply one file's `TextEdit`s (as `quick_fixes` emits them) to its text.
fn apply_text_edits(text: &str, edits: &[OneOf<TextEdit, AnnotatedTextEdit>]) -> String {
    let mut plain: Vec<&TextEdit> = edits
        .iter()
        .map(|e| match e {
            OneOf::Left(t) => t,
            OneOf::Right(_) => panic!("unexpected annotated edit"),
        })
        .collect();
    plain.sort_by_key(|e| (e.range.start.line, e.range.start.character));
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for e in plain {
        let start = position::position_to_offset(text, e.range.start).expect("start offset");
        let end = position::position_to_offset(text, e.range.end).expect("end offset");
        out.push_str(&text[last..start]);
        out.push_str(&e.new_text);
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

#[test]
fn unused_capability_quick_fix_round_trips() {
    let root = setup_project("unused", &[("billing/charge.bynk", UNUSED_CAP)]);
    let result = bynk_ide::diagnose_project(&root, &HashMap::new());
    let file = result
        .files
        .iter()
        .find(|f| f.source_path == Path::new("billing/charge.bynk"))
        .expect("context file analysed");
    let diag = file
        .diagnostics
        .iter()
        .find(|d| d.error.category == "bynk.given.unused_capability")
        .expect("the unused-capability diagnostic");

    // The request range sits on the squiggle (the diagnostic's span) — far
    // from where the edit lands (the `given` clause).
    let uri = Url::from_file_path(root.join("billing/charge.bynk")).unwrap();
    let actions = code_actions::quick_fixes(
        &file.text,
        &file.diagnostics,
        diag.error.span,
        &uri,
        Some(3),
    );
    assert_eq!(actions.len(), 1, "exactly one quick-fix offered");
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction");
    };
    assert_eq!(action.title, "remove `Clock` from the `given` clause");
    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));

    // The edit is versioned with the analysed document version.
    let Some(DocumentChanges::Edits(doc_edits)) = &action.edit.as_ref().unwrap().document_changes
    else {
        panic!("expected versioned document edits");
    };
    assert_eq!(doc_edits.len(), 1);
    assert_eq!(doc_edits[0].text_document.version, Some(3));
    assert_eq!(doc_edits[0].text_document.uri, uri);

    // Applying the WorkspaceEdit drops ` given Clock` exactly, and the
    // edited project re-diagnoses clean.
    let fixed = apply_text_edits(&file.text, &doc_edits[0].edits);
    assert_eq!(fixed, UNUSED_CAP.replace(" given Clock", ""));
    let abs = root.join("billing/charge.bynk");
    let canonical = abs.canonicalize().unwrap_or(abs);
    let mut overlay = HashMap::new();
    overlay.insert(canonical, fixed);
    let post = bynk_ide::diagnose_project(&root, &overlay);
    assert!(
        post.files.iter().all(|f| f.diagnostics.is_empty()),
        "applied fix re-diagnoses clean"
    );
}

#[test]
fn range_away_from_the_diagnostic_offers_nothing() {
    let root = setup_project("away", &[("billing/charge.bynk", UNUSED_CAP)]);
    let result = bynk_ide::diagnose_project(&root, &HashMap::new());
    let file = result
        .files
        .iter()
        .find(|f| f.source_path == Path::new("billing/charge.bynk"))
        .expect("context file analysed");
    let uri = Url::from_file_path(root.join("billing/charge.bynk")).unwrap();
    // A cursor at the top of the file intersects no diagnostic.
    let actions = code_actions::quick_fixes(
        &file.text,
        &file.diagnostics,
        bynk_syntax::span::Span::new(0, 0),
        &uri,
        None,
    );
    assert!(actions.is_empty());
}
