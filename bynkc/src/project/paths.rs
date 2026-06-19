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

/// v0.9.1: per-project source-tree layout, read from `bynk.toml`'s `[paths]`
/// section.
#[derive(Debug, Clone)]
pub struct ProjectPaths {
    /// Source-unit root, relative to the project root.
    pub src: PathBuf,
    /// Test-unit root, relative to the project root.
    pub tests: PathBuf,
}

impl ProjectPaths {
    /// The conventional layout used when `bynk.toml` is absent: sources under
    /// `src/`, tests under `tests/`.
    pub fn conventional() -> Self {
        ProjectPaths {
            src: PathBuf::from("src"),
            tests: PathBuf::from("tests"),
        }
    }
}

/// v0.9.1: read `bynk.toml` from `project_root`. Returns the conventional
/// layout if the file is missing or doesn't declare `[paths]`. Only `src` and
/// `tests` keys under `[paths]` are honoured; anything else is ignored. A
/// minimal hand-rolled TOML reader — we only need string-valued keys here.
pub fn read_project_paths(project_root: &Path) -> ProjectPaths {
    let toml_path = project_root.join("bynk.toml");
    let mut paths = ProjectPaths::conventional();
    let Ok(content) = fs::read_to_string(&toml_path) else {
        return paths;
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
        let key = key.trim();
        let value = value.trim();
        let unquoted = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        match key {
            "src" => paths.src = PathBuf::from(unquoted),
            "tests" => paths.tests = PathBuf::from(unquoted),
            _ => {}
        }
    }
    paths
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

/// Does a file's relative path match a qualified name? Two arrangements are
/// valid:
/// - **Single-file**: `a/b/c.bynk` declaring `a.b.c`.
/// - **Multi-file**: `a/b/c/<any>.bynk` declaring `a.b.c`.
///
/// #47: in split-paths mode a test file may use the self-identifying
/// `<target-path>.test.bynk` form as well as the bare `<target-path>.bynk`
/// (single-tree mode already uses the suffixed form). Normalise the former to
/// the latter so the two conventions are unified for path-alignment matching.
pub(crate) fn strip_test_infix(rel_path: &Path) -> PathBuf {
    if let Some(name) = rel_path.file_name().and_then(|n| n.to_str())
        && let Some(base) = name.strip_suffix(".test.bynk")
    {
        return rel_path.with_file_name(format!("{base}.bynk"));
    }
    rel_path.to_path_buf()
}

/// v0.9.1: shared between source-unit and test-unit path validation. The
/// caller decides which root to strip from the file path before calling.
pub(crate) fn unit_path_matches(rel_path: &Path, qualified_name: &str) -> bool {
    let name_parts: Vec<&str> = qualified_name.split('.').collect();
    let stem = rel_path.with_extension("");
    let stem_parts: Vec<String> = stem
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    let parent_parts: Vec<String> = if stem_parts.is_empty() {
        Vec::new()
    } else {
        stem_parts[..stem_parts.len() - 1].to_vec()
    };
    let single_file_match = stem_parts.len() == name_parts.len()
        && stem_parts
            .iter()
            .zip(name_parts.iter())
            .all(|(a, b)| a == b);
    let multi_file_match = parent_parts.len() == name_parts.len()
        && parent_parts
            .iter()
            .zip(name_parts.iter())
            .all(|(a, b)| a == b);
    single_file_match || multi_file_match
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

    #[test]
    fn strip_test_infix_normalises_the_dot_test_suffix() {
        // `<path>.test.bynk` → `<path>.bynk`; the bare form is untouched.
        assert_eq!(
            strip_test_infix(Path::new("a/b/c.test.bynk")),
            PathBuf::from("a/b/c.bynk")
        );
        assert_eq!(
            strip_test_infix(Path::new("demo.test.bynk")),
            PathBuf::from("demo.bynk")
        );
        assert_eq!(
            strip_test_infix(Path::new("a/b/c.bynk")),
            PathBuf::from("a/b/c.bynk")
        );
    }

    #[test]
    fn test_path_alignment_accepts_either_form_after_normalisation() {
        // #47: both the bare and `.test.bynk` forms align for the same target.
        for p in ["a/b.bynk", "a/b.test.bynk", "a/b/x.bynk", "a/b/x.test.bynk"] {
            assert!(
                unit_path_matches(&strip_test_infix(Path::new(p)), "a.b"),
                "{p} should align with `a.b`"
            );
        }
        // A genuinely misaligned `.test.bynk` is still rejected.
        assert!(!unit_path_matches(
            &strip_test_infix(Path::new("a/z.test.bynk")),
            "a.b"
        ));
    }
}
