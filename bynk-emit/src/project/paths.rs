use super::*;

/// v0.17 [DECISION L] stub: a version range is *unpinned* — and rejected — when
/// it is empty, `*`/`x`/`latest`, or otherwise carries no concrete version
/// number. A pinned range names at least one digit (`^5`, `~1.2`, `1.2.3`,
/// `>=1.0 <2`). No allow-list or registry check yet.
pub(crate) fn is_unpinned_range(range: &str) -> bool {
    let r = range.trim();
    if r.is_empty() || r == "*" || r.eq_ignore_ascii_case("x") || r.eq_ignore_ascii_case("latest") {
        return true;
    }
    !r.chars().any(|c| c.is_ascii_digit())
}

/// Render a minimal `package.json` carrying the adapter-declared dependencies.
pub(crate) fn render_package_json(deps: &std::collections::BTreeMap<String, String>) -> String {
    let mut out = String::from("{\n  \"dependencies\": {\n");
    let entries: Vec<String> = deps
        .iter()
        .map(|(pkg, range)| format!("    {}: {}", json_string(pkg), json_string(range)))
        .collect();
    out.push_str(&entries.join(",\n"));
    out.push_str("\n  }\n}\n");
    out
}

/// Minimal JSON string escaping for package names and version ranges.
fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Normalise a relative path by resolving `.` and `..` components, so a binding
/// clause like `./tokens.binding.ts` beside `src/tokens.bynk` yields the output
/// path `tokens.binding.ts`.
pub(crate) fn normalize_rel(p: &Path) -> PathBuf {
    let mut out: Vec<std::ffi::OsString> = Vec::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(s) => out.push(s.to_os_string()),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    out.iter().collect()
}

/// v0.113 (DECISION S): the project's source tree, read from `bynk.toml`'s
/// `[paths]` section. Test-ness is a property of the `suite` declaration, not of
/// a directory, so the layout is a flat **`include`** list of trees to compile
/// and an **`exclude`** list of subtrees to skip — not the role-named
/// `src`/`tests` split. Each `include` entry is a root walked for `.bynk` files;
/// a file's identity path is relative to the `include` root that contains it.
#[derive(Debug, Clone)]
pub struct ProjectPaths {
    /// Trees to compile, relative to the project root. Defaults to the
    /// conventional roots that exist (`src`, and `tests` when present), else the
    /// project root itself.
    pub include: Vec<PathBuf>,
    /// Subtrees to skip during discovery (monorepo, vendored, or generated
    /// `.bynk`), relative to the project root.
    pub exclude: Vec<PathBuf>,
}

impl ProjectPaths {
    /// The default layout when `bynk.toml` declares no `[paths] include`: the
    /// conventional `src`/`tests` roots that exist under `project_root`, or the
    /// project root itself when neither does. This keeps a conventional
    /// `src/`(+`tests/`) project working with no config, and lets a flat project
    /// (`.bynk` at the root, no `src/`) compile with no config either.
    pub fn conventional(project_root: &Path) -> Self {
        let mut include = Vec::new();
        for role in ["src", "tests"] {
            if project_root.join(role).is_dir() {
                include.push(PathBuf::from(role));
            }
        }
        if include.is_empty() {
            include.push(PathBuf::from("."));
        }
        ProjectPaths {
            include,
            exclude: Vec::new(),
        }
    }
}

/// v0.113: read `bynk.toml` from `project_root`. Returns the conventional layout
/// (see [`ProjectPaths::conventional`]) if the file is missing or declares no
/// `[paths] include`. Honours `include` / `exclude` under `[paths]`, each an
/// array of path strings; anything else is ignored. A minimal hand-rolled TOML
/// reader — we only need string and string-array values here.
pub fn read_project_paths(project_root: &Path) -> ProjectPaths {
    let mut include: Vec<PathBuf> = Vec::new();
    let mut exclude: Vec<PathBuf> = Vec::new();
    let toml_path = project_root.join("bynk.toml");
    let Ok(content) = fs::read_to_string(&toml_path) else {
        return ProjectPaths::conventional(project_root);
    };
    let mut in_paths_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(section) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_paths_section = section.trim() == "paths";
            continue;
        }
        if !in_paths_section {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        match key.trim() {
            "include" => include = parse_path_array(value.trim()),
            "exclude" => exclude = parse_path_array(value.trim()),
            _ => {}
        }
    }
    if include.is_empty() {
        include = ProjectPaths::conventional(project_root).include;
    }
    ProjectPaths { include, exclude }
}

/// Parse a TOML string-array value (`["a", "b"]`) into paths. Tolerates a bare
/// quoted string (`"a"`) as a one-element list. Whitespace and quotes are
/// stripped from each element; empty elements are dropped.
fn parse_path_array(value: &str) -> Vec<PathBuf> {
    let inner = value
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(value);
    inner
        .split(',')
        .map(|el| el.trim())
        .map(|el| {
            el.strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| el.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(el)
        })
        .filter(|el| !el.is_empty())
        .map(PathBuf::from)
        .collect()
}

pub(crate) fn commons_dir_for(name: &str) -> PathBuf {
    let parts: Vec<&str> = name.split('.').collect();
    let mut p = PathBuf::new();
    for part in parts {
        p.push(part);
    }
    p
}

pub(crate) fn ts_output_path(source: &Path) -> PathBuf {
    let mut out = source.to_path_buf();
    out.set_extension("ts");
    out
}

/// v0.8: directory name of a Worker for a given context, with dots replaced
/// by dashes (`commerce.payment` → `commerce-payment`).
pub fn worker_dir_name(context: &str) -> String {
    context.replace('.', "-")
}

/// v0.8: project-relative synthetic source path of the workers-mode
/// handlers file for a given context. Used so the emitter's relative-import
/// machinery resolves correctly against the workers layout.
pub fn worker_handlers_source_path(context: &str) -> PathBuf {
    PathBuf::from(format!(
        "workers/{}/handlers.bynk",
        worker_dir_name(context)
    ))
}

/// v0.8: project-relative output path of the workers-mode handlers file.
pub fn worker_handlers_output_path(context: &str) -> PathBuf {
    PathBuf::from(format!("workers/{}/handlers.ts", worker_dir_name(context)))
}

/// The src-stripped stem components of a path (`learner/uln.bynk` → `["learner",
/// "uln"]`), dropping the extension and any non-`Normal` components.
fn stem_parts(rel_path: &Path) -> Vec<String> {
    rel_path
        .with_extension("")
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect()
}

/// v0.9.1: shared between source-unit and test-unit path validation. The
/// caller decides which root to strip from the file path before calling.
///
/// A file belongs to `qualified_name` when it is either the single file
/// `<name>.bynk` (`single_file_match`: stem parts == name parts) or one file of
/// the directory layout `<name>/*.bynk` (`multi_file_match`: parent-dir parts ==
/// name parts). These two branches are the single source of truth the v0.132
/// barrel trigger reads via [`is_multi_file_layout`].
pub(crate) fn unit_path_matches(rel_path: &Path, qualified_name: &str) -> bool {
    let name_parts: Vec<&str> = qualified_name.split('.').collect();
    let stem_parts = stem_parts(rel_path);
    let single_file_match = stem_parts.len() == name_parts.len()
        && stem_parts
            .iter()
            .zip(name_parts.iter())
            .all(|(a, b)| a == b);
    single_file_match || is_multi_file_parts(&stem_parts, &name_parts)
}

/// True when `stem_parts` is one file of the `<name>/*.bynk` directory layout —
/// the file's parent-directory parts equal the name parts.
fn is_multi_file_parts(stem_parts: &[String], name_parts: &[&str]) -> bool {
    if stem_parts.is_empty() {
        return false;
    }
    let parent_parts = &stem_parts[..stem_parts.len() - 1];
    parent_parts.len() == name_parts.len()
        && parent_parts
            .iter()
            .zip(name_parts.iter())
            .all(|(a, b)| a == b)
}

/// v0.132: does `rel_path` (src-stripped) place `qualified_name` under a
/// directory of that name — the `multi_file_match` branch of
/// [`unit_path_matches`]?
///
/// This is the layout where production emits `out/<name>/*.ts` per file and no
/// aggregate `out/<name>.ts`, so the test path's `import * as ns from
/// "./<name>.js"` dangles and needs an aggregating barrel. A single-file commons
/// (`<name>.bynk`) already owns `out/<name>.ts` and returns false, so a barrel
/// keyed on this predicate can never collide with it.
pub(crate) fn is_multi_file_layout(rel_path: &Path, qualified_name: &str) -> bool {
    let name_parts: Vec<&str> = qualified_name.split('.').collect();
    is_multi_file_parts(&stem_parts(rel_path), &name_parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    // -- is_unpinned_range ----------------------------------------------------
    #[test]
    fn is_unpinned_range_true_for_wildcards_and_digitless() {
        assert!(is_unpinned_range(""));
        assert!(is_unpinned_range("*"));
        assert!(is_unpinned_range("x"));
        assert!(is_unpinned_range("X"));
        assert!(is_unpinned_range("latest"));
        assert!(is_unpinned_range("LATEST"));
        assert!(is_unpinned_range("  *  ")); // trimmed before the checks
        assert!(is_unpinned_range("workspace:*")); // no ascii digit
        assert!(is_unpinned_range("beta"));
    }

    #[test]
    fn is_unpinned_range_false_when_a_digit_is_present() {
        assert!(!is_unpinned_range("1.0.0"));
        assert!(!is_unpinned_range("^1.2"));
        assert!(!is_unpinned_range("~0.1"));
        assert!(!is_unpinned_range(">=2"));
        assert!(!is_unpinned_range("18"));
    }

    // -- normalize_rel --------------------------------------------------------
    #[test]
    fn normalize_rel_resolves_dot_and_parent() {
        assert_eq!(
            normalize_rel(Path::new("./tokens.binding.ts")),
            PathBuf::from("tokens.binding.ts")
        );
        assert_eq!(normalize_rel(Path::new("a/./b")), PathBuf::from("a/b"));
        assert_eq!(normalize_rel(Path::new("a/../b")), PathBuf::from("b"));
        assert_eq!(normalize_rel(Path::new("a/b/../../c")), PathBuf::from("c"));
        assert_eq!(normalize_rel(Path::new("a/b")), PathBuf::from("a/b"));
    }

    #[test]
    fn normalize_rel_drops_root_and_pops_through_empty() {
        // RootDir / Prefix components are dropped.
        assert_eq!(normalize_rel(Path::new("/a/b")), PathBuf::from("a/b"));
        // A leading `..` pops an empty stack (a no-op), so it vanishes.
        assert_eq!(normalize_rel(Path::new("../a")), PathBuf::from("a"));
    }

    // -- commons_dir_for / ts_output_path -------------------------------------
    #[test]
    fn commons_dir_for_splits_dotted_name_into_dirs() {
        assert_eq!(commons_dir_for("a.b.c"), PathBuf::from("a/b/c"));
        assert_eq!(commons_dir_for("foo"), PathBuf::from("foo"));
    }

    #[test]
    fn ts_output_path_sets_ts_extension() {
        assert_eq!(
            ts_output_path(Path::new("foo.bynk")),
            PathBuf::from("foo.ts")
        );
        assert_eq!(
            ts_output_path(Path::new("a/b.bynk")),
            PathBuf::from("a/b.ts")
        );
        assert_eq!(ts_output_path(Path::new("foo")), PathBuf::from("foo.ts"));
    }

    // -- worker path helpers --------------------------------------------------
    #[test]
    fn worker_paths_dasherise_and_root_under_workers() {
        assert_eq!(worker_dir_name("commerce.payment"), "commerce-payment");
        assert_eq!(worker_dir_name("plain"), "plain");
        assert_eq!(
            worker_handlers_source_path("commerce.payment"),
            PathBuf::from("workers/commerce-payment/handlers.bynk")
        );
        assert_eq!(
            worker_handlers_output_path("commerce.payment"),
            PathBuf::from("workers/commerce-payment/handlers.ts")
        );
    }

    // -- unit_path_matches ----------------------------------------------------
    #[test]
    fn unit_path_matches_single_file_layout() {
        assert!(unit_path_matches(Path::new("a/b/c.bynk"), "a.b.c"));
        assert!(unit_path_matches(Path::new("foo.bynk"), "foo"));
    }

    #[test]
    fn unit_path_matches_multi_file_layout() {
        // `a/b/c/<any>.bynk` declaring `a.b.c` (the directory is the unit).
        assert!(unit_path_matches(Path::new("a/b/c/handlers.bynk"), "a.b.c"));
        assert!(unit_path_matches(Path::new("a/b/c/anything.bynk"), "a.b.c"));
    }

    #[test]
    fn unit_path_matches_rejects_misalignment() {
        assert!(!unit_path_matches(Path::new("a/b.bynk"), "a.b.c"));
        assert!(!unit_path_matches(Path::new("x/y/z.bynk"), "a.b.c"));
    }

    // -- is_multi_file_layout (v0.132 barrel trigger) -------------------------
    #[test]
    fn is_multi_file_layout_true_only_for_directory_layout() {
        // Directory layout: `<name>/*.bynk` — the branch with no `out/<name>.ts`.
        assert!(is_multi_file_layout(Path::new("thing/a.bynk"), "thing"));
        assert!(is_multi_file_layout(Path::new("thing/b.bynk"), "thing"));
        // Dotted commons split across `src/a/b/*.bynk`.
        assert!(is_multi_file_layout(Path::new("a/b/one.bynk"), "a.b"));
    }

    #[test]
    fn is_multi_file_layout_false_for_single_file_and_misalignment() {
        // Single file `<name>.bynk` already owns `out/<name>.ts` — no barrel.
        assert!(!is_multi_file_layout(Path::new("thing.bynk"), "thing"));
        // Dotted single file `a/b.bynk` for `a.b` — the file *is* `out/a/b.ts`.
        assert!(!is_multi_file_layout(Path::new("a/b.bynk"), "a.b"));
        // Wrong directory — not this unit's file.
        assert!(!is_multi_file_layout(Path::new("other/a.bynk"), "thing"));
    }
}
