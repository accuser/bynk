//! v0.34 (ADR 0067): the call-hierarchy graph — caller→callee `CallEdge`s
//! assembled by preserving each `RefEdge`'s `owner` (resolved to the caller's
//! `SymbolKey`). `Fn` callees only; any indexed owner may be a caller; method
//! owners (`"T.m"`) are not index symbols and so record no edge — the same
//! boundary as the deferred index kinds. The fixture exercises all three:
//! a free fn caller, a service caller, and the excluded method caller.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use karnc::index::{ProjectIndex, SymbolKey, SymbolKind};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/positive/215_call_hierarchy/src")
}

fn analyse(root: &Path) -> ProjectIndex {
    let result = karnc::diagnose_project(root, &HashMap::new());
    for f in &result.files {
        assert!(
            f.diagnostics.is_empty(),
            "fixture should be clean, got in {}: {:?}",
            f.source_path.display(),
            f.diagnostics
                .iter()
                .map(|d| d.error.category)
                .collect::<Vec<_>>()
        );
    }
    result.index
}

fn key(unit: &str, kind: SymbolKind, name: &str) -> SymbolKey {
    SymbolKey {
        unit: unit.to_string(),
        kind,
        name: name.to_string(),
    }
}

#[test]
fn call_graph_records_fn_and_service_callers_and_excludes_methods() {
    let index = analyse(&fixture_root());

    let helper = key("callgraph", SymbolKind::Fn, "helper");
    let caller = key("callgraph", SymbolKind::Fn, "caller");
    let api = key("callgraph", SymbolKind::Service, "api");

    // Incoming: `helper` is called by the free fn `caller` and by the method
    // `Counter.bump`. Only the free-fn caller is an indexed owner, so the
    // method call records no edge.
    let into_helper: Vec<&SymbolKey> = index.calls_into(&helper).map(|e| &e.caller).collect();
    assert_eq!(
        into_helper,
        vec![&caller],
        "helper's only call edge is from the free fn `caller`; the `Counter.bump` method caller is excluded"
    );

    // The exclusion is visible against the plain reference set: `helper` has
    // two reference sites (the fn call and the method call) but one call edge.
    assert_eq!(
        index.symbols[&helper].refs.len(),
        2,
        "helper is referenced from both `caller` and `Counter.bump`"
    );
    assert_eq!(index.calls_into(&helper).count(), 1);

    // Incoming: `caller` is called by the service handler `api`.
    let into_caller: Vec<&SymbolKey> = index.calls_into(&caller).map(|e| &e.caller).collect();
    assert_eq!(
        into_caller,
        vec![&api],
        "an indexed service owner is a valid caller"
    );

    // Outgoing mirrors incoming off the same table.
    let from_caller: Vec<&SymbolKey> = index.calls_from(&caller).map(|e| &e.callee).collect();
    assert_eq!(from_caller, vec![&helper]);
    let from_api: Vec<&SymbolKey> = index.calls_from(&api).map(|e| &e.callee).collect();
    assert_eq!(from_api, vec![&caller]);

    // `helper` calls nothing.
    assert!(index.calls_from(&helper).next().is_none());

    // Each call site spells the callee name (sanity: spans are name segments).
    for e in &index.calls {
        assert!(
            !e.site.span.range().is_empty(),
            "call-site span must be non-empty"
        );
    }
}

#[test]
fn unknown_key_has_no_calls() {
    let index = analyse(&fixture_root());
    let ghost = key("callgraph", SymbolKind::Fn, "nope");
    assert_eq!(index.calls_into(&ghost).count(), 0);
    assert_eq!(index.calls_from(&ghost).count(), 0);
}
