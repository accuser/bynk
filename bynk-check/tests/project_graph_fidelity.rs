//! P8.3 (#1514): proves [`bynk_check::project_graph::project_graph_for`]
//! isn't just plausible — its `edges` agree with `project_model.rs`'s own
//! resolved `unit_uses`/`unit_consumes` maps on a real, multi-unit fixture
//! project run through the full discovery→parse→group→resolve pipeline
//! (`bynk_check::analysis::analyse_project`'s own phase sequence), not a
//! hand-built map that could accidentally paper over a real divergence.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use bynk_check::project_graph::{EdgeKind, project_graph_for};
use bynk_check::project_model;
use bynk_project::Roots;

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

const CONSUMER_SRC: &str = r#"context demo.svc

uses demo.shared

consumes demo.time { Clock }

service api from http {
  on GET("/widgets/:id") (id: String) -> Effect[HttpResult[Widget]] by Visitor given Clock {
    Ok(Widget { id: id })
  }
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
/// builds a `ProjectGraph` from the result.
fn project_graph_for_fixture(
    root: &std::path::Path,
    overlay: &HashMap<PathBuf, String>,
) -> bynk_check::project_graph::ProjectGraph {
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

    project_graph_for(&parsed, &groups, &kinds, &unit_uses, &unit_consumes)
}

#[test]
fn project_graph_edges_agree_with_the_resolved_uses_and_consumes_maps_on_a_real_fixture() {
    let (root, overlay) = setup_project(
        "three-units",
        &[
            ("demo/shared.bynk", SHARED_SRC),
            ("demo/time.bynk", PROVIDER_SRC),
            ("demo/svc.bynk", CONSUMER_SRC),
        ],
    );

    let graph = project_graph_for_fixture(&root, &overlay);

    // Every unit discovered by the real pipeline is present.
    let unit_names: HashSet<String> = graph.units.keys().map(|id| id.0.clone()).collect();
    assert_eq!(
        unit_names,
        HashSet::from([
            "demo.shared".to_string(),
            "demo.time".to_string(),
            "demo.svc".to_string(),
        ])
    );

    // `demo.svc` uses `demo.shared` — a real, resolved `uses` fact.
    assert!(graph.edges.contains(&(
        bynk_check::unit_signature::UnitId("demo.svc".to_string()),
        bynk_check::unit_signature::UnitId("demo.shared".to_string()),
        EdgeKind::Uses,
    )));

    // `demo.svc` consumes `demo.time` — a real, resolved `consumes` fact —
    // and, per [DECISION C], `demo.time` provides to `demo.svc` as the dual.
    assert!(graph.edges.contains(&(
        bynk_check::unit_signature::UnitId("demo.svc".to_string()),
        bynk_check::unit_signature::UnitId("demo.time".to_string()),
        EdgeKind::Consumes,
    )));
    assert!(graph.edges.contains(&(
        bynk_check::unit_signature::UnitId("demo.time".to_string()),
        bynk_check::unit_signature::UnitId("demo.svc".to_string()),
        EdgeKind::Provides,
    )));

    // No other edges exist — `demo.shared`/`demo.time` neither use nor
    // consume anything, so the edge set above is exhaustive, not partial.
    assert_eq!(graph.edges.len(), 3, "{:#?}", graph.edges);

    // `files` is the exact reverse of every `Unit::files` entry.
    for (id, unit) in &graph.units {
        for file in &unit.files {
            assert_eq!(graph.files.get(file), Some(id));
        }
    }
}
