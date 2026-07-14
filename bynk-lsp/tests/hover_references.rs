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
//! `legend_drift.rs` established). The hover request itself lives on `Backend`
//! and cannot be called from here, so [`hover_at`] mirrors `Backend::hover`'s
//! resolution ladder — the ordering *is* the behaviour under test (gap B is a
//! fall-through bug), so it is replayed rather than approximated.

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
#[path = "../src/symbols.rs"]
mod symbols;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bynk_check::locals::LocalBinding;
use bynk_ide::ProjectDiagnostics;

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

/// Mirrors `Backend::hover`'s resolution ladder over analysed project output.
/// Each rung is the one the real handler runs, in the same order.
fn hover_at(r: &ProjectDiagnostics, rel: &Path, text: &str, offset: usize) -> Option<String> {
    let empty: Vec<LocalBinding> = Vec::new();
    let locals = r
        .locals
        .iter()
        .find(|(p, _)| p.to_string_lossy().ends_with("todos.bynk"))
        .map(|(_, l)| l)
        .unwrap_or(&empty);

    // 1. Binding-index path: a resolved symbol → describe its declaration.
    if let Some((key, _def)) = index_queries::definition_at(&r.index, rel, offset)
        && let Some(content) = symbols::describe_symbol(text, &key.name)
    {
        return Some(content);
    }
    // 2. #611 (gap C): a `store` field's operation.
    if let Some(content) = symbols::describe_store_op_at(text, offset, locals) {
        return Some(content);
    }
    // 3. A local / parameter → its inferred type.
    if let Some(content) = locals_nav::describe_local_at(locals, text, offset) {
        return Some(content);
    }
    // 4. The lexical fallback: a top-level declaration by name, then the agent
    //    `key`/`store` state path.
    let name = ident_at(text, offset)?;
    symbols::describe_symbol(text, name).or_else(|| symbols::describe_agent_state_at(text, offset))
}

fn ident_at(text: &str, offset: usize) -> Option<&str> {
    let tokens = bynk_syntax::lexer::tokenize_expanding_holes(text).ok()?;
    tokens
        .iter()
        .find(|t| {
            t.kind == bynk_syntax::lexer::TokenKind::Ident
                && t.span.start <= offset
                && offset < t.span.end
        })
        .map(|t| &text[t.span.start..t.span.end])
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
