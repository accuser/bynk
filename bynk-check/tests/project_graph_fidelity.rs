//! P8.3 (#1514): proves [`bynk_check::project_graph::project_graph_for`]
//! isn't just plausible — its `edges` agree with `project_model.rs`'s own
//! resolved `unit_uses`/`unit_consumes` maps on a real, multi-unit fixture
//! project run through the full discovery→parse→group→resolve pipeline
//! (`bynk_check::analysis::analyse_project`'s own phase sequence), not a
//! hand-built map that could accidentally paper over a real divergence.
//!
//! PR #1519's own bot review (finding #2): the first draft's fixture had one
//! file per unit and no first-party injection, so the reverse-map invariant
//! (`graph.files.get(file) == Some(id)`) was only ever exercised in the case
//! that cannot fail — a unit with more than one file, or two units colliding
//! on the same `FileId`, were both unreachable. `expected_edges` below also
//! derives the expected edge set generically from `unit_uses`/`unit_consumes`
//! instead of three hardcoded tuples, so this fixture (and any later one)
//! proves genuine agreement, not just agreement with itself for one
//! hand-picked shape.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use bynk_check::project_graph::{EdgeKind, ProjectGraph, project_graph_for};
use bynk_check::project_model;
use bynk_check::unit_signature::UnitId;
use bynk_project::Roots;
use bynk_syntax::span::FileId;

const SHARED_SRC: &str = "commons demo.shared\n\ntype Widget = {\n  id: String,\n}\n";

const PROVIDER_SRC: &str = r#"context demo.time

exports capability { Clock }

capability Clock {
  fn now() -> Effect[Int]
}

provides Clock = SystemClock {
  fn now() -> Effect[Int] {
    0
  }
}
"#;

/// One of two files contributing to `demo.svc` (R3.7: "a unit may be
/// contributed to by many files") — [DECISION-adjacent, PR #1519's own bot
/// review finding #2] proving `Unit::files` genuinely collects every
/// contributing file, not just the first or last.
const CONSUMER_SRC_A: &str = r#"context demo.svc

uses demo.shared

consumes demo.time { Clock }

service api from http {
  on GET("/widgets/:id") (id: String) -> Effect[HttpResult[Widget]] by Visitor given Clock {
    Ok(Widget { id: id })
  }
}
"#;

const CONSUMER_SRC_B: &str = r#"context demo.svc

fn helper(x: Int) -> Int {
  x
}
"#;

/// Triggers first-party synthetic-unit injection (`bynk.map`, and — per the
/// `uses_map || uses_locale || uses bynk.list` cascade,
/// `project_model.rs:503` — `bynk.list` too). PR #1519's own bot review
/// (finding #1): those synthetic units are tokenised via `lexer::tokenize`,
/// which stamps every span `FileId::UNKNOWN` — the case that made the
/// original fixture's reverse-map invariant vacuously true.
const MAPPER_SRC: &str = r#"context demo.mapper

uses bynk.map

fn noop() -> Int {
  0
}
"#;

fn setup_project(test_name: &str, files: &[(&str, &str)]) -> (PathBuf, HashMap<PathBuf, String>) {
    let root = std::env::temp_dir().join(format!(
        "bynk-check-project-graph-fidelity-{test_name}-{}",
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

/// Runs `project_model.rs`'s own discovery→parse→group→resolve phase
/// sequence (the same one `analysis::analyse_project` runs, up to the point
/// `unit_uses`/`unit_consumes` exist) against a real fixture project, then
/// builds a `ProjectGraph` from the result. Returns the graph alongside the
/// two resolved maps it was built from, so a caller can derive the expected
/// edge set independently rather than trusting the builder's own output.
#[allow(clippy::type_complexity)]
fn project_graph_for_fixture(
    root: &std::path::Path,
    overlay: &HashMap<PathBuf, String>,
) -> (
    ProjectGraph,
    HashMap<String, Vec<String>>,
    HashMap<String, Vec<String>>,
) {
    let roots = Roots::Single(root.to_path_buf());
    let trees = roots.trees();
    let excludes = roots.excludes();

    let mut errors = project_model::ErrorSink::new();
    let mut snapshots = Vec::new();

    let file_lists =
        project_model::phase_discovery(&trees, &excludes, &mut errors).expect("discovery");
    let (parsed, consumes_bynk, consumes_cloudflare) =
        project_model::phase_parse(&trees, &file_lists, overlay, &mut errors, &mut snapshots)
            .expect("parse");

    let (groups, kinds, _test_groups, _integration_groups, _adapter_bindings, _npm_deps) =
        project_model::phase_group(
            &parsed,
            &trees,
            bynk_check::firstparty::Platform::default(),
            consumes_bynk,
            consumes_cloudflare,
            overlay,
            &mut errors,
        );

    let unit_tables = project_model::phase_symbol_tables(&groups, &kinds, &parsed, &mut errors);
    let unit_uses =
        project_model::phase_resolve_uses(&groups, &kinds, &parsed, &unit_tables, &mut errors);
    let mut refs = bynk_check::index::RefSink::new();
    let (unit_consumes, _unit_flattened) = project_model::phase_resolve_consumes(
        &groups,
        &kinds,
        &parsed,
        &unit_tables,
        &mut errors,
        &mut refs,
    );

    assert!(
        errors.is_empty(),
        "fixture must resolve cleanly: {:?}",
        errors
            .into_all()
            .iter()
            .map(|a| (a.error.category, a.error.message.clone()))
            .collect::<Vec<_>>()
    );

    let graph = project_graph_for(&parsed, &groups, &kinds, &unit_uses, &unit_consumes);
    (graph, unit_uses, unit_consumes)
}

/// The independently-derived expected edge set: every `uses`/`consumes` edge
/// `unit_uses`/`unit_consumes` names, plus the `Provides` dual of each
/// `consumes` edge ([DECISION C]) — computed the same way
/// `project_graph_for`'s own doc comment describes its builder working, but
/// written here from scratch rather than by calling into it, so this
/// fixture can't pass merely because it agrees with itself.
fn expected_edges(
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
) -> BTreeSet<(UnitId, UnitId, EdgeKind)> {
    let mut expected = BTreeSet::new();
    for (name, targets) in unit_uses {
        for target in targets {
            expected.insert((UnitId(name.clone()), UnitId(target.clone()), EdgeKind::Uses));
        }
    }
    for (name, targets) in unit_consumes {
        for target in targets {
            expected.insert((
                UnitId(name.clone()),
                UnitId(target.clone()),
                EdgeKind::Consumes,
            ));
            expected.insert((
                UnitId(target.clone()),
                UnitId(name.clone()),
                EdgeKind::Provides,
            ));
        }
    }
    expected
}

#[test]
fn project_graph_edges_agree_with_the_resolved_uses_and_consumes_maps_on_a_real_fixture() {
    let (root, overlay) = setup_project(
        "multi-file-and-firstparty",
        &[
            ("demo/shared.bynk", SHARED_SRC),
            ("demo/time.bynk", PROVIDER_SRC),
            ("demo/svc/a.bynk", CONSUMER_SRC_A),
            ("demo/svc/b.bynk", CONSUMER_SRC_B),
            ("demo/mapper.bynk", MAPPER_SRC),
        ],
    );

    let (graph, unit_uses, unit_consumes) = project_graph_for_fixture(&root, &overlay);

    // Every production unit discovered by the real pipeline is present,
    // including the two synthetic first-party units `uses bynk.map` pulls in
    // (`bynk.map` itself, and `bynk.list` via the cascade).
    let unit_names: HashSet<String> = graph.units.keys().map(|id| id.0.clone()).collect();
    assert_eq!(
        unit_names,
        HashSet::from([
            "demo.shared".to_string(),
            "demo.time".to_string(),
            "demo.svc".to_string(),
            "demo.mapper".to_string(),
            "bynk.map".to_string(),
            "bynk.list".to_string(),
        ])
    );

    // The edge set agrees with an independently-derived expectation — not
    // just with itself.
    let actual: BTreeSet<(UnitId, UnitId, EdgeKind)> = graph.edges.iter().cloned().collect();
    assert_eq!(actual, expected_edges(&unit_uses, &unit_consumes));

    // `demo.svc` is contributed to by both its files (R3.7).
    let svc = &graph.units[&UnitId("demo.svc".to_string())];
    assert_eq!(svc.files.len(), 2, "{:?}", svc.files);

    // A synthetic first-party unit has no on-disk file — `files` is empty,
    // not a `FileId::UNKNOWN` sentinel (PR #1519's own bot review, finding #1).
    let map_unit = &graph.units[&UnitId("bynk.map".to_string())];
    assert!(map_unit.files.is_empty(), "{:?}", map_unit.files);
    let list_unit = &graph.units[&UnitId("bynk.list".to_string())];
    assert!(list_unit.files.is_empty(), "{:?}", list_unit.files);

    // `FileId::UNKNOWN` never becomes a key in the reverse map.
    assert!(!graph.files.contains_key(&FileId::UNKNOWN));

    // `files` is the exact reverse of every `Unit::files` entry, for every
    // unit — including the two-file one, so a builder that only recorded the
    // first or last contributing file would fail this.
    for (id, unit) in &graph.units {
        for file in &unit.files {
            assert_eq!(graph.files.get(file), Some(id));
        }
    }
    assert_eq!(
        graph.files.len(),
        graph.units.values().map(|u| u.files.len()).sum::<usize>(),
        "no two units may share a FileId"
    );
}
