//! Regression: `write_output` never enumerated or pruned its destination, so
//! deleting a `.bynk` unit left its previously emitted `.ts` on disk — still
//! covered by the emitted `tsconfig.json`'s `include: **/*.ts`, so a later
//! `tsc` run fails against a module the project no longer has, reported as
//! "tsc reported errors against out/tsconfig.json" with no hint that the real
//! cause is a stale build artefact.
//!
//! Builds a real two-unit project on disk, compiles it, then deletes one unit
//! and recompiles into the *same* output directory — `write_output` must
//! remove the first unit's stale `.ts` (and, separately, must not touch a
//! same-named file the pruning logic is deliberately narrow about).

use std::path::PathBuf;

fn write(path: &std::path::Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn deleting_a_unit_prunes_its_stale_emitted_output() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("write-output-reconcile-src");
    let out_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("write-output-reconcile-out");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&out_dir);

    write(
        &root.join("src/keep.bynk"),
        "commons keep\n\nfn f() -> Int {\n  1\n}\n",
    );
    write(
        &root.join("src/gone.bynk"),
        "commons gone\n\nfn g() -> Int {\n  2\n}\n",
    );

    let compile = || {
        let paths = bynkc::read_project_paths(&root);
        bynkc::compile_project(&bynkc::CompileOptions::split(root.clone(), paths)).unwrap_or_else(
            |f| {
                panic!(
                    "compile failed: {:?}",
                    f.errors
                        .iter()
                        .map(|e| (&e.source_path, e.error.category, &e.error.message))
                        .collect::<Vec<_>>()
                )
            },
        )
    };

    let out = compile();
    bynkc::write_output(&out, &out_dir).expect("write_output (first build)");
    assert!(
        out_dir.join("keep.ts").is_file(),
        "keep.ts should be written"
    );
    assert!(
        out_dir.join("gone.ts").is_file(),
        "gone.ts should be written"
    );

    // A file the compiler did not write, of a kind it's allowed to prune —
    // must survive, because it IS in the expected set (part of every build).
    assert!(
        out_dir.join("tsconfig.json").is_file(),
        "tsconfig.json should be part of every build's output"
    );

    std::fs::remove_file(root.join("src/gone.bynk")).unwrap();
    let out = compile();
    bynkc::write_output(&out, &out_dir).expect("write_output (second build)");

    assert!(
        !out_dir.join("gone.ts").exists(),
        "gone.ts should have been pruned after gone.bynk was deleted"
    );
    assert!(
        out_dir.join("keep.ts").is_file(),
        "keep.ts must still be present — pruning must not be over-broad"
    );
    assert!(
        out_dir.join("tsconfig.json").is_file(),
        "tsconfig.json is still part of the (second) build's output"
    );
}

#[test]
fn a_node_modules_subtree_is_never_descended_into() {
    // A defensive test for the exclusion itself, independent of the fixture
    // above: a `.json` file under `node_modules/` must survive reconciliation
    // even though `.json` is one of the prunable extensions, and even though
    // it is not part of any `ProjectOutput`.
    let out_dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("write-output-reconcile-node-modules");
    let _ = std::fs::remove_dir_all(&out_dir);
    write(
        &out_dir.join("node_modules/some-pkg/package.json"),
        "{\"name\":\"some-pkg\"}",
    );

    let root =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("write-output-reconcile-node-modules-src");
    let _ = std::fs::remove_dir_all(&root);
    write(
        &root.join("src/only.bynk"),
        "commons only\n\nfn f() -> Int {\n  1\n}\n",
    );
    let paths = bynkc::read_project_paths(&root);
    let out =
        bynkc::compile_project(&bynkc::CompileOptions::split(root, paths)).unwrap_or_else(|f| {
            panic!(
                "compile failed: {:?}",
                f.errors
                    .iter()
                    .map(|e| (&e.source_path, e.error.category, &e.error.message))
                    .collect::<Vec<_>>()
            )
        });
    bynkc::write_output(&out, &out_dir).expect("write_output");

    assert!(
        out_dir.join("node_modules/some-pkg/package.json").is_file(),
        "a file under node_modules must never be pruned"
    );
}
