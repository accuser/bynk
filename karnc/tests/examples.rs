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
    let paths = karnc::read_project_paths(&root);
    for target in [karnc::BuildTarget::Bundle, karnc::BuildTarget::Workers] {
        let out = karnc::compile_project_with_split_paths_full(&root, target, &paths)
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
    let workers =
        karnc::compile_project_with_split_paths_full(&root, karnc::BuildTarget::Workers, &paths)
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
