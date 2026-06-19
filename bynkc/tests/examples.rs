//! The `examples/` projects are showcase material — this gate keeps them
//! compiling as the language moves. Each example must check cleanly, build
//! on both targets, and its tests must type-check (the runner itself needs
//! node and is exercised by the example's own instructions).

use std::path::{Path, PathBuf};

fn example_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../examples")
        .join(name)
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
