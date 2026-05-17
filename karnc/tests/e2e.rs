//! End-to-end fixture-driven tests.
//!
//! Each subdirectory under `tests/fixtures/positive/` contains an `input.karn`
//! and an `expected.ts`; the test compiles the input and asserts the output
//! matches the expectation exactly.
//!
//! Each subdirectory under `tests/fixtures/negative/` contains an `input.karn`
//! and an `expected_error.txt` describing the expected error category and a
//! substring of the diagnostic message; the test compiles the input and
//! asserts compilation fails with a matching error.

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

#[test]
fn positive_fixtures() {
    let dirs = fixture_dirs("positive");
    assert!(!dirs.is_empty(), "no positive fixtures found");
    let mut failures = Vec::new();
    for dir in dirs {
        let input = dir.join("input.karn");
        let expected = dir.join("expected.ts");
        let source = read(&input);
        let name = input.display().to_string();
        match karnc::compile(&source, &name) {
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
                let rendered = karnc::render_errors(&errors, &source, &name);
                failures.push(format!(
                    "\n=== {} ===\nexpected compile success but got errors:\n{}",
                    dir.display(),
                    rendered,
                ));
            }
        }
    }
    if !failures.is_empty() {
        panic!("positive fixtures failed:\n{}", failures.join("\n"));
    }
}

#[test]
fn negative_fixtures() {
    let dirs = fixture_dirs("negative");
    assert!(!dirs.is_empty(), "no negative fixtures found");
    let mut failures = Vec::new();
    for dir in dirs {
        let input = dir.join("input.karn");
        let expected = dir.join("expected_error.txt");
        let source = read(&input);
        let want = read(&expected);
        let want = want.trim();
        // Each line of the expectation file is an alternative substring; the
        // test passes if every line matches somewhere in the diagnostic output
        // (typically a category and a message fragment).
        let name = input.display().to_string();
        match karnc::compile(&source, &name) {
            Ok(_) => {
                failures.push(format!(
                    "\n=== {} ===\nexpected compile failure but compilation succeeded",
                    dir.display(),
                ));
            }
            Err(errors) => {
                // Use a plain string of categories + messages so the matcher
                // doesn't get tangled in ariadne's colour codes.
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
    }
    if !failures.is_empty() {
        panic!("negative fixtures failed:\n{}", failures.join("\n"));
    }
}
