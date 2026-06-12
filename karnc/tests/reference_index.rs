//! v0.25 (ADR 0053): the project-wide binding index — binding-correct
//! use→def edges for every in-scope symbol kind, assembled on the v0.24
//! project pass. The fixture matrix exercises every must-capture kind from
//! the proposal: cross-file types (annotation and static-receiver position),
//! fns, capabilities (bare `given`, dotted `B.Cap`, flattened
//! `consumes U { Cap }`), clause-list occurrences (`exports`, `consumes`
//! selection), cross-context service calls, and test-unit references — and
//! proves same-named symbols in different units are not conflated.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use karnc::index::{ProjectIndex, SymbolKey, SymbolKind};

fn smoke_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/positive/105_context_test_with_consumed_mock/src")
}

fn analyse(root: &Path) -> (ProjectIndex, HashMap<String, String>) {
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
    let snapshots = result
        .files
        .iter()
        .map(|f| {
            (
                f.source_path.to_string_lossy().replace('\\', "/"),
                f.text.clone(),
            )
        })
        .collect();
    (result.index, snapshots)
}

fn key(unit: &str, kind: SymbolKind, name: &str) -> SymbolKey {
    SymbolKey {
        unit: unit.to_string(),
        kind,
        name: name.to_string(),
    }
}

/// Every recorded span must cover exactly the symbol's name segment.
fn assert_sites_spell(
    index: &ProjectIndex,
    snapshots: &HashMap<String, String>,
    k: &SymbolKey,
    expected_at: &[&str],
) {
    let entry = index
        .symbols
        .get(k)
        .unwrap_or_else(|| panic!("symbol {k:?} missing from index"));
    let def = entry.def.as_ref().expect("def site");
    let mut seen_files: Vec<String> = Vec::new();
    for site in std::iter::once(def).chain(entry.refs.iter()) {
        let path = site.path.to_string_lossy().replace('\\', "/");
        let text = snapshots
            .get(&path)
            .unwrap_or_else(|| panic!("no snapshot for {path}"));
        let segment = &text[site.span.range()];
        assert_eq!(
            segment, k.name,
            "site span in {path} must cover the name segment only"
        );
        seen_files.push(path);
    }
    for f in expected_at {
        assert!(
            seen_files.iter().any(|s| s == f),
            "{k:?}: expected a site in {f}, saw {seen_files:?}"
        );
    }
}

fn matrix_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/index/refproject/src")
}

fn refs_in<'a>(
    index: &'a ProjectIndex,
    k: &SymbolKey,
    file: &str,
) -> Vec<&'a karnc::index::SiteRef> {
    index.symbols[k]
        .refs
        .iter()
        .filter(|s| s.path.to_string_lossy().replace('\\', "/") == file)
        .collect()
}

#[test]
fn cross_file_type_and_fn_resolve_to_one_definition() {
    let (index, snapshots) = analyse(&matrix_root());

    // Type: annotation position in its own file and cross-file (billing's
    // `fn fee(m: Money)`), static-receiver position (`Money.of`).
    let money = key("shop.util", SymbolKind::Type, "Money");
    assert_sites_spell(
        &index,
        &snapshots,
        &money,
        &["shop/util.karn", "shop/billing.karn"],
    );
    // Static-receiver: `Money.of(x)` records the receiver segment, so
    // util.karn carries both the annotation and the receiver refs.
    assert!(
        refs_in(&index, &money, "shop/util.karn").len() >= 2,
        "annotation + static-receiver refs in util.karn"
    );

    // Qualified variant receiver: `Status.Open` references `Status`.
    assert_sites_spell(
        &index,
        &snapshots,
        &key("shop.util", SymbolKind::Type, "Status"),
        &["shop/util.karn"],
    );

    // Fn: called cross-file (billing) and from the test unit.
    assert_sites_spell(
        &index,
        &snapshots,
        &key("shop.util", SymbolKind::Fn, "double"),
        &[
            "shop/util.karn",
            "shop/billing.karn",
            "tests/billing.test.karn",
        ],
    );
}

#[test]
fn same_named_symbols_in_different_units_are_not_conflated() {
    let (index, _) = analyse(&matrix_root());

    // `other.util` declares its own `Money` and `double`; nothing imports
    // it, so neither carries any reference — and the `shop.util` symbols'
    // references never land in other/util.karn.
    let other_money = key("other.util", SymbolKind::Type, "Money");
    let other_double = key("other.util", SymbolKind::Fn, "double");
    assert!(index.symbols[&other_money].refs.is_empty());
    assert!(index.symbols[&other_double].refs.is_empty());

    for k in [
        key("shop.util", SymbolKind::Type, "Money"),
        key("shop.util", SymbolKind::Fn, "double"),
    ] {
        assert!(
            refs_in(&index, &k, "other/util.karn").is_empty(),
            "{k:?} must not pick up other.util's same-named declaration sites"
        );
    }
}

#[test]
fn capability_references_cover_every_clause_and_call_form() {
    let (index, snapshots) = analyse(&matrix_root());
    let pay = key("shop.billing", SymbolKind::Capability, "Pay");

    // Every site spells exactly `Pay` — dotted `shop.billing.Pay` and
    // flattened forms record the name segment only.
    assert_sites_spell(
        &index,
        &snapshots,
        &pay,
        &["shop/billing.karn", "shop/checkout.karn", "ops/audit.karn"],
    );

    // Declaring context: `exports capability { Pay }`, `provides Pay`,
    // `given Pay`, `Pay.charge(...)`.
    assert_eq!(refs_in(&index, &pay, "shop/billing.karn").len(), 4);
    // Flattening consumer: `consumes shop.billing { Pay }` selection,
    // `given Pay` (flattened bare), `Pay.charge(...)`.
    assert_eq!(refs_in(&index, &pay, "shop/checkout.karn").len(), 3);
    // Dotted consumer: `given shop.billing.Pay`, `shop.billing.Pay.charge`.
    assert_eq!(refs_in(&index, &pay, "ops/audit.karn").len(), 2);
}

#[test]
fn cross_context_service_call_and_type_export_clause_are_indexed() {
    let (index, snapshots) = analyse(&matrix_root());

    // `shop.billing.bill(3)` in ops/audit.karn references the service, and
    // the test body's `bill.call(d)` references it from the test unit.
    assert_sites_spell(
        &index,
        &snapshots,
        &key("shop.billing", SymbolKind::Service, "bill"),
        &[
            "shop/billing.karn",
            "ops/audit.karn",
            "tests/billing.test.karn",
        ],
    );

    // `exports transparent { Receipt }` is a reference to the type.
    let receipt = key("shop.billing", SymbolKind::Type, "Receipt");
    assert_sites_spell(&index, &snapshots, &receipt, &["shop/billing.karn"]);
    assert!(
        refs_in(&index, &receipt, "shop/billing.karn").len() >= 3,
        "exports clause + return annotation + construction"
    );
}

#[test]
fn smoke_existing_fixture_indexes_cleanly() {
    let (index, snapshots) = analyse(&smoke_root());
    // The consumed context's exported type is referenced from the consuming
    // context (annotation position) and the test file (mock signature).
    assert_sites_spell(
        &index,
        &snapshots,
        &key("commerce.payment", SymbolKind::Type, "AuthId"),
        &[
            "commerce/payment.karn",
            "commerce/orders.karn",
            "tests/orders.test.karn",
        ],
    );
    // The capability declared and provided in the payment context.
    assert_sites_spell(
        &index,
        &snapshots,
        &key("commerce.payment", SymbolKind::Capability, "Logger"),
        &["commerce/payment.karn"],
    );
}
