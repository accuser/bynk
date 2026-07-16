//! Slice A (#647): the LSP analyses the project `bynkc` compiles.
//!
//! The defect: `bynk-lsp` reduced `bynk.toml`'s `[paths] include` to a single
//! `src_dir` string — first entry only, `exclude` ignored — and handed
//! `<root>/src` to the analyser *as* a project root. So a secondary `include`
//! tree was invisible, `exclude` did nothing, and a flat project failed.
//!
//! These fixtures measure the **file set**, which is the thing that was wrong.
//! They read sibling directories (`../examples`, `../bynkc`) deliberately — the
//! point is agreement with the real compiler on real projects — so this file
//! rides `Cargo.toml`'s `exclude` list alongside `hover_references.rs` and
//! `declaration_spans.rs`, keeping it out of the published tarball where those
//! siblings do not exist.
//!
//! They go through `bynk_ide` rather than `Backend` because `bynk-lsp` has no
//! `[lib]` target (that is slice C). What `Backend` itself does with these roots
//! is pinned in-crate, hermetically, in `main.rs`'s test module.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn rel_files(root: &Path) -> Vec<String> {
    let r = bynk_ide::diagnose_project_with(
        &bynk_ide::AnalysisRoots::Project(root.to_path_buf()),
        &HashMap::new(),
    );
    let mut v: Vec<String> = r
        .files
        .iter()
        .map(|f| f.source_path.to_string_lossy().replace('\\', "/"))
        .collect();
    v.sort();
    v
}

/// The regression this slice exists for. `examples/todo/bynk.toml` declares no
/// `[paths]`, so `ProjectPaths::conventional` yields `include = ["src",
/// "tests"]` — and `examples/todo/tests/todos.bynk` is a real `suite` the
/// compiler compiles. Before slice A the LSP could not see it at all: the
/// analyser was handed `<root>/src`.
///
/// Also pins ADR 0198's identity: the two files share the basename `todos.bynk`
/// and must not collide.
#[test]
fn the_todo_example_s_tests_tree_is_analysed() {
    let root = workspace().join("examples/todo");
    assert!(root.join("tests/todos.bynk").is_file(), "fixture present");
    assert!(
        !root
            .join("bynk.toml")
            .read_to_string_lossy()
            .contains("[paths]"),
        "the example must still declare no [paths] — that is what makes it the fixture",
    );

    let files = rel_files(&root);
    assert!(
        files.contains(&"tests/todos.bynk".to_string()),
        "the LSP must analyse the tests tree `bynkc` compiles; got {files:?}",
    );
    assert!(
        files.contains(&"src/todos.bynk".to_string()),
        "…without losing the src tree; got {files:?}",
    );
}

trait ReadLossy {
    fn read_to_string_lossy(&self) -> String;
}
impl ReadLossy for PathBuf {
    fn read_to_string_lossy(&self) -> String {
        std::fs::read_to_string(self).unwrap_or_default()
    }
}

// -- the manifest matrix --------------------------------------------------
//
// Each case is a real project on disk, analysed through the roots the LSP now
// resolves. These are hermetic (no sibling reads) but live here beside the
// `examples/todo` regression because they are the same claim at different
// layouts: the LSP's file set is the manifest's, not a guess.

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str, files: &[(&str, &str)]) -> Scratch {
    let dir = std::env::temp_dir().join(format!(
        "bynk_sliceA_{tag}_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
    }
    Scratch(dir)
}

const MANIFEST: &str = "[project]\nname = \"m\"\n";

/// A conventional `src/`-only project: one `include` root, discovered without
/// any `[paths]` at all.
#[test]
fn conventional_src_only() {
    let s = scratch(
        "srconly",
        &[("bynk.toml", MANIFEST), ("src/a.bynk", "context a\n")],
    );
    assert_eq!(rel_files(&s.0), vec!["src/a.bynk"]);
}

/// The layout `examples/todo` has: `conventional()` picks up `tests/` because
/// it exists. The primary tree alone is what the LSP used to see.
#[test]
fn conventional_src_and_tests() {
    let s = scratch(
        "srctests",
        &[
            ("bynk.toml", MANIFEST),
            ("src/a.bynk", "context a\n"),
            ("tests/a.bynk", "suite a\n"),
        ],
    );
    assert_eq!(rel_files(&s.0), vec!["src/a.bynk", "tests/a.bynk"]);
}

/// An explicit two-root `include` with names that are not `src`/`tests` — the
/// flat layout ADR 0147 settled, where test-ness is structural rather than
/// directory-based.
#[test]
fn explicit_two_root_include() {
    let s = scratch(
        "tworoot",
        &[
            (
                "bynk.toml",
                "[project]\nname = \"m\"\n\n[paths]\ninclude = [\"lib\", \"spec\"]\n",
            ),
            ("lib/a.bynk", "context a\n"),
            ("spec/a.bynk", "suite a\n"),
            // Not under any include root — must not be analysed.
            ("src/stray.bynk", "context stray\n"),
        ],
    );
    assert_eq!(rel_files(&s.0), vec!["lib/a.bynk", "spec/a.bynk"]);
}

/// A flat project: `.bynk` at the root, no `src/`. `conventional()` yields
/// `include = ["."]`. The LSP used to look for a nonexistent `src/` and find
/// nothing; ADR 0198 normalises the `.` so paths stay unprefixed.
#[test]
fn flat_project_with_a_manifest() {
    let s = scratch(
        "flat",
        &[("bynk.toml", MANIFEST), ("a.bynk", "context a\n")],
    );
    assert_eq!(
        rel_files(&s.0),
        vec!["a.bynk"],
        "a flat project's paths are unprefixed — never `./a.bynk`"
    );
}

/// `exclude` is honoured. The LSP previously parsed it and threw it away
/// (`project.rs` said so in a comment), so a generated tree was analysed.
#[test]
fn exclude_is_honoured() {
    let s = scratch(
        "exclude",
        &[
            (
                "bynk.toml",
                "[project]\nname = \"m\"\n\n[paths]\ninclude = [\".\"]\nexclude = [\"generated\"]\n",
            ),
            ("a.bynk", "context a\n"),
            ("generated/gen.bynk", "context gen\n"),
        ],
    );
    assert_eq!(
        rel_files(&s.0),
        vec!["a.bynk"],
        "an excluded subtree must not be analysed"
    );
}

/// The tool's own caches are skipped even when `include` is the project root —
/// otherwise a flat project would sweep its own build output back in.
#[test]
fn out_and_node_modules_are_skipped() {
    let s = scratch(
        "caches",
        &[
            ("bynk.toml", MANIFEST),
            ("a.bynk", "context a\n"),
            ("out/emitted.bynk", "context emitted\n"),
            ("node_modules/dep/d.bynk", "context d\n"),
        ],
    );
    assert_eq!(rel_files(&s.0), vec!["a.bynk"]);
}
