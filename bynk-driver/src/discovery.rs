//! Walking a directory tree for `.bynk` files and reading them — the driver's
//! side of `CompileOptions.sources` (#1077, R2.3/T0.7 residue).
//!
//! The real CLI entry points (`project_options`/`try_project_options` below)
//! now walk and read every file up front, via this module, and hand the
//! result to `CompileOptions::sources` — the CLI path no longer implicitly
//! relies on `bynk-emit` reading anything itself.
//!
//! `bynk-emit`'s own on-disk discovery (`project::discover_bynk_files`) and
//! `read_source`'s overlay-miss fallback are **not** removed, and can't be
//! yet: `analyse_project_with` (the LSP's own analysis path,
//! `bynk-emit/src/project.rs`) hardcodes `discovered: None` and passes an
//! overlay that deliberately covers only open editor buffers — it depends on
//! that exact fallback to see every other project file. Closing it fully is
//! #1079's scope (`bynk-lsp` becoming the sole file-content owner: the
//! open-buffer overlay *and* a full on-disk sweep for the rest), not this
//! module's. So `fs_below_driver` does not move for `bynk-emit` from this
//! alone — `discovery.rs` stays flagged until #1079 lands too. What *does*
//! move: the CLI/test compile path is now verifiably fallback-free.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bynk_emit::project::ProjectPaths;

/// Walk `root` for every `.bynk` file (skipping `excludes` and hidden
/// directories, mirroring `bynk-emit`'s former `discover_bynk_files` exactly)
/// and read each one's content. Keys are the same literal (non-canonicalised)
/// path shape a plain recursive walk produces — `CompileOptions.sources`'s own
/// contract, and what `read_source`'s overlay lookup tries first.
///
/// Panics on any I/O error (a missing root, an unreadable directory, a file
/// that vanishes between listing and reading) — every caller today already
/// treats a broken fixture tree as a test-setup bug, not a case to handle
/// gracefully, so this matches the `.unwrap()` idiom already at every call
/// site rather than inventing a `Result` threading obligation none of them had.
pub fn read_bynk_tree(root: &Path, excludes: &[PathBuf]) -> HashMap<PathBuf, String> {
    let mut out = HashMap::new();
    let is_excluded = |dir: &Path| {
        excludes.iter().any(|ex| dir == ex || dir.starts_with(ex))
            || dir
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.') && n != ".")
    };
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) => panic!("could not read directory `{}`: {e}", dir.display()),
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if !is_excluded(&p) {
                    stack.push(p);
                }
            } else if p.extension().and_then(|e| e.to_str()) == Some("bynk") {
                let text = std::fs::read_to_string(&p)
                    .unwrap_or_else(|e| panic!("could not read `{}`: {e}", p.display()));
                out.insert(p, text);
            }
        }
    }
    out
}

/// [`read_bynk_tree`] with no excludes — the common case for a single-root
/// project (`CompileOptions::single`, whose own `Roots::excludes()` is always
/// empty) and for test fixtures, which rarely declare an `exclude` list.
pub fn read_bynk_tree_single(root: &Path) -> HashMap<PathBuf, String> {
    read_bynk_tree(root, &[])
}

/// [`read_bynk_tree`] merged across a split project's up-to-two `include`
/// roots (`src`/`tests`), matching `Roots::Split`'s own `resolve()` shape —
/// the primary root's `paths.exclude` (plus the tool's own `out`/
/// `node_modules` caches) applies to both.
pub fn read_bynk_tree_split(
    primary_root: &Path,
    secondary_root: &Path,
    excludes: &[PathBuf],
) -> HashMap<PathBuf, String> {
    let mut out = read_bynk_tree(primary_root, excludes);
    if secondary_root != primary_root && secondary_root.exists() {
        out.extend(read_bynk_tree(secondary_root, excludes));
    }
    out
}

/// The complete `CompileOptions.sources` map for a [`bynk_emit::project::Roots::Split`]
/// project — every `.bynk` file under `project_root.join(paths.include[..2])`,
/// skipping `paths.exclude` and the tool's own `out`/`node_modules` caches.
/// Mirrors `Roots::resolve`/`Roots::excludes`'s own (private, single-crate)
/// logic exactly, since those live in `bynk-emit` and can't be called from here.
pub fn sources_for_split(project_root: &Path, paths: &ProjectPaths) -> HashMap<PathBuf, String> {
    let primary = project_root.join(paths.include.first().cloned().unwrap_or_default());
    let secondary = paths
        .include
        .get(1)
        .map(|p| project_root.join(p))
        .unwrap_or_else(|| primary.clone());
    let mut excludes: Vec<PathBuf> = paths.exclude.iter().map(|p| project_root.join(p)).collect();
    for cache in ["out", "node_modules"] {
        excludes.push(project_root.join(cache));
    }
    read_bynk_tree_split(&primary, &secondary, &excludes)
}
