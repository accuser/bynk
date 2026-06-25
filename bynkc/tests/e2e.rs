//! End-to-end fixture-driven tests.
//!
//! Each subdirectory under `tests/fixtures/positive/` is one fixture. There
//! are two supported shapes:
//!
//! - **Single-file**: `input.bynk` + `expected.ts`. The compiler runs in
//!   single-file mode and the output is compared against `expected.ts`.
//! - **Project**: a `src/` directory and an `expected/` directory mirroring
//!   the same source tree, with `.bynk` files rewritten to `.ts`. The
//!   compiler runs in project mode (`compile_project`) and every generated
//!   file is compared against its counterpart under `expected/`.
//!
//! Each subdirectory under `tests/fixtures/negative/` contains either an
//! `input.bynk` (single-file) or a `src/` (project) input plus an
//! `expected_error.txt` listing category strings the diagnostics must
//! contain.

use std::fs;
use std::path::{Path, PathBuf};

fn fixture_dirs(category: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(category);
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&root) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

fn collect_expected_ts(expected_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![expected_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(rd) = fs::read_dir(&dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                    let is_package_json =
                        p.file_name().and_then(|n| n.to_str()) == Some("package.json");
                    if ext == "ts" || ext == "toml" || is_package_json {
                        out.push(p);
                    }
                }
            }
        }
    }
    out.sort();
    out
}

/// Read the build target marker from a fixture root, if present.
/// Defaults to bundle when no `target.txt` is present.
fn fixture_target(dir: &Path) -> bynkc::BuildTarget {
    let marker = dir.join("target.txt");
    if let Ok(s) = fs::read_to_string(&marker)
        && s.trim() == "workers"
    {
        return bynkc::BuildTarget::Workers;
    }
    bynkc::BuildTarget::Bundle
}

/// v0.18: read the deploy-platform marker from a fixture root, if present.
/// Defaults to cloudflare when no `platform.txt` is present.
fn fixture_platform(dir: &Path) -> bynkc::Platform {
    let marker = dir.join("platform.txt");
    if let Ok(s) = fs::read_to_string(&marker)
        && s.trim() == "node"
    {
        return bynkc::Platform::Node;
    }
    bynkc::Platform::Cloudflare
}

/// v0.9.1: compile a project-form fixture. If the fixture root has a
/// `bynk.toml`, use split-paths mode rooted at the fixture root. Otherwise,
/// fall back to the legacy single-tree mode rooted at `src/`.
fn compile_fixture(
    fixture_root: &Path,
    target: bynkc::BuildTarget,
) -> Result<bynkc::ProjectOutput, Vec<bynkc::CompileError>> {
    let bynk_toml = fixture_root.join("bynk.toml");
    if bynk_toml.exists() {
        // Split-paths mode compiles at the default platform — a bynk.toml
        // fixture with a platform.txt would silently ignore it.
        assert!(
            !fixture_root.join("platform.txt").exists(),
            "{}: platform.txt is not supported in bynk.toml fixtures",
            fixture_root.display()
        );
        let paths = bynkc::read_project_paths(fixture_root);
        bynkc::compile_project(
            &bynkc::CompileOptions::split(fixture_root.to_path_buf(), paths).target(target),
        )
        .map_err(bynkc::ProjectFailure::flatten)
    } else {
        let src_dir = fixture_root.join("src");
        let platform = fixture_platform(fixture_root);
        bynkc::compile_project(
            &bynkc::CompileOptions::single(src_dir)
                .target(target)
                .platform(platform),
        )
        .map_err(bynkc::ProjectFailure::flatten)
    }
}

#[test]
fn positive_fixtures() {
    let dirs = fixture_dirs("positive");
    assert!(!dirs.is_empty(), "no positive fixtures found");
    let mut failures = Vec::new();
    for dir in dirs {
        let input = dir.join("input.bynk");
        let src_dir = dir.join("src");
        if input.exists() {
            let expected = dir.join("expected.ts");
            let source = read(&input);
            let name = input.display().to_string();
            match bynkc::compile(&source, &name) {
                Ok(actual) => {
                    let want = read(&expected);
                    if actual.trim_end() != want.trim_end() {
                        failures.push(format!(
                            "\n=== {} ===\n--- expected ---\n{}\n--- actual ---\n{}\n",
                            dir.display(),
                            want,
                            actual,
                        ));
                    }
                }
                Err(errors) => {
                    let rendered = bynkc::render_errors(&errors, &source, &name);
                    failures.push(format!(
                        "\n=== {} ===\nexpected compile success but got errors:\n{}",
                        dir.display(),
                        rendered,
                    ));
                }
            }
        } else if src_dir.is_dir() {
            let expected_dir = dir.join("expected");
            let target = fixture_target(&dir);
            match compile_fixture(&dir, target) {
                Ok(out) => {
                    // Build expected set by walking expected_dir.
                    let expected_files = collect_expected_ts(&expected_dir);
                    let mut actual_by_path: std::collections::HashMap<PathBuf, String> =
                        std::collections::HashMap::new();
                    // Skip project-wide boilerplate (runtime.ts, tsconfig.json):
                    // emitted identically for every project, separately unit
                    // tested. Excluding them keeps per-fixture snapshots focused
                    // on the per-context emission.
                    for f in &out.files {
                        let p = f.output_path.to_string_lossy();
                        if p == "runtime.ts" || p == "tsconfig.json" {
                            continue;
                        }
                        actual_by_path.insert(f.output_path.clone(), f.typescript.clone());
                    }
                    // For each expected .ts file, compare.
                    let mut all_ok = true;
                    let mut report = String::new();
                    for ef in &expected_files {
                        let rel = ef.strip_prefix(&expected_dir).unwrap().to_path_buf();
                        let want = read(ef);
                        let actual = actual_by_path.get(&rel);
                        match actual {
                            Some(a) => {
                                if a.trim_end() != want.trim_end() {
                                    all_ok = false;
                                    report.push_str(&format!(
                                        "\n--- {} ---\n--- expected ---\n{}\n--- actual ---\n{}\n",
                                        rel.display(),
                                        want,
                                        a,
                                    ));
                                }
                            }
                            None => {
                                all_ok = false;
                                report.push_str(&format!(
                                    "\n--- missing output: {} ---\n",
                                    rel.display()
                                ));
                            }
                        }
                    }
                    // Check there are no surplus outputs we didn't expect.
                    let mut expected_rels: std::collections::HashSet<PathBuf> = expected_files
                        .iter()
                        .map(|p| p.strip_prefix(&expected_dir).unwrap().to_path_buf())
                        .collect();
                    for f in &out.files {
                        let p = f.output_path.to_string_lossy();
                        if p == "runtime.ts" || p == "tsconfig.json" {
                            continue;
                        }
                        if !expected_rels.remove(&f.output_path) {
                            all_ok = false;
                            report.push_str(&format!(
                                "\n--- unexpected output: {} ---\n--- actual ---\n{}\n",
                                f.output_path.display(),
                                f.typescript,
                            ));
                        }
                    }
                    // ADR 0117: assert the build's non-failing warnings against
                    // `expected_warnings.txt` (substrings, like
                    // `expected_error.txt`). Absent file ⇒ the build must emit
                    // no warnings — so a positive fixture cannot start warning
                    // silently.
                    let rendered_warnings: String = out
                        .warnings
                        .iter()
                        .map(|w| format!("{} {}\n", w.error.category, w.error.message))
                        .collect();
                    let want_warn = dir.join("expected_warnings.txt");
                    if want_warn.exists() {
                        for line in read(&want_warn)
                            .lines()
                            .map(str::trim)
                            .filter(|l| !l.is_empty() && !l.starts_with('#'))
                        {
                            if !rendered_warnings.contains(line) {
                                all_ok = false;
                                report.push_str(&format!(
                                    "\n--- missing warning `{line}` ---\n--- got ---\n{rendered_warnings}\n"
                                ));
                            }
                        }
                    } else if !out.warnings.is_empty() {
                        all_ok = false;
                        report.push_str(&format!(
                            "\n--- unexpected warning(s) ---\n{rendered_warnings}\n"
                        ));
                    }
                    if !all_ok {
                        failures.push(format!("\n=== {} ==={}", dir.display(), report));
                    }
                }
                Err(errors) => {
                    let rendered = bynkc::render_project_errors(&errors);
                    failures.push(format!(
                        "\n=== {} ===\nexpected compile success but got errors:\n{}",
                        dir.display(),
                        rendered,
                    ));
                }
            }
        } else {
            failures.push(format!(
                "\n=== {} ===\nfixture has neither `input.bynk` nor `src/`",
                dir.display()
            ));
        }
    }
    if !failures.is_empty() {
        panic!("positive fixtures failed:\n{}", failures.join("\n"));
    }
}

/// Regenerate the `expected/` snapshot of every project-form positive fixture
/// from the current emitter. Gated behind `BYNK_BLESS=1` so it never runs in a
/// normal `cargo test`. Use after an intentional emission change, then review
/// the resulting `git diff` to confirm the changes are what you meant.
#[test]
fn bless_positive_fixtures() {
    if std::env::var("BYNK_BLESS").is_err() {
        return;
    }
    for dir in fixture_dirs("positive") {
        let src_dir = dir.join("src");
        if !src_dir.is_dir() {
            continue;
        }
        let target = fixture_target(&dir);
        let out = match compile_fixture(&dir, target) {
            Ok(out) => out,
            Err(errors) => {
                panic!(
                    "bless: {} failed to compile:\n{}",
                    dir.display(),
                    bynkc::render_project_errors(&errors)
                );
            }
        };
        let expected_dir = dir.join("expected");
        let _ = fs::remove_dir_all(&expected_dir);
        for f in &out.files {
            let p = f.output_path.to_string_lossy();
            // Mirror the comparison's skip list: project-wide boilerplate is
            // unit-tested separately and excluded from per-fixture snapshots.
            if p == "runtime.ts" || p == "tsconfig.json" {
                continue;
            }
            let target_path = expected_dir.join(&f.output_path);
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&target_path, &f.typescript).unwrap();
        }
    }
}

/// The `/* unknown */` placeholder must never reach emitted output. It is the
/// signature of an unresolved lowering path (historically the agent
/// method-call regression). This guard compiles every positive fixture and
/// fails if any emitted file contains the marker — the regression backstop the
/// v0.7 fix lacked.
#[test]
fn no_unknown_placeholder_in_emitted_output() {
    // Every emitter placeholder is the signature of an unresolved lowering path
    // that would otherwise ship invalid TypeScript. `/* unknown */` is the
    // instance-method-call fallback (the historical v0.7 regression);
    // `complex is-receiver` is the `is`-pattern receiver fallback. Markers use
    // distinct text, so the guard checks for each one explicitly — add new
    // markers here as they are introduced.
    const MARKERS: &[&str] = &["/* unknown */", "complex is-receiver"];
    let hit = |s: &str| MARKERS.iter().any(|m| s.contains(m));
    let mut offenders = Vec::new();
    for dir in fixture_dirs("positive") {
        let input = dir.join("input.bynk");
        let src_dir = dir.join("src");
        if input.exists() {
            let source = read(&input);
            if let Ok(actual) = bynkc::compile(&source, &input.display().to_string())
                && hit(&actual)
            {
                offenders.push(dir.display().to_string());
            }
        } else if src_dir.is_dir() {
            let target = fixture_target(&dir);
            if let Ok(out) = compile_fixture(&dir, target) {
                for f in &out.files {
                    if hit(&f.typescript) {
                        offenders.push(format!("{} :: {}", dir.display(), f.output_path.display()));
                    }
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "emitted output contains an emitter placeholder ({MARKERS:?}) in:\n{}",
        offenders.join("\n"),
    );
}

#[test]
fn negative_fixtures() {
    let dirs = fixture_dirs("negative");
    assert!(!dirs.is_empty(), "no negative fixtures found");
    let mut failures = Vec::new();
    for dir in dirs {
        let input = dir.join("input.bynk");
        let src_dir = dir.join("src");
        let expected = dir.join("expected_error.txt");
        let want = read(&expected);
        let want = want.trim();
        if input.exists() {
            let source = read(&input);
            let name = input.display().to_string();
            match bynkc::compile(&source, &name) {
                Ok(_) => {
                    failures.push(format!(
                        "\n=== {} ===\nexpected compile failure but compilation succeeded",
                        dir.display(),
                    ));
                }
                Err(errors) => {
                    let haystack: String = errors
                        .iter()
                        .map(|e| format!("{} {}\n", e.category, e.message))
                        .collect();
                    for needle in want.lines() {
                        let needle = needle.trim();
                        if needle.is_empty() || needle.starts_with('#') {
                            continue;
                        }
                        if !haystack.contains(needle) {
                            failures.push(format!(
                                "\n=== {} ===\nexpected error containing `{}`, but got:\n{}",
                                dir.display(),
                                needle,
                                haystack,
                            ));
                        }
                    }
                }
            }
        } else if src_dir.is_dir() {
            let target = fixture_target(&dir);
            match compile_fixture(&dir, target) {
                Ok(_) => {
                    failures.push(format!(
                        "\n=== {} ===\nexpected compile failure but compilation succeeded",
                        dir.display(),
                    ));
                }
                Err(errors) => {
                    let haystack: String = errors
                        .iter()
                        .map(|e| format!("{} {}\n", e.category, e.message))
                        .collect();
                    for needle in want.lines() {
                        let needle = needle.trim();
                        if needle.is_empty() || needle.starts_with('#') {
                            continue;
                        }
                        if !haystack.contains(needle) {
                            failures.push(format!(
                                "\n=== {} ===\nexpected error containing `{}`, but got:\n{}",
                                dir.display(),
                                needle,
                                haystack,
                            ));
                        }
                    }
                }
            }
        } else {
            failures.push(format!(
                "\n=== {} ===\nfixture has neither `input.bynk` nor `src/`",
                dir.display()
            ));
        }
    }
    if !failures.is_empty() {
        panic!("negative fixtures failed:\n{}", failures.join("\n"));
    }
}
