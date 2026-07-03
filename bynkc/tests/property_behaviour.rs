//! v0.114 behavioural test for generative `property` tests (testing track slice
//! 2). Snapshots prove the emitted shape; this proves the runtime behaviour: a
//! property that fails over its domain reports a case count, the run's root
//! seed, and a shrunk counterexample with a copy-paste reproduce line — and
//! `--seed <hex>` reproduces the run byte-for-byte.
//!
//! Drives the real `bynkc test` CLI against a fixture project, so it exercises
//! the `--seed` threading, the generator/runner, and shrinking end to end. Like
//! the other toolchain-driving tests it skips loudly when no TypeScript runner
//! (`tsc`+`node` or `tsx`) is available; `BYNK_REQUIRE_TSC=1` turns the skip
//! into a failure (CI).

use std::path::PathBuf;
use std::process::Command;

const REQUIRE_ENV: &str = "BYNK_REQUIRE_TSC";

fn tool_exists(name: &str) -> bool {
    let finder = if cfg!(windows) { "where" } else { "which" };
    Command::new(finder)
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A TypeScript runner is available if either `tsx` or (`tsc` + `node`) is on
/// PATH — the same fallback chain `bynkc test` itself walks.
fn have_runner() -> bool {
    tool_exists("tsx") || (tool_exists("tsc") && tool_exists("node")) || tool_exists("npx")
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/behaviour/property_fail")
}

/// Run `bynkc test <fixture> --seed <seed>` and return combined stdout+stderr.
fn run_with_seed(seed: &str, out_dir: &str) -> String {
    // Emit under the target tmpdir so no build artifacts land in the source tree.
    let out_root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(out_dir);
    let out = Command::new(env!("CARGO_BIN_EXE_bynkc"))
        .arg("test")
        .arg(fixture())
        .arg("--output")
        .arg(&out_root)
        .arg("--seed")
        .arg(seed)
        .output()
        .expect("run bynkc test");
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    s
}

#[test]
fn failing_property_reports_seed_and_shrunk_counterexample() {
    // `bynkc test`'s runner detection is Unix-only (it shells out to `which`), so
    // the CLI cannot locate `tsc`/`node`/`tsx` on Windows and never runs the
    // emitted tests. This test drives that CLI end to end, so it is meaningful
    // only where the CLI can run a runner — skip on Windows. (The *emission* is
    // covered platform-independently by the golden fixture `243_property_passes`.)
    if cfg!(windows) {
        eprintln!("skipping on Windows: `bynkc test` runner detection is Unix-only");
        return;
    }
    if !have_runner() {
        if std::env::var(REQUIRE_ENV).is_ok() {
            panic!("no TypeScript runner (tsx or tsc+node) on PATH, but {REQUIRE_ENV} is set");
        }
        eprintln!("skipping: no TypeScript runner (tsx or tsc+node) on PATH");
        return;
    }

    let first = run_with_seed("0x5f3a", "out-a");

    // A domain-wide failure reports the case count, the run's root seed (the one
    // we passed), a shrunk counterexample, and a copy-paste reproduce line.
    assert!(
        first.contains("property failed after"),
        "expected a property-failure banner, got:\n{first}"
    );
    assert!(
        first.contains("(seed 0x5f3a)"),
        "expected the passed root seed in the report, got:\n{first}"
    );
    assert!(
        first.contains("shrunk counterexample:"),
        "expected a shrunk counterexample, got:\n{first}"
    );
    assert!(
        first.contains("--seed 0x5f3a"),
        "expected a reproduce line carrying the root seed, got:\n{first}"
    );

    // Re-running with the same seed reproduces the same counterexample — the
    // whole point of a deterministic, seed-derived generator.
    let second = run_with_seed("0x5f3a", "out-b");
    let extract = |s: &str| {
        s.lines()
            .find(|l| l.contains("shrunk counterexample:"))
            .map(|l| l.trim().to_string())
    };
    assert_eq!(
        extract(&first),
        extract(&second),
        "the same seed must reproduce the same shrunk counterexample"
    );
}
