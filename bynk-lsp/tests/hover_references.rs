//! #611: hover on *references* inside an agent handler body.
//!
//! Hover worked on `store`/`key` declarations but not on the uses in the body:
//! a store-field reference, a record-construction field label, and a store
//! method call each resolved nowhere (or, for a label, mis-bound to a same-named
//! handler param). Every hover test before this one used declaration offsets,
//! which is why the gaps slipped through — so these fixtures are pinned at
//! *reference* offsets in `examples/todo/src/todos.bynk`, the file the issue
//! reproduces in, against real `diagnose_project` output.
//!
//! bynk-lsp is a binary crate: include the pure modules directly (the pattern
//! `legend_drift.rs` established). `Backend::hover` is transport and cannot be
//! called from here — but the resolution *ladder* it runs is
//! [`hover::hover_content`], which is pure, so [`hover_at`] calls **the real
//! thing**. Reordering the rungs breaks these tests, which is the point: gap B
//! was a fall-through bug, so the ordering is the behaviour under test, and a
//! replica of it would agree with the original only until one of them changed.

#[allow(dead_code)]
#[path = "../src/completion.rs"]
mod completion;
#[allow(dead_code)]
#[path = "../src/hover.rs"]
mod hover;
#[allow(dead_code)]
#[path = "../src/index_queries.rs"]
mod index_queries;
#[allow(dead_code)]
#[path = "../src/locals_nav.rs"]
mod locals_nav;
#[allow(dead_code)]
#[path = "../src/position.rs"]
mod position;
#[allow(dead_code)]
#[path = "../src/signature_help.rs"]
mod signature_help;
#[allow(dead_code)]
#[path = "../src/symbols.rs"]
mod symbols;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bynk_ide::ProjectDiagnostics;
use tower_lsp::lsp_types::Url;

/// The analysed `examples/todo` project — the issue's reproduction.
fn todos() -> (ProjectDiagnostics, PathBuf, String) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/todo/src");
    let r = bynk_ide::diagnose_project(&root, &HashMap::new());
    let file = r
        .files
        .iter()
        .find(|f| f.source_path.to_string_lossy().ends_with("todos.bynk"))
        .expect("todos.bynk analysed");
    let (path, text) = (file.source_path.clone(), file.text.clone());
    (r, path, text)
}

/// The project-relative path the index keys spans against (the index and the
/// `files` list can disagree on absolute-vs-relative; match by file name).
fn index_path(r: &ProjectDiagnostics) -> PathBuf {
    r.index
        .symbols
        .values()
        .filter_map(|e| e.def.as_ref())
        .map(|d| d.path.clone())
        .find(|p| p.to_string_lossy().ends_with("todos.bynk"))
        .expect("todos.bynk is in the index")
}

/// Drive **the real hover ladder** (`Backend::hover`'s pure core) over analysed
/// project output. The snapshot and the live buffer are the same text here — the
/// file is on disk and unedited — which is exactly the steady state a saved
/// document is in.
fn hover_at(r: &ProjectDiagnostics, rel: &Path, text: &str, offset: usize) -> Option<String> {
    let snapshots: HashMap<PathBuf, String> = r
        .files
        .iter()
        .map(|f| (index_key(r, &f.source_path), f.text.clone()))
        .collect();
    let uri = Url::parse("file:///todos.bynk").unwrap();
    hover::hover_content(&hover::HoverInput {
        analysis: Some(hover::HoverAnalysis {
            index: &r.index,
            snapshots: &snapshots,
            locals: &r.locals,
            expr_types: &r.expr_types,
            rel,
            offset,
        }),
        doc: Some((text, offset)),
        uri: &uri,
        // No src_root: the cross-file / first-party rungs are not what these
        // fixtures exercise, and `examples/todo` is single-file.
        src_root: None,
    })
}

/// The path the round's tables key `p` under (the index/locals/expr_types maps
/// and the `files` list can disagree on absolute-vs-relative; match by name).
fn index_key(r: &ProjectDiagnostics, p: &Path) -> PathBuf {
    r.locals
        .keys()
        .chain(
            r.index
                .symbols
                .values()
                .filter_map(|e| e.def.as_ref())
                .map(|d| &d.path),
        )
        .find(|k| k.file_name() == p.file_name())
        .cloned()
        .unwrap_or_else(|| p.to_path_buf())
}

/// The byte offset of `needle` within the `add` handler body — the issue's
/// reproduction site. Anchored to the handler so a `needle` that also occurs
/// earlier in the file (`items` declares before it is used) resolves to the use.
fn in_add_handler(text: &str, needle: &str) -> usize {
    let body = text.find("on call add(").expect("the `add` handler");
    let end = text[body..].find("\n  }").expect("the handler's end") + body;
    body + text[body..end].find(needle).unwrap_or_else(|| {
        panic!(
            "`{needle}` is not in the `add` handler body:\n{}",
            &text[body..end]
        )
    })
}

/// Gap A — a `store`-field reference (`lastSeq` in `let next = lastSeq + 1`)
/// hovers as its `store lastSeq: Cell[Int]` declaration. Observed: nothing.
#[test]
fn store_field_reference_hovers_as_its_declaration() {
    let (r, _, text) = todos();
    let rel = index_path(&r);
    let hover = hover_at(&r, &rel, &text, in_add_handler(&text, "lastSeq + 1"))
        .expect("hover on the `lastSeq` reference");
    assert!(hover.contains("store lastSeq: Cell[Int]"), "{hover}");

    // The declaration hovered before this change and still does — identically.
    let decl = hover_at(&r, &rel, &text, text.find("store lastSeq").unwrap())
        .expect("hover on the declaration");
    assert_eq!(hover, decl);
}

/// Gap B — record-construction field labels (`Stored { seq: …, title: …, done:
/// … }`) hover as the `Stored` type's fields. Observed: `title:` showed the
/// handler param `title`; `seq:`/`done:` showed nothing.
#[test]
fn record_construction_labels_hover_as_the_types_fields() {
    let (r, _, text) = todos();
    let rel = index_path(&r);
    let ctor = in_add_handler(&text, "Stored { seq: next");

    for (label, expected) in [
        ("seq: next", "seq: Int"),
        ("title: title", "title: Title"),
        ("done: false", "done: Bool"),
    ] {
        let offset = ctor + text[ctor..].find(label).expect("the label");
        let hover = hover_at(&r, &rel, &text, offset)
            .unwrap_or_else(|| panic!("no hover on the `{label}` label"));
        assert!(
            hover.contains(expected) && hover.contains("A field of `Stored`"),
            "`{label}` should describe the `Stored` field, got:\n{hover}"
        );
    }

    // The regression the fall-through caused: `title:` bound to the enclosing
    // `add(title: Title)` param. The param itself still hovers as a param.
    let param = text
        .find("title: Title) -> Effect")
        .expect("the handler param");
    let hover = hover_at(&r, &rel, &text, param).expect("hover on the param");
    assert!(
        hover.contains("param"),
        "the param still hovers as one: {hover}"
    );
}

/// Gap C — a store method call (`items.put(id, item)`) hovers: the receiver as
/// the `store items` field, the operation as its signature. Observed: nothing.
#[test]
fn store_method_call_hovers_receiver_and_operation() {
    let (r, _, text) = todos();
    let rel = index_path(&r);
    let call = in_add_handler(&text, "items.put(id, item)");

    let recv = hover_at(&r, &rel, &text, call).expect("hover on the `items` receiver");
    assert!(recv.contains("store items: Map[String, Stored]"), "{recv}");

    let op = hover_at(&r, &rel, &text, call + "items.".len()).expect("hover on `put`");
    assert!(op.contains("put(key: K, value: V) -> Effect[()]"), "{op}");
    assert!(op.contains("store items: Map[String, Stored]"), "{op}");
}

/// The rung *order* is the contract, stated rather than left implicit.
///
/// Gap B was a fall-through: a rung that resolved the offset correctly but
/// rendered nothing let a later name-matching rung answer. These two offsets are
/// the ones where the rungs actively disagree, so they pin the precedence:
/// hoisting the locals rung above the index rung reintroduces #611 and fails
/// here — which is what makes calling the real `hover_content` (rather than a
/// replica of it) load-bearing.
#[test]
fn a_structural_rung_outranks_the_name_matching_locals_rung() {
    let (r, _, text) = todos();
    let rel = index_path(&r);

    // `title:` — the index resolves `Stored.title`; the locals rung would match
    // the in-scope `add(title: Title)` param by name. The index must win.
    let ctor = in_add_handler(&text, "Stored { seq: next");
    let label = ctor + text[ctor..].find("title: title").expect("the label");
    let hover = hover_at(&r, &rel, &text, label).expect("hover on the label");
    assert!(
        hover.contains("A field of `Stored`") && !hover.contains("param"),
        "the index rung must outrank the locals rung, got:\n{hover}"
    );

    // `put` — the store-op rung matches structurally off the declared field. No
    // local named `put` exists here, so this pins the rung is reached at all
    // (it sits between the index and locals rungs).
    let call = in_add_handler(&text, "items.put(id, item)");
    let op = hover_at(&r, &rel, &text, call + "items.".len()).expect("hover on `put`");
    assert!(op.contains("store operation"), "{op}");
}

/// The example this issue reproduces in must stay clean — every assertion above
/// reads real analysis output, which a broken fixture would silently empty.
#[test]
fn the_todo_example_analyses_cleanly() {
    let (r, _, _) = todos();
    let errors: Vec<_> = r
        .files
        .iter()
        .flat_map(|f| f.diagnostics.iter())
        .map(|d| d.error.category)
        .collect();
    assert!(
        errors.is_empty(),
        "examples/todo has diagnostics: {errors:?}"
    );
    assert!(!r.index.symbols.is_empty(), "the index is populated");
}
