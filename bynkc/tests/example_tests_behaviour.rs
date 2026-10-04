//! #291: `examples.rs` proves every example *builds*; this runs the `tests/`
//! suites the examples ship, so a showcase test that stops passing fails CI.
//! Several of them (`sessions`, `event-log`, `webhook-relay`) drive handlers on
//! platform capabilities, so they also pin the deterministic test doubles
//! `bynkc test` provides for `bynk`'s `Clock`, `Secrets`, `Fetch` and `Logger`.
//!
//! Like the other toolchain-driving tests it skips loudly when no TypeScript
//! runner is available; `BYNK_REQUIRE_TSC=1` turns the skip into a failure.

use std::path::{Path, PathBuf};
use std::process::Command;

const REQUIRE_ENV: &str = "BYNK_REQUIRE_TSC";

fn tool_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The same fallback chain `bynkc test` walks: `tsx`, or `tsc` + `node`, or `npx`.
fn have_runner() -> bool {
    tool_exists("tsx") || (tool_exists("tsc") && tool_exists("node")) || tool_exists("npx")
}

/// Every example project with a `tests/` directory, sorted for a stable report.
fn examples_with_tests() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("read examples")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("bynk.toml").is_file() && p.join("tests").is_dir())
        .collect();
    out.sort();
    out
}

// One CLI-driving `#[test]` per file, running the examples in turn: concurrent
// `bynkc test` runs from one test binary interleave (see
// `adapter_flattened_capability_stub_behaviour.rs`).
#[test]
fn every_example_test_suite_passes() {
    if cfg!(windows) {
        eprintln!("skipping on Windows: `bynkc test` runner detection is Unix-only");
        return;
    }
    if !have_runner() {
        if std::env::var(REQUIRE_ENV).is_ok() {
            panic!("no TypeScript runner (tsx or tsc+node) on PATH, but {REQUIRE_ENV} is set");
        }
        eprintln!(
            "\n!!! EXAMPLE TEST SUITES SKIPPED !!!\nno TypeScript runner (tsx or tsc+node) on PATH.\n"
        );
        return;
    }

    let examples = examples_with_tests();
    assert!(
        examples.len() >= 3,
        "expected the examples' test suites, found {examples:?}"
    );
    let scratch = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("example-tests");
    let mut problems: Vec<String> = Vec::new();
    for example in &examples {
        let name = example.file_name().expect("example name").to_string_lossy();
        // Each run gets its own parent directory: the runner writes its
        // executed `out-js` tree as a sibling of `--output`.
        let output = scratch.join(name.as_ref()).join("out");
        let _ = std::fs::remove_dir_all(scratch.join(name.as_ref()));
        let out = Command::new(env!("CARGO_BIN_EXE_bynkc"))
            .arg("test")
            .arg(example)
            .arg("--output")
            .arg(&output)
            .arg("--format")
            .arg("json")
            .output()
            .expect("run bynkc test");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let report = serde_json::from_str::<serde_json::Value>(&stdout).ok();
        let passed = report.as_ref().and_then(|d| d["passed"].as_u64());
        let failed = report.as_ref().and_then(|d| d["failed"].as_u64());
        if !out.status.success() || failed != Some(0) || passed.unwrap_or(0) == 0 {
            problems.push(format!(
                "examples/{name}: `bynkc test` exited {:?} (passed {passed:?}, failed {failed:?})\nstdout:\n{stdout}\nstderr:\n{}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n\n"));
}
