//! The `examples/` projects are showcase material — this gate keeps them
//! compiling as the language moves. They are also the source the "Bynk by
//! Example" gallery extracts (documentation track, slice 4), so every project
//! must keep building or the gallery would ship code that no longer compiles.
//! Each example must build the deployable Worker; `hello-world` is checked more
//! deeply (both targets + the Worker output shape) as the representative.

use std::path::{Path, PathBuf};

/// Every project under `examples/`. Kept in the `examples/README.md` reading
/// order. Adding an example here gates it and lets the gallery extract it.
const EXAMPLES: &[&str] = &[
    "hello-world",
    "link-shortener",
    "feature-flags",
    "todo",
    "orders",
    "sessions",
    "event-log",
    "rate-limiter",
    "uptime-monitor",
    "webhook-relay",
];

fn example_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../examples")
        .join(name)
}

#[test]
fn every_example_builds_for_workers() {
    for name in EXAMPLES {
        let root = example_root(name);
        let paths = bynkc::read_project_paths(&root);
        bynkc::compile_project(
            &bynkc::CompileOptions::split(root.clone(), paths).target(bynkc::BuildTarget::Workers),
        )
        .unwrap_or_else(|failure| {
            panic!(
                "examples/{name} failed on Workers: {:?}",
                failure
                    .errors
                    .iter()
                    .map(|e| (&e.source_path, e.error.category, &e.error.message))
                    .collect::<Vec<_>>()
            )
        });
    }
}

#[test]
fn hello_world_builds_on_both_targets() {
    let root = example_root("hello-world");
    let paths = bynkc::read_project_paths(&root);
    for target in [bynkc::BuildTarget::Bundle, bynkc::BuildTarget::Workers] {
        let out = bynkc::compile_project(
            &bynkc::CompileOptions::split(root.clone(), paths.clone()).target(target),
        )
        .unwrap_or_else(|failure| {
            panic!(
                "examples/hello-world failed on {target:?}: {:?}",
                failure
                    .errors
                    .iter()
                    .map(|e| (&e.source_path, e.error.category, &e.error.message))
                    .collect::<Vec<_>>()
            )
        });
        assert!(!out.files.is_empty());
    }
    // The workers build must produce the deployable Worker directory.
    let workers = bynkc::compile_project(
        &bynkc::CompileOptions::split(root, paths).target(bynkc::BuildTarget::Workers),
    )
    .unwrap_or_else(|_| panic!("workers build failed"));
    for needed in [
        "workers/hello-web/index.ts",
        "workers/hello-web/wrangler.toml",
        "runtime.ts",
    ] {
        assert!(
            workers
                .files
                .iter()
                .any(|f| f.output_path == Path::new(needed)),
            "workers output must include {needed}"
        );
    }
}
