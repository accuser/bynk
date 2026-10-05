//! Drift guard: #1676 — every workflow that runs the workspace suite runs it
//! with the same harness and the same profile, so the retry and isolation
//! policy is decided once, in `.config/nextest.toml`'s `ci` profile.
//!
//! The three gates drifted once already: the PR gate ran nextest under `--profile
//! ci` (one process per test, one retry) while the release ran `cargo test`
//! (threads in a shared process, no retry), so a release could fail a test the
//! PR gate passed for a harness reason alone. This guard turns the next such
//! drift into a failing test rather than a comment that quietly became false.

use std::path::PathBuf;

/// The command every suite-running gate invokes.
const SUITE_COMMAND: &str = "cargo nextest run --workspace --locked --profile ci";

/// The workflows that run the workspace suite: the PR gate, the release gate,
/// and the bootstrap's verify job.
const SUITE_WORKFLOWS: [&str; 3] = ["ci.yml", "release.yml", "release-bootstrap.yml"];

fn workflow(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.github/workflows")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

/// The non-comment lines of a workflow, so prose that *mentions* a command
/// (as these workflows' comments do) is never mistaken for a step that runs it.
fn run_lines(yaml: &str) -> Vec<&str> {
    yaml.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .collect()
}

#[test]
fn every_suite_gate_runs_nextest_under_the_ci_profile() {
    for name in SUITE_WORKFLOWS {
        let yaml = workflow(name);
        assert!(
            run_lines(&yaml).iter().any(|l| l.ends_with(SUITE_COMMAND)),
            "{name} must run the workspace suite as `{SUITE_COMMAND}` — the one \
             harness and profile every gate shares (.config/nextest.toml). A \
             release that needs a different policy gets a named profile there, \
             not a different command here."
        );
    }
}

#[test]
fn no_suite_gate_runs_the_workspace_suite_under_cargo_test() {
    for name in SUITE_WORKFLOWS {
        let yaml = workflow(name);
        let offenders: Vec<&str> = run_lines(&yaml)
            .into_iter()
            .filter(|l| l.contains("cargo test --workspace"))
            .collect();
        assert!(
            offenders.is_empty(),
            "{name} runs the workspace suite under `cargo test` (threads in one \
             process, no retry) — use `{SUITE_COMMAND}` like the other gates: \
             {offenders:?}"
        );
    }
}
