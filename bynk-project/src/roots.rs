use std::path::{Path, PathBuf};

use crate::paths::ProjectPaths;

/// Distinguishes a commons from a context (and from a test) in the project
/// graph. Tests are a third kind in v0.7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKind {
    Commons,
    Context,
    Test,
    /// v0.16: a `test integration` multi-Worker integration test.
    Integration,
    /// v0.17: an `adapter` — the host boundary (capability contract + binding).
    Adapter,
}

impl UnitKind {
    pub fn display(self) -> &'static str {
        match self {
            UnitKind::Commons => "commons",
            UnitKind::Context => "context",
            UnitKind::Test => "test",
            UnitKind::Integration => "integration test",
            UnitKind::Adapter => "adapter",
        }
    }
}

/// Where a project's `.bynk` files live.
#[derive(Clone)]
pub enum Roots {
    /// A single tree walked as one root (in-memory builds and legacy
    /// single-file/single-tree inputs).
    Single(PathBuf),
    /// v0.113 (DECISION S): a project rooted at `project_root`, with a flat
    /// `include`/`exclude` layout (`ProjectPaths`). Test-ness is structural (a
    /// `suite` declaration), so there is no source/test role split — the tree is
    /// walked for `.bynk` files and each declaration is partitioned by kind.
    Split {
        project_root: PathBuf,
        paths: ProjectPaths,
    },
}

impl Roots {
    /// Every `include` tree this project's roots resolve to, each paired with
    /// its project-root-relative `include` prefix — joined onto that tree's
    /// (root-relative) `source_path` to build each file's `identity_path`
    /// (empty for a single-root project). A file's identity path is relative
    /// to the root that contains it.
    ///
    /// R3.9 (#1113): `[paths] include` is no longer capped at a
    /// hardcoded primary/secondary pair — every entry becomes its own tree,
    /// walked and discovered like any other. `pub` so `bynk-driver` walks
    /// exactly the trees `compile_project` would, rather than a
    /// hand-duplicated copy that can silently drift from this one.
    pub fn trees(&self) -> Vec<(PathBuf, PathBuf)> {
        match self {
            Roots::Single(root) => vec![(root.clone(), PathBuf::new())],
            Roots::Split {
                project_root,
                paths,
            } => paths
                .include
                .iter()
                .map(|p| {
                    let root = project_root.join(p);
                    // `.` normalises to an empty prefix — a join identity.
                    // `ProjectPaths::conventional` pushes `"."` for the flat
                    // layout (`.bynk` at the project root, no `src/`), and
                    // `Path::new(".").join("x.bynk")` is `./x.bynk`, not
                    // `x.bynk` (`Components` keeps the leading `CurDir`), which
                    // would leak a `./` into every snapshot key and reported
                    // path.
                    let prefix = if p.as_path() == Path::new(".") {
                        PathBuf::new()
                    } else {
                        p.clone()
                    };
                    (root, prefix)
                })
                .collect(),
        }
    }

    /// Where `bynk.schema.lock` lives — distinct from [`Self::trees`]'s
    /// per-`include` roots, since a `Split` layout's roots are subdirectories
    /// of the project root, but the registry belongs beside `bynk.toml`, one
    /// level up, the same place `bynk.deploy.lock` lives.
    ///
    /// #1085 review: used only to give `schema_registry::parse`'s corruption
    /// diagnostic a real location again — #1078 made `bynk-emit` disk-free
    /// for this file, which cost the absolute path the old message carried.
    /// Never used for I/O; `bynk-driver`'s own `schema_lock::lock_path` is
    /// the one that actually names the file for its own error messages.
    pub fn project_root(&self) -> &Path {
        match self {
            Roots::Single(root) => root,
            Roots::Split { project_root, .. } => project_root,
        }
    }

    /// Absolute subtrees to skip during discovery: the author's `exclude` list
    /// plus the tool's own build-output and dependency caches (`out`,
    /// `node_modules`), so a project whose `include` is the root does not sweep
    /// up generated or vendored files.
    pub fn excludes(&self) -> Vec<PathBuf> {
        match self {
            Roots::Single(_) => Vec::new(),
            Roots::Split {
                project_root,
                paths,
            } => {
                let mut ex: Vec<PathBuf> =
                    paths.exclude.iter().map(|p| project_root.join(p)).collect();
                for cache in ["out", "node_modules"] {
                    ex.push(project_root.join(cache));
                }
                ex
            }
        }
    }
}

/// [`crate::CompileOptions`]-equivalent's `schema_registry` value in
/// `bynk-emit` — whether a build reconciles `bynk.schema.lock`, and if so,
/// its current content.
///
/// A plain `Option<String>` was rejected for this: "off" and "on, fresh
/// project" would both spell `None`, and a future edit that lost the disk
/// read (a real bug, not a hypothetical — see #1078's history) would still
/// type-check as "on, fresh project" instead of failing to compile. The two
/// states are kept structurally distinct instead.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SchemaLock {
    /// Reconciliation is off for this build — `bynk-emit` never touches
    /// `unit_tables`' event shapes against any registry, and emits each
    /// event's `schemaVersion` straight from `EventDecl::schema_version()`.
    #[default]
    Off,
    /// On. `existing` is `bynk.schema.lock`'s current content, pre-read by
    /// the caller (never by `bynk-emit`). `None` means **verified absent** —
    /// the caller checked and there is no lock file yet, so this is the
    /// project's first-ever reconciliation and every event baselines
    /// silently. It must not mean "content unavailable for some other
    /// reason" (a permission error, a read that was skipped) — that would
    /// silently re-baseline a real registry's history, exactly the
    /// corruption `schema_registry::parse` exists to refuse when the content
    /// *is* present but unparseable.
    On { existing: Option<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_root_is_one_empty_prefixed_tree() {
        let roots = Roots::Single(PathBuf::from("/proj"));
        assert_eq!(
            roots.trees(),
            vec![(PathBuf::from("/proj"), PathBuf::new())]
        );
        assert!(roots.excludes().is_empty());
    }

    #[test]
    fn split_conventional_two_roots_prefix_by_their_own_name() {
        let roots = Roots::Split {
            project_root: PathBuf::from("/proj"),
            paths: ProjectPaths {
                include: vec![PathBuf::from("src"), PathBuf::from("tests")],
                exclude: Vec::new(),
            },
        };
        assert_eq!(
            roots.trees(),
            vec![
                (PathBuf::from("/proj/src"), PathBuf::from("src")),
                (PathBuf::from("/proj/tests"), PathBuf::from("tests")),
            ]
        );
    }

    #[test]
    fn split_flat_dot_include_normalises_to_an_empty_prefix() {
        let roots = Roots::Split {
            project_root: PathBuf::from("/proj"),
            paths: ProjectPaths {
                include: vec![PathBuf::from(".")],
                exclude: Vec::new(),
            },
        };
        assert_eq!(
            roots.trees(),
            vec![(PathBuf::from("/proj"), PathBuf::new())]
        );
    }

    /// R3.9 (#1113): three or more `include` roots are no longer silently
    /// dropped past the second — every entry becomes its own walked tree.
    #[test]
    fn split_three_or_more_include_roots_are_all_resolved() {
        let roots = Roots::Split {
            project_root: PathBuf::from("/proj"),
            paths: ProjectPaths {
                include: vec![
                    PathBuf::from("src"),
                    PathBuf::from("tests"),
                    PathBuf::from("examples"),
                ],
                exclude: Vec::new(),
            },
        };
        assert_eq!(
            roots.trees(),
            vec![
                (PathBuf::from("/proj/src"), PathBuf::from("src")),
                (PathBuf::from("/proj/tests"), PathBuf::from("tests")),
                (PathBuf::from("/proj/examples"), PathBuf::from("examples")),
            ]
        );
    }

    /// A single-entry `include` collapses to one walked tree — no invented
    /// second tree mirroring the first (the pre-R3.9 `Roots::resolve` shape
    /// this replaces always did that structurally; `trees()` does it because
    /// there is genuinely only one entry to iterate).
    #[test]
    fn split_single_include_entry_is_one_tree() {
        let roots = Roots::Split {
            project_root: PathBuf::from("/p"),
            paths: ProjectPaths {
                include: vec![PathBuf::from("src")],
                exclude: Vec::new(),
            },
        };
        assert_eq!(
            roots.trees(),
            vec![(PathBuf::from("/p/src"), PathBuf::from("src"))]
        );
    }

    #[test]
    fn excludes_always_add_the_tool_caches() {
        let roots = Roots::Split {
            project_root: PathBuf::from("/proj"),
            paths: ProjectPaths {
                include: vec![PathBuf::from("src")],
                exclude: vec![PathBuf::from("vendor")],
            },
        };
        let ex = roots.excludes();
        assert!(ex.contains(&PathBuf::from("/proj/vendor")));
        assert!(ex.contains(&PathBuf::from("/proj/out")));
        assert!(ex.contains(&PathBuf::from("/proj/node_modules")));
    }
}
