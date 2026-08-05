//! Walking a directory tree for `.bynk` files and reading them — the driver's
//! side of `CompileOptions.sources` (#1077, R2.3/T0.7 residue).
//!
//! The real CLI entry points (`project_options`/`try_project_options` below)
//! now walk and read every file up front, via this module, and hand the
//! result to `CompileOptions::sources`.
//!
//! #1077 review: this module's own original doc claimed the CLI path was
//! already fully independent of `bynk-emit`'s disk reads — false as written.
//! `project_options`/`try_project_options` still read `bynk.toml` itself via
//! `read_project_paths`/`try_read_project_paths`, which reach
//! `discovery::read_source`'s overlay-miss fallback (`fs::read_to_string`)
//! with an always-empty overlay — one real disk read inside `bynk-emit` on
//! every real CLI invocation, unnoticed because `fs_below_driver` scans
//! `bynk-emit`'s own text, not who calls into it. Both entry points now read
//! `bynk.toml` themselves (`manifest_overlay`) and hand it to
//! `try_read_project_paths_with`'s overlay instead, so that read genuinely
//! moves above `bynk-emit` **when `bynk.toml` exists**. A conventional
//! project with no manifest at all still reaches `try_read_project_paths_with`
//! with an empty overlay, which still tries (and fails to find) `bynk.toml`
//! via `read_source`'s own `fs::read_to_string` before falling back to
//! `ProjectPaths::conventional` — a real, if harmless (the read fails and is
//! discarded), disk touch still inside `bynk-emit` for that case.
//!
//! `bynk-emit`'s own on-disk discovery (`project::discover_bynk_files`) and
//! `read_source`'s overlay-miss fallback are **not** removed, and can't be
//! yet: `analyse_project_with` (the LSP's own analysis path,
//! `bynk-emit/src/project.rs`) hardcodes `discovered: None` and passes an
//! overlay that deliberately covers only open editor buffers — it depends on
//! that exact fallback to see every other project file, including its own
//! `bynk.toml` read (`AnalysisRoots::lower`, `bynk-ide/src/lib.rs`, has the
//! same gap this module just closed for the CLI, unclosed). `#1079`'s issue
//! text scopes only `bynk-ide`'s `completion.rs`/`symbols.rs` — it does not
//! mention `analyse_project_with` or the `diagnose_project(&root,
//! &HashMap::new())` pattern that is `bynk-ide`'s (and `bynkc`'s test suite's)
//! dominant way of exercising this whole analysis path (100+ call sites
//! spanning `bynk-ide`'s own inline test modules, `bynk-lsp/tests`, and
//! `bynkc/tests`) — every one depends on `read_source`'s fallback today.
//! Closing `discovery.rs` for good needs all of that migrated first, which is
//! bigger than either #1077 or #1079's issue text currently describes; #1077
//! stays open pending that combined, design-reviewed effort. `fs_below_driver`
//! does not move for `bynk-emit` from this module alone.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bynk_emit::project::Roots;

/// An I/O failure while walking a project tree for `.bynk` files, or reading
/// one found there.
///
/// #1081 review: `read_bynk_tree` used to `panic!` on any such failure, on the
/// premise that every caller was a test fixture. That stopped being true the
/// moment `project_options`/`try_project_options` (below) became its
/// production callers — a `bynk.toml` with an `include` root that does not
/// exist yet (or any other unreadable directory) is a normal, recoverable
/// input on the live CLI path, not a broken fixture, and `try_project_options`
/// already has a `Result` error channel for exactly this kind of thing.
#[derive(Debug)]
pub struct DiscoveryError {
    path: PathBuf,
    source: std::io::Error,
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "could not read project files under `{}`: {}",
            self.path.display(),
            self.source
        )
    }
}

impl std::error::Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Walk `root` for every `.bynk` file (skipping `excludes` and hidden
/// directories, mirroring `bynk-emit`'s former `discover_bynk_files` exactly)
/// and read each one's content. Keys are the same literal (non-canonicalised)
/// path shape a plain recursive walk produces — `CompileOptions.sources`'s own
/// contract, and what `read_source`'s overlay lookup tries first.
pub fn read_bynk_tree(
    root: &Path,
    excludes: &[PathBuf],
) -> Result<HashMap<PathBuf, String>, DiscoveryError> {
    let mut out = HashMap::new();
    let is_excluded = |dir: &Path| {
        excludes.iter().any(|ex| dir == ex || dir.starts_with(ex))
            || dir
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.') && n != ".")
    };
    let to_err = |path: &Path, source: std::io::Error| DiscoveryError {
        path: path.to_path_buf(),
        source,
    };
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = std::fs::read_dir(&dir).map_err(|e| to_err(&dir, e))?;
        for entry in rd {
            let entry = entry.map_err(|e| to_err(&dir, e))?;
            let p = entry.path();
            if p.is_dir() {
                if !is_excluded(&p) {
                    stack.push(p);
                }
            } else if p.extension().and_then(|e| e.to_str()) == Some("bynk") {
                let text = std::fs::read_to_string(&p).map_err(|e| to_err(&p, e))?;
                out.insert(p, text);
            }
        }
    }
    Ok(out)
}

/// [`read_bynk_tree`] with no excludes — the common case for a single-root
/// project (`CompileOptions::single`, whose own `Roots::excludes()` is always
/// empty) and for test fixtures, which rarely declare an `exclude` list.
pub fn read_bynk_tree_single(root: &Path) -> Result<HashMap<PathBuf, String>, DiscoveryError> {
    read_bynk_tree(root, &[])
}

/// [`read_bynk_tree`] merged across a split project's up-to-two `include`
/// roots (`src`/`tests`), matching `Roots::Split`'s own `resolve()` shape —
/// the primary root's `paths.exclude` (plus the tool's own `out`/
/// `node_modules` caches) applies to both.
///
/// #1081 review: the secondary root is optional in a way the primary is not —
/// a conventional `src`-only project has no `tests/` at all — so only its
/// absence is tolerated (`.exists()`); a missing *primary* root surfaces as a
/// real [`DiscoveryError`], same as any other unreadable directory.
pub fn read_bynk_tree_split(
    primary_root: &Path,
    secondary_root: &Path,
    excludes: &[PathBuf],
) -> Result<HashMap<PathBuf, String>, DiscoveryError> {
    let mut out = read_bynk_tree(primary_root, excludes)?;
    if secondary_root != primary_root && secondary_root.exists() {
        out.extend(read_bynk_tree(secondary_root, excludes)?);
    }
    Ok(out)
}

/// The complete `CompileOptions.sources` map for a project rooted at `roots`.
///
/// #1081 review: this used to re-implement `Roots::resolve`/`Roots::excludes`
/// verbatim (down to the hardcoded `out`/`node_modules` cache list) because
/// those were private to `bynk-emit`. Now that they're `pub`, this calls them
/// directly — one definition of "which files are in this project", shared by
/// the CLI's own walk and `compile_project`'s in-memory partitioning, so the
/// two can no longer drift.
pub fn sources_for_roots(roots: &Roots) -> Result<HashMap<PathBuf, String>, DiscoveryError> {
    let (primary, secondary) = roots.resolve();
    read_bynk_tree_split(&primary, &secondary, &roots.excludes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bynk_emit::project::{ProjectPaths, read_project_paths};
    use std::fs;

    /// A throwaway on-disk directory tree, removed on drop (including on
    /// panic) — mirrors `bynk-driver/tests/project_diagnostics.rs`'s own
    /// `Scratch` helper.
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(tag: &str, files: &[(&str, &str)]) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "bynk_driver_discovery_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        for (rel, body) in files {
            let p = dir.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, body).unwrap();
        }
        Scratch(dir)
    }

    #[test]
    fn walks_nested_files_and_skips_hidden_directories() {
        let root = scratch(
            "hidden",
            &[
                ("a.bynk", "context a\n"),
                ("nested/b.bynk", "context b\n"),
                (".git/c.bynk", "context c\n"),
            ],
        );
        let found = read_bynk_tree(&root.0, &[]).expect("a real tree must not error");
        assert_eq!(
            found.len(),
            2,
            "found: {:?}",
            found.keys().collect::<Vec<_>>()
        );
        assert!(found.contains_key(&root.0.join("a.bynk")));
        assert!(found.contains_key(&root.0.join("nested/b.bynk")));
    }

    #[test]
    fn excludes_skip_the_named_subtree() {
        let root = scratch(
            "excludes",
            &[("a.bynk", "context a\n"), ("out/b.bynk", "context b\n")],
        );
        let found = read_bynk_tree(&root.0, &[root.0.join("out")]).expect("must not error");
        assert_eq!(found.len(), 1);
        assert!(found.contains_key(&root.0.join("a.bynk")));
    }

    #[test]
    fn an_empty_tree_is_not_an_error() {
        let root = scratch("empty", &[(".keep", "")]);
        let found = read_bynk_tree(&root.0, &[]).expect("an empty tree is a valid, empty result");
        assert!(found.is_empty());
    }

    #[test]
    fn a_missing_root_is_a_discovery_error_not_a_panic() {
        let missing = std::env::temp_dir().join(format!(
            "bynk_driver_discovery_missing_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&missing);
        let err = read_bynk_tree(&missing, &[]).expect_err("a nonexistent root must not panic");
        assert!(err.to_string().contains(&missing.display().to_string()));
    }

    #[test]
    fn split_secondary_root_is_optional_but_primary_is_not() {
        // `tests/` need not exist for a `src`-only project.
        let root = scratch("split_no_secondary", &[("src/a.bynk", "context a\n")]);
        let found = read_bynk_tree_split(&root.0.join("src"), &root.0.join("tests"), &[])
            .expect("a missing secondary root is tolerated");
        assert_eq!(found.len(), 1);

        // A missing *primary* root is a real error, matching the review's
        // "missing include root panics instead of erroring" finding.
        let missing_primary = root.0.join("does-not-exist");
        read_bynk_tree_split(&missing_primary, &root.0.join("tests"), &[])
            .expect_err("a missing primary root must error, not panic");
    }

    /// The `include.len()` == 0/1/2 branches `Roots::Split::resolve` takes,
    /// exercised through `sources_for_roots` the way `project_options` calls
    /// it — the file set a real `bynk.toml` project resolves to.
    #[test]
    fn sources_for_roots_matches_a_conventional_split_project() {
        let root = scratch(
            "conventional",
            &[
                ("bynk.toml", "[project]\nname = \"x\"\n"),
                ("src/a.bynk", "context a\n"),
                ("tests/a.bynk", "suite a\n"),
            ],
        );
        let paths: ProjectPaths = read_project_paths(&root.0);
        let roots = Roots::Split {
            project_root: root.0.clone(),
            paths,
        };
        let found = sources_for_roots(&roots).expect("a well-formed project must not error");
        assert_eq!(found.len(), 2);
        assert!(found.contains_key(&root.0.join("src/a.bynk")));
        assert!(found.contains_key(&root.0.join("tests/a.bynk")));
    }

    #[test]
    fn sources_for_roots_on_a_flat_include_root_excludes_its_own_caches() {
        // `include = ["."]` (`ProjectPaths::conventional`'s flat fallback):
        // one root, and `node_modules`/`out` must still be swept out of it.
        let root = scratch(
            "flat",
            &[
                ("bynk.toml", "[project]\nname = \"x\"\n"),
                ("a.bynk", "context a\n"),
                ("node_modules/dep.bynk", "context dep\n"),
                ("out/built.bynk", "context built\n"),
            ],
        );
        let paths = ProjectPaths {
            include: vec![PathBuf::from(".")],
            exclude: Vec::new(),
        };
        let roots = Roots::Split {
            project_root: root.0.clone(),
            paths,
        };
        let found = sources_for_roots(&roots).expect("must not error");
        assert_eq!(
            found.len(),
            1,
            "found: {:?}",
            found.keys().collect::<Vec<_>>()
        );
        assert!(found.contains_key(&root.0.join("a.bynk")));
    }
}
