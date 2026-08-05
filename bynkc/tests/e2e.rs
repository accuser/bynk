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
//! Either shape may additionally carry an optional `expected_contains.txt`
//! and/or `expected_absent.txt` (#58) — each a newline-separated list of
//! substrings (`#`-prefixed lines and blanks ignored) that must, respectively,
//! appear or not appear somewhere in the emitted output. These compose with
//! the whole-file snapshot above rather than replacing it — a granularity
//! between "byte-identical" and "nothing at all" for a fixture that only
//! cares about one emitted fragment. See [`check_expected_contains_absent`].
//!
//! Each subdirectory under `tests/fixtures/negative/` contains either an
//! `input.bynk` (single-file) or a `src/` (project) input, plus at least one
//! of: an `expected_error.txt` listing `category message` substrings the
//! diagnostics must contain, or (project-mode only, #58) an
//! `expected_diagnostics.txt` listing exact `category<TAB>path:line:col`
//! lines — see [`check_expected_diagnostics`].

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
                    // Every kind the emitter writes: `.ts`, `wrangler.toml`,
                    // and JSON (`package.json` for an adapter fixture,
                    // `bynk-secrets.json` for a context declaring an auth
                    // secret). Bless writes *all* emitted files, so anything
                    // this walk skips is invisible to the expected-side
                    // comparison and then flagged as a surplus output — which
                    // is why the JSON case is a bare extension check rather
                    // than the one-filename special case it started as.
                    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if matches!(ext, "ts" | "toml" | "json") {
                        out.push(p);
                    }
                }
            }
        }
    }
    out.sort();
    out
}

/// #58: `FileCheck`-style substring assertions (rustc's `tests/codegen`
/// precedent) — a granularity between whole-file byte identity and a bare
/// diagnostic category string. `expected_contains.txt` lines must each appear
/// somewhere in `haystack`; `expected_absent.txt` lines must each not. Either
/// file is optional and independent of the other, and both compose with a
/// fixture's ordinary `expected.ts`/`expected/` snapshot rather than replacing
/// it — a fixture that only cares about one emitted fragment doesn't have to
/// stop pinning the rest of the file to also assert that fragment precisely.
fn check_expected_contains_absent(dir: &Path, haystack: &str, failures: &mut Vec<String>) {
    let contains = dir.join("expected_contains.txt");
    if contains.exists() {
        for needle in read(&contains)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            if !haystack.contains(needle) {
                failures.push(format!(
                    "\n=== {} ===\nexpected_contains.txt: expected output to contain `{}`, but it did not",
                    dir.display(),
                    needle,
                ));
            }
        }
    }
    let absent = dir.join("expected_absent.txt");
    if absent.exists() {
        for needle in read(&absent)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            if haystack.contains(needle) {
                failures.push(format!(
                    "\n=== {} ===\nexpected_absent.txt: expected output to NOT contain `{}`, but it did",
                    dir.display(),
                    needle,
                ));
            }
        }
    }
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
    if let Ok(s) = fs::read_to_string(&marker) {
        match s.trim() {
            "node" => return bynkc::Platform::Node,
            "browser" => return bynkc::Platform::Browser,
            _ => {}
        }
    }
    bynkc::Platform::Cloudflare
}

/// v0.9.1: compile a project-form fixture. If the fixture root has a
/// `bynk.toml`, use split-paths mode rooted at the fixture root. Otherwise,
/// fall back to the legacy single-tree mode rooted at `src/`.
fn compile_fixture(
    fixture_root: &Path,
    target: bynkc::BuildTarget,
) -> Result<bynkc::ProjectOutput, bynkc::ProjectFailure> {
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
            &bynk_testkit::compile_options_split(fixture_root.to_path_buf(), paths).target(target),
        )
    } else {
        let src_dir = fixture_root.join("src");
        let platform = fixture_platform(fixture_root);
        bynkc::compile_project(
            &bynk_testkit::compile_options_single(src_dir)
                .target(target)
                .platform(platform),
        )
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
                    check_expected_contains_absent(&dir, &actual, &mut failures);
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
                    // #58: check across every emitted file — fixture-level, not
                    // per-file, since a substring assertion doesn't need to name
                    // which file it expects the fragment in.
                    let haystack: String = out
                        .files
                        .iter()
                        .map(|f| f.typescript.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    check_expected_contains_absent(&dir, &haystack, &mut failures);
                }
                Err(failure) => {
                    let rendered = bynkc::render_project_errors(&failure.flatten());
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
            // #527: single-file fixtures (input.bynk → expected.ts) bless too;
            // previously an emission change left them to fail by hand.
            let input = dir.join("input.bynk");
            if input.exists() {
                let source = read(&input);
                let ts =
                    bynkc::compile(&source, &input.display().to_string()).unwrap_or_else(|e| {
                        panic!("bless: {} failed to compile: {e:?}", dir.display())
                    });
                fs::write(dir.join("expected.ts"), ts).unwrap();
            }
            continue;
        }
        let target = fixture_target(&dir);
        let out = match compile_fixture(&dir, target) {
            Ok(out) => out,
            Err(failure) => {
                panic!(
                    "bless: {} failed to compile:\n{}",
                    dir.display(),
                    bynkc::render_project_errors(&failure.flatten())
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

/// #58 (adjacent to #696): checks a project fixture's `expected_diagnostics.txt`
/// — optional, `category<TAB>path:line:col` lines — against the *attributed*
/// errors a project-mode compile produced. Unlike `expected_error.txt`'s
/// substring-over-`category message` check, this pins the file and position
/// too: `project.rs:5013-5017`'s and `bynk-driver/tests/project_diagnostics.rs`'s
/// (#696) escapes were both an identity collision the category-only check
/// could not have caught, because neither carried a path to disagree on.
fn check_expected_diagnostics(
    dir: &Path,
    errors: &[bynkc::AttributedError],
    snapshots: &[(PathBuf, String)],
    failures: &mut Vec<String>,
) {
    let expected = dir.join("expected_diagnostics.txt");
    if !expected.exists() {
        return;
    }
    let want = read(&expected);
    let actual: Vec<String> = errors
        .iter()
        .map(|e| {
            let path = e
                .source_path
                .as_deref()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|| "<unattributed>".to_string());
            let source = e
                .source_path
                .as_deref()
                .and_then(|p| snapshots.iter().find(|(sp, _)| sp == p))
                .map(|(_, s)| s.as_str())
                .unwrap_or("");
            let (line, col) = bynk_syntax::span::line_col(source, e.error.span.start);
            format!("{}\t{path}:{line}:{col}", e.error.category)
        })
        .collect();
    for needle in want.lines() {
        let needle = needle.trim();
        if needle.is_empty() || needle.starts_with('#') {
            continue;
        }
        if !actual.iter().any(|a| a == needle) {
            failures.push(format!(
                "\n=== {} ===\nexpected_diagnostics.txt: expected `{}`, but got:\n{}",
                dir.display(),
                needle,
                actual.join("\n"),
            ));
        }
    }
}

#[test]
fn negative_fixtures() {
    let dirs = fixture_dirs("negative");
    assert!(!dirs.is_empty(), "no negative fixtures found");
    let mut failures = Vec::new();
    for dir in dirs {
        let input = dir.join("input.bynk");
        let src_dir = dir.join("src");
        let expected_error = dir.join("expected_error.txt");
        let has_expected_error = expected_error.exists();
        // `expected_diagnostics.txt` needs path attribution, which only a
        // project-mode compile carries — a single-file fixture must still
        // supply `expected_error.txt`, or nothing here would ever check it.
        let has_expected_diagnostics =
            src_dir.is_dir() && dir.join("expected_diagnostics.txt").exists();
        assert!(
            has_expected_error || has_expected_diagnostics,
            "{}: negative fixture has neither expected_error.txt nor \
             (a project-mode) expected_diagnostics.txt",
            dir.display()
        );
        let want = if has_expected_error {
            read(&expected_error)
        } else {
            String::new()
        };
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
                    // Each line carries the diagnostic's 1-indexed position as
                    // an ` @ line:col` suffix, so a fixture can pin the span
                    // (`code message @ 5:22`) — plain `code message` needles
                    // keep matching as substrings of the same line.
                    let haystack: String = errors
                        .iter()
                        .map(|e| {
                            let (line, col) = bynk_syntax::span::line_col(&source, e.span.start);
                            format!("{} {} @ {line}:{col}\n", e.category, e.message)
                        })
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
                Err(failure) => {
                    let haystack: String = failure
                        .errors
                        .iter()
                        .map(|e| format!("{} {}\n", e.error.category, e.error.message))
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
                    check_expected_diagnostics(
                        &dir,
                        &failure.errors,
                        &failure.snapshots,
                        &mut failures,
                    );
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
